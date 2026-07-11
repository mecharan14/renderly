//! Shared wgpu blit pipeline for the native preview surface.

use super::PreviewError;
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use std::sync::OnceLock;

static INSTANCE: OnceLock<wgpu::Instance> = OnceLock::new();

pub fn wgpu_instance() -> &'static wgpu::Instance {
    INSTANCE.get_or_init(|| {
        wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        })
    })
}

/// Create a wgpu surface from raw platform handles.
///
/// Win32 supplies an explicit `WindowsDisplayHandle` because child HWNDs do not implement
/// `HasDisplayHandle`; macOS/Linux callers pass their native display connection handles.
pub fn create_surface_from_raw(
    instance: &wgpu::Instance,
    raw_display_handle: RawDisplayHandle,
    raw_window_handle: RawWindowHandle,
) -> Result<wgpu::Surface<'static>, PreviewError> {
    let target = wgpu::SurfaceTargetUnsafe::RawHandle {
        raw_display_handle: Some(raw_display_handle),
        raw_window_handle,
    };
    unsafe {
        instance
            .create_surface_unsafe(target)
            .map_err(|e| PreviewError::Wgpu(e.to_string()))
    }
}

pub struct GfxState {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl GfxState {
    pub fn new(
        raw_display_handle: RawDisplayHandle,
        raw_window_handle: RawWindowHandle,
        width: u32,
        height: u32,
    ) -> Result<Self, PreviewError> {
        let instance = wgpu_instance();
        let surface = create_surface_from_raw(instance, raw_display_handle, raw_window_handle)?;
        pollster::block_on(Self::init_from_surface(surface, width, height))
    }

    /// Build from an already-created wgpu surface (e.g. platform-specific surface setup).
    #[allow(dead_code)]
    pub fn from_surface(
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
    ) -> Result<Self, PreviewError> {
        pollster::block_on(Self::init_from_surface(surface, width, height))
    }

    async fn init_from_surface(
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
    ) -> Result<Self, PreviewError> {
        let instance = wgpu_instance();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .await
            .map_err(|_| PreviewError::Wgpu("no GPU adapter".into()))?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("renderly-preview"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|e| PreviewError::Wgpu(e.to_string()))?;

        let caps = surface.get_capabilities(&adapter);
        // FFmpeg delivers display-referred 8-bit RGBA (already gamma-encoded). Blitting
        // those bytes into an *sRGB* swapchain makes the GPU treat them as linear and
        // re-encode → washed-out / oversaturated preview. Prefer a non-sRGB surface so
        // the present path is a passthrough matching the export compositor (Rgba8Unorm).
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| {
                matches!(
                    f,
                    wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm
                )
            })
            .or_else(|| caps.formats.iter().copied().find(|f| !f.is_srgb()))
            .unwrap_or(caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            // Explicit SDR — Auto can still pick a wide-gamut path on some drivers and
            // fight the non-sRGB Unorm surface choice above.
            color_space: wgpu::SurfaceColorSpace::Srgb,
        };
        surface.configure(&device, &config);

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("preview-sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("preview-layer"),
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
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("preview-blits"),
            source: wgpu::ShaderSource::Wgsl(include_str!("preview_blit.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("preview"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("preview"),
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
                    format,
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
        });

        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            bind_group_layout,
            sampler,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), PreviewError> {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
        }
        Ok(())
    }

    pub fn present_rgba(
        &mut self,
        pixels: &[u8],
        width: u32,
        height: u32,
    ) -> Result<(), PreviewError> {
        let expected = (width * height * 4) as usize;
        if pixels.len() < expected {
            return Err(PreviewError::Wgpu("RGBA buffer too small".into()));
        }

        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("preview-frame"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        // `write_texture` requires bytes_per_row to be a multiple of 256 when height > 1.
        // Project sizes like 1080×1920 yield 4320 B/row (not aligned) — uploading tight
        // rows caused progressive horizontal smear / tearing in the native preview.
        let unpadded_bpr = width * 4;
        let padded_bpr = wgpu::util::align_to(unpadded_bpr, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let padded_storage: Option<Vec<u8>> = if padded_bpr == unpadded_bpr {
            None
        } else {
            let mut buf = vec![0u8; (padded_bpr * height) as usize];
            for row in 0..height as usize {
                let src = row * unpadded_bpr as usize;
                let dst = row * padded_bpr as usize;
                buf[dst..dst + unpadded_bpr as usize]
                    .copy_from_slice(&pixels[src..src + unpadded_bpr as usize]);
            }
            Some(buf)
        };
        let upload_slice: &[u8] = match &padded_storage {
            Some(buf) => buf.as_slice(),
            None => &pixels[..expected],
        };

        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            upload_slice,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bpr),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        let view = texture.create_view(&Default::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("preview-bg"),
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
            ],
        });

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture)
            | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => texture,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
            other => {
                return Err(PreviewError::Wgpu(format!(
                    "surface unavailable: {other:?}"
                )));
            }
        };
        let target = frame.texture.create_view(&Default::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("preview"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("preview"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
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
            pass.draw(0..3, 0..1);
        }

        self.queue.submit(Some(encoder.finish()));
        self.queue.present(frame);
        Ok(())
    }
}
