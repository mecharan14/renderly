use super::{PreviewBounds, PreviewError};
use std::sync::{Once, OnceLock};

static PREVIEW_CLASS: OnceLock<Vec<u16>> = OnceLock::new();
static REGISTER_CLASS: Once = Once::new();

fn preview_class_name() -> &'static [u16] {
    PREVIEW_CLASS.get_or_init(|| wide("UppercutPreviewPanel"))
}

fn register_preview_class() {
    REGISTER_CLASS.call_once(|| {
        use windows::core::PCWSTR;
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows::Win32::UI::WindowsAndMessaging::{
            RegisterClassW, CS_HREDRAW, CS_VREDRAW, WNDCLASSW,
        };

        let class_name = preview_class_name();
        unsafe {
            let hinstance = GetModuleHandleW(None).expect("module handle");
            let class = WNDCLASSW {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(def_preview_wnd_proc),
                hInstance: hinstance.into(),
                lpszClassName: PCWSTR(class_name.as_ptr()),
                ..Default::default()
            };
            RegisterClassW(&class);
        }
    });
}

#[derive(Clone, Copy)]
pub struct NativeWindow {
    pub hwnd: isize,
}

pub struct PlatformPreview {
    parent: Option<isize>,
    child: Option<isize>,
    gfx: Option<GfxState>,
}

impl PlatformPreview {
    pub fn new() -> Self {
        Self {
            parent: None,
            child: None,
            gfx: None,
        }
    }

    pub fn attach_parent(&mut self, parent: NativeWindow) {
        self.parent = Some(parent.hwnd);
    }

    pub fn set_bounds(&mut self, bounds: PreviewBounds) -> Result<(), PreviewError> {
        let parent = self.parent.ok_or(PreviewError::NotInitialized)?;
        if bounds.width == 0 || bounds.height == 0 {
            eprintln!(
                "preview: set_bounds got a zero dimension ({}x{} at {},{}), skipping — \
                 present_rgba will report NotInitialized until a non-zero call arrives",
                bounds.width, bounds.height, bounds.x, bounds.y
            );
            return Ok(());
        }

        // Bounds come from the webview's getBoundingClientRect — already in the
        // parent client coordinate space. Do not run ScreenToClient on them.
        let x = bounds.x;
        let y = bounds.y;
        let child = ensure_child_window(self.child, parent, x, y, bounds.width, bounds.height)?;
        self.child = Some(child);

        if self.gfx.is_none() {
            match GfxState::new(child, bounds.width, bounds.height) {
                Ok(gfx) => self.gfx = Some(gfx),
                Err(e) => {
                    eprintln!("preview: GfxState::new failed: {e}");
                    return Err(e);
                }
            }
        } else if let Some(gfx) = &mut self.gfx {
            if let Err(e) = gfx.resize(bounds.width, bounds.height) {
                eprintln!("preview: resize failed ({e}), recreating GfxState");
                self.gfx = Some(GfxState::new(child, bounds.width, bounds.height)?);
            }
        }
        Ok(())
    }

    pub fn present_rgba(
        &mut self,
        pixels: &[u8],
        width: u32,
        height: u32,
    ) -> Result<(), PreviewError> {
        let gfx = self.gfx.as_mut().ok_or(PreviewError::NotInitialized)?;
        gfx.present_rgba(pixels, width, height)
    }
}

struct GfxState {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl GfxState {
    fn new(hwnd: isize, width: u32, height: u32) -> Result<Self, PreviewError> {
        pollster::block_on(Self::new_async(hwnd, width, height))
    }

    async fn new_async(hwnd: isize, width: u32, height: u32) -> Result<Self, PreviewError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });

        let surface = unsafe {
            use raw_window_handle::{HasWindowHandle, RawDisplayHandle, WindowsDisplayHandle};

            // NOT `SurfaceTargetUnsafe::from_window` — it unconditionally sets
            // `raw_display_handle: None` (see wgpu's `api/surface.rs`), regardless of
            // whether the target implements `HasDisplayHandle`. With no display handle on
            // either the target or the `Instance` (we build the instance with
            // `new_without_display_handle()`), `create_surface_unsafe` always fails with
            // `MissingDisplayHandle`. Win32 has no real "display connection" concept, so
            // we supply the no-op `WindowsDisplayHandle` marker explicitly instead.
            let window_handle = PreviewWindowHandle(hwnd)
                .window_handle()
                .map_err(|e| PreviewError::Wgpu(e.to_string()))?
                .as_raw();
            let target = wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: Some(RawDisplayHandle::Windows(WindowsDisplayHandle::new())),
                raw_window_handle: window_handle,
            };
            instance
                .create_surface_unsafe(target)
                .map_err(|e| PreviewError::Wgpu(e.to_string()))?
        };

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
                label: Some("uppercut-preview"),
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

    fn resize(&mut self, width: u32, height: u32) -> Result<(), PreviewError> {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
        }
        Ok(())
    }

    fn present_rgba(&mut self, pixels: &[u8], width: u32, height: u32) -> Result<(), PreviewError> {
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
                return Ok(())
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

struct PreviewWindowHandle(isize);

impl raw_window_handle::HasWindowHandle for PreviewWindowHandle {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        use raw_window_handle::{RawWindowHandle, Win32WindowHandle, WindowHandle};
        use std::num::NonZeroIsize;

        let hwnd = NonZeroIsize::new(self.0).ok_or(raw_window_handle::HandleError::Unavailable)?;
        let handle = Win32WindowHandle::new(hwnd);
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(handle)) })
    }
}

fn ensure_child_window(
    existing: Option<isize>,
    parent: isize,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<isize, PreviewError> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, ShowWindow,
        GWL_EXSTYLE, HWND_TOP, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_SHOW,
        WINDOW_EX_STYLE, WINDOW_STYLE, WS_CHILD, WS_EX_NOACTIVATE, WS_EX_TRANSPARENT, WS_VISIBLE,
    };

    register_preview_class();
    let class_name = preview_class_name();

    unsafe {
        if let Some(hwnd) = existing {
            let child = HWND(hwnd as *mut _);
            let ex = GetWindowLongPtrW(child, GWL_EXSTYLE) as u32;
            let want = ex | WS_EX_TRANSPARENT.0 | WS_EX_NOACTIVATE.0;
            if ex != want {
                SetWindowLongPtrW(child, GWL_EXSTYLE, want as isize);
            }
            let _ = SetWindowPos(
                child,
                Some(HWND_TOP),
                x,
                y,
                width as i32,
                height as i32,
                SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_FRAMECHANGED,
            );
            return Ok(hwnd);
        }

        let hinstance = GetModuleHandleW(None).map_err(|e| PreviewError::Wgpu(e.to_string()))?;
        // WS_EX_TRANSPARENT: preview is display-only; clicks pass through to the webview.
        let child = CreateWindowExW(
            WINDOW_EX_STYLE(WS_EX_NOACTIVATE.0 | WS_EX_TRANSPARENT.0),
            PCWSTR(class_name.as_ptr()),
            PCWSTR(wide("Preview").as_ptr()),
            WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0),
            x,
            y,
            width as i32,
            height as i32,
            Some(HWND(parent as *mut _)),
            None,
            Some(hinstance.into()),
            None,
        )
        .map_err(|e| PreviewError::Wgpu(e.to_string()))?;

        let _ = ShowWindow(child, SW_SHOW);
        Ok(child.0 as isize)
    }
}

unsafe extern "system" fn def_preview_wnd_proc(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{DefWindowProcW, WM_DESTROY};

    if msg == WM_DESTROY {
        return windows::Win32::Foundation::LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn wide(s: &str) -> Vec<u16> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
}
