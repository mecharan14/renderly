//! Built-in caption style presets and CPU text rasterization for export (Phase 1).
//! GPU text via cosmic-text/glyphon lands in Phase 2 preview; export burns captions here.

use crate::media::RgbaFrame;
use ab_glyph::{point, Font, FontArc, Glyph, PxScale, ScaleFont};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CaptionError {
    #[error("no usable font found; set UPPERCUT_FONT_PATH to a .ttf file")]
    NoFont,
    #[error("font load failed: {0}")]
    FontLoad(String),
}

/// Built-in TikTok-style caption presets (Phase 1).
pub const STYLE_TIKTOK_BOLD_YELLOW: &str = "tiktok-bold-yellow";
pub const STYLE_TIKTOK_MINIMAL: &str = "tiktok-minimal";
pub const STYLE_TIKTOK_BOX: &str = "tiktok-box";
pub const STYLE_YOUTUBE_LOWER: &str = "youtube-lower-thirds";

pub const BUILTIN_STYLES: &[&str] = &[
    STYLE_TIKTOK_BOLD_YELLOW,
    STYLE_TIKTOK_MINIMAL,
    STYLE_TIKTOK_BOX,
    STYLE_YOUTUBE_LOWER,
];

struct StyleSpec {
    font_scale: f32,
    text_rgba: [u8; 4],
    outline_rgba: Option<[u8; 4]>,
    shadow_offset: Option<(i32, i32)>,
    box_rgba: Option<[u8; 4]>,
    vertical_anchor: f32, // 0=top, 1=bottom
}

fn style_spec(style_id: &str) -> StyleSpec {
    match style_id {
        STYLE_TIKTOK_MINIMAL => StyleSpec {
            font_scale: 48.0,
            text_rgba: [255, 255, 255, 255],
            outline_rgba: None,
            shadow_offset: Some((2, 2)),
            box_rgba: None,
            vertical_anchor: 0.82,
        },
        STYLE_TIKTOK_BOX => StyleSpec {
            font_scale: 44.0,
            text_rgba: [255, 255, 255, 255],
            outline_rgba: None,
            shadow_offset: None,
            box_rgba: Some([0, 0, 0, 160]),
            vertical_anchor: 0.80,
        },
        STYLE_YOUTUBE_LOWER => StyleSpec {
            font_scale: 36.0,
            text_rgba: [255, 255, 255, 255],
            outline_rgba: None,
            shadow_offset: None,
            box_rgba: Some([20, 20, 20, 200]),
            vertical_anchor: 0.88,
        },
        _ => StyleSpec {
            // tiktok-bold-yellow (default)
            font_scale: 52.0,
            text_rgba: [255, 255, 255, 255],
            outline_rgba: Some([255, 220, 0, 255]),
            shadow_offset: None,
            box_rgba: None,
            vertical_anchor: 0.78,
        },
    }
}

fn load_font() -> Result<FontArc, CaptionError> {
    if let Ok(path) = std::env::var("UPPERCUT_FONT_PATH") {
        let data = std::fs::read(&path).map_err(|e| CaptionError::FontLoad(e.to_string()))?;
        return FontArc::try_from_vec(data).map_err(|e| CaptionError::FontLoad(e.to_string()));
    }

    for candidate in font_candidates() {
        if candidate.is_file() {
            if let Ok(data) = std::fs::read(&candidate) {
                if let Ok(font) = FontArc::try_from_vec(data) {
                    return Ok(font);
                }
            }
        }
    }

    Err(CaptionError::NoFont)
}

fn font_candidates() -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();
    #[cfg(windows)]
    {
        paths.push(Path::new(r"C:\Windows\Fonts\arialbd.ttf").to_path_buf());
        paths.push(Path::new(r"C:\Windows\Fonts\arial.ttf").to_path_buf());
    }
    #[cfg(target_os = "macos")]
    {
        paths.push(Path::new("/System/Library/Fonts/Supplemental/Arial Bold.ttf").to_path_buf());
    }
    #[cfg(target_os = "linux")]
    {
        paths.push(Path::new("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf").to_path_buf());
    }
    paths
}

/// Rasterize a single caption line into an RGBA frame sized to the output resolution.
pub fn render_caption(
    text: &str,
    style_id: &str,
    width: u32,
    height: u32,
) -> Result<RgbaFrame, CaptionError> {
    let font = load_font()?;
    let spec = style_spec(style_id);
    let scale = PxScale::from(spec.font_scale);
    let font_scaled = font.as_scaled(scale);

    let mut pixels = vec![0u8; (width * height * 4) as usize];
    let text = text.trim();
    if text.is_empty() {
        return Ok(RgbaFrame {
            width,
            height,
            pixels,
        });
    }

    let glyph_width: f32 = text
        .chars()
        .map(|c| font_scaled.h_advance(font.glyph_id(c)))
        .sum();
    let start_x = ((width as f32 - glyph_width) / 2.0).max(0.0) as i32;
    let baseline_y = (height as f32 * spec.vertical_anchor) as i32;

    if let Some(box_rgba) = spec.box_rgba {
        let pad_x = 24;
        let pad_y = 12;
        let bx0 = (start_x - pad_x).max(0) as u32;
        let by0 = (baseline_y - spec.font_scale as i32 - pad_y).max(0) as u32;
        let bx1 = (start_x as u32 + glyph_width as u32 + pad_x as u32).min(width);
        let by1 = (baseline_y as u32 + pad_y as u32).min(height);
        fill_rect(&mut pixels, width, height, bx0, by0, bx1, by1, box_rgba);
    }

    if let Some((dx, dy)) = spec.shadow_offset {
        draw_text(
            &font,
            scale,
            text,
            start_x + dx,
            baseline_y + dy,
            [0, 0, 0, 180],
            &mut pixels,
            width,
            height,
        );
    }

    if let Some(outline) = spec.outline_rgba {
        for ox in -2..=2 {
            for oy in -2..=2 {
                if ox == 0 && oy == 0 {
                    continue;
                }
                draw_text(
                    &font,
                    scale,
                    text,
                    start_x + ox,
                    baseline_y + oy,
                    outline,
                    &mut pixels,
                    width,
                    height,
                );
            }
        }
    }

    draw_text(
        &font,
        scale,
        text,
        start_x,
        baseline_y,
        spec.text_rgba,
        &mut pixels,
        width,
        height,
    );

    Ok(RgbaFrame {
        width,
        height,
        pixels,
    })
}

#[allow(clippy::too_many_arguments)]
fn fill_rect(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
    rgba: [u8; 4],
) {
    for y in y0..y1 {
        for x in x0..x1 {
            blend_pixel(pixels, width, height, x as i32, y as i32, rgba);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_text(
    font: &FontArc,
    scale: PxScale,
    text: &str,
    origin_x: i32,
    origin_y: i32,
    rgba: [u8; 4],
    pixels: &mut [u8],
    width: u32,
    height: u32,
) {
    let font_scaled = font.as_scaled(scale);
    let mut cursor_x = origin_x as f32;
    let baseline_y = origin_y as f32;

    for ch in text.chars() {
        let glyph_id = font.glyph_id(ch);
        let glyph: Glyph = glyph_id.with_scale_and_position(scale, point(cursor_x, baseline_y));
        if let Some(outlined) = font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            outlined.draw(|x, y, coverage| {
                let px = bounds.min.x as i32 + x as i32;
                let py = bounds.min.y as i32 + y as i32;
                let mut c = rgba;
                c[3] = (coverage * rgba[3] as f32) as u8;
                blend_pixel(pixels, width, height, px, py, c);
            });
        }
        cursor_x += font_scaled.h_advance(glyph_id);
    }
}

fn blend_pixel(pixels: &mut [u8], width: u32, height: u32, x: i32, y: i32, rgba: [u8; 4]) {
    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 || rgba[3] == 0 {
        return;
    }
    let idx = ((y as u32 * width + x as u32) * 4) as usize;
    let alpha = rgba[3] as f32 / 255.0;
    for i in 0..3 {
        let dst = pixels[idx + i] as f32;
        let src = rgba[i] as f32;
        pixels[idx + i] = (src * alpha + dst * (1.0 - alpha)) as u8;
    }
    pixels[idx + 3] = 255;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_styles_list_is_non_empty() {
        assert!(BUILTIN_STYLES.contains(&STYLE_TIKTOK_BOLD_YELLOW));
    }

    #[test]
    fn render_caption_produces_pixels_when_font_available() {
        if load_font().is_err() {
            eprintln!("skipping caption render test: no font");
            return;
        }
        let frame = render_caption("test caption", STYLE_TIKTOK_MINIMAL, 640, 360).unwrap();
        assert_eq!(frame.pixels.len(), 640 * 360 * 4);
        assert!(frame.pixels.iter().any(|&b| b > 0));
    }

    /// "F" is left-heavy; mirrored rendering puts more ink on the right half of its bbox.
    #[test]
    fn caption_text_is_not_mirrored() {
        if load_font().is_err() {
            eprintln!("skipping caption mirror test: no font");
            return;
        }
        let frame = render_caption("F", STYLE_TIKTOK_MINIMAL, 400, 200).unwrap();
        let (mut min_x, mut max_x, mut sum_x, mut count) = (u32::MAX, 0u32, 0f64, 0u64);
        for y in 0..frame.height {
            for x in 0..frame.width {
                let idx = ((y * frame.width + x) * 4) as usize;
                let alpha = frame.pixels[idx + 3];
                if alpha > 32 {
                    min_x = min_x.min(x);
                    max_x = max_x.max(x);
                    sum_x += x as f64;
                    count += 1;
                }
            }
        }
        assert!(count > 50, "expected visible glyph pixels");
        let centroid = sum_x / count as f64;
        let mid = (min_x + max_x) as f64 / 2.0;
        assert!(
            centroid < mid,
            "caption glyph appears mirrored (centroid {centroid} >= mid {mid})"
        );
    }
}
