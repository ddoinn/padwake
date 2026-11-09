//! Your own lists (row B3): `data\collections.json`, shared by everyone on the PC like the games and settings.
//! `{"lists": [{"id", "name", "games": [game ids], "createdAt"}]}`, oldest list first, a list's games in the order they
//! were added. A game id stays in a list while its folder is gone, so the game is back in its lists when it returns;
//! the page shows only the games in the library. Smart lists aren't stored: the page works them out from each user's
//! time and journal every time it shows them (`ui/library/lists.js`).
//! Every change is sent to the page as the `lists` event (all of them) and written whole (temp file, then rename).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// The longest list name, in characters.
pub const NAME_MAX: usize = 32;

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct List {
    pub id: String,
    pub name: String,
    pub games: Vec<String>,
    pub created_at: u64,
}

#[derive(Serialize, Deserialize, Default, Debug)]
#[serde(default)]
struct File {
    lists: Vec<List>,
}

/// One change at a time: each reads the file, changes it and writes it back.
static LOCK: Mutex<()> = Mutex::new(());
pub(super) static NOTIFY: OnceLock<Box<dyn Fn(&[List]) + Send + Sync>> = OnceLock::new();

fn file() -> PathBuf {
    crate::data::dir().join("collections.json")
}

/// The lists in `path`; none when it's missing. A broken file reads as none (and is replaced at the next change).
pub fn read(path: &Path) -> Vec<List> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<File>(&t).ok())
        .map(|f| f.lists)
        .unwrap_or_default()
}

fn write(path: &Path, lists: &[List]) -> Result<(), String> {
    let text = serde_json::to_vec_pretty(&serde_json::json!({ "lists": lists })).map_err(|e| e.to_string())?;
    super::write_file(path, &text)
}

/// A name as it's kept: trimmed, inner runs of spaces made one. Refused when empty, too long, or another list (not
/// `except`) already has it, whatever its case.
pub fn clean_name(lists: &[List], name: &str, except: Option<&str>) -> Result<String, String> {
    let n = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if n.is_empty() {
        return Err("Type a name for the list.".into());
    }
    if n.chars().count() > NAME_MAX {
        return Err(format!("A list name can be up to {NAME_MAX} characters."));
    }
    if lists.iter().any(|l| Some(l.id.as_str()) != except && l.name.to_lowercase() == n.to_lowercase()) {
        return Err(format!("There's already a list called {n}."));
    }
    Ok(n)
}

/// A new list, with `game` in it if one is given. Its id is new: `l` and the time in base 36.
pub fn create(lists: &mut Vec<List>, name: &str, game: Option<&str>, now: u64) -> Result<List, String> {
    let name = clean_name(lists, name, None)?;
    let mut t = now;
    let id = loop {
        let id = format!("l{}", base36(t));
        if !lists.iter().any(|l| l.id == id) {
            break id;
        }
        t += 1;
    };
    let list = List { id, name, games: game.map(|g| vec![g.to_string()]).unwrap_or_default(), created_at: now };
    lists.push(list.clone());
    Ok(list)
}

pub fn rename(lists: &mut [List], id: &str, name: &str) -> Result<(), String> {
    let name = clean_name(lists, name, Some(id))?;
    find(lists, id)?.name = name;
    Ok(())
}

pub fn delete(lists: &mut Vec<List>, id: &str) -> Result<(), String> {
    find(lists, id)?;
    lists.retain(|l| l.id != id);
    Ok(())
}

/// Puts `game` in the list or takes it out. In twice is still once; out when it isn't in changes nothing.
pub fn toggle(lists: &mut [List], id: &str, game: &str, on: bool) -> Result<(), String> {
    let l = find(lists, id)?;
    let has = l.games.iter().any(|g| g == game);
    if on && !has {
        l.games.push(game.to_string());
    } else if !on {
        l.games.retain(|g| g != game);
    }
    Ok(())
}

fn find<'a>(lists: &'a mut [List], id: &str) -> Result<&'a mut List, String> {
    lists.iter_mut().find(|l| l.id == id).ok_or_else(|| "That list isn't there any more.".to_string())
}

fn base36(mut n: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// Reads the lists, applies `f`, writes them and tells the page. Nothing is written when `f` refuses.
fn change<T>(f: impl FnOnce(&mut Vec<List>) -> Result<T, String>) -> Result<(T, Vec<List>), String> {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let path = file();
    let mut lists = read(&path);
    let out = f(&mut lists)?;
    write(&path, &lists)?;
    if let Some(notify) = NOTIFY.get() {
        notify(&lists);
    }
    Ok((out, lists))
}

fn now() -> u64 {
    super::scan::ms(std::time::SystemTime::now())
}

// ---------- commands (the library area: host.library.lists and the rest) ----------

#[tauri::command]
pub fn lists() -> Vec<List> {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    read(&file())
}

/// Resolves with the new list; the `lists` event follows.
#[tauri::command]
pub fn list_create(name: String, game: Option<String>) -> Result<List, String> {
    change(|l| create(l, &name, game.as_deref(), now())).map(|(list, _)| list)
}

#[tauri::command]
pub fn list_rename(id: String, name: String) -> Result<Vec<List>, String> {
    change(|l| rename(l, &id, &name)).map(|(_, lists)| lists)
}

#[tauri::command]
pub fn list_delete(id: String) -> Result<Vec<List>, String> {
    change(|l| delete(l, &id)).map(|(_, lists)| lists)
}

#[tauri::command]
pub fn list_toggle(id: String, game: String, on: bool) -> Result<Vec<List>, String> {
    change(|l| toggle(l, &id, &game, on)).map(|(_, lists)| lists)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("padwake-lists-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("collections.json")
    }

    #[test]
    fn lists_are_made_renamed_and_deleted_and_survive_a_write() {
        let path = temp("crud");
        assert!(read(&path).is_empty(), "no file: no lists");
        let mut lists = Vec::new();
        let couch = create(&mut lists, "  Couch   co-op ", Some("celeste"), 1_000).unwrap();
        assert_eq!(couch.name, "Couch co-op", "trimmed, inner spaces made one");
        assert_eq!(couch.games, vec!["celeste"]);
        let later = create(&mut lists, "Later", None, 1_000).unwrap();
        assert_ne!(couch.id, later.id, "two lists made in the same millisecond get their own ids");
        rename(&mut lists, &later.id, "Someday").unwrap();
        write(&path, &lists).unwrap();
        let back = read(&path);
        assert_eq!(back, lists);
        assert_eq!(back.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["Couch co-op", "Someday"], "oldest first");
        delete(&mut lists, &couch.id).unwrap();
        assert_eq!(lists.len(), 1);
        assert!(delete(&mut lists, &couch.id).is_err(), "a deleted list can't be deleted again");
    }

    #[test]
    fn names_must_be_there_short_enough_and_not_taken() {
        let mut lists = Vec::new();
        let a = create(&mut lists, "Favourites", None, 1).unwrap();
        assert!(create(&mut lists, "   ", None, 2).is_err(), "empty");
        assert!(create(&mut lists, &"x".repeat(NAME_MAX + 1), None, 2).is_err(), "too long");
        assert!(create(&mut lists, &"é".repeat(NAME_MAX), None, 2).is_ok(), "counted in characters, not bytes");
        assert!(create(&mut lists, "favourites", None, 3).is_err(), "taken, whatever the case");
        assert!(rename(&mut lists, &a.id, "FAVOURITES").is_ok(), "a list may change its own name's case");
        assert!(rename(&mut lists, "nope", "Other").is_err(), "a list that isn't there");
    }

    #[test]
    fn a_game_goes_in_once_and_comes_out() {
        let mut lists = Vec::new();
        let l = create(&mut lists, "Co-op", None, 1).unwrap();
        toggle(&mut lists, &l.id, "hades", true).unwrap();
        toggle(&mut lists, &l.id, "hades", true).unwrap();
        toggle(&mut lists, &l.id, "celeste", true).unwrap();
        assert_eq!(lists[0].games, vec!["hades", "celeste"], "in twice is once, in the order added");
        toggle(&mut lists, &l.id, "hades", false).unwrap();
        toggle(&mut lists, &l.id, "elden-ring", false).unwrap();
        assert_eq!(lists[0].games, vec!["celeste"]);
        assert!(toggle(&mut lists, "gone", "hades", true).is_err());
    }

    #[test]
    fn a_broken_file_reads_as_no_lists() {
        let path = temp("broken");
        std::fs::write(&path, "{\"lists\": [ {\"id\": ").unwrap();
        assert!(read(&path).is_empty());
        std::fs::write(&path, "{\"lists\": [{\"id\": \"l1\", \"name\": \"Old\"}]}").unwrap();
        assert_eq!(read(&path), vec![List { id: "l1".into(), name: "Old".into(), games: vec![], created_at: 0 }], "missing fields default");
    }
}
