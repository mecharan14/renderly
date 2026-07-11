//! Dual-texture WGSL transition pass (Phase 3).

use crate::project::TransitionKind;
use wgpu::util::DeviceExt;

use super::ComposeError;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct TransitionUniform {
    progress: f32,
    kind: u32,
    _pad0: f32,
    _pad1: f32,
}

struct Offscreen {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    width: u32,
    height: u32,
}

pub struct TransitionPass {
    sampler: wgpu::Sampler,
    bind_group_layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    rt_a: Option<Offscreen>,
    rt_b: Option<Offscreen>,
}

impl TransitionPass {
    pub fn new(device: &wgpu::Device) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("transition-linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("transition"),
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
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
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

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("transition"),
            source: wgpu::ShaderSource::Wgsl(include_str!("transition.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("transition"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("transition"),
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

        Self {
            sampler,
            bind_group_layout,
            pipeline,
            rt_a: None,
            rt_b: None,
        }
    }

    pub fn ensure_rts(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        let needs = |rt: &Option<Offscreen>| {
            rt.as_ref()
                .map(|r| r.width != width || r.height != height)
                .unwrap_or(true)
        };
        if needs(&self.rt_a) {
            self.rt_a = Some(create_offscreen(device, "trans-a", width, height));
        }
        if needs(&self.rt_b) {
            self.rt_b = Some(create_offscreen(device, "trans-b", width, height));
        }
    }

    pub fn view_a(&self) -> &wgpu::TextureView {
        &self.rt_a.as_ref().expect("trans-a").view
    }

    pub fn view_b(&self) -> &wgpu::TextureView {
        &self.rt_b.as_ref().expect("trans-b").view
    }

    pub fn blend(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        output_view: &wgpu::TextureView,
        kind: TransitionKind,
        progress: f32,
    ) -> Result<(), ComposeError> {
        let u = TransitionUniform {
            progress: progress.clamp(0.0, 1.0),
            kind: kind.shader_id(),
            _pad0: 0.0,
            _pad1: 0.0,
        };
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("transition-params"),
            contents: bytemuck::bytes_of(&u),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("transition-bind"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(self.view_a()),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(self.view_b()),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: params.as_entire_binding(),
                },
            ],
        });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("transition-blend"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: output_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
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
        Ok(())
    }
}

fn create_offscreen(device: &wgpu::Device, label: &str, width: u32, height: u32) -> Offscreen {
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
    Offscreen {
        _texture: texture,
        view,
        width,
        height,
    }
}
