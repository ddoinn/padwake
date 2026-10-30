//! Captures (decision 5: View = screenshot): View while a game is in front saves the screen as a JPEG in
//! `data\captures\<game id>\`, with a 480 px thumbnail beside it, and toasts "Screenshot saved." over the game. The
//! Media Gallery (`ui/gallery`) lists them.
//! - The copy is the BitBlt Control Center's still uses (`backdrop.rs`), with our toast windows left out. Exclusive
//!   full-screen and protected (DRM) output copy as black: an all-black frame isn't saved, the toast says so.
//! - Encoding runs on its own thread (≈200 ms for 1080p), so the pad thread never waits; one capture at a time.

use super::backdrop;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Runtime};

const THUMB_W: u32 = 480;
static BUSY: AtomicBool = AtomicBool::new(false);

/// A saved screenshot: what `games|captures` lists, newest first.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Capture {
    /// the game's id (its folder in `captures\`)
    pub game: String,
    /// the game's name as the library knows it, else the id
    pub name: String,
    pub path: PathBuf,
    pub thumb: Option<PathBuf>,
    pub taken_at: u64,
}

pub fn dir() -> PathBuf {
    let d = crate::data::dir().join("captures");
    std::path::absolute(&d).unwrap_or(d)
}

/// Takes a screenshot of the game in front, in the background. `name` and `tile` are for the toast.
pub fn take<R: Runtime>(app: &AppHandle<R>, id: &str, name: &str, tile: Option<PathBuf>) {
    if BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    let (app, id, name) = (app.clone(), id.to_string(), name.to_string());
    let spawned = std::thread::Builder::new().name("capture".into()).spawn(move || {
        let shot = crate::system::without_toasts(&app, backdrop::grab);
        let result = shot.and_then(|(w, h, bgra)| save(&dir().join(&id), w, h, &bgra, SystemTime::now()));
        BUSY.store(false, Ordering::SeqCst);
        let message = match &result {
            Ok(Some(path)) => {
                super::log(&format!("capture: {}", path.display()));
                "Screenshot saved."
            }
            Ok(None) => {
                super::log("capture: the screen copied as black (exclusive full screen or protected video)");
                "This game can't be captured."
            }
            Err(e) => {
                super::log(&format!("capture failed: {e}"));
                "The screenshot wasn't saved."
            }
        };
        crate::system::overlay_toast(&app, &name, message, tile);
    });
    if spawned.is_err() {
        BUSY.store(false, Ordering::SeqCst);
    }
}

/// Saves a top-down BGRA frame as `<dir>\<yyyy-mm-dd hh-mm-ss>.jpg` plus `.thumb.jpg`. `None` for an all-black frame.
fn save(dir: &Path, w: u32, h: u32, bgra: &[u8], at: SystemTime) -> Result<Option<PathBuf>, String> {
    if is_black(bgra) {
        return Ok(None);
    }
    let rgb: Vec<u8> = bgra.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0]]).collect();
    let img = image::RgbImage::from_raw(w, h, rgb).ok_or("the frame has the wrong size")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let stem = unique_stem(dir, &stamp(at));
    let path = dir.join(format!("{stem}.jpg"));
    jpeg(&img, 92, &path)?;
    let th = THUMB_W * h / w.max(1);
    let thumb = image::imageops::resize(&img, THUMB_W, th.max(1), image::imageops::FilterType::Triangle);
    jpeg(&thumb, 85, &dir.join(format!("{stem}.thumb.jpg")))?;
    Ok(Some(path))
}

fn jpeg(img: &image::RgbImage, quality: u8, path: &Path) -> Result<(), String> {
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality)
        .encode_image(img)
        .map_err(|e| format!("jpeg: {e}"))?;
    crate::library::write_file(path, &bytes)
}

/// Every pixel near black (sampled: one in 64).
fn is_black(bgra: &[u8]) -> bool {
    bgra.chunks_exact(4).step_by(64).all(|p| p[0] < 10 && p[1] < 10 && p[2] < 10)
}

/// `2026-10-30 22-21-07Z`: UTC, as the library's log (the gallery shows local time, from the file's time).
fn stamp(at: SystemTime) -> String {
    let secs = at.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}-{:02}-{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

fn unique_stem(dir: &Path, base: &str) -> String {
    let mut stem = base.to_string();
    let mut n = 2;
    while dir.join(format!("{stem}.jpg")).exists() {
        stem = format!("{base} ({n})");
        n += 1;
    }
    stem
}

/// Every capture, newest first. `name_of` turns a game id into its name.
pub fn list(name_of: &dyn Fn(&str) -> Option<String>) -> Vec<Capture> {
    let mut out = Vec::new();
    let Ok(games) = std::fs::read_dir(dir()) else { return out };
    for g in games.flatten().filter(|e| e.path().is_dir()) {
        let game = g.file_name().to_string_lossy().into_owned();
        let name = name_of(&game).unwrap_or_else(|| game.clone());
        let Ok(files) = std::fs::read_dir(g.path()) else { continue };
        for f in files.flatten() {
            let path = f.path();
            let file = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            if !file.ends_with(".jpg") || file.ends_with(".thumb.jpg") {
                continue;
            }
            let thumb = path.with_extension("thumb.jpg");
            let taken_at = f
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            out.push(Capture { game: game.clone(), name: name.clone(), thumb: thumb.is_file().then_some(thumb), path, taken_at });
        }
    }
    out.sort_by(|a, b| b.taken_at.cmp(&a.taken_at).then_with(|| b.path.cmp(&a.path)));
    out
}

/// Deletes a capture and its thumbnail. Only files inside `captures\` can go.
pub fn delete(path: &Path) -> Result<(), String> {
    let root = dir();
    let p = std::path::absolute(path).map_err(|e| e.to_string())?;
    let inside = p.starts_with(&root) && p.components().count() == root.components().count() + 2;
    let is_jpg = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("jpg"));
    if !inside || !is_jpg || p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err("not a capture".into());
    }
    std::fs::remove_file(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    let _ = std::fs::remove_file(p.with_extension("thumb.jpg"));
    if let Some(game_dir) = p.parent() {
        let _ = std::fs::remove_dir(game_dir); // only if it's empty now
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn frame(w: u32, h: u32, v: u8) -> Vec<u8> {
        (0..w * h).flat_map(|i| [v, (i % 251) as u8 & v, v / 2, 0]).collect()
    }

    #[test]
    fn a_frame_saves_with_a_thumbnail_and_a_black_one_doesnt() {
        let dir = std::env::current_exe().unwrap().parent().unwrap().join(format!("ch-capture-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let at = UNIX_EPOCH + Duration::from_secs(1_793_398_867); // 2026-10-30 22:21:07Z
        let path = save(&dir, 64, 36, &frame(64, 36, 200), at).unwrap().unwrap();
        assert_eq!(path.file_name().unwrap(), "2026-10-30 22-21-07Z.jpg");
        let thumb = image::open(path.with_extension("thumb.jpg")).unwrap();
        assert_eq!((thumb.width(), thumb.height()), (THUMB_W, 270));
        let again = save(&dir, 64, 36, &frame(64, 36, 200), at).unwrap().unwrap();
        assert_eq!(again.file_name().unwrap(), "2026-10-30 22-21-07Z (2).jpg", "two in one second both stay");
        assert_eq!(save(&dir, 64, 36, &frame(64, 36, 0), at).unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_captures_can_be_deleted() {
        let root = dir();
        assert!(delete(&root.join("..").join("library.json")).is_err());
        assert!(delete(&root.join("hades").join("..").join("..").join("settings.json")).is_err());
        assert!(delete(&root.join("x.jpg")).is_err(), "a capture is in a game's folder");
        assert!(delete(Path::new(r"C:\Windows\win.ini")).is_err());
    }
}
