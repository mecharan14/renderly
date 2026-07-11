//! Clip mask / matte application (Phase 4). CPU path shared by preview and export.

use crate::media::RgbaFrame;
use crate::project::{ClipMask, ClipMaskKind, MaskShape};

/// Apply a clip mask to an RGBA frame in place (modulates alpha).
pub fn apply_clip_mask(frame: &mut RgbaFrame, mask: &ClipMask) {
    if !mask.enabled {
        return;
    }
    match &mask.kind {
        ClipMaskKind::None => {}
        ClipMaskKind::Rect { .. } | ClipMaskKind::Ellipse { .. } => {
            if let Some(shape) = mask.kind.as_shape() {
                apply_shape_mask(frame, &shape, mask);
            }
        }
        ClipMaskKind::Raster { path } => {
            if let Ok(matte) = image::open(path) {
                apply_raster_matte(frame, &matte.to_luma8(), mask.invert, mask.feather);
            }
        }
        ClipMaskKind::Generated { cache_dir } => {
            let matte_path = std::path::Path::new(cache_dir).join("matte.png");
            if let Ok(matte) = image::open(&matte_path) {
                apply_raster_matte(frame, &matte.to_luma8(), mask.invert, mask.feather);
            }
        }
        ClipMaskKind::Luma {
            pixels,
            width,
            height,
        } => {
            let matte = image::GrayImage::from_raw(*width, *height, pixels.clone()).unwrap();
            apply_raster_matte(frame, &matte, mask.invert, mask.feather);
        }
    }
}

fn apply_shape_mask(frame: &mut RgbaFrame, shape: &MaskShape, mask: &ClipMask) {
    let w = frame.width as f32;
    let h = frame.height as f32;
    let feather = mask.feather.clamp(0.0, 1.0) as f32;
    for y in 0..frame.height {
        for x in 0..frame.width {
            let u = (x as f32 + 0.5) / w;
            let v = (y as f32 + 0.5) / h;
            let mut a = shape_alpha(shape, u, v, feather);
            if mask.invert {
                a = 1.0 - a;
            }
            let idx = ((y * frame.width + x) * 4) as usize;
            let src_a = frame.pixels[idx + 3] as f32 / 255.0;
            frame.pixels[idx + 3] = ((src_a * a) * 255.0).clamp(0.0, 255.0) as u8;
        }
    }
}

fn shape_alpha(shape: &MaskShape, u: f32, v: f32, feather: f32) -> f32 {
    match shape {
        MaskShape::Rect {
            x,
            y,
            width,
            height,
        } => {
            let x0 = *x as f32;
            let y0 = *y as f32;
            let x1 = x0 + (*width as f32).max(0.0);
            let y1 = y0 + (*height as f32).max(0.0);
            let dx = if u < x0 {
                x0 - u
            } else if u > x1 {
                u - x1
            } else {
                0.0
            };
            let dy = if v < y0 {
                y0 - v
            } else if v > y1 {
                v - y1
            } else {
                0.0
            };
            let dist = (dx * dx + dy * dy).sqrt();
            soft_edge(dist, feather * 0.15)
        }
        MaskShape::Ellipse { cx, cy, rx, ry } => {
            let rx = (*rx as f32).max(1e-4);
            let ry = (*ry as f32).max(1e-4);
            let nx = (u - *cx as f32) / rx;
            let ny = (v - *cy as f32) / ry;
            let d = (nx * nx + ny * ny).sqrt();
            soft_edge((d - 1.0).max(0.0), feather.max(0.001))
        }
    }
}

fn soft_edge(dist_outside: f32, feather: f32) -> f32 {
    if dist_outside <= 0.0 {
        1.0
    } else if feather <= 1e-6 {
        0.0
    } else {
        (1.0 - dist_outside / feather).clamp(0.0, 1.0)
    }
}

pub fn apply_luma_matte(
    frame: &mut RgbaFrame,
    matte: &image::GrayImage,
    invert: bool,
    feather: f64,
) {
    apply_raster_matte(frame, matte, invert, feather);
}

fn apply_raster_matte(frame: &mut RgbaFrame, matte: &image::GrayImage, invert: bool, feather: f64) {
    let mw = matte.width();
    let mh = matte.height();
    let feather = feather.clamp(0.0, 1.0) as f32;
    for y in 0..frame.height {
        for x in 0..frame.width {
            let mx = (x as u64 * mw as u64 / frame.width.max(1) as u64) as u32;
            let my = (y as u64 * mh as u64 / frame.height.max(1) as u64) as u32;
            let mut a = matte.get_pixel(mx.min(mw - 1), my.min(mh - 1))[0] as f32 / 255.0;
            if invert {
                a = 1.0 - a;
            }
            // Light feather as contrast curve around mid-gray.
            if feather > 0.0 {
                let t = feather * 0.5;
                a = smoothstep(0.5 - t, 0.5 + t, a);
            }
            let idx = ((y * frame.width + x) * 4) as usize;
            let src_a = frame.pixels[idx + 3] as f32 / 255.0;
            frame.pixels[idx + 3] = ((src_a * a) * 255.0).clamp(0.0, 255.0) as u8;
        }
    }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0).max(1e-6)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Heuristic / CLI segmentation matte used when no ONNX runtime is linked.
pub fn generate_heuristic_matte(frame: &RgbaFrame, threshold: f64) -> image::GrayImage {
    let threshold = (threshold.clamp(0.0, 1.0) * 255.0) as u8;
    let mut out = image::GrayImage::new(frame.width, frame.height);
    for y in 0..frame.height {
        for x in 0..frame.width {
            let i = ((y * frame.width + x) * 4) as usize;
            let r = frame.pixels[i] as i32;
            let g = frame.pixels[i + 1] as i32;
            let b = frame.pixels[i + 2] as i32;
            // Subject = not green-screen-ish and above luma threshold.
            let luma = ((r * 30 + g * 59 + b * 11) / 100) as u8;
            let green_spill = g > r + 20 && g > b + 20;
            let alpha = if green_spill || luma < threshold {
                0
            } else {
                255
            };
            out.put_pixel(x, y, image::Luma([alpha]));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{ClipMask, ClipMaskKind};

    #[test]
    fn rect_mask_zeros_outside_alpha() {
        let mut frame = RgbaFrame {
            width: 4,
            height: 4,
            pixels: vec![255u8; 4 * 4 * 4],
        };
        let mask = ClipMask {
            enabled: true,
            invert: false,
            feather: 0.0,
            kind: ClipMaskKind::Rect {
                x: 0.25,
                y: 0.25,
                width: 0.5,
                height: 0.5,
            },
        };
        apply_clip_mask(&mut frame, &mask);
        // Corner pixel should be transparent.
        assert_eq!(frame.pixels[3], 0);
        // Center-ish pixel (2,2) should stay opaque.
        let idx = ((2 * 4 + 2) * 4 + 3) as usize;
        assert_eq!(frame.pixels[idx], 255);
    }
}
