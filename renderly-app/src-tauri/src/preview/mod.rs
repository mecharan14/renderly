//! Native wgpu preview panel (Phase 2).

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
mod gfx;

#[cfg(windows)]
mod win32;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
mod stub;

use thiserror::Error;

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct PreviewBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Error)]
pub enum PreviewError {
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    #[error("preview not supported on this platform yet")]
    Unsupported,
    #[error("preview not initialized")]
    NotInitialized,
    #[error("{0}")]
    Wgpu(String),
}

#[cfg(windows)]
pub type NativeWindow = win32::NativeWindow;
#[cfg(windows)]
type PlatformPreview = win32::PlatformPreview;

#[cfg(target_os = "macos")]
pub type NativeWindow = macos::NativeWindow;
#[cfg(target_os = "macos")]
type PlatformPreview = macos::PlatformPreview;

#[cfg(target_os = "linux")]
pub type NativeWindow = linux::NativeWindow;
#[cfg(target_os = "linux")]
type PlatformPreview = linux::PlatformPreview;

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub type NativeWindow = stub::NativeWindow;
#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
type PlatformPreview = stub::PlatformPreview;

pub struct PreviewPanel {
    inner: PlatformPreview,
}

impl PreviewPanel {
    pub fn new() -> Self {
        Self {
            inner: PlatformPreview::new(),
        }
    }

    pub fn attach_parent(&mut self, parent: NativeWindow) {
        self.inner.attach_parent(parent);
    }

    pub fn set_bounds(&mut self, bounds: PreviewBounds) -> Result<(), PreviewError> {
        self.inner.set_bounds(bounds)
    }

    pub fn present_rgba(
        &mut self,
        pixels: &[u8],
        width: u32,
        height: u32,
    ) -> Result<(), PreviewError> {
        self.inner.present_rgba(pixels, width, height)
    }
}
