use super::super::gfx::GfxState;
use super::super::{PreviewBounds, PreviewError};
use raw_window_handle::{
    HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle, XlibDisplayHandle,
    XlibWindowHandle,
};
use std::ptr::NonNull;
use x11::xfixes::XFixesSetWindowShapeRegion;
use x11::xlib::{
    Display, ShapeInput, ShapeSet, Window, XCreateSimpleWindow, XDestroyWindow, XFlush, XMapWindow,
    XMoveResizeWindow,
};

#[derive(Clone, Copy, Debug)]
pub struct Parent {
    pub display: *mut Display,
    pub window: u32,
}

pub struct Preview {
    parent: Parent,
    child: Option<u32>,
    gfx: Option<GfxState>,
}

impl Preview {
    pub fn new(parent: Parent) -> Self {
        Self {
            parent,
            child: None,
            gfx: None,
        }
    }

    pub fn set_bounds(&mut self, bounds: PreviewBounds) -> Result<(), PreviewError> {
        if bounds.width == 0 || bounds.height == 0 {
            eprintln!(
                "preview: set_bounds got a zero dimension ({}x{} at {},{}), skipping — \
                 present_rgba will report NotInitialized until a non-zero call arrives",
                bounds.width, bounds.height, bounds.x, bounds.y
            );
            return Ok(());
        }

        let child = ensure_child_window(
            self.child,
            self.parent,
            bounds.x,
            bounds.y,
            bounds.width,
            bounds.height,
        )?;
        self.child = Some(child);

        if self.gfx.is_none() {
            match preview_gfx_state(self.parent.display, child, bounds.width, bounds.height) {
                Ok(gfx) => self.gfx = Some(gfx),
                Err(e) => {
                    eprintln!("preview: GfxState::new failed: {e}");
                    return Err(e);
                }
            }
        } else if let Some(gfx) = &mut self.gfx {
            if let Err(e) = gfx.resize(bounds.width, bounds.height) {
                eprintln!("preview: resize failed ({e}), recreating GfxState");
                self.gfx = Some(preview_gfx_state(
                    self.parent.display,
                    child,
                    bounds.width,
                    bounds.height,
                )?);
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

fn preview_gfx_state(
    display: *mut Display,
    window: u32,
    width: u32,
    height: u32,
) -> Result<GfxState, PreviewError> {
    let window_handle = X11WindowHandle { display, window }
        .window_handle()
        .map_err(|e| PreviewError::Wgpu(e.to_string()))?
        .as_raw();
    let display_handle = X11WindowHandle { display, window }
        .display_handle()
        .map_err(|e| PreviewError::Wgpu(e.to_string()))?
        .as_raw();
    GfxState::new(display_handle, window_handle, width, height)
}

struct X11WindowHandle {
    display: *mut Display,
    window: u32,
}

impl HasWindowHandle for X11WindowHandle {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        use std::num::NonZeroU32;

        let window =
            NonZeroU32::new(self.window).ok_or(raw_window_handle::HandleError::Unavailable)?;
        let handle = XlibWindowHandle::new(window);
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(RawWindowHandle::Xlib(handle)) })
    }
}

impl HasDisplayHandle for X11WindowHandle {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        let display =
            NonNull::new(self.display.cast()).ok_or(raw_window_handle::HandleError::Unavailable)?;
        let handle = XlibDisplayHandle::new(display);
        Ok(unsafe { raw_window_handle::DisplayHandle::borrow_raw(RawDisplayHandle::Xlib(handle)) })
    }
}

fn ensure_child_window(
    existing: Option<u32>,
    parent: Parent,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<u32, PreviewError> {
    unsafe {
        if let Some(child) = existing {
            XMoveResizeWindow(parent.display, child as Window, x, y, width, height);
            set_click_through(parent.display, child);
            XFlush(parent.display);
            return Ok(child);
        }

        let child = XCreateSimpleWindow(
            parent.display,
            parent.window as Window,
            x,
            y,
            width,
            height,
            0,
            0,
            0,
        );
        if child == 0 {
            return Err(PreviewError::Wgpu("XCreateSimpleWindow failed".into()));
        }

        set_click_through(parent.display, child as u32);
        XMapWindow(parent.display, child);
        XFlush(parent.display);
        Ok(child as u32)
    }
}

/// Empty ShapeInput region — mouse events pass through to the webview below.
fn set_click_through(display: *mut Display, window: u32) {
    unsafe {
        XFixesSetWindowShapeRegion(
            display,
            window as Window,
            ShapeInput as i32,
            0,
            0,
            std::ptr::null_mut(),
            0,
            ShapeSet as i32,
        );
    }
}

impl Drop for Preview {
    fn drop(&mut self) {
        if let Some(child) = self.child {
            unsafe {
                XDestroyWindow(self.parent.display, child as Window);
                XFlush(self.parent.display);
            }
        }
    }
}
