//! Control Center over a running game shows the game behind it. Our window is opaque, so the screen is copied as Guide
//! is pressed (before the window shows) and the page draws that still under the PS5's scrims.
//! - BitBlt from the screen DC: one composed frame, ~60 ms for 1920×1080 with a game in front.
//! - The page gets it in two steps, as BMPs (no encoding; nothing on disk): first at half size (2 MB, on screen within
//!   the 300 ms budget), then at full size (8 MB, 170 ms over IPC plus 60 ms to decode), swapped in when decoded.
//!   A JPEG took 200-245 ms just to encode a 1080p frame in a release build, so it would be slower than either.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

/// A screen copy: top-down BGRA.
struct Shot {
    w: u32,
    h: u32,
    bgra: Vec<u8>,
}

static STILL: Mutex<Option<Arc<Shot>>> = Mutex::new(None);

/// Copies the primary screen. Returns how long it took.
pub fn capture() -> Result<Duration, String> {
    let t0 = Instant::now();
    let (w, h, bgra) = grab()?;
    *STILL.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(Shot { w, h, bgra }));
    Ok(t0.elapsed())
}

/// The last still as a BMP file's bytes: full size, or half size (`small`).
pub fn still(small: bool) -> Option<Vec<u8>> {
    let shot = STILL.lock().unwrap_or_else(|e| e.into_inner()).clone()?;
    Some(if small {
        let (w, h, px) = half(shot.w, shot.h, &shot.bgra);
        bmp(w, h, &px)
    } else {
        bmp(shot.w, shot.h, &shot.bgra)
    })
}

/// Drops the still (the game ended).
pub fn clear() {
    *STILL.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Half size: each 2×2 block averaged.
fn half(w: u32, h: u32, bgra: &[u8]) -> (u32, u32, Vec<u8>) {
    let (hw, hh) = (w / 2, h / 2);
    let mut out = vec![0u8; (hw * hh * 4) as usize];
    let row = (w * 4) as usize;
    for y in 0..hh as usize {
        let (a, b) = (&bgra[2 * y * row..(2 * y + 1) * row], &bgra[(2 * y + 1) * row..(2 * y + 2) * row]);
        for x in 0..hw as usize {
            let i = 8 * x;
            for c in 0..4 {
                let sum = u16::from(a[i + c]) + u16::from(a[i + 4 + c]) + u16::from(b[i + c]) + u16::from(b[i + 4 + c]);
                out[(y * hw as usize + x) * 4 + c] = ((sum + 2) / 4) as u8;
            }
        }
    }
    (hw, hh, out)
}

/// The primary screen as top-down BGRA.
fn grab() -> Result<(u32, u32, Vec<u8>), String> {
    unsafe {
        let (w, h) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
        if w <= 0 || h <= 0 {
            return Err("no screen size".into());
        }
        let screen = GetDC(None);
        if screen.is_invalid() {
            return Err("no screen DC".into());
        }
        let mem = CreateCompatibleDC(Some(screen));
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let result = match CreateDIBSection(Some(mem), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(dib) => {
                let old = SelectObject(mem, dib.into());
                let copied = BitBlt(mem, 0, 0, w, h, Some(screen), 0, 0, SRCCOPY);
                let out = copied
                    .map_err(|e| format!("BitBlt: {e}"))
                    .map(|()| std::slice::from_raw_parts(bits as *const u8, (w * h * 4) as usize).to_vec());
                SelectObject(mem, old);
                let _ = DeleteObject(dib.into());
                out
            }
            Err(e) => Err(format!("CreateDIBSection: {e}")),
        };
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        result.map(|px| (w as u32, h as u32, px))
    }
}

/// A 32-bit top-down BMP. GDI leaves alpha at 0, which some decoders read as transparent, so it is set opaque.
fn bmp(w: u32, h: u32, bgra: &[u8]) -> Vec<u8> {
    let size = 54 + bgra.len();
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(-(h as i32)).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&[0u8; 24]); // BI_RGB, image size 0, resolution, palette
    out.extend_from_slice(bgra);
    for px in out[54..].chunks_exact_mut(4) {
        px[3] = 255;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bmp_header_and_opaque_alpha() {
        let b = bmp(2, 1, &[1, 2, 3, 0, 4, 5, 6, 0]);
        assert_eq!(&b[..2], b"BM");
        assert_eq!(u32::from_le_bytes(b[2..6].try_into().unwrap()) as usize, b.len());
        assert_eq!(i32::from_le_bytes(b[22..26].try_into().unwrap()), -1, "top-down");
        assert_eq!(&b[54..], &[1, 2, 3, 255, 4, 5, 6, 255]);
    }

    #[test]
    fn half_size_averages_blocks() {
        // 2×2 BGRA: one block -> one pixel, the average
        let px = [0, 0, 0, 0, 100, 4, 8, 0, 0, 0, 0, 0, 100, 4, 8, 0];
        let (w, h, out) = half(2, 2, &px);
        assert_eq!((w, h), (1, 1));
        assert_eq!(out, vec![50, 2, 4, 0]);
    }

    #[test]
    #[ignore = "real screen: copies the desktop"]
    fn real_capture_timing() {
        for _ in 0..4 {
            let took = capture().unwrap();
            let t1 = Instant::now();
            let small = still(true).unwrap();
            let t2 = Instant::now();
            let full = still(false).unwrap();
            println!(
                "copy {:.1} ms; half BMP {:.1} ms ({} KB), full BMP {:.1} ms ({} KB)",
                took.as_secs_f64() * 1000.0,
                (t2 - t1).as_secs_f64() * 1000.0,
                small.len() / 1024,
                t2.elapsed().as_secs_f64() * 1000.0,
                full.len() / 1024
            );
        }
    }
}
