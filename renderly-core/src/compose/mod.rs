//! Offscreen wgpu compositor for Phase 0 export. Decoded frames are uploaded as textures,
//! scaled to the output resolution, and read back as RGBA for the FFmpeg encoder.
//! Phase 3.1 adds per-layer user transform (translate / scale / rotate) and opacity.
//! Phase 3.4 runs builtin effect chains on each layer before the cover+transform draw.
//! Phase 3 adds dual-texture WGSL transitions.

mod chroma;
pub mod effects;
mod transition;

pub use chroma::apply_chroma_effects;
pub use effects::{builtin_effect_ids, default_params, BUILTIN_EFFECT_IDS};

use crate::media::RgbaFrame;
use crate::project::{ClipTransform, EffectInstance, TransitionKind};
use effects::EffectProcessor;
use thiserror::Error;
use transition::TransitionPass;
use wgpu::util::DeviceExt;

#[derive(Debug, Error)]
pub enum ComposeError {
    #[error("no suitable GPU adapter found")]
    NoAdapter,
    #[error("wgpu error: {0}")]
    Wgpu(String),
}

/// Marks a layer as one side of an outgoing→incoming transition pair.
#[derive(Debug, Clone, Copy)]
pub struct LayerTransition {
    pub kind: TransitionKind,
    pub progress: f32,
    pub is_incoming: bool,
}

/// One composited layer: decoded (or caption) RGBA plus evaluated transform at frame time.
pub struct ComposeLayer {
    pub frame: RgbaFrame,
    pub transform: ClipTransform,
    /// Builtin effect instances (Phase 3.4). Empty / all-disabled → identity path.
    pub effects: Vec<EffectInstance>,
    /// When set on two consecutive layers (outgoing then incoming), uses the transition pass.
    pub transition: Option<LayerTransition>,
}

impl From<RgbaFrame> for ComposeLayer {
    fn from(frame: RgbaFrame) -> Self {
        Self {
            frame,
            transform: ClipTransform::default(),
            effects: Vec::new(),
            transition: None,
        }
    }
}

/// Matches `LayerParams` in composite.wgsl (48 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct LayerParamsUniform {
    cover_scale: [f32; 2],
    cover_offset: [f32; 2],
    user_translate: [f32; 2],
    user_scale: [f32; 2],
    rotation_rad: f32,
    opacity: f32,
    _pad: [f32; 2],
}

/// "Cover" fit: scale/offset (in source-texture UV space) that fill the output rect
/// without distorting the layer's aspect ratio, cropping whichever axis overflows.
/// Identity when the layer already matches the output aspect ratio (e.g. caption layers,
/// which are already rendered at the output resolution).
fn cover_uv(layer_w: u32, layer_h: u32, out_w: u32, out_h: u32) -> ([f32; 2], [f32; 2]) {
    let layer_aspect = layer_w as f32 / layer_h as f32;
    let out_aspect = out_w as f32 / out_h as f32;
    let (scale_x, scale_y) = if layer_aspect > out_aspect {
        // Layer is relatively wider than the output: crop its left/right edges.
        (out_aspect / layer_aspect, 1.0)
    } else {
        // Layer is relatively taller (or equal) than the output: crop top/bottom.
        (1.0, layer_aspect / out_aspect)
    };
    (
        [scale_x, scale_y],
        [(1.0 - scale_x) / 2.0, (1.0 - scale_y) / 2.0],
    )
}

fn layer_params(
    layer_w: u32,
    layer_h: u32,
    out_w: u32,
    out_h: u32,
    transform: &ClipTransform,
) -> LayerParamsUniform {
    let (cover_scale, cover_offset) = cover_uv(layer_w, layer_h, out_w, out_h);
    let t = transform.clamp_opacity();
    LayerParamsUniform {
        cover_scale,
        cover_offset,
        user_translate: [t.x as f32, t.y as f32],
        user_scale: [t.scale_x as f32, t.scale_y as f32],
        rotation_rad: (t.rotation_deg as f32).to_radians(),
        opacity: t.opacity as f32,
        _pad: [0.0, 0.0],
    }
}

pub struct Compositor {
    device: wgpu::Device,
    queue: wgpu::Queue,
    width: u32,
    height: u32,
    output_texture: wgpu::Texture,
    output_view: wgpu::TextureView,
    readback_buffer: wgpu::Buffer,
    bind_group_layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    effects: EffectProcessor,
    transitions: TransitionPass,
}

impl Compositor {
    pub fn new(width: u32, height: u32) -> Result<Self, ComposeError> {
        pollster::block_on(Self::new_async(width, height))
    }

    async fn new_async(width: u32, height: u32) -> Result<Self, ComposeError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .await
            .map_err(|_| ComposeError::NoAdapter)?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("renderly-export"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|e| ComposeError::Wgpu(e.to_string()))?;

        let output_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("export-target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let output_view = output_texture.create_view(&Default::default());

        let bytes_per_row = width * 4;
        let padded_bytes_per_row =
            wgpu::util::align_to(bytes_per_row, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) as usize;
        let readback_size = (padded_bytes_per_row * height as usize) as u64;

        let readback_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: readback_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("layer"),
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
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("composite"),
            source: wgpu::ShaderSource::Wgsl(include_str!("composite.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("composite"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("composite"),
            layout: Some(&pipeline_layout),
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
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let effects = EffectProcessor::new(&device);
        let transitions = TransitionPass::new(&device);

        Ok(Self {
            device,
            queue,
            width,
            height,
            output_texture,
            output_view,
            readback_buffer,
            bind_group_layout,
            pipeline,
            sampler,
            effects,
            transitions,
        })
    }

    /// Composite layers in order (first = bottom). Empty → solid black frame.
    ///
    /// Per layer: upload → optional builtin effect chain (ping-pong) → cover+transform
    /// composite draw. Layers with empty/disabled effects take the same path as before.
    pub fn composite(&mut self, layers: &[ComposeLayer]) -> Result<Vec<u8>, ComposeError> {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("export-frame"),
            });

        // Clear once up front.
        {
            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("composite-clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.output_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }

        // Hold textures until submit.
        let mut keep_alive: Vec<wgpu::Texture> = Vec::with_capacity(layers.len());

        let mut i = 0;
        while i < layers.len() {
            let pair = layers.get(i).and_then(|a| {
                let ta = a.transition?;
                if ta.is_incoming {
                    return None;
                }
                let b = layers.get(i + 1)?;
                let tb = b.transition?;
                if !tb.is_incoming || ta.kind != tb.kind {
                    return None;
                }
                Some((ta.kind, ta.progress))
            });

            if let Some((kind, progress)) = pair {
                self.transitions
                    .ensure_rts(&self.device, self.width, self.height);
                self.draw_layer_to_transition_rt(&layers[i], true, &mut encoder, &mut keep_alive)?;
                self.draw_layer_to_transition_rt(
                    &layers[i + 1],
                    false,
                    &mut encoder,
                    &mut keep_alive,
                )?;
                self.transitions.blend(
                    &self.device,
                    &mut encoder,
                    &self.output_view,
                    kind,
                    progress,
                )?;
                i += 2;
                continue;
            }

            self.draw_layer_to_output(&layers[i], &mut encoder, &mut keep_alive)?;
            i += 1;
        }

        let bytes_per_row = self.width * 4;
        let padded_bytes_per_row =
            wgpu::util::align_to(bytes_per_row, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.output_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );

        self.queue.submit(Some(encoder.finish()));
        drop(keep_alive);

        let slice = self.readback_buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .map_err(|e| ComposeError::Wgpu(format!("{e:?}")))?;
        receiver
            .recv()
            .map_err(|_| ComposeError::Wgpu("readback channel closed".into()))?
            .map_err(|e| ComposeError::Wgpu(format!("{e:?}")))?;

        let mapped = slice
            .get_mapped_range()
            .map_err(|e| ComposeError::Wgpu(e.to_string()))?;
        let mut out = vec![0u8; (self.width * self.height * 4) as usize];
        for row in 0..self.height as usize {
            let src_start = row * padded_bytes_per_row as usize;
            let dst_start = row * bytes_per_row as usize;
            out[dst_start..dst_start + bytes_per_row as usize]
                .copy_from_slice(&mapped[src_start..src_start + bytes_per_row as usize]);
        }
        drop(mapped);
        self.readback_buffer.unmap();

        Ok(out)
    }

    fn draw_layer_to_output(
        &mut self,
        layer: &ComposeLayer,
        encoder: &mut wgpu::CommandEncoder,
        keep_alive: &mut Vec<wgpu::Texture>,
    ) -> Result<(), ComposeError> {
        self.draw_layer(layer, DrawTarget::Output, encoder, keep_alive)
    }

    fn draw_layer_to_transition_rt(
        &mut self,
        layer: &ComposeLayer,
        to_a: bool,
        encoder: &mut wgpu::CommandEncoder,
        keep_alive: &mut Vec<wgpu::Texture>,
    ) -> Result<(), ComposeError> {
        self.draw_layer(
            layer,
            if to_a {
                DrawTarget::TransitionA
            } else {
                DrawTarget::TransitionB
            },
            encoder,
            keep_alive,
        )
    }

    fn draw_layer(
        &mut self,
        layer: &ComposeLayer,
        target: DrawTarget,
        encoder: &mut wgpu::CommandEncoder,
        keep_alive: &mut Vec<wgpu::Texture>,
    ) -> Result<(), ComposeError> {
        let frame = &layer.frame;
        let texture = self.device.create_texture_with_data(
            &self.queue,
            &wgpu::TextureDescriptor {
                label: Some("layer"),
                size: wgpu::Extent3d {
                    width: frame.width,
                    height: frame.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &frame.pixels,
        );
        let uploaded_view = texture.create_view(&Default::default());

        let use_effects = self.effects.apply(
            &self.device,
            encoder,
            &uploaded_view,
            frame.width,
            frame.height,
            &layer.effects,
        )?;

        let params = layer_params(
            frame.width,
            frame.height,
            self.width,
            self.height,
            &layer.transform,
        );
        let params_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("layer-params"),
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });

        let sample_view = if use_effects {
            self.effects.result_view()
        } else {
            &uploaded_view
        };

        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("layer-bind"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(sample_view),
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
        });

        let clear = matches!(target, DrawTarget::TransitionA | DrawTarget::TransitionB);
        let dest_view = match target {
            DrawTarget::Output => &self.output_view,
            DrawTarget::TransitionA => self.transitions.view_a(),
            DrawTarget::TransitionB => self.transitions.view_b(),
        };

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("composite-layer"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dest_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: if clear {
                            wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.0,
                                g: 0.0,
                                b: 0.0,
                                a: 1.0,
                            })
                        } else {
                            wgpu::LoadOp::Load
                        },
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..6, 0..1);
        }

        keep_alive.push(texture);
        Ok(())
    }
}

enum DrawTarget {
    Output,
    TransitionA,
    TransitionB,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{EffectInstance, Id};
    use std::collections::BTreeMap;

    fn solid_frame(w: u32, h: u32, r: u8, g: u8, b: u8) -> RgbaFrame {
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        for px in pixels.chunks_exact_mut(4) {
            px[0] = r;
            px[1] = g;
            px[2] = b;
            px[3] = 255;
        }
        RgbaFrame {
            width: w,
            height: h,
            pixels,
        }
    }

    fn mean_luma(rgba: &[u8]) -> f64 {
        let mut sum = 0.0f64;
        let mut n = 0usize;
        for px in rgba.chunks_exact(4) {
            sum += 0.2126 * px[0] as f64 + 0.7152 * px[1] as f64 + 0.0722 * px[2] as f64;
            n += 1;
        }
        sum / n as f64
    }

    fn effect(effect_id: &str, params: BTreeMap<String, f64>) -> EffectInstance {
        EffectInstance {
            id: Id::new_v4(),
            effect_id: effect_id.into(),
            enabled: true,
            params,
        }
    }

    #[test]
    fn cover_uv_is_identity_for_matching_aspect_ratio() {
        let (scale, offset) = cover_uv(1080, 1920, 1080, 1920);
        assert!((scale[0] - 1.0).abs() < 1e-6);
        assert!((scale[1] - 1.0).abs() < 1e-6);
        assert!((offset[0] - 0.0).abs() < 1e-6);
        assert!((offset[1] - 0.0).abs() < 1e-6);
    }

    #[test]
    fn cover_uv_crops_sides_for_wider_landscape_source_into_vertical_output() {
        // 16:9 gameplay footage into a 9:16 TikTok export: the source is relatively wider
        // than the output, so covering it means cropping the left/right edges (scale_x < 1)
        // while using the full height (scale_y == 1) — never stretching either axis.
        let (scale, offset) = cover_uv(1920, 1080, 1080, 1920);
        assert!(
            scale[0] < 1.0,
            "expected horizontal crop, got scale={scale:?}"
        );
        assert!(
            (scale[1] - 1.0).abs() < 1e-6,
            "expected full height, got scale={scale:?}"
        );
        assert!(
            (offset[0] - (1.0 - scale[0]) / 2.0).abs() < 1e-6,
            "crop should be centered"
        );
        assert!((offset[1] - 0.0).abs() < 1e-6);
    }

    #[test]
    fn cover_uv_crops_top_bottom_for_taller_source_into_landscape_output() {
        let (scale, offset) = cover_uv(1080, 1920, 1920, 1080);
        assert!(
            (scale[0] - 1.0).abs() < 1e-6,
            "expected full width, got scale={scale:?}"
        );
        assert!(
            scale[1] < 1.0,
            "expected vertical crop, got scale={scale:?}"
        );
        assert!((offset[0] - 0.0).abs() < 1e-6);
        assert!(
            (offset[1] - (1.0 - scale[1]) / 2.0).abs() < 1e-6,
            "crop should be centered"
        );
    }

    #[test]
    fn identity_transform_params_match_cover_only() {
        let params = layer_params(1920, 1080, 1080, 1920, &ClipTransform::default());
        let (scale, offset) = cover_uv(1920, 1080, 1080, 1920);
        assert_eq!(params.cover_scale, scale);
        assert_eq!(params.cover_offset, offset);
        assert_eq!(params.user_translate, [0.0, 0.0]);
        assert_eq!(params.user_scale, [1.0, 1.0]);
        assert!((params.rotation_rad).abs() < 1e-6);
        assert!((params.opacity - 1.0).abs() < 1e-6);
    }

    #[test]
    fn color_adjust_exposure_changes_luma() {
        let mut c = match Compositor::new(32, 32) {
            Ok(c) => c,
            Err(ComposeError::NoAdapter) => return,
            Err(e) => panic!("{e}"),
        };
        let frame = solid_frame(32, 32, 128, 128, 128);
        let base = c
            .composite(&[ComposeLayer {
                frame: frame.clone(),
                transform: ClipTransform::default(),
                effects: vec![],
                transition: None,
            }])
            .unwrap();
        let bright = c
            .composite(&[ComposeLayer {
                frame,
                transform: ClipTransform::default(),
                effects: vec![effect(
                    "builtin:color_adjust",
                    [("exposure".into(), 1.0)].into_iter().collect(),
                )],
                transition: None,
            }])
            .unwrap();
        assert!(
            mean_luma(&bright) > mean_luma(&base) + 10.0,
            "exposure+1 should raise luma (base={}, bright={})",
            mean_luma(&base),
            mean_luma(&bright)
        );
    }

    #[test]
    fn empty_and_disabled_effects_match_identity() {
        let mut c = match Compositor::new(16, 16) {
            Ok(c) => c,
            Err(ComposeError::NoAdapter) => return,
            Err(e) => panic!("{e}"),
        };
        let frame = solid_frame(16, 16, 40, 80, 120);
        let identity = c
            .composite(&[ComposeLayer {
                frame: frame.clone(),
                transform: ClipTransform::default(),
                effects: vec![],
                transition: None,
            }])
            .unwrap();
        let empty_chain = c
            .composite(&[ComposeLayer {
                frame: frame.clone(),
                transform: ClipTransform::default(),
                effects: vec![],
                transition: None,
            }])
            .unwrap();
        assert_eq!(identity, empty_chain);

        let mut disabled = effect(
            "builtin:color_adjust",
            [("exposure".into(), 2.0)].into_iter().collect(),
        );
        disabled.enabled = false;
        let disabled_out = c
            .composite(&[ComposeLayer {
                frame,
                transform: ClipTransform::default(),
                effects: vec![disabled],
                transition: None,
            }])
            .unwrap();
        assert_eq!(identity, disabled_out);
    }

    #[test]
    fn blur_radius_zero_is_identity_ish() {
        let mut c = match Compositor::new(16, 16) {
            Ok(c) => c,
            Err(ComposeError::NoAdapter) => return,
            Err(e) => panic!("{e}"),
        };
        let frame = solid_frame(16, 16, 200, 100, 50);
        let identity = c
            .composite(&[ComposeLayer {
                frame: frame.clone(),
                transform: ClipTransform::default(),
                effects: vec![],
                transition: None,
            }])
            .unwrap();
        let blurred = c
            .composite(&[ComposeLayer {
                frame,
                transform: ClipTransform::default(),
                effects: vec![effect(
                    "builtin:blur",
                    [("radius".into(), 0.0)].into_iter().collect(),
                )],
                transition: None,
            }])
            .unwrap();
        // radius 0 skips the blur passes entirely → bit-exact identity.
        assert_eq!(identity, blurred);
    }

    #[test]
    fn builtin_effect_ids_lists_locked_set() {
        let ids = builtin_effect_ids();
        assert_eq!(ids.len(), 6);
        assert!(ids.contains(&"builtin:color_adjust"));
        assert!(ids.contains(&"builtin:blur"));
        assert!(ids.contains(&"builtin:lut_contrast"));
        assert!(ids.contains(&"builtin:lut_warm"));
        assert!(ids.contains(&"builtin:glitch"));
        assert!(ids.contains(&"builtin:chroma_key"));
    }
}
