//! Half-block (`▀`) terminal images. Sixel / kitty / iTerm2 are v2.

use image::{DynamicImage, ImageBuffer, ImageFormat, RgbaImage};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use crate::Error;

const MAX_SAVE_EDGE: u32 = 4096;

pub fn encode_png_rgba(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, Error> {
    let (width, height, rgba) = cap_rgba(width, height, rgba)?;
    let img: RgbaImage = ImageBuffer::from_raw(width, height, rgba).ok_or(Error::InvalidRgba)?;
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, ImageFormat::Png)?;
    Ok(buf.into_inner())
}

pub fn decode_rgba(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), Error> {
    let img = image::load_from_memory(bytes)?.to_rgba8();
    Ok((img.width(), img.height(), img.into_raw()))
}

pub fn looks_like_image(name: &str) -> bool {
    let name = name
        .split('|')
        .next()
        .unwrap_or(name)
        .split('#')
        .next()
        .unwrap_or(name)
        .trim();
    let ext = std::path::Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp"
    )
}

/// Scale to at most `max_cols` × `max_rows` cells (two source pixels per row).
pub fn rgba_to_half_block(
    rgba: &[u8],
    width: u32,
    height: u32,
    max_cols: u16,
    max_rows: u16,
    bg: Color,
) -> Vec<Line<'static>> {
    if width == 0
        || height == 0
        || max_cols == 0
        || max_rows == 0
        || rgba.len() < width as usize * height as usize * 4
    {
        return Vec::new();
    }
    let max_px_w = max_cols as u32;
    let max_px_h = (max_rows as u32).saturating_mul(2).max(1);
    let scale = (max_px_w as f32 / width as f32)
        .min(max_px_h as f32 / height as f32)
        .min(1.0);
    let out_w = (width as f32 * scale).round().max(1.0) as u32;
    let mut out_h = (height as f32 * scale).round().max(1.0) as u32;
    if out_h % 2 == 1 {
        out_h += 1;
    }
    let mut lines = Vec::with_capacity((out_h / 2) as usize);
    for y in (0..out_h).step_by(2) {
        let mut spans = Vec::with_capacity(out_w as usize);
        for x in 0..out_w {
            let top = sample(rgba, width, height, x, y, out_w, out_h);
            let bot = sample(rgba, width, height, x, y + 1, out_w, out_h);
            let fg = blend(top, bg);
            let bg_c = blend(bot, bg);
            spans.push(Span::styled("▀", Style::default().fg(fg).bg(bg_c)));
        }
        lines.push(Line::from(spans));
    }
    lines
}

fn cap_rgba(width: u32, height: u32, rgba: &[u8]) -> Result<(u32, u32, Vec<u8>), Error> {
    let expected = width as usize * height as usize * 4;
    if rgba.len() != expected {
        return Err(Error::InvalidRgba);
    }
    if width <= MAX_SAVE_EDGE && height <= MAX_SAVE_EDGE {
        return Ok((width, height, rgba.to_vec()));
    }
    let img: RgbaImage =
        ImageBuffer::from_raw(width, height, rgba.to_vec()).ok_or(Error::InvalidRgba)?;
    let scale = (MAX_SAVE_EDGE as f32 / width as f32).min(MAX_SAVE_EDGE as f32 / height as f32);
    let nw = (width as f32 * scale).max(1.0) as u32;
    let nh = (height as f32 * scale).max(1.0) as u32;
    let resized =
        DynamicImage::ImageRgba8(img).resize(nw, nh, image::imageops::FilterType::Triangle);
    let rgba = resized.to_rgba8();
    Ok((rgba.width(), rgba.height(), rgba.into_raw()))
}

fn sample(rgba: &[u8], src_w: u32, src_h: u32, x: u32, y: u32, out_w: u32, out_h: u32) -> [u8; 4] {
    let sx = (x * src_w / out_w.max(1)).min(src_w.saturating_sub(1));
    let sy = (y * src_h / out_h.max(1)).min(src_h.saturating_sub(1));
    let i = ((sy * src_w + sx) * 4) as usize;
    [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
}

fn blend(px: [u8; 4], bg: Color) -> Color {
    let (br, bgc, bb) = match bg {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => (0, 0, 0),
    };
    let a = px[3] as u16;
    if a == 0 {
        return bg;
    }
    if a == 255 {
        return Color::Rgb(px[0], px[1], px[2]);
    }
    let inv = 255 - a;
    Color::Rgb(
        ((px[0] as u16 * a + br as u16 * inv) / 255) as u8,
        ((px[1] as u16 * a + bgc as u16 * inv) / 255) as u8,
        ((px[2] as u16 * a + bb as u16 * inv) / 255) as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_by_two_becomes_one_row() {
        // top red, bottom blue
        let mut rgba = vec![0u8; 2 * 2 * 4];
        rgba[0] = 255;
        rgba[3] = 255;
        rgba[4] = 255;
        rgba[7] = 255;
        rgba[10] = 255;
        rgba[11] = 255;
        rgba[14] = 255;
        rgba[15] = 255;
        let lines = rgba_to_half_block(&rgba, 2, 2, 8, 8, Color::Black);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans.len(), 2);
        assert_eq!(lines[0].spans[0].content.as_ref(), "▀");
        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Rgb(255, 0, 0)));
        assert_eq!(lines[0].spans[0].style.bg, Some(Color::Rgb(0, 0, 255)));
    }

    #[test]
    fn roundtrip_png() {
        let rgba = vec![10, 20, 30, 255, 40, 50, 60, 255];
        let png = encode_png_rgba(2, 1, &rgba).expect("png");
        let (w, h, out) = decode_rgba(&png).expect("decode");
        assert_eq!((w, h), (2, 1));
        assert_eq!(out.len(), 8);
    }
}
