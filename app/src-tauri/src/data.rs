//! The data dir (`data\` beside the exe, or the folder in `PADWAKE_DATA`) and `settings.json` in it.
//! Every area reads settings through here; the settings area owns the get/set commands.

use serde::de::DeserializeOwned;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

pub fn dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::var_os("PADWAKE_DATA")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::current_exe().ok()?.parent().map(|p| p.join("data")))
            .unwrap_or_else(|| PathBuf::from("data"));
        if let Err(e) = std::fs::create_dir_all(&dir) {
            eprintln!("data dir {}: {e}", dir.display());
        }
        dir
    })
}

fn defaults() -> Map<String, Value> {
    // debug builds run on the dev laptop against its stand-in library
    let games_root = if cfg!(debug_assertions) { r"W:\games-test" } else { r"C:\Games" };
    let v = json!({ "gamesRoot": games_root, "rumble": true, "uiSounds": true, "music": true });
    v.as_object().cloned().unwrap_or_default()
}

fn settings_path() -> PathBuf {
    dir().join("settings.json")
}

fn store() -> MutexGuard<'static, Map<String, Value>> {
    static STORE: OnceLock<Mutex<Map<String, Value>>> = OnceLock::new();
    STORE
        .get_or_init(|| {
            let mut s = defaults();
            let saved = std::fs::read_to_string(settings_path()).ok();
            match saved.as_deref().map(serde_json::from_str::<Map<String, Value>>) {
                Some(Ok(saved)) => s.extend(saved),
                Some(Err(e)) => eprintln!("settings.json is not valid JSON, using defaults: {e}"),
                None => {
                    if let Err(e) = write(&s) {
                        eprintln!("settings.json: {e}");
                    }
                }
            }
            Mutex::new(s)
        })
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn write(s: &Map<String, Value>) -> Result<(), String> {
    let text = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    let tmp = settings_path().with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, settings_path()).map_err(|e| e.to_string())
}

/// All settings: the defaults with `settings.json` on top.
pub fn settings() -> Map<String, Value> {
    store().clone()
}

/// One setting, or `None` if it is missing or has another type.
pub fn setting<T: DeserializeOwned>(key: &str) -> Option<T> {
    store().get(key).cloned().and_then(|v| serde_json::from_value(v).ok())
}

/// Merges `patch` into the settings and saves them. Returns the full settings.
pub fn update_settings(patch: Map<String, Value>) -> Result<Map<String, Value>, String> {
    let mut s = store();
    s.extend(patch);
    write(&s)?;
    Ok(s.clone())
}

pub fn games_root() -> PathBuf {
    PathBuf::from(setting::<String>("gamesRoot").unwrap_or_default())
}
