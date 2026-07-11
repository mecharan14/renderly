//! Linux native preview: runtime dispatch between X11 and Wayland.
//!
//! Selected from the raw window/display handle Tauri exposes for the main webview.
//! X11 uses a child X window; Wayland uses a `wl_subsurface` of the GTK parent surface.

mod wayland;
mod x11;

use super::{PreviewBounds, PreviewError};
use std::os::raw::c_void;

#[derive(Clone, Copy, Debug)]
pub enum NativeWindow {
    X11 {
        display: *mut ::x11::xlib::Display,
        window: u32,
    },
    Wayland {
        display: *mut c_void,
        surface: *mut c_void,
    },
}

pub enum PlatformPreview {
    Unattached,
    X11(x11::Preview),
    Wayland(wayland::Preview),
}

impl PlatformPreview {
    pub fn new() -> Self {
        Self::Unattached
    }

    pub fn attach_parent(&mut self, parent: NativeWindow) {
        *self = match parent {
            NativeWindow::X11 { display, window } => {
                Self::X11(x11::Preview::new(x11::Parent { display, window }))
            }
            NativeWindow::Wayland { display, surface } => {
                Self::Wayland(wayland::Preview::new(wayland::Parent { display, surface }))
            }
        };
    }

    pub fn set_bounds(&mut self, bounds: PreviewBounds) -> Result<(), PreviewError> {
        match self {
            Self::Unattached => Err(PreviewError::NotInitialized),
            Self::X11(p) => p.set_bounds(bounds),
            Self::Wayland(p) => p.set_bounds(bounds),
        }
    }

    pub fn present_rgba(
        &mut self,
        pixels: &[u8],
        width: u32,
        height: u32,
    ) -> Result<(), PreviewError> {
        match self {
            Self::Unattached => Err(PreviewError::NotInitialized),
            Self::X11(p) => p.present_rgba(pixels, width, height),
            Self::Wayland(p) => p.present_rgba(pixels, width, height),
        }
    }
}
