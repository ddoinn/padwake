//! Image work for the library, ported from the `W.Img` helpers in `extras/antcloud/sync.ps1`. The backdrop is only
//! cropped, not darkened: the page draws the PS5's scrims itself (`design/ps5/1-home.md`).

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageFormat, RgbImage, RgbaImage};
use std::path::Path;

pub fn decode(bytes: &[u8]) -> Result<DynamicImage, String> {
    image::load_from_memory(bytes).map_err(|e| e.to_string())
}

/// Mean brightness (0–255) of the opaque pixels; 255 when there are none.
pub fn luma(img: &DynamicImage) -> f64 {
    // a 1/4 sample is plenty for a mean and keeps a 3840 px hero cheap
    let small = img.thumbnail(img.width().div_ceil(4).max(1), img.height().div_ceil(4).max(1)).to_rgba8();
    let (mut sum, mut n) = (0.0, 0u64);
    for p in small.pixels() {
        if p[3] > 128 {
            sum += 0.299 * p[0] as f64 + 0.587 * p[1] as f64 + 0.114 * p[2] as f64;
            n += 1;
        }
    }
    if n == 0 {
        255.0
    } else {
        sum / n as f64
    }
}

/// A square tile from a portrait cover: its top square (box art puts the title and hero there).
pub fn square_top(img: DynamicImage) -> DynamicImage {
    let (w, h) = img.dimensions();
    if h as f64 <= w as f64 * 1.05 {
        return img;
    }
    img.crop_imm(0, 0, w, w)
}

/// The middle 16:9 of the image (what the backdrop shows).
pub fn crop_16_9(img: DynamicImage) -> DynamicImage {
    let (w, h) = img.dimensions();
    let (cw, ch) = if w as f64 / h as f64 > 16.0 / 9.0 { (h * 16 / 9, h) } else { (w, w * 9 / 16) };
    img.crop_imm((w - cw) / 2, (h - ch) / 2, cw, ch)
}

/// Shrinks to at most `max_w` pixels wide.
pub fn fit(img: DynamicImage, max_w: u32) -> DynamicImage {
    if img.width() <= max_w {
        return img;
    }
    let h = (img.height() as u64 * max_w as u64 / img.width() as u64).max(1) as u32;
    img.resize_exact(max_w, h, FilterType::CatmullRom)
}

/// Saves as JPEG; transparent pixels go on black.
pub fn save_jpeg(img: &DynamicImage, path: &Path, quality: u8) -> Result<(), String> {
    let rgba = img.to_rgba8();
    let rgb = RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let p = rgba.get_pixel(x, y);
        let a = p[3] as u16;
        image::Rgb([(p[0] as u16 * a / 255) as u8, (p[1] as u16 * a / 255) as u8, (p[2] as u16 * a / 255) as u8])
    });
    let mut out = Vec::new();
    rgb.write_with_encoder(JpegEncoder::new_with_quality(&mut out, quality)).map_err(|e| e.to_string())?;
    super::write_file(path, &out)
}

pub fn save_png(img: &RgbaImage, path: &Path) -> Result<(), String> {
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png).map_err(|e| e.to_string())?;
    super::write_file(path, &out.into_inner())
}

/// A logo that is mostly transparent with dark artwork turns white for the dark UI (coloured accents stay). A logo
/// with white next to dark (white text on a dark plate, or shading) is left alone. Returns true if it changed.
pub fn whiten_dark_glyph(img: &mut RgbaImage) -> bool {
    let (w, h) = img.dimensions();
    let dark = |p: &image::Rgba<u8>| p[3] > 128 && p[0].max(p[1]).max(p[2]) < 100;
    let (mut opaque, mut darks, mut edge) = (0u64, 0u64, 0u64);
    let total = w as u64 * h as u64;
    for y in 0..h {
        for x in 0..w {
            let p = img.get_pixel(x, y);
            if p[3] <= 128 {
                continue;
            }
            opaque += 1;
            if dark(p) {
                darks += 1;
                continue;
            }
            if p[0].min(p[1]).min(p[2]) <= 180 {
                continue;
            }
            let near_dark = (y.saturating_sub(2)..=(y + 2).min(h - 1))
                .any(|yy| (x.saturating_sub(2)..=(x + 2).min(w - 1)).any(|xx| dark(img.get_pixel(xx, yy))));
            if near_dark {
                edge += 1;
            }
        }
    }
    let o = opaque as f64;
    if opaque == 0 || o > total as f64 * 0.7 || (darks as f64) < o * 0.35 || edge as f64 > o * 0.03 {
        return false;
    }
    for p in img.pixels_mut() {
        if p[0].max(p[1]).max(p[2]) < 100 {
            p[0] = 255;
            p[1] = 255;
            p[2] = 255;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn logo(fill: impl Fn(u32, u32) -> Rgba<u8>) -> RgbaImage {
        RgbaImage::from_fn(40, 20, |x, y| if (5..35).contains(&x) && (5..15).contains(&y) { fill(x, y) } else { Rgba([0, 0, 0, 0]) })
    }

    #[test]
    fn dark_logos_turn_white_but_plates_stay() {
        let mut black_text = logo(|_, _| Rgba([20, 20, 30, 255]));
        assert!(whiten_dark_glyph(&mut black_text));
        assert_eq!(*black_text.get_pixel(10, 10), Rgba([255, 255, 255, 255]));
        // white letters on a dark plate (Red Dead): unchanged
        let mut plate = logo(|x, _| if x % 6 < 3 { Rgba([250, 250, 250, 255]) } else { Rgba([10, 10, 10, 255]) });
        assert!(!whiten_dark_glyph(&mut plate));
        // an already light logo: unchanged
        let mut light = logo(|_, _| Rgba([240, 200, 60, 255]));
        assert!(!whiten_dark_glyph(&mut light));
    }

    #[test]
    fn crops() {
        let cover = DynamicImage::new_rgb8(600, 900);
        assert_eq!(square_top(cover).dimensions(), (600, 600));
        assert_eq!(crop_16_9(DynamicImage::new_rgb8(3840, 1240)).dimensions(), (2204, 1240));
        assert_eq!(crop_16_9(DynamicImage::new_rgb8(1920, 1200)).dimensions(), (1920, 1080));
        assert_eq!(fit(DynamicImage::new_rgb8(1920, 1080), 1280).dimensions(), (1280, 720));
    }
}
