//! Decode-free pixel buffer type shared by `mask`, `compose`, and (natively) `media`.
//!
//! This lives outside `media` so it stays available on `wasm32` builds, which exclude
//! `media`'s FFmpeg-backed decode machinery (see `lib.rs`'s target gating and
//! docs/preview-webview.md P2). Native code continues to get `RgbaFrame` via
//! `media::RgbaFrame`, which is a re-export of this type.

/// A decoded RGBA8 frame: `width * height * 4` bytes, row-major, no padding.
#[derive(Debug, Clone)]
pub struct RgbaFrame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}
