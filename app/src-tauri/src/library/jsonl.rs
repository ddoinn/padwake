//! Append-only logs of one JSON value per line: each user's session log (`sessions.rs`) and event log (`events.rs`).
//! - Crash-safe without a rewrite: a line is written whole and synced. A line a crash cut short doesn't parse and is
//!   skipped when read, and the next append starts on a new line after it.
//! - A backup's copy merges in by a key, so a line is never there twice and none from either PC is lost.

use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Mutex;

/// One writer or reader of a log at a time (a session ends while a restore merges).
static LOCK: Mutex<()> = Mutex::new(());

/// Adds one line to the end of the log at `path`.
pub fn append<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let _one = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let at = |e: std::io::Error| format!("{}: {e}", path.display());
    let mut f = std::fs::OpenOptions::new().create(true).read(true).append(true).open(path).map_err(at)?;
    let mut line = serde_json::to_string(value).map_err(|e| e.to_string())?;
    line.push('\n');
    // a line a crash cut short gets its own line, so this one isn't glued onto it
    if f.seek(SeekFrom::End(0)).map_err(at)? > 0 {
        let mut last = [0u8];
        f.seek(SeekFrom::End(-1)).map_err(at)?;
        f.read_exact(&mut last).map_err(at)?;
        if last[0] != b'\n' {
            line.insert(0, '\n');
        }
    }
    f.write_all(line.as_bytes()).map_err(at)?;
    f.sync_data().map_err(at)
}

/// The log at `path`, oldest first. Lines that don't parse (cut short by a crash) are skipped; no log is empty.
pub fn read<T: DeserializeOwned>(path: &Path) -> Vec<T> {
    let _one = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    read_unlocked(path)
}

fn read_unlocked<T: DeserializeOwned>(path: &Path) -> Vec<T> {
    let text = std::fs::read(path).unwrap_or_default();
    String::from_utf8_lossy(&text).lines().filter_map(|l| serde_json::from_str(l.trim()).ok()).collect()
}

/// Both logs as one, in key order: lines with the same key are there once (the first one seen).
pub fn merge<T: Clone, K: Ord>(a: &[T], b: &[T], key: impl Fn(&T) -> K) -> Vec<T> {
    let mut all: BTreeMap<K, T> = BTreeMap::new();
    for x in a.iter().chain(b) {
        all.entry(key(x)).or_insert_with(|| x.clone());
    }
    all.into_values().collect()
}

/// Merges `other` (a backup's log) into the log at `path`, which is rewritten whole only if it gains lines. Returns
/// how many it gained.
pub fn absorb<T, K>(path: &Path, other: &[T], key: impl Fn(&T) -> K) -> Result<usize, String>
where
    T: Serialize + DeserializeOwned + Clone,
    K: Ord,
{
    let _one = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let here: Vec<T> = read_unlocked(path);
    let before = merge(&here, &[], &key).len();
    let merged = merge(&here, other, &key);
    let gained = merged.len() - before;
    if gained == 0 {
        return Ok(0);
    }
    let mut text = String::new();
    for x in &merged {
        text.push_str(&serde_json::to_string(x).map_err(|e| e.to_string())?);
        text.push('\n');
    }
    super::write_file(path, text.as_bytes())?;
    Ok(gained)
}
