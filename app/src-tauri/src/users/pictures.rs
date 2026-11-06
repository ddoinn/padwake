//! Your own pictures as avatars. Padwake lists the images in `data\avatars\mine\` and in Windows' Pictures, Downloads
//! and Desktop folders (their top level, and Pictures\Screenshots), newest first. So "uploading" one is saving or copying
//! it there: a browser download, File Explorer, a USB drive.
//! - The page can only show files in the data dir, so each listed image gets a 160 px thumbnail in
//!   `data\avatars\thumbs\` (named by its path, size and time, so a changed file gets a new one).
//! - Picking one saves its centre square at 256 px as `data\avatars\pic-<key>.jpg`; the avatar is then `pic:<key>`.
//!   The original is never changed.

use crate::library::art;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const MAX: usize = 60;
const THUMB: u32 = 160;
const SIZE: u32 = 256;
const EXTENSIONS: [&str; 3] = ["jpg", "jpeg", "png"];
/// bigger files are skipped: a phone photo is a few MB, a 40 MB file is not an avatar
const MAX_BYTES: u64 = 30 << 20;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Picture {
    /// the image itself, wherever it is
    pub path: PathBuf,
    pub name: String,
    /// which place it is in: "Padwake", "Pictures", "Downloads", "Desktop", "Screenshots"
    pub place: String,
    pub modified: u64,
    /// its thumbnail in the data dir, for the page
    pub thumb: PathBuf,
}

fn avatars_dir() -> PathBuf {
    let d = crate::data::dir().join("avatars");
    let _ = std::fs::create_dir_all(&d);
    std::path::absolute(&d).unwrap_or(d)
}

/// `data\avatars\mine\`, made if missing, so there is always a place to put pictures.
pub fn mine() -> PathBuf {
    let d = avatars_dir().join("mine");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// The places Padwake looks, with their names.
fn places() -> Vec<(String, PathBuf)> {
    let mut out = vec![("Padwake".to_string(), mine())];
    if let Some(home) = std::env::var_os("USERPROFILE").map(PathBuf::from) {
        out.push(("Pictures".into(), home.join("Pictures")));
        out.push(("Screenshots".into(), home.join("Pictures").join("Screenshots")));
        out.push(("Downloads".into(), home.join("Downloads")));
        out.push(("Desktop".into(), home.join("Desktop")));
    }
    out
}

fn is_image(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| EXTENSIONS.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// A short key for a file as it is now: its path, size and time, hashed (FNV-1a).
fn key(path: &Path, len: u64, modified: u64) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in path.to_string_lossy().to_lowercase().bytes().chain(len.to_le_bytes()).chain(modified.to_le_bytes()) {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

fn modified_ms(m: &std::fs::Metadata) -> u64 {
    m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// The images in `places`, newest first, at most `MAX`.
fn find(places: &[(String, PathBuf)]) -> Vec<(String, PathBuf, String, u64, u64)> {
    let mut found = Vec::new();
    for (place, dir) in places {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            let Ok(m) = e.metadata() else { continue };
            if m.is_file() && is_image(&p) && m.len() > 0 && m.len() <= MAX_BYTES {
                let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                found.push((place.clone(), p, name, m.len(), modified_ms(&m)));
            }
        }
    }
    found.sort_by(|a, b| b.4.cmp(&a.4));
    found.truncate(MAX);
    found
}

fn thumb_path(k: &str) -> PathBuf {
    let d = avatars_dir().join("thumbs");
    let _ = std::fs::create_dir_all(&d);
    d.join(format!("{k}.jpg"))
}

/// The centre square of an image at `size` px.
fn square(path: &Path, size: u32) -> Result<image::DynamicImage, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let img = art::decode(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(img.resize_to_fill(size, size, image::imageops::FilterType::CatmullRom))
}

/// Your pictures, newest first, each with a thumbnail (made six at a time, kept for next time). An image that can't
/// be read is left out.
pub fn list() -> Vec<Picture> {
    let found = find(&places());
    let mut out = Vec::new();
    for chunk in found.chunks(6) {
        let thumbs: Vec<Option<PathBuf>> = std::thread::scope(|s| {
            let jobs: Vec<_> = chunk
                .iter()
                .map(|(_, p, _, len, at)| {
                    s.spawn(move || {
                        let t = thumb_path(&key(p, *len, *at));
                        if t.is_file() {
                            return Some(t);
                        }
                        let made = square(p, THUMB).and_then(|img| art::save_jpeg(&img, &t, 82));
                        match made {
                            Ok(()) => Some(t),
                            Err(e) => {
                                crate::system::log(&format!("avatars: {e}"));
                                None
                            }
                        }
                    })
                })
                .collect();
            jobs.into_iter().map(|j| j.join().ok().flatten()).collect()
        });
        for ((place, path, name, _, modified), thumb) in chunk.iter().zip(thumbs) {
            if let Some(thumb) = thumb {
                out.push(Picture { path: path.clone(), name: name.clone(), place: place.clone(), modified: *modified, thumb });
            }
        }
    }
    out
}

/// The avatar picture of `pic:<key>`.
pub fn avatar(k: &str) -> PathBuf {
    avatars_dir().join(format!("pic-{k}.jpg"))
}

/// Makes `path` (one of the listed pictures) an avatar picture. Returns its key and a name for it.
pub fn take(path: &Path) -> Result<(String, String), String> {
    let path = std::path::absolute(path).map_err(|e| e.to_string())?;
    let in_place = places().iter().any(|(_, d)| path.parent() == Some(d.as_path()));
    if !in_place || !is_image(&path) {
        return Err("Pick a picture from the list.".into());
    }
    let m = std::fs::metadata(&path).map_err(|_| "That picture isn't there any more.".to_string())?;
    if m.len() > MAX_BYTES {
        return Err("That picture is too big.".into());
    }
    let k = key(&path, m.len(), modified_ms(&m));
    let out = avatar(&k);
    if !out.is_file() {
        art::save_jpeg(&square(&path, SIZE)?, &out, 90)?;
    }
    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Picture".into());
    Ok((k, name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_images_first_and_nothing_else() {
        let dir = std::env::temp_dir().join(format!("pw-pictures-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, body) in [("old.png", &b"x"[..]), ("notes.txt", b"x"), ("empty.jpg", b""), ("new.JPG", b"x")] {
            std::fs::write(dir.join(name), body).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let found = find(&[("Downloads".into(), dir.clone())]);
        let names: Vec<_> = found.iter().map(|f| f.2.as_str()).collect();
        assert_eq!(names, ["new", "old"], "images only, not empty ones, newest first");
        assert_eq!(found[0].0, "Downloads");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_changed_file_gets_a_new_key() {
        let p = Path::new(r"C:\Users\x\Pictures\me.jpg");
        assert_eq!(key(p, 10, 5), key(Path::new(r"c:\users\x\pictures\ME.JPG"), 10, 5), "paths are case-blind on Windows");
        assert_ne!(key(p, 10, 5), key(p, 11, 5));
        assert_ne!(key(p, 10, 5), key(p, 10, 6));
        assert_eq!(key(p, 10, 5).len(), 16);
    }

    #[test]
    fn only_listed_places_can_be_taken() {
        assert!(take(Path::new(r"C:\Windows\System32\secret.png")).is_err());
        assert!(take(&mine().join("x.txt")).is_err(), "images only");
    }
}
