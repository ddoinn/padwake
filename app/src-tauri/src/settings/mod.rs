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

/// True if `path` is an existing folder (the games folder picker checks before saving).
#[tauri::command]
fn folder_exists(path: String) -> bool {
    !path.trim().is_empty() && std::path::Path::new(path.trim()).is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_exists_only_for_folders() {
        let here = std::env::current_dir().unwrap();
        assert!(folder_exists(here.display().to_string()));
        assert!(!folder_exists(here.join("Cargo.toml").display().to_string()));
        assert!(!folder_exists("   ".into()));
        assert!(!folder_exists(r"W:\no\such\folder\here".into()));
    }
}
