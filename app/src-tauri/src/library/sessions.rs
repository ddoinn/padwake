//! Each user's session log (rows B1, B1b): `users\<id>\sessions.jsonl`, one line per finished session, oldest first.
//! The monthly and yearly recaps (row B4) and later stats are worked out from it; `play.json` keeps only the totals.
//! - A game's line is appended where `record_session` adds the time, so it goes to whoever started the game (no user
//!   switches while a game runs). A media app's line (`"kind":"media"`, `app`) adds no time.
//! - Crash-safe without a rewrite (`jsonl.rs`).
//! - Row B1b adds optional fields (where the time went, idle, launch, how it ended, CPU and memory, pad, display and
//!   power at start and end, Guide opens, screenshots, trophies, achievements, version). Lines without them still
//!   read. Nothing here is a key, a button, a window title, search text or note text.
//! - While a game runs, `session-open.json` beside the log holds the line so far, updated once a minute. If Padwake
//!   restarts mid-game, the next start turns it into a line that ended "restart" (`library::recover_open_session`).
//! - The log is in the GitHub backup (`portable.rs`). Two logs merge by game (or app) and start time, so a session is
//!   never counted twice and none from either PC is lost.

use super::jsonl;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// One finished session. Times are Unix milliseconds.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct Session {
    /// the game's id in the library (empty for a media app)
    #[serde(skip_serializing_if = "String::is_empty")]
    pub game: String,
    pub start: u64,
    pub end: u64,
    pub secs: u64,
    /// "media" for a media app; absent for a game
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// the media app's id (`media.json`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    /// the Padwake version that logged it
    #[serde(skip_serializing_if = "Option::is_none")]
    pub v: Option<String>,
    /// where it was launched from: home | library | search | switch
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// how it ended: quit | close (Close Game) | crash (a non-zero exit) | restart (Padwake restarted meanwhile)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended: Option<String>,
    /// the exit code, when it crashed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit: Option<u32>,
    /// from launch to its first window in front
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_ms: Option<u64>,
    /// where the time went
    #[serde(skip_serializing_if = "Option::is_none")]
    pub front: Option<Front>,
    /// time in stretches of 5+ min with no pad, keyboard or mouse input
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idle_secs: Option<u64>,
    /// CPU time of the session's job (processes outside it aren't counted)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_ms: Option<u64>,
    /// the job's peak memory (committed), in MB
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak_mb: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at_start: Option<Snapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at_end: Option<Snapshot>,
    /// how often Guide brought Padwake up over it
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guide: Option<u32>,
    /// screenshots taken (Guide + View)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screenshots: Option<u32>,
    /// Padwake trophies earned by this session (ids, as `ui/journal/trophies.js`)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub trophies: Vec<String>,
    /// unlocked achievements before and after, when the game has an unlock file (`progress.rs`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub achievements: Option<[u32; 2]>,
}

/// Seconds of a session by what was in front.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct Front {
    pub game: u64,
    /// any Padwake screen over the game but Control Center
    pub home: u64,
    pub control_center: u64,
    /// another app's window
    pub other: u64,
}

/// The PC as a session started or ended.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(default)]
pub struct Snapshot {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pad: Option<Pad>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display: Option<Display>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub power: Option<Power>,
}

/// The pad last used (or the first connected).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(default)]
pub struct Pad {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// wired | wireless | receiver
    pub connection: String,
    /// empty | low | medium | full; absent when wired or unknown
    #[serde(skip_serializing_if = "Option::is_none")]
    pub battery: Option<String>,
}

/// The primary display's mode.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(default)]
pub struct Display {
    pub width: u32,
    pub height: u32,
    pub hz: u32,
    pub hdr: bool,
}

/// Mains or battery.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(default)]
pub struct Power {
    pub ac: bool,
    /// percent; absent without a battery
    #[serde(skip_serializing_if = "Option::is_none")]
    pub battery: Option<u8>,
}

impl Session {
    /// A game session of `secs` that ended at `end`.
    pub fn ended(game: &str, secs: u64, end: u64) -> Session {
        Session { game: game.to_string(), start: end.saturating_sub(secs * 1000), end, secs, ..Default::default() }
    }

    fn key(&self) -> (u64, String, String) {
        (self.start, self.game.clone(), self.app.clone().unwrap_or_default())
    }

    /// The same session (game or app, and start), however much of it each copy knows.
    pub fn same_session(&self, other: &Session) -> bool {
        self.key() == other.key()
    }
}

/// Padwake's six trophies (`ui/journal/trophies.js`, keep them the same): an id and what earns it, from the game's
/// total seconds and sessions.
const TROPHIES: [(&str, fn(u64, u32) -> bool); 6] = [
    ("first-steps", |secs, _| secs >= 60),
    ("regular", |_, n| n >= 10),
    ("invested", |secs, _| secs >= 10 * 3600),
    ("old-friend", |_, n| n >= 50),
    ("devoted", |secs, _| secs >= 50 * 3600),
    ("centurion", |secs, _| secs >= 100 * 3600),
];

/// The trophies a session earned: those earned with the totals after it and not before. Totals are (seconds, sessions).
pub fn trophies_earned(before: (u64, u32), after: (u64, u32)) -> Vec<String> {
    TROPHIES.iter().filter(|(_, earned)| earned(after.0, after.1) && !earned(before.0, before.1)).map(|(id, _)| id.to_string()).collect()
}

/// `user`'s log.
pub fn file(user: &str) -> PathBuf {
    crate::users::dir_of(user).join("sessions.jsonl")
}

/// `user`'s session under way (the line so far).
pub fn open_file(user: &str) -> PathBuf {
    crate::users::dir_of(user).join("session-open.json")
}

/// Adds one session to the end of the log at `path`.
pub fn append(path: &Path, s: &Session) -> Result<(), String> {
    jsonl::append(path, s)
}

/// The log at `path`, oldest first. Lines that don't parse (cut short by a crash) are skipped; no log is empty.
pub fn read(path: &Path) -> Vec<Session> {
    jsonl::read(path)
}

/// Both logs as one, oldest first: a session in both (same game or app, and start) is there once.
pub fn merge(a: &[Session], b: &[Session]) -> Vec<Session> {
    jsonl::merge(a, b, Session::key)
}

/// Merges `other` (a backup's log) into the log at `path`, which is rewritten whole only if it gains sessions.
/// Returns how many it gained.
pub fn absorb(path: &Path, other: &[Session]) -> Result<usize, String> {
    jsonl::absorb(path, other, Session::key)
}

/// Keeps the session under way at `path` (written whole, so a crash leaves the last copy).
pub fn save_open(path: &Path, s: &Session) -> Result<(), String> {
    super::write_file(path, &serde_json::to_vec(s).map_err(|e| e.to_string())?)
}

/// The session that was under way at `path` when Padwake stopped, if any; the file goes.
pub fn take_open(path: &Path) -> Option<Session> {
    let text = std::fs::read(path).ok()?;
    let _ = std::fs::remove_file(path);
    serde_json::from_slice(&text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

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
        // nor can a game and a media app, but both would stay too
        let media = [Session { game: String::new(), kind: Some("media".into()), app: Some("youtube".into()), ..here[0].clone() }];
        assert_eq!(merge(&log, &media).len(), 4);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// Every B1b field, as a line: the names B4 and later stats read. A field that isn't known is absent, not null.
    #[test]
    fn every_field_round_trips_with_its_name() {
        let pad = |battery: Option<&str>| Pad { name: Some("8BitDo Ultimate 2C Wireless Controller".into()), connection: "receiver".into(), battery: battery.map(Into::into) };
        let s = Session {
            v: Some("0.3.4".into()),
            from: Some("library".into()),
            ended: Some("crash".into()),
            exit: Some(3221225477),
            launch_ms: Some(812),
            front: Some(Front { game: 50, home: 5, control_center: 3, other: 2 }),
            idle_secs: Some(0),
            cpu_ms: Some(1250),
            peak_mb: Some(412),
            at_start: Some(Snapshot {
                pad: Some(pad(Some("full"))),
                display: Some(Display { width: 1920, height: 1080, hz: 60, hdr: false }),
                power: Some(Power { ac: false, battery: Some(87) }),
            }),
            at_end: Some(Snapshot { pad: Some(pad(None)), display: None, power: Some(Power { ac: true, battery: None }) }),
            guide: Some(2),
            screenshots: Some(1),
            trophies: vec!["first-steps".into()],
            achievements: Some([3, 5]),
            ..Session::ended("hades", 60, 1_793_196_309_000)
        };
        let line = serde_json::to_string(&s).unwrap();
        for part in [
            r#""v":"0.3.4""#,
            r#""from":"library""#,
            r#""ended":"crash""#,
            r#""exit":3221225477"#,
            r#""launchMs":812"#,
            r#""front":{"game":50,"home":5,"controlCenter":3,"other":2}"#,
            r#""idleSecs":0"#,
            r#""cpuMs":1250"#,
            r#""peakMb":412"#,
            r#""atStart":{"pad":{"name":"8BitDo Ultimate 2C Wireless Controller","connection":"receiver","battery":"full"},"display":{"width":1920,"height":1080,"hz":60,"hdr":false},"power":{"ac":false,"battery":87}}"#,
            r#""atEnd":{"pad":{"name":"8BitDo Ultimate 2C Wireless Controller","connection":"receiver"},"power":{"ac":true}}"#,
            r#""guide":2"#,
            r#""screenshots":1"#,
            r#""trophies":["first-steps"]"#,
            r#""achievements":[3,5]"#,
        ] {
            assert!(line.contains(part), "{part} in {line}");
        }
        assert!(!line.contains("null"), "{line}");
        assert_eq!(serde_json::from_str::<Session>(&line).unwrap(), s);
        // a B1 line reads with none of them
        let old: Session = serde_json::from_str(r#"{"game":"hades","start":1,"end":5001,"secs":5}"#).unwrap();
        assert_eq!(old, Session::ended("hades", 5, 5001));
    }

    #[test]
    fn a_media_line_has_its_app_and_no_game() {
        let s = Session { game: String::new(), kind: Some("media".into()), app: Some("youtube".into()), ..Session::ended("", 30, 90_000) };
        let line = serde_json::to_string(&s).unwrap();
        assert_eq!(line, r#"{"start":60000,"end":90000,"secs":30,"kind":"media","app":"youtube"}"#);
    }

    #[test]
    fn trophies_are_the_ones_a_session_crossed() {
        assert_eq!(trophies_earned((0, 0), (59, 1)), Vec::<String>::new());
        assert_eq!(trophies_earned((0, 0), (60, 1)), ["first-steps"]);
        assert_eq!(trophies_earned((60, 1), (120, 2)), Vec::<String>::new(), "had it already");
        assert_eq!(trophies_earned((9 * 3600, 9), (10 * 3600, 10)), ["regular", "invested"]);
        assert_eq!(trophies_earned((99 * 3600, 49), (100 * 3600, 50)), ["old-friend", "centurion"]);
        // the same six as the page's table
        let js = include_str!("../../../ui/journal/trophies.js");
        for (id, _) in TROPHIES {
            assert!(js.contains(&format!("id: '{id}'")), "{id} is in trophies.js");
        }
        assert_eq!(js.matches("{ id: '").count(), TROPHIES.len());
    }

    #[test]
    fn the_open_session_is_taken_once() {
        let path = temp("open").with_file_name("session-open.json");
        assert_eq!(take_open(&path), None);
        let s = Session { from: Some("home".into()), ..Session::ended("hades", 120, 9_000_000) };
        save_open(&path, &s).unwrap();
        save_open(&path, &Session { secs: 180, ..s.clone() }).unwrap();
        assert_eq!(take_open(&path).map(|s| s.secs), Some(180), "the last copy");
        assert_eq!(take_open(&path), None, "and it's gone");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
