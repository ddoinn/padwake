//! Media uses the shared sleeping, Guide and Close session lifecycle.
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{AppHandle, Runtime, Emitter};
mod icons;

#[derive(Clone, Serialize, Deserialize)]
struct Art {
    tile: String,
    background: String,
}
#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    id: String,
    name: String,
    url: String,
    art: Art,
    #[serde(default)]
    tagline: String,
    #[serde(default)]
    icon: Option<icons::CachedIcon>,
}

pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("media").invoke_handler(tauri::generate_handler![list, open, refresh]).build()
}

fn defaults(dir: &Path) -> Result<Vec<Entry>, String> {
    let dir = std::path::absolute(dir).map_err(|e| e.to_string())?;
    let art_dir = dir.join("media");
    std::fs::create_dir_all(&art_dir).map_err(|e| e.to_string())?;
    macro_rules! entries {
        ($(($id:literal, $name:literal, $url:literal)),* $(,)?) => {{
            let mut entries = Vec::new();
            $(
                let tile = art_dir.join(concat!($id, ".svg"));
                let background = art_dir.join(concat!($id, "-background.svg"));
                if !tile.is_file() { std::fs::write(&tile, include_str!(concat!("../../../ui/media/", $id, ".svg"))).map_err(|e| e.to_string())?; }
                if !background.is_file() { std::fs::write(&background, include_str!(concat!("../../../ui/media/", $id, "-background.svg"))).map_err(|e| e.to_string())?; }
                entries.push(Entry { id: $id.into(), name: $name.into(), url: $url.into(), tagline: icons::description($id).into(), icon: None, art: Art {
                    tile: tile.to_string_lossy().into_owned(), background: background.to_string_lossy().into_owned(),
                }});
            )*
            Ok(entries)
        }};
    }
    entries! {
        ("youtube", "YouTube", "https://www.youtube.com/"),
        ("netflix", "Netflix", "https://www.netflix.com/"),
        ("spotify", "Spotify", "https://open.spotify.com/"),
        ("twitch", "Twitch", "https://www.twitch.tv/"),
        ("plex", "Plex", "https://app.plex.tv/"),
        ("crunchyroll", "Crunchyroll", "https://www.crunchyroll.com/"),
    }
}

fn read_entries(dir: &Path) -> Result<Vec<Entry>, String> {
    let mut entries: Vec<Entry> = match std::fs::read_to_string(dir.join("media.json")) {
        Ok(json) => serde_json::from_str(&json).map_err(|e| format!("media.json: {e}"))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => defaults(dir)?,
        Err(e) => return Err(format!("media.json: {e}")),
    };
    let root = std::path::absolute(dir).map_err(|e| e.to_string())?;
    for entry in &mut entries {
        if entry.tagline.is_empty() { entry.tagline = icons::description(&entry.id).into(); }
        for path in [&mut entry.art.tile, &mut entry.art.background] {
            if Path::new(path).is_relative() { *path = root.join(&*path).to_string_lossy().into_owned(); }
        }
        if !Path::new(&entry.art.tile).is_file() {
            if let Some(fallback) = defaults(dir)?.into_iter().find(|e| e.id == entry.id) {
                entry.art = fallback.art;
                entry.icon = None;
            }
        }
    }
    let mut ids = std::collections::HashSet::new();
    for e in &entries {
        let url = tauri::Url::parse(&e.url).map_err(|err| format!("media {}: {err}", e.id))?;
        if e.id.is_empty() || !e.id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') || e.name.is_empty() || !ids.insert(&e.id) || !matches!(url.scheme(), "https" | "http") || url.host_str().is_none() {
            return Err(format!("media.json: invalid or duplicate entry {}", e.id));
        }
    }
    Ok(entries)
}
#[tauri::command]
fn list<R: Runtime>(app: AppHandle<R>) -> Result<Vec<Entry>, String> {
    let entries = read_entries(crate::data::dir())?;
    icons::start(app, false);
    Ok(entries)
}
/// Enqueues a refresh; the UI remains usable and existing art stays visible.
#[tauri::command]
fn refresh<R: Runtime>(app: AppHandle<R>) { icons::start(app, true); }

fn save_entries(dir: &Path, entries: &[Entry]) -> Result<(), String> {
    let root = std::path::absolute(dir).map_err(|e| e.to_string())?;
    let mut saved = entries.to_vec();
    for entry in &mut saved {
        for path in [&mut entry.art.tile, &mut entry.art.background] {
            *path = Path::new(path).strip_prefix(&root).unwrap_or(Path::new(path)).to_string_lossy().into_owned();
        }
    }
    crate::library::write_file(&dir.join("media.json"), &serde_json::to_vec_pretty(&saved).map_err(|e| e.to_string())?)
}

/// Installed Edge first; otherwise the newest numeric EdgeCore version.
fn find_edge(roots: &[PathBuf]) -> Result<PathBuf, String> {
    for root in roots {
        let installed = root.join("Microsoft/Edge/Application/msedge.exe");
        if installed.is_file() { return Ok(installed); }
    }
    let mut core = Vec::new();
    for root in roots {
        if let Ok(dirs) = std::fs::read_dir(root.join("Microsoft/EdgeCore")) {
            for dir in dirs.flatten() {
                let version = dir.file_name().to_string_lossy().split('.').map(str::parse::<u32>).collect::<Result<Vec<_>, _>>();
                if let Ok(version) = version {
                    let exe = dir.path().join("msedge.exe");
                    if exe.is_file() { core.push((version, exe)); }
                }
            }
        }
    }
    core.sort_by(|a, b| b.0.cmp(&a.0));
    core.into_iter().next().map(|(_, p)| p).ok_or_else(|| "Microsoft Edge was not found. Install Edge to open Media.".into())
}
fn arguments(profile: &Path, url: &str) -> Vec<OsString> {
    let mut data = OsString::from("--user-data-dir="); data.push(profile);
    vec![data, "--no-first-run".into(), "--no-default-browser-check".into(),
        "--start-fullscreen".into(), "--disable-background-mode".into(),
        "--autoplay-policy=no-user-gesture-required".into(), format!("--app={url}").into()]
}
#[tauri::command]
async fn open<R: Runtime>(app: AppHandle<R>, id: String) -> Result<(), String> {
    if crate::games::current().is_some_and(|s| s.kind == crate::games::Kind::Media && s.id == id) {
        return crate::games::back_to_session(&app);
    }
    let entry = read_entries(crate::data::dir())?.into_iter().find(|e| e.id == id).ok_or_else(|| format!("no media {id}"))?;
    let roots = ["ProgramFiles(x86)", "ProgramFiles"].into_iter().filter_map(std::env::var_os).map(PathBuf::from).collect::<Vec<_>>();
    let edge = find_edge(&roots)?;
    let profile = crate::data::dir().join("media-profile");
    std::fs::create_dir_all(&profile).map_err(|e| e.to_string())?;
    let mut cmd = Command::new(edge); cmd.args(arguments(&profile, &entry.url));
    // The private profile prevents handoff to personal Edge. Only track the job: the profile isn't an
    // executable folder, and tracking Edge's install folder could close somebody's personal browser.
    crate::games::start(&app, crate::games::Kind::Media, &id, &entry.name, cmd, None)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn scratch() -> PathBuf {
        let p = PathBuf::from(r"W:\tmp").join(format!("a12-test-{}", std::process::id()));
        std::fs::create_dir_all(&p).unwrap(); p
    }
    #[test]
    fn lookup_prefers_installed_then_newest_core() {
        let root = scratch().join("lookup");
        for v in ["99.0.1.1", "156.0.1.1", "junk"] {
            let p = root.join("Microsoft/EdgeCore").join(v);
            std::fs::create_dir_all(&p).unwrap(); std::fs::write(p.join("msedge.exe"), "").unwrap();
        }
        assert_eq!(find_edge(&[root.clone()]).unwrap(), root.join("Microsoft/EdgeCore/156.0.1.1/msedge.exe"));
        let installed = root.join("Microsoft/Edge/Application/msedge.exe");
        std::fs::create_dir_all(installed.parent().unwrap()).unwrap(); std::fs::write(&installed, "").unwrap();
        assert_eq!(find_edge(&[root.clone()]).unwrap(), installed);
        assert!(find_edge(&[root.join("missing")]).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn args_keep_spaces_and_url_in_single_arguments_without_tv_spoofing() {
        let args = arguments(Path::new(r"W:\data with spaces\media-profile"), "https://www.youtube.com/?a=1&b=2");
        assert_eq!(args[0], r"--user-data-dir=W:\data with spaces\media-profile");
        assert_eq!(args.last().unwrap(), "--app=https://www.youtube.com/?a=1&b=2");
        assert!(args.contains(&OsString::from("--start-fullscreen")));
        assert!(!args.iter().any(|a| a.to_string_lossy().contains("user-agent")));
    }
    #[test]
    fn defaults_and_user_edits() {
        let root = scratch().join("entries");
        let entries = read_entries(&root).unwrap(); assert_eq!(entries.len(), 6);
        assert!(Path::new(&entries[0].art.tile).is_file());
        let mut custom = entries[..1].to_vec(); custom[0].name = "My video".into();
        std::fs::write(root.join("media.json"), serde_json::to_string(&custom).unwrap()).unwrap();
        assert_eq!(read_entries(&root).unwrap()[0].name, "My video");
        custom[0].url = "file:///secret".into();
        std::fs::write(root.join("media.json"), serde_json::to_string(&custom).unwrap()).unwrap();
        assert!(read_entries(&root).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
