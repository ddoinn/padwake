//! Settings: `settings.json` in the data dir, read and written through `crate::data`. Every change emits the
//! `settings` event with the whole object (the input layer reads `rumble` and `uiSounds`, Home reads `music`).
//! The Windows settings the Settings screen changes live in the `system` and `startup` areas.

use serde_json::{Map, Value};
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{AppHandle, Emitter, Runtime};

pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("settings").invoke_handler(tauri::generate_handler![get, set, folder_exists]).build()
}

#[tauri::command]
fn get() -> Map<String, Value> {
    crate::data::settings()
}

/// Merges `patch` into the settings, saves them, emits `settings` and returns them all.
#[tauri::command]
fn set<R: Runtime>(app: AppHandle<R>, patch: Map<String, Value>) -> Result<Map<String, Value>, String> {
    let all = crate::data::update_settings(patch)?;
    let _ = app.emit("settings", &all);
    Ok(all)
}

/// Row B2: a game's display profile, if it has one. Profiles are shared settings: `displayProfiles`, keyed by game id
/// (Home's Options menu sets them through `set`).
pub fn display_profile(id: &str) -> Option<crate::system::DisplayProfile> {
    profile_in(&crate::data::settings(), id)
}

fn profile_in(all: &Map<String, Value>, id: &str) -> Option<crate::system::DisplayProfile> {
    let p: crate::system::DisplayProfile = serde_json::from_value(all.get("displayProfiles")?.get(id)?.clone()).ok()?;
    (p.mode.is_some() || p.hdr.is_some()).then_some(p)
}

/// True if `path` is an existing folder (the games folder picker checks before saving).
#[tauri::command]
fn folder_exists(path: String) -> bool {
    !path.trim().is_empty() && std::path::Path::new(path.trim()).is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_games_display_profile_is_found_by_its_id() {
        let all: Map<String, Value> = serde_json::from_str(
            r#"{"music":true,"displayProfiles":{"hades":{"mode":{"width":1280,"height":720,"hz":60},"hdr":true},
                "celeste":{},"broken":{"mode":"1280x720"}}}"#,
        )
        .unwrap();
        let p = profile_in(&all, "hades").unwrap();
        assert_eq!((p.mode.map(|m| (m.width, m.height, m.hz)), p.hdr), (Some((1280, 720, 60)), Some(true)));
        assert_eq!(profile_in(&all, "celeste"), None, "Use PC Setting for both: no profile");
        assert_eq!(profile_in(&all, "broken"), None, "a profile that doesn't read is no profile");
        assert_eq!(profile_in(&all, "elden-ring"), None);
        assert_eq!(profile_in(&Map::new(), "hades"), None);
    }

    #[test]
    fn folder_exists_only_for_folders() {
        let here = std::env::current_dir().unwrap();
        assert!(folder_exists(here.display().to_string()));
        assert!(!folder_exists(here.join("Cargo.toml").display().to_string()));
        assert!(!folder_exists("   ".into()));
        assert!(!folder_exists(r"W:\no\such\folder\here".into()));
    }
}
