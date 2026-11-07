//! Each user's event log (row B1b): `users\<id>\events.jsonl`, one dated line per thing that happened outside a
//! session, oldest first. Journal status set or cleared, rating changed, note edited (never its text), user switched
//! to, Padwake started and stopped. Crash-safe like the session log (`jsonl.rs`), and in the GitHub backup
//! (`portable.rs`), where two logs merge without doubles: a line is the same event if all of it is the same.

use super::{jsonl, Journal, Status};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// One event. `at` is Unix milliseconds.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub at: u64,
    #[serde(flatten)]
    pub what: What,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum What {
    /// a journal status set, or cleared (`status` null)
    Status { game: String, status: Option<Status> },
    /// stars set (1–5), or cleared (null)
    Rating { game: String, rating: Option<u8> },
    /// the note edited: whether there is one now (its text is never logged)
    Note { game: String, note: bool },
    /// this user became the one playing
    User {},
    /// Padwake started; `restarted` after a crash
    Started { v: String, restarted: bool },
    Stopped {},
}

impl Event {
    fn key(&self) -> (u64, String) {
        (self.at, serde_json::to_string(&self.what).unwrap_or_default())
    }
}

/// `user`'s log.
pub fn file(user: &str) -> PathBuf {
    crate::users::dir_of(user).join("events.jsonl")
}

/// What a journal change logs: one event per field that changed, the note only as "there is one" or not.
pub fn journal_changes(game: &str, before: &Journal, after: &Journal, at: u64) -> Vec<Event> {
    let mut out = Vec::new();
    let game = game.to_string();
    if before.status != after.status {
        out.push(Event { at, what: What::Status { game: game.clone(), status: after.status } });
    }
    if before.rating != after.rating {
        out.push(Event { at, what: What::Rating { game: game.clone(), rating: after.rating } });
    }
    if before.note != after.note {
        out.push(Event { at, what: What::Note { game, note: after.note.is_some() } });
    }
    out
}

pub fn append(path: &Path, e: &Event) -> Result<(), String> {
    jsonl::append(path, e)
}

pub fn read(path: &Path) -> Vec<Event> {
    jsonl::read(path)
}

/// Both logs as one, oldest first; an event in both is there once.
pub fn merge(a: &[Event], b: &[Event]) -> Vec<Event> {
    jsonl::merge(a, b, Event::key)
}

/// Merges a backup's events into the log at `path`. Returns how many it gained.
pub fn absorb(path: &Path, other: &[Event]) -> Result<usize, String> {
    jsonl::absorb(path, other, Event::key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("padwake-events-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("events.jsonl")
    }

    fn line(e: &Event) -> String {
        serde_json::to_string(e).unwrap()
    }

    #[test]
    fn each_event_is_one_dated_line() {
        let ev = |what| Event { at: 5, what };
        let cases = [
            (ev(What::Status { game: "hades".into(), status: Some(Status::Finished) }), r#"{"at":5,"type":"status","game":"hades","status":"finished"}"#),
            (ev(What::Status { game: "hades".into(), status: None }), r#"{"at":5,"type":"status","game":"hades","status":null}"#),
            (ev(What::Rating { game: "hades".into(), rating: Some(4) }), r#"{"at":5,"type":"rating","game":"hades","rating":4}"#),
            (ev(What::Rating { game: "hades".into(), rating: None }), r#"{"at":5,"type":"rating","game":"hades","rating":null}"#),
            (ev(What::Note { game: "hades".into(), note: true }), r#"{"at":5,"type":"note","game":"hades","note":true}"#),
            (ev(What::User {}), r#"{"at":5,"type":"user"}"#),
            (ev(What::Started { v: "0.3.4".into(), restarted: false }), r#"{"at":5,"type":"started","v":"0.3.4","restarted":false}"#),
            (ev(What::Stopped {}), r#"{"at":5,"type":"stopped"}"#),
        ];
        for (e, want) in cases {
            assert_eq!(line(&e), want);
            assert_eq!(serde_json::from_str::<Event>(want).unwrap(), e);
        }
    }

    #[test]
    fn a_journal_change_logs_what_changed_and_never_the_note() {
        let before = Journal { rating: Some(3), status: Some(Status::Playing), note: None };
        let after = Journal { rating: Some(3), status: Some(Status::Finished), note: Some("loved the ending, secret: 42".into()) };
        let events = journal_changes("hades", &before, &after, 9);
        let lines: Vec<String> = events.iter().map(line).collect();
        assert_eq!(lines, [r#"{"at":9,"type":"status","game":"hades","status":"finished"}"#, r#"{"at":9,"type":"note","game":"hades","note":true}"#]);
        assert!(!lines.concat().contains("ending"));
        // cleared: status null, rating null, note false
        let events = journal_changes("hades", &after, &Journal::default(), 10);
        let kinds: Vec<_> = events.iter().map(|e| e.what.clone()).collect();
        assert_eq!(
            kinds,
            [
                What::Status { game: "hades".into(), status: None },
                What::Rating { game: "hades".into(), rating: None },
                What::Note { game: "hades".into(), note: false }
            ]
        );
        assert!(journal_changes("hades", &after, &after, 11).is_empty(), "nothing changed, nothing logged");
    }

    #[test]
    fn a_backup_merge_never_doubles_or_drops_events() {
        let path = temp("merge");
        let a = Event { at: 1_000, what: What::Started { v: "0.3.4".into(), restarted: false } };
        let b = Event { at: 2_000, what: What::Rating { game: "hades".into(), rating: Some(5) } };
        let c = Event { at: 2_000, what: What::Status { game: "hades".into(), status: Some(Status::Playing) } };
        let d = Event { at: 1_500, what: What::User {} };
        for e in [&a, &b, &c] {
            append(&path, e).unwrap();
        }
        // the other PC: one event both have, one only it has
        let there = [b.clone(), d.clone()];
        assert_eq!(absorb(&path, &there).unwrap(), 1);
        let log = read(&path);
        assert_eq!(log.len(), 4);
        assert_eq!(log.iter().map(|e| e.at).collect::<Vec<_>>(), [1_000, 1_500, 2_000, 2_000], "oldest first");
        assert!(log.contains(&b) && log.contains(&c), "two events at the same time both stay");
        assert_eq!(absorb(&path, &there).unwrap(), 0, "the same backup again changes nothing");
        assert_eq!(merge(&log, &there), log);
        assert_eq!(merge(&there, &[a, b, c]), log);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
