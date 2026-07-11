//! Builtin effect registry + GPU ping-pong chain (Phase 3.4).
//!
//! Effects run on each layer's uploaded texture *before* the cover+transform composite
//! draw. Unknown `effect_id`s are rejected at command validation; this module only
//! executes the locked builtins below.

use crate::project::EffectInstance;
use std::collections::BTreeMap;
use wgpu::util::DeviceExt;

use super::ComposeError;

/// Locked builtin effect ids (also useful for GUI pickers).
pub const BUILTIN_EFFECT_IDS: &[&str] = &[
    "builtin:color_adjust",
    "builtin:blur",
    "builtin:lut_contrast",
    "builtin:lut_warm",
    "builtin:glitch",
    "builtin:chroma_key",
];

/// Public list of builtin effect ids for GUI / CLI discovery.
pub fn builtin_effect_ids() -> &'static [&'static str] {
    BUILTIN_EFFECT_IDS
}

pub fn is_builtin_effect_id(effect_id: &str) -> bool {
    BUILTIN_EFFECT_IDS.contains(&effect_id)
}

/// Default params for a builtin (empty map if unknown).
pub fn default_params(effect_id: &str) -> BTreeMap<String, f64> {
    let mut m = BTreeMap::new();
    match effect_id {
        "builtin:color_adjust" => {
            m.insert("exposure".into(), 0.0);
            m.insert("contrast".into(), 1.0);
            m.insert("saturation".into(), 1.0);
        }
        "builtin:blur" => {
            m.insert("radius".into(), 0.0);
        }
        "builtin:lut_contrast" | "builtin:lut_warm" => {
            m.insert("intensity".into(), 1.0);
        }
        "builtin:glitch" => {
            m.insert("intensity".into(), 0.5);
            m.insert("slice".into(), 0.5);
        }
        "builtin:chroma_key" => {
            m.insert("key_r".into(), 0.0);
            m.insert("key_g".into(), 1.0);
            m.insert("key_b".into(), 0.0);
            m.insert("tolerance".into(), 0.3);
            m.insert("softness".into(), 0.1);
        }
        _ => {}
    }
    m
}

/// Clamp known params into finite, reasonable ranges. Unknown keys left unchanged.
pub fn clamp_effect_params(effect_id: &str, params: &mut BTreeMap<String, f64>) {
    for (k, v) in params.iter_mut() {
        *v = match (effect_id, k.as_str()) {
            ("builtin:color_adjust", "exposure") => v.clamp(-5.0, 5.0),
            ("builtin:color_adjust", "contrast") => v.clamp(0.0, 4.0),
            ("builtin:color_adjust", "saturation") => v.clamp(0.0, 4.0),
            ("builtin:blur", "radius") => v.clamp(0.0, 64.0),
            ("builtin:lut_contrast" | "builtin:lut_warm", "intensity") => v.clamp(0.0, 1.0),
            ("builtin:glitch", "intensity") => v.clamp(0.0, 1.0),
            ("builtin:glitch", "slice") => v.clamp(0.0, 1.0),
            ("builtin:chroma_key", "key_r" | "key_g" | "key_b") => v.clamp(0.0, 1.0),
            ("builtin:chroma_key", "tolerance" | "softness") => v.clamp(0.0, 1.0),
            _ => *v,
        };
    }
}

fn param_or(params: &BTreeMap<String, f64>, key: &str, default: f64) -> f64 {
    params.get(key).copied().unwrap_or(default)
}

fn has_enabled_effects(effects: &[EffectInstance]) -> bool {
    effects.iter().any(|e| e.enabled)
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ColorAdjustUniform {
    exposure: f32,
    contrast: f32,
    saturation: f32,
    _pad: f32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BlurUniform {
    texel: [f32; 2],
    radius: f32,
    _pad: f32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct LutUniform {
    intensity: f32,
    mode: u32,
    _pad0: f32,
    _pad1: f32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GlitchUniform {
    intensity: f32,
    slice: f32,
    time_seed: f32,
    _pad: f32,
}

enum EffectKind {
    ColorAdjust,
    Blur,
    Lut,
    Glitch,
}

struct PingPongRt {
    /// Kept alive so `view` remains valid.
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    width: u32,
    height: u32,
}

/// GPU resources for the builtin effect chain (owned by [`super::Compositor`]).
pub struct EffectProcessor {
    sampler: wgpu::Sampler,
    bind_group_layout: wgpu::BindGroupLayout,
    color_adjust_pipeline: wgpu::RenderPipeline,
    blur_pipeline: wgpu::RenderPipeline,
    lut_pipeline: wgpu::RenderPipeline,
    glitch_pipeline: wgpu::RenderPipeline,
    ping: Option<PingPongRt>,
    pong: Option<PingPongRt>,
    /// After a successful write, index of the RT holding the result (0=ping, 1=pong).
    result_slot: u8,
}

impl EffectProcessor {
    pub fn new(device: &wgpu::Device) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("effect-linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("effect"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("effect"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let color_adjust_pipeline = make_effect_pipeline(
            device,
            &pipeline_layout,
            "color_adjust",
            include_str!("color_adjust.wgsl"),
        );
        let blur_pipeline =
            make_effect_pipeline(device, &pipeline_layout, "blur", include_str!("blur.wgsl"));
        let lut_pipeline =
            make_effect_pipeline(device, &pipeline_layout, "lut", include_str!("lut.wgsl"));
        let glitch_pipeline = make_effect_pipeline(
            device,
            &pipeline_layout,
            "glitch",
            include_str!("glitch.wgsl"),
        );

        Self {
            sampler,
            bind_group_layout,
            color_adjust_pipeline,
            blur_pipeline,
            lut_pipeline,
            glitch_pipeline,
            ping: None,
            pong: None,
            result_slot: 0,
        }
    }

    fn ensure_rts(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        let needs = |rt: &Option<PingPongRt>| {
            rt.as_ref()
                .map(|r| r.width != width || r.height != height)
                .unwrap_or(true)
        };
        if needs(&self.ping) {
            self.ping = Some(create_rt(device, "effect-ping", width, height));
        }
        if needs(&self.pong) {
            self.pong = Some(create_rt(device, "effect-pong", width, height));
        }
    }

    pub fn result_view(&self) -> &wgpu::TextureView {
        match self.result_slot {
            0 => &self.ping.as_ref().expect("effect ping RT").view,
            _ => &self.pong.as_ref().expect("effect pong RT").view,
        }
    }

    /// Run enabled effects on `src_view` into ping-pong RTs. Returns `false` when nothing
    /// was written (caller keeps using `src_view`).
    pub fn apply(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        src_view: &wgpu::TextureView,
        width: u32,
        height: u32,
        effects: &[EffectInstance],
    ) -> Result<bool, ComposeError> {
        if !has_enabled_effects(effects) {
            return Ok(false);
        }

        self.ensure_rts(device, width, height);

        let mut read_src = true;
        let mut wrote = false;

        for effect in effects.iter().filter(|e| e.enabled) {
            match effect.effect_id.as_str() {
                "builtin:color_adjust" => {
                    let u = ColorAdjustUniform {
                        exposure: param_or(&effect.params, "exposure", 0.0) as f32,
                        contrast: param_or(&effect.params, "contrast", 1.0) as f32,
                        saturation: param_or(&effect.params, "saturation", 1.0) as f32,
                        _pad: 0.0,
                    };
                    self.draw_pass(
                        device,
                        encoder,
                        src_view,
                        read_src,
                        EffectKind::ColorAdjust,
                        bytemuck::bytes_of(&u),
                    )?;
                    read_src = false;
                    wrote = true;
                }
                "builtin:blur" => {
                    let radius = param_or(&effect.params, "radius", 0.0) as f32;
                    if radius < 0.5 {
                        continue;
                    }
                    let u_h = BlurUniform {
                        texel: [1.0 / width as f32, 0.0],
                        radius,
                        _pad: 0.0,
                    };
                    self.draw_pass(
                        device,
                        encoder,
                        src_view,
                        read_src,
                        EffectKind::Blur,
                        bytemuck::bytes_of(&u_h),
                    )?;
                    read_src = false;
                    wrote = true;

                    let u_v = BlurUniform {
                        texel: [0.0, 1.0 / height as f32],
                        radius,
                        _pad: 0.0,
                    };
                    self.draw_pass(
                        device,
                        encoder,
                        src_view,
                        read_src,
                        EffectKind::Blur,
                        bytemuck::bytes_of(&u_v),
                    )?;
                }
                "builtin:lut_contrast" | "builtin:lut_warm" => {
                    let intensity = param_or(&effect.params, "intensity", 1.0) as f32;
                    if intensity <= 0.0 {
                        continue;
                    }
                    let mode = if effect.effect_id == "builtin:lut_contrast" {
                        0u32
                    } else {
                        1u32
                    };
                    let u = LutUniform {
                        intensity,
                        mode,
                        _pad0: 0.0,
                        _pad1: 0.0,
                    };
                    self.draw_pass(
                        device,
                        encoder,
                        src_view,
                        read_src,
                        EffectKind::Lut,
                        bytemuck::bytes_of(&u),
                    )?;
                    read_src = false;
                    wrote = true;
                }
                "builtin:glitch" => {
                    let intensity = param_or(&effect.params, "intensity", 0.5) as f32;
                    if intensity <= 0.001 {
                        continue;
                    }
                    let u = GlitchUniform {
                        intensity,
                        slice: param_or(&effect.params, "slice", 0.5) as f32,
                        time_seed: param_or(&effect.params, "seed", 0.0) as f32,
                        _pad: 0.0,
                    };
                    self.draw_pass(
                        device,
                        encoder,
                        src_view,
                        read_src,
                        EffectKind::Glitch,
                        bytemuck::bytes_of(&u),
                    )?;
                    read_src = false;
                    wrote = true;
                }
                _ => continue,
            }
        }

        Ok(wrote)
    }

    fn draw_pass(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        src_view: &wgpu::TextureView,
        read_src: bool,
        kind: EffectKind,
        uniform_bytes: &[u8],
    ) -> Result<(), ComposeError> {
        let dest_slot = if read_src { 0u8 } else { 1 - self.result_slot };

        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("effect-params"),
            contents: uniform_bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        });

        // Resolve input/dest views and pipeline without holding conflicting borrows.
        let input_is_src = read_src;
        let input_slot = self.result_slot;
        let pipeline = match kind {
            EffectKind::ColorAdjust => &self.color_adjust_pipeline,
            EffectKind::Blur => &self.blur_pipeline,
            EffectKind::Lut => &self.lut_pipeline,
            EffectKind::Glitch => &self.glitch_pipeline,
        };

        let bind_group = {
            let input_view = if input_is_src {
                src_view
            } else if input_slot == 0 {
                &self.ping.as_ref().unwrap().view
            } else {
                &self.pong.as_ref().unwrap().view
            };
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("effect-bind"),
                layout: &self.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(input_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: params_buffer.as_entire_binding(),
                    },
                ],
            })
        };

        let dest_view = if dest_slot == 0 {
            &self.ping.as_ref().unwrap().view
        } else {
            &self.pong.as_ref().unwrap().view
        };

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("effect-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dest_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..6, 0..1);
        }

        self.result_slot = dest_slot;
        Ok(())
    }
}

fn create_rt(device: &wgpu::Device, label: &str, width: u32, height: u32) -> PingPongRt {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    PingPongRt {
        _texture: texture,
        view,
        width,
        height,
    }
}

fn make_effect_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    label: &str,
    wgsl: &str,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(wgsl.into()),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}
