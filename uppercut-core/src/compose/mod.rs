//! Offscreen wgpu compositor for Phase 0 export. Decoded frames are uploaded as textures,
//! scaled to the output resolution, and read back as RGBA for the FFmpeg encoder.

use crate::media::RgbaFrame;
use thiserror::Error;
use wgpu::util::DeviceExt;

#[derive(Debug, Error)]
pub enum ComposeError {
    #[error("no suitable GPU adapter found")]
    NoAdapter,
    #[error("wgpu error: {0}")]
    Wgpu(String),
}

/// Matches the `LayerTransform` uniform struct in composite.wgsl: two tightly-packed
/// `vec2<f32>` fields (scale, then offset), 16 bytes total, no padding.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct LayerTransformUniform {
    scale: [f32; 2],
    offset: [f32; 2],
}

/// "Cover" fit: scale/offset (in source-texture UV space) that fill the output rect
/// without distorting the layer's aspect ratio, cropping whichever axis overflows.
/// Identity when the layer already matches the output aspect ratio (e.g. caption layers,
/// which are already rendered at the output resolution).
fn cover_transform(layer_w: u32, layer_h: u32, out_w: u32, out_h: u32) -> LayerTransformUniform {
    let layer_aspect = layer_w as f32 / layer_h as f32;
    let out_aspect = out_w as f32 / out_h as f32;
    let (scale_x, scale_y) = if layer_aspect > out_aspect {
        // Layer is relatively wider than the output: crop its left/right edges.
        (out_aspect / layer_aspect, 1.0)
    } else {
        // Layer is relatively taller (or equal) than the output: crop top/bottom.
        (1.0, layer_aspect / out_aspect)
    };
    LayerTransformUniform {
        scale: [scale_x, scale_y],
        offset: [(1.0 - scale_x) / 2.0, (1.0 - scale_y) / 2.0],
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
                label: Some("uppercut-export"),
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
                    visibility: wgpu::ShaderStages::VERTEX,
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
        })
    }

    /// Composite layers in order (first = bottom). Empty → solid black frame.
    pub fn composite(&mut self, layers: &[RgbaFrame]) -> Result<Vec<u8>, ComposeError> {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("export-frame"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("composite"),
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

            pass.set_pipeline(&self.pipeline);

            for layer in layers {
                let texture = self.device.create_texture_with_data(
                    &self.queue,
                    &wgpu::TextureDescriptor {
                        label: Some("layer"),
                        size: wgpu::Extent3d {
                            width: layer.width,
                            height: layer.height,
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
                    &layer.pixels,
                );
                let view = texture.create_view(&Default::default());

                // A dedicated buffer per layer (created with its final contents up front,
                // rather than `queue.write_buffer`'d into a shared buffer) so each draw
                // call in this single render pass gets its own transform: `write_buffer`
                // calls made while recording — before the encoder is submitted — would all
                // land before any of this pass's draws execute on the GPU, leaving a
                // shared buffer holding only the last layer's value for every draw.
                let transform = cover_transform(layer.width, layer.height, self.width, self.height);
                let transform_buffer =
                    self.device
                        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("layer-transform"),
                            contents: bytemuck::bytes_of(&transform),
                            usage: wgpu::BufferUsages::UNIFORM,
                        });

                let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("layer-bind"),
                    layout: &self.bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: transform_buffer.as_entire_binding(),
                        },
                    ],
                });

                pass.set_bind_group(0, &bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_transform_is_identity_for_matching_aspect_ratio() {
        let t = cover_transform(1080, 1920, 1080, 1920);
        assert!((t.scale[0] - 1.0).abs() < 1e-6);
        assert!((t.scale[1] - 1.0).abs() < 1e-6);
        assert!((t.offset[0] - 0.0).abs() < 1e-6);
        assert!((t.offset[1] - 0.0).abs() < 1e-6);
    }

    #[test]
    fn cover_transform_crops_sides_for_wider_landscape_source_into_vertical_output() {
        // 16:9 gameplay footage into a 9:16 TikTok export: the source is relatively wider
        // than the output, so covering it means cropping the left/right edges (scale_x < 1)
        // while using the full height (scale_y == 1) — never stretching either axis.
        let t = cover_transform(1920, 1080, 1080, 1920);
        assert!(t.scale[0] < 1.0, "expected horizontal crop, got {t:?}");
        assert!(
            (t.scale[1] - 1.0).abs() < 1e-6,
            "expected full height, got {t:?}"
        );
        assert!(
            (t.offset[0] - (1.0 - t.scale[0]) / 2.0).abs() < 1e-6,
            "crop should be centered"
        );
        assert!((t.offset[1] - 0.0).abs() < 1e-6);
    }

    #[test]
    fn cover_transform_crops_top_bottom_for_taller_source_into_landscape_output() {
        let t = cover_transform(1080, 1920, 1920, 1080);
        assert!(
            (t.scale[0] - 1.0).abs() < 1e-6,
            "expected full width, got {t:?}"
        );
        assert!(t.scale[1] < 1.0, "expected vertical crop, got {t:?}");
        assert!((t.offset[0] - 0.0).abs() < 1e-6);
        assert!(
            (t.offset[1] - (1.0 - t.scale[1]) / 2.0).abs() < 1e-6,
            "crop should be centered"
        );
    }
}
