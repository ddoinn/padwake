//! Automatic progress for the journal: how far into a game the player is, without them saying so.
//! - Achievements first: a game run from a folder keeps its unlocks in a file its Steam layer writes, keyed by Steam
//!   app id: `achievements.json` (`{"NAME": {"earned": true}}`) or `achievements.ini` (`[NAME]` with `Achieved=1`), in
//!   the user's AppData / Public Documents or under the game's folder. Unlocked ÷ total, where the total is the
//!   folder's `steam_settings\achievements.json` (a list) or the Steam store's count.
//! - Else playtime ÷ the game's typical main-story length (HowLongToBeat, `hltb.rs`), at most 99 %: only the player
//!   knows when the story is done.
//! - The journal's own `progress` (set by the player) always wins; the page shows this one only while that is unset.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Achievements,
    Playtime,
}

/// What the app worked out. `unlocked`/`total` for achievements, `typical_sec` for playtime.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Auto {
    pub percent: u8,
    pub source: Source,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unlocked: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub typical_sec: Option<u64>,
}

/// Unlocks found on disk for a game: how many, and the total its folder lists (if it does).
#[derive(Debug, PartialEq)]
pub struct Unlocks {
    pub unlocked: u32,
    pub folder_total: Option<u32>,
}

/// How deep under the game's folder to look for `steam_appid.txt` and achievement files.
const DEPTH: usize = 4;
/// Folders under the game's folder never worth walking (big, and never hold the files).
const SKIP: [&str; 6] = ["_commonredist", "redist", "movies", "videos", "paks", "content"];

/// The game's unlocks, or `None` when no achievement file was found (then playtime is used).
pub fn unlocks(folder: &Path, store_id: Option<u64>) -> Option<Unlocks> {
    let found = walk(folder);
    let mut ids = found.app_ids;
    ids.extend(store_id);
    ids.sort_unstable();
    ids.dedup();
    let mut states: Vec<PathBuf> = found.states;
    for id in &ids {
        states.extend(user_files(*id).into_iter().filter(|p| p.is_file()));
    }
    // the file written last is the one the game uses now
    let newest = states.into_iter().filter_map(|p| Some((std::fs::metadata(&p).ok()?.modified().ok()?, p))).max()?.1;
    let unlocked = count_unlocked(&newest)?;
    Some(Unlocks { unlocked, folder_total: found.total })
}

/// Unlocked ÷ total as a whole percent, at most 100. `None` without a total.
pub fn from_achievements(unlocked: u32, total: Option<u32>) -> Option<Auto> {
    let total = total.filter(|&t| t > 0)?;
    let percent = ((f64::from(unlocked.min(total)) * 100.0 / f64::from(total)).round()) as u8;
    Some(Auto { percent, source: Source::Achievements, unlocked: Some(unlocked.min(total)), total: Some(total), typical_sec: None })
}

/// Playtime ÷ the typical main-story length, at most 99 %. `None` before any play or without a length.
pub fn from_playtime(played_sec: u64, typical_sec: Option<u64>) -> Option<Auto> {
    let typical = typical_sec.filter(|&t| t > 0)?;
    if played_sec == 0 {
        return None;
    }
    let percent = ((played_sec as f64 * 100.0 / typical as f64).round() as u64).min(99) as u8;
    Some(Auto { percent, source: Source::Playtime, unlocked: None, total: None, typical_sec: Some(typical) })
}

/// Where Steam layers outside the game's folder keep a game's unlocks.
fn user_files(id: u64) -> Vec<PathBuf> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let mut out = Vec::new();
    if let Some(appdata) = env("APPDATA") {
        out.push(appdata.join("Goldberg SteamEmu Saves").join(id.to_string()).join("achievements.json"));
        out.push(appdata.join("GSE Saves").join(id.to_string()).join("achievements.json"));
        out.push(appdata.join("Steam").join("CODEX").join(id.to_string()).join("achievements.ini"));
    }
    if let Some(public) = env("PUBLIC") {
        let docs = public.join("Documents");
        out.push(docs.join("Steam").join("CODEX").join(id.to_string()).join("achievements.ini"));
        out.push(docs.join("Steam").join("RUNE").join(id.to_string()).join("achievements.ini"));
        out.push(docs.join("OnlineFix").join(id.to_string()).join("Stats").join("Achievements.ini"));
    }
    out
}

#[derive(Default)]
struct Walked {
    app_ids: Vec<u64>,
    /// unlock files under the folder (a local save)
    states: Vec<PathBuf>,
    /// the length of `steam_settings\achievements.json`
    total: Option<u32>,
}

fn walk(folder: &Path) -> Walked {
    let mut w = Walked::default();
    let mut dirs = vec![(folder.to_path_buf(), 0)];
    while let Some((dir, depth)) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let Ok(kind) = e.file_type() else { continue };
            let name = e.file_name().to_string_lossy().to_lowercase();
            let path = e.path();
            if kind.is_dir() {
                if depth < DEPTH && !SKIP.contains(&name.as_str()) {
                    dirs.push((path, depth + 1));
                }
                continue;
            }
            match name.as_str() {
                "steam_appid.txt" => {
                    if let Some(id) = std::fs::read_to_string(&path).ok().and_then(|t| t.trim().parse().ok()) {
                        w.app_ids.push(id);
                    }
                }
                "achievements.json" => match read_json(&path) {
                    // a list is the schema (every achievement); an object is the unlocks
                    Some(Value::Array(list)) => w.total = Some(w.total.unwrap_or(0).max(list.len() as u32)),
                    Some(Value::Object(_)) => w.states.push(path),
                    _ => {}
                },
                "achievements.ini" => w.states.push(path),
                _ => {}
            }
        }
    }
    w
}

fn read_json(path: &Path) -> Option<Value> {
    let text = std::fs::read(path).ok()?;
    serde_json::from_slice(strip_bom(&text)).ok()
}

fn strip_bom(b: &[u8]) -> &[u8] {
    b.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(b)
}

/// Unlocked achievements in a `.json` or `.ini` unlock file.
fn count_unlocked(path: &Path) -> Option<u32> {
    let is_json = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("json"));
    if is_json {
        let Value::Object(map) = read_json(path)? else { return None };
        return Some(map.values().filter(|v| v["earned"].as_bool() == Some(true) || v["earned"] == 1).count() as u32);
    }
    let text = String::from_utf8_lossy(strip_bom(&std::fs::read(path).ok()?)).into_owned();
    Some(count_ini(&text))
}

/// `[NAME]` sections with `Achieved=1` / `achieved=true` / `unlocked=1`; `[SteamAchievements]` is only an index.
fn count_ini(text: &str) -> u32 {
    let (mut section, mut n) = (String::new(), 0);
    for line in text.lines().map(str::trim) {
        if let Some(s) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = s.to_lowercase();
            continue;
        }
        if section.is_empty() || section == "steamachievements" || section == "steam" {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let (k, v) = (k.trim().to_lowercase(), v.trim().to_lowercase());
        if matches!(k.as_str(), "achieved" | "unlocked" | "earned") && matches!(v.as_str(), "1" | "true") {
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("padwake-progress-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn percent_from_achievements_and_playtime() {
        assert_eq!(from_achievements(12, Some(51)).unwrap().percent, 24);
        assert_eq!(from_achievements(60, Some(51)).unwrap().percent, 100, "a stale schema can't go over 100");
        assert_eq!(from_achievements(3, None), None);
        assert_eq!(from_achievements(3, Some(0)), None);
        assert_eq!(from_playtime(13 * 3600, Some(26 * 3600)).unwrap().percent, 50);
        assert_eq!(from_playtime(90 * 3600, Some(26 * 3600)).unwrap().percent, 99, "only the player says it's done");
        assert_eq!(from_playtime(0, Some(3600)), None);
        assert_eq!(from_playtime(3600, None), None);
    }

    #[test]
    fn ini_counts_achieved_sections_only() {
        let codex = "[SteamAchievements]\nCount=3\n00000=A\n00001=B\n\n[A]\nAchieved=1\nUnlockTime=1700000000\n[B]\nAchieved=0\n[C]\nachieved=true\n";
        assert_eq!(count_ini(codex), 2);
        assert_eq!(count_ini(""), 0);
    }

    #[test]
    fn a_local_save_and_schema_under_the_folder() {
        let game = temp("local");
        std::fs::write(game.join("steam_appid.txt"), "999999999\n").unwrap();
        let settings = game.join("bin").join("steam_settings");
        std::fs::create_dir_all(&settings).unwrap();
        std::fs::write(settings.join("achievements.json"), r#"[{"name":"A"},{"name":"B"},{"name":"C"},{"name":"D"}]"#).unwrap();
        let save = game.join("bin").join("saves").join("999999999");
        std::fs::create_dir_all(&save).unwrap();
        std::fs::write(save.join("achievements.json"), "\u{feff}{\"A\":{\"earned\":true,\"earned_time\":1},\"B\":{\"earned\":false}}").unwrap();
        assert_eq!(unlocks(&game, None), Some(Unlocks { unlocked: 1, folder_total: Some(4) }));
        let _ = std::fs::remove_dir_all(&game);
    }

    #[test]
    fn no_files_no_unlocks() {
        let game = temp("none");
        std::fs::write(game.join("game.exe"), b"MZ").unwrap();
        assert_eq!(unlocks(&game, None), None);
        let _ = std::fs::remove_dir_all(&game);
    }
}
