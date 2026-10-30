//! The library as a backup (the cloud area keeps it in a private gist): what can't be fetched again, per game. That
//! is the time, sessions and dates the app recorded and the player's journal; art, media and paths stay on the PC.
//! A restore merges a backup into the library without losing or doubling anything:
//! - a game here (same id, folder name or Steam app) takes the larger time and sessions, the earlier first play and
//!   the later last play; its journal comes from the backup only if it has none here;
//! - a played or reviewed game that isn't here joins the journal as "from your backup" (`restoredAt`), and gets its
//!   time and journal back when a folder of the same name appears in the games folder.

use super::{Game, Journal};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub const FORMAT: u32 = 1;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Backup {
    /// "padwake", so a stray file isn't read as a backup
    pub app: String,
    pub format: u32,
    pub saved_at: u64,
    /// the PC it came from
    pub machine: String,
    pub games: Vec<Entry>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Entry {
    pub id: String,
    pub name: String,
    /// the game's folder name in the games folder
    pub folder_name: String,
    pub steam_app_id: Option<u64>,
    pub playtime_sec: u64,
    pub sessions: u32,
    pub first_played: Option<u64>,
    pub last_played: Option<u64>,
    pub added_at: Option<u64>,
    pub removed_at: Option<u64>,
    pub journal: Journal,
}

impl Entry {
    fn worth_keeping(&self) -> bool {
        self.playtime_sec > 0 || self.last_played.is_some() || !self.journal.is_empty()
    }
}

fn folder_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.to_string_lossy().into_owned())
}

/// The backup of `games` (the library) and `kept` (removed games in the journal). Content only: `saved_at` and
/// `machine` are set by the caller, so an unchanged library gives the same games list.
pub fn export(games: &[Game], kept: &[Game]) -> Vec<Entry> {
    games
        .iter()
        .chain(kept)
        .map(|g| Entry {
            id: g.id.clone(),
            name: g.name.clone(),
            folder_name: folder_name(&g.folder),
            steam_app_id: g.steam_app_id,
            playtime_sec: g.playtime_sec,
            sessions: g.sessions,
            first_played: g.first_played,
            last_played: g.last_played,
            added_at: g.added_at,
            removed_at: g.removed_at,
            journal: g.journal.clone(),
        })
        .collect()
}

/// What a restore did.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Restored {
    /// games here that got time, sessions or a journal from the backup
    pub updated: u32,
    /// games that joined the journal from the backup
    pub added: u32,
}

/// Merges a backup's entries into the library (`games`) and the journal's removed games (`kept`). `now` stamps the
/// entries that join.
pub fn merge(games: &mut [Game], kept: &mut Vec<Game>, entries: &[Entry], now: u64) -> Restored {
    let mut r = Restored::default();
    let mut taken: HashSet<String> = games.iter().chain(kept.iter()).map(|g| g.id.clone()).collect();
    for e in entries.iter().filter(|e| e.worth_keeping()) {
        let same = |g: &Game| {
            g.id == e.id
                || folder_name(&g.folder).eq_ignore_ascii_case(&e.folder_name)
                || (e.steam_app_id.is_some() && g.steam_app_id == e.steam_app_id)
        };
        let here = match games.iter().position(|g| same(g)) {
            Some(i) => Some(&mut games[i]),
            None => kept.iter_mut().find(|g| same(g)),
        };
        if let Some(g) = here {
            if take(g, e) {
                r.updated += 1;
            }
            continue;
        }
        let mut id = e.id.clone();
        let mut n = 2;
        while taken.contains(&id) {
            id = format!("{}-{n}", e.id);
            n += 1;
        }
        taken.insert(id.clone());
        kept.push(Game {
            id,
            name: e.name.clone(),
            folder: PathBuf::from(&e.folder_name),
            steam_app_id: e.steam_app_id,
            playtime_sec: e.playtime_sec,
            sessions: e.sessions,
            first_played: e.first_played,
            last_played: e.last_played,
            added_at: e.added_at,
            removed_at: Some(e.removed_at.unwrap_or(now)),
            restored_at: Some(now),
            journal: e.journal.clone(),
            ..Default::default()
        });
        r.added += 1;
    }
    r
}

/// What to upload: this PC's entries, each with what `remote` (the backup there now) knows better, plus the remote's
/// played or reviewed games this PC doesn't have. So a backup never loses another PC's games, or time this PC hasn't
/// restored.
pub fn combine(local: Vec<Entry>, remote: &[Entry]) -> Vec<Entry> {
    let mut games: Vec<Game> = local.iter().map(entry_game).collect();
    let mut extra = Vec::new();
    merge(&mut games, &mut extra, remote, 0);
    let mut out: Vec<Entry> = local
        .into_iter()
        .zip(&games)
        .map(|(e, g)| Entry {
            playtime_sec: g.playtime_sec,
            sessions: g.sessions,
            first_played: g.first_played,
            last_played: g.last_played,
            journal: g.journal.clone(),
            ..e
        })
        .collect();
    // the remote's own entries, as they were (merge stamped them as restored)
    out.extend(extra.iter().filter_map(|x| remote.iter().find(|e| e.folder_name == folder_name(&x.folder)).cloned()));
    out
}

fn entry_game(e: &Entry) -> Game {
    Game {
        id: e.id.clone(),
        name: e.name.clone(),
        folder: PathBuf::from(&e.folder_name),
        steam_app_id: e.steam_app_id,
        playtime_sec: e.playtime_sec,
        sessions: e.sessions,
        first_played: e.first_played,
        last_played: e.last_played,
        journal: e.journal.clone(),
        ..Default::default()
    }
}

/// Takes what the backup knows better. True if anything changed.
fn take(g: &mut Game, e: &Entry) -> bool {
    let before = (g.playtime_sec, g.sessions, g.first_played, g.last_played, g.journal.clone());
    g.playtime_sec = g.playtime_sec.max(e.playtime_sec);
    g.sessions = g.sessions.max(e.sessions);
    g.first_played = match (g.first_played, e.first_played) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    g.last_played = g.last_played.max(e.last_played);
    if g.journal.is_empty() {
        g.journal = e.journal.clone();
    }
    before != (g.playtime_sec, g.sessions, g.first_played, g.last_played, g.journal.clone())
}

#[cfg(test)]
mod tests {
    use super::super::Status;
    use super::*;

    fn game(id: &str, folder: &str) -> Game {
        Game { id: id.into(), name: id.into(), folder: PathBuf::from(folder), ..Default::default() }
    }

    #[test]
    fn export_keeps_what_cant_be_fetched_again() {
        let mut g = game("hades", r"C:\Games\Hades");
        g.playtime_sec = 3600;
        g.sessions = 2;
        g.journal.rating = Some(5);
        g.art.tile = Some(PathBuf::from(r"W:\data\library\hades\tile.jpg"));
        let e = &export(&[g], &[])[0];
        assert_eq!((e.folder_name.as_str(), e.playtime_sec, e.sessions, e.journal.rating), ("Hades", 3600, 2, Some(5)));
        let text = serde_json::to_string(e).unwrap();
        assert!(!text.contains("tile.jpg") && !text.contains(r"C:\\Games"), "no paths in a backup: {text}");
    }

    #[test]
    fn a_restore_never_loses_or_doubles_time() {
        let mut here = game("hades", r"D:\Games\Hades");
        here.playtime_sec = 100;
        here.sessions = 1;
        here.first_played = Some(5_000);
        here.last_played = Some(9_000);
        let mut games = vec![here];
        let mut kept = Vec::new();
        let mut e = export(&[game("hades", r"C:\Games\Hades")], &[]).remove(0);
        e.playtime_sec = 7200;
        e.sessions = 4;
        e.first_played = Some(1_000);
        e.last_played = Some(8_000);
        e.journal = Journal { rating: Some(4), status: Some(Status::Playing), ..Default::default() };

        let r = merge(&mut games, &mut kept, &[e.clone()], 10_000);
        assert_eq!(r, Restored { updated: 1, added: 0 });
        let g = &games[0];
        assert_eq!((g.playtime_sec, g.sessions, g.first_played, g.last_played), (7200, 4, Some(1_000), Some(9_000)));
        assert_eq!(g.journal.rating, Some(4));
        // a second restore of the same backup changes nothing
        assert_eq!(merge(&mut games, &mut kept, &[e], 10_000), Restored::default());
        assert_eq!(games[0].playtime_sec, 7200);
    }

    #[test]
    fn an_upload_keeps_the_other_pcs_games_and_time() {
        let mut hades = game("hades", r"C:\Games\Hades");
        hades.playtime_sec = 50;
        let local = export(&[hades, game("celeste", r"C:\Games\Celeste")], &[]);
        let mut remote = export(&[game("hades", r"E:\Hades"), game("sekiro", r"E:\Sekiro"), game("gris", r"E:\Gris")], &[]);
        remote[0].playtime_sec = 900;
        remote[1].journal.rating = Some(5); // reviewed on the other PC
        let out = combine(local, &remote);
        let ids: Vec<_> = out.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["hades", "celeste", "sekiro"], "gris was never played or reviewed");
        assert_eq!(out[0].playtime_sec, 900);
        assert_eq!(out[2].journal.rating, Some(5));
        assert_eq!(out[2].removed_at, None, "the other PC's entry is uploaded as it was");
    }

    #[test]
    fn a_journal_written_here_wins() {
        let mut here = game("celeste", r"C:\Games\Celeste");
        here.journal.note = Some("mine".into());
        let mut games = vec![here];
        let mut e = export(&games, &[]).remove(0);
        e.journal = Journal { note: Some("old".into()), rating: Some(2), ..Default::default() };
        merge(&mut games, &mut Vec::new(), &[e], 1);
        assert_eq!(games[0].journal.note.as_deref(), Some("mine"));
        assert_eq!(games[0].journal.rating, None);
    }

    #[test]
    fn games_not_here_join_the_journal_and_unplayed_ones_dont() {
        let mut games = vec![game("hades", r"C:\Games\Hades")];
        let mut kept = Vec::new();
        let mut played = export(&[game("hades", r"C:\Games\Hades II")], &[]).remove(0); // same id, other folder
        played.folder_name = "Hades II".into();
        played.id = "hades".into();
        played.steam_app_id = Some(1);
        played.playtime_sec = 60;
        let unplayed = export(&[game("celeste", r"C:\Games\Celeste")], &[]).remove(0);
        let r = merge(&mut games, &mut kept, &[unplayed, played], 42);
        // matched by id: "hades" here takes the time (ids are stable per folder, so this is the same game)
        assert_eq!(r, Restored { updated: 1, added: 0 });

        let mut other = export(&[game("sekiro", r"C:\Games\Sekiro")], &[]).remove(0);
        other.playtime_sec = 30;
        let r = merge(&mut games, &mut kept, &[other], 42);
        assert_eq!(r, Restored { updated: 0, added: 1 });
        assert_eq!(kept[0].folder, PathBuf::from("Sekiro"));
        assert_eq!((kept[0].removed_at, kept[0].restored_at), (Some(42), Some(42)));
    }
}
