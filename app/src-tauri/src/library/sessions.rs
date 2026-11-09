//! Each user's session log (row B1): `users\<id>\sessions.jsonl`, one line per finished session, oldest first. The
//! monthly and yearly recaps (row B4) are worked out from it; `play.json` keeps only the totals.
//! - A line is appended where `record_session` adds the time, so it goes to whoever started the game (no user switches
//!   while a game runs).
//! - Crash-safe without a rewrite: a line is written whole and synced. A line a crash cut short doesn't parse and is
//!   skipped when read, and the next append starts on a new line after it.
//! - The log is in the GitHub backup (`portable.rs`). Two logs merge by game id and start time, so a session is never
//!   counted twice and none from either PC is lost.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// One finished session. Times are Unix milliseconds.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(default)]
pub struct Session {
    /// the game's id in the library
    pub game: String,
    pub start: u64,
    pub end: u64,
    pub secs: u64,
}

impl Session {
    /// A session of `secs` that ended at `end`.
    pub fn ended(game: &str, secs: u64, end: u64) -> Session {
        Session { game: game.to_string(), start: end.saturating_sub(secs * 1000), end, secs }
    }

    fn key(&self) -> (u64, String) {
        (self.start, self.game.clone())
    }
}

/// One writer or reader of a log at a time (a session ends while a restore merges).
static LOCK: Mutex<()> = Mutex::new(());

/// `user`'s log.
pub fn file(user: &str) -> PathBuf {
    crate::users::dir_of(user).join("sessions.jsonl")
}

/// Adds one session to the end of the log at `path`.
pub fn append(path: &Path, s: &Session) -> Result<(), String> {
    let _one = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let at = |e: std::io::Error| format!("{}: {e}", path.display());
    let mut f = std::fs::OpenOptions::new().create(true).read(true).append(true).open(path).map_err(at)?;
    let mut line = serde_json::to_string(s).map_err(|e| e.to_string())?;
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
pub fn read(path: &Path) -> Vec<Session> {
    let _one = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    read_unlocked(path)
}

fn read_unlocked(path: &Path) -> Vec<Session> {
    let text = std::fs::read(path).unwrap_or_default();
    String::from_utf8_lossy(&text).lines().filter_map(|l| serde_json::from_str(l.trim()).ok()).collect()
}

/// Both logs as one, oldest first: a session in both (same game and start) is there once.
pub fn merge(a: &[Session], b: &[Session]) -> Vec<Session> {
    let mut all: BTreeMap<(u64, String), Session> = BTreeMap::new();
    for s in a.iter().chain(b) {
        all.entry(s.key()).or_insert_with(|| s.clone());
    }
    all.into_values().collect()
}

/// Merges `other` (a backup's log) into the log at `path`, which is rewritten whole only if it gains sessions.
/// Returns how many it gained.
pub fn absorb(path: &Path, other: &[Session]) -> Result<usize, String> {
    let _one = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let here = read_unlocked(path);
    let merged = merge(&here, other);
    let gained = merged.len() - merge(&here, &[]).len();
    if gained == 0 {
        return Ok(0);
    }
    let mut text = String::new();
    for s in &merged {
        text.push_str(&serde_json::to_string(s).map_err(|e| e.to_string())?);
        text.push('\n');
    }
    super::write_file(path, text.as_bytes())?;
    Ok(gained)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("padwake-sessions-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("sessions.jsonl")
    }

    #[test]
    fn a_session_starts_its_seconds_before_it_ended() {
        let s = Session::ended("hades", 90, 1_793_196_309_000);
        assert_eq!((s.start, s.end, s.secs), (1_793_196_219_000, 1_793_196_309_000, 90));
        let line = serde_json::to_string(&s).unwrap();
        assert_eq!(line, r#"{"game":"hades","start":1793196219000,"end":1793196309000,"secs":90}"#);
    }

    #[test]
    fn appends_survive_a_crash_mid_write() {
        let path = temp("crash");
        let a = Session::ended("hades", 60, 10_000_000);
        append(&path, &a).unwrap();
        // a crash cut the next line short: no newline, not valid JSON
        let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(br#"{"game":"celeste","start":20000"#).unwrap();
        drop(f);
        assert_eq!(read(&path), [a.clone()], "the broken line isn't read back");

        let b = Session::ended("celeste", 30, 30_000_000);
        append(&path, &b).unwrap();
        assert_eq!(read(&path), [a, b], "the next session isn't glued onto the broken line");
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 3, "{text}");
        assert!(text.ends_with('\n'));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn no_log_reads_empty() {
        assert!(read(&temp("none")).is_empty());
    }

    #[test]
    fn a_backup_merge_never_doubles_or_drops_sessions() {
        let path = temp("merge");
        let here = [Session::ended("hades", 60, 1_000_000), Session::ended("celeste", 30, 5_000_000)];
        for s in &here {
            append(&path, s).unwrap();
        }
        // the other PC's log: one session both PCs have (restored before), one only it has
        let there = [Session::ended("celeste", 30, 5_000_000), Session::ended("sekiro", 600, 3_000_000)];
        assert_eq!(absorb(&path, &there).unwrap(), 1);
        let log = read(&path);
        let games: Vec<_> = log.iter().map(|s| s.game.as_str()).collect();
        assert_eq!(games, ["hades", "sekiro", "celeste"], "each once, oldest first");
        // the same backup again changes nothing
        assert_eq!(absorb(&path, &there).unwrap(), 0);
        assert_eq!(read(&path), log);
        // what goes up: this PC's log with the other's, each session once
        assert_eq!(merge(&log, &there), log);
        assert_eq!(merge(&there, &here), log);
        // two games can't share a start, but if they did, both stay
        let same_start = [Session { game: "gris".into(), ..here[0].clone() }];
        assert_eq!(merge(&log, &same_start).len(), 4);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
