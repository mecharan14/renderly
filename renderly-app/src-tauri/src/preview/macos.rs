use super::gfx::GfxState;
use super::{PreviewBounds, PreviewError};
use objc2::define_class;
use objc2::rc::Retained;
use objc2::{msg_send, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSView};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use raw_window_handle::{
    AppKitDisplayHandle, AppKitWindowHandle, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle,
};
use std::ptr::NonNull;

#[derive(Clone, Copy, Debug)]
pub struct NativeWindow {
    pub ns_view: usize,
}

pub struct PlatformPreview {
    parent: Option<usize>,
    child: Option<usize>,
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
        self.parent = Some(parent.ns_view);
    }

    pub fn set_bounds(&mut self, bounds: PreviewBounds) -> Result<(), PreviewError> {
        let _mtm = MainThreadMarker::new().ok_or_else(|| {
            PreviewError::Wgpu("preview must run on the AppKit main thread".into())
        })?;
        let parent = self.parent.ok_or(PreviewError::NotInitialized)?;
        if bounds.width == 0 || bounds.height == 0 {
            eprintln!(
                "preview: set_bounds got a zero dimension ({}x{} at {},{}), skipping — \
                 present_rgba will report NotInitialized until a non-zero call arrives",
                bounds.width, bounds.height, bounds.x, bounds.y
            );
            return Ok(());
        }

        let child = ensure_child_view(
            self.child,
            parent,
            bounds.x,
            bounds.y,
            bounds.width,
            bounds.height,
        )?;
        self.child = Some(child);

        if self.gfx.is_none() {
            match preview_gfx_state(child, bounds.width, bounds.height) {
                Ok(gfx) => self.gfx = Some(gfx),
                Err(e) => {
                    eprintln!("preview: GfxState::new failed: {e}");
                    return Err(e);
                }
            }
        } else if let Some(gfx) = &mut self.gfx {
            if let Err(e) = gfx.resize(bounds.width, bounds.height) {
                eprintln!("preview: resize failed ({e}), recreating GfxState");
                self.gfx = Some(preview_gfx_state(child, bounds.width, bounds.height)?);
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

define_class!(
    #[unsafe(super(NSView))]
    #[name = "RenderlyPreviewView"]
    #[thread_kind = MainThreadOnly]
    struct PreviewView;

    impl PreviewView {
        /// Pass all mouse hits through to views below (webview chrome).
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }
    }
);

fn preview_gfx_state(ns_view: usize, width: u32, height: u32) -> Result<GfxState, PreviewError> {
    let window_handle = PreviewWindowHandle(ns_view)
        .window_handle()
        .map_err(|e| PreviewError::Wgpu(e.to_string()))?
        .as_raw();
    let display_handle = PreviewWindowHandle(ns_view)
        .display_handle()
        .map_err(|e| PreviewError::Wgpu(e.to_string()))?
        .as_raw();
    GfxState::new(display_handle, window_handle, width, height)
}

struct PreviewWindowHandle(usize);

impl HasWindowHandle for PreviewWindowHandle {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        let ns_view = NonNull::new(self.0 as *mut core::ffi::c_void)
            .ok_or(raw_window_handle::HandleError::Unavailable)?;
        let handle = AppKitWindowHandle::new(ns_view);
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(RawWindowHandle::AppKit(handle)) })
    }
}

impl HasDisplayHandle for PreviewWindowHandle {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        let handle = AppKitDisplayHandle::new();
        Ok(unsafe {
            raw_window_handle::DisplayHandle::borrow_raw(RawDisplayHandle::AppKit(handle))
        })
    }
}

fn retain_ns_view(ptr: usize) -> Result<Retained<NSView>, PreviewError> {
    unsafe {
        Retained::retain(ptr as *mut NSView)
            .ok_or_else(|| PreviewError::Wgpu("invalid NSView pointer".into()))
    }
}

fn ensure_child_view(
    existing: Option<usize>,
    parent: usize,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<usize, PreviewError> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| PreviewError::Wgpu("preview must run on the AppKit main thread".into()))?;
    let parent_view = retain_ns_view(parent)?;
    let parent_height = parent_view.frame().size.height;
    // Webview bounds are top-left origin; AppKit superview coordinates are bottom-left.
    let flipped_y = parent_height - y as f64 - height as f64;
    let frame = NSRect::new(
        NSPoint::new(x as f64, flipped_y),
        NSSize::new(width as f64, height as f64),
    );

    if let Some(child_ptr) = existing {
        let child = retain_ns_view(child_ptr)?;
        child.setFrame(frame);
        child.setNeedsDisplay(true);
        return Ok(child_ptr);
    }

    let allocated = PreviewView::alloc(mtm);
    // DefinedClass subclasses must set ivars before calling a superclass init via `super`.
    let child: Retained<PreviewView> =
        unsafe { msg_send![super(allocated.set_ivars(())), initWithFrame: frame] };
    child.setWantsLayer(true);
    child.setAutoresizingMask(NSAutoresizingMaskOptions::ViewNotSizable);
    parent_view.addSubview(&child);
    Ok(Retained::as_ptr(&child) as usize)
}
