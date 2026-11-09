//! Local users: everyone who plays on this PC has their own name, avatar, time played, journal, trophies, screenshots
//! and GitHub backup. The games folder, the art and every system setting are shared.
//! - `data\users.json` lists the users and who is playing now (`current`), which the next start keeps.
//! - Each user's things live in `data\users\<id>\`: `play.json` (time and journal, `library/mod.rs`), `captures\`
//!   (`games/capture.rs`), `backup.json` and `github.token` (`cloud/backup.rs`).
//! - The first start of a version with users makes one user, "Player 1", who gets everything recorded before: the
//!   time and journal in `library.json`, the screenshots and the backup sign-in.
//! - No user switches while a game runs, so a session's time, trophies and screenshots go to whoever started it.
//! - Avatars (`ui/users/avatars.js`): Padwake's own, a game's tile (`game:<id>`), a character (`char:<id>`,
//!   `characters.rs`), or your own picture (`pic:<key>`, `pictures.rs`). A new user, and the first one, start as `random`: one of the popular characters nobody has yet,
//!   picked as soon as AniList answers (at start and after each add; offline they keep an initial until then).
//! - Event `users`: `{users, current}` after any change.

mod characters;
mod pictures;

use crate::system::log;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{AppHandle, Emitter, Runtime};

pub const NAME_MAX: usize = 16;
const AVATAR_MAX: usize = 64;
const FIRST_NAME: &str = "Player 1";
/// a popular character nobody has yet, picked when AniList answers (`fill_random`)
const RANDOM: &str = "random";

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct User {
    /// stable and safe as a folder name: `u` and the time it was made, in base 36
    pub id: String,
    pub name: String,
    /// one of the UI's avatars (`ui/users/avatars.js`), `game:<id>` for a game's tile, `char:<id>` for a character,
    /// or `random`
    pub avatar: String,
    /// a character avatar's name, for Settings
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_name: Option<String>,
    /// a character avatar's picture (`users|list` and the event only; not saved)
    #[serde(skip_deserializing, skip_serializing_if = "Option::is_none")]
    pub image: Option<PathBuf>,
    pub created_at: u64,
}

/// `users.json`, and what `users|list` and the `users` event carry.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Users {
    pub users: Vec<User>,
    pub current: String,
}

fn path() -> PathBuf {
    crate::data::dir().join("users.json")
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn store() -> MutexGuard<'static, Users> {
    static STORE: OnceLock<Mutex<Users>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(load())).lock().unwrap_or_else(|e| e.into_inner())
}

fn load() -> Users {
    let mut u: Users = std::fs::read(path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    if u.users.is_empty() {
        let first = User { id: new_id(&[]), name: FIRST_NAME.into(), avatar: RANDOM.into(), created_at: now_ms(), ..User::default() };
        adopt_older_files(&first.id);
        log(&format!("users: made {} ({})", first.name, first.id));
        u = Users { current: first.id.clone(), users: vec![first] };
        save(&u);
    }
    if !u.users.iter().any(|x| x.id == u.current) {
        u.current = u.users[0].id.clone();
    }
    u
}

fn save(u: &Users) {
    match serde_json::to_vec_pretty(u) {
        Ok(b) => {
            if let Err(e) = crate::library::write_file(&path(), &b) {
                log(&format!("users: {e}"));
            }
        }
        Err(e) => log(&format!("users: {e}")),
    }
}

/// Before users, screenshots and the backup sign-in were in the data dir itself: they become the first user's.
/// (`library.json`'s time and journal move when the library loads, `library/mod.rs`.)
fn adopt_older_files(id: &str) {
    let to = dir_of(id);
    for name in ["captures", "backup.json", "github.token"] {
        let from = crate::data::dir().join(name);
        if from.exists() {
            if let Err(e) = std::fs::rename(&from, to.join(name)) {
                log(&format!("users: moving {name} to {}: {e}", to.display()));
            }
        }
    }
}

fn new_id(taken: &[User]) -> String {
    let mut n = now_ms();
    loop {
        let id = format!("u{}", base36(n));
        if !taken.iter().any(|u| u.id == id) {
            return id;
        }
        n += 1;
    }
}

fn base36(mut n: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut s = Vec::new();
    loop {
        s.push(DIGITS[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    s.reverse();
    String::from_utf8(s).unwrap_or_default()
}

/// A user's folder in the data dir, made if missing.
pub fn dir_of(id: &str) -> PathBuf {
    let d = crate::data::dir().join("users").join(id);
    let _ = std::fs::create_dir_all(&d);
    std::path::absolute(&d).unwrap_or(d)
}

/// Who is playing now.
pub fn current_id() -> String {
    store().current.clone()
}

/// The folder of who is playing now.
pub fn dir() -> PathBuf {
    dir_of(&current_id())
}

/// Every user's id.
pub fn ids() -> Vec<String> {
    store().users.iter().map(|u| u.id.clone()).collect()
}

/// Everyone, with each character avatar's picture.
pub fn list() -> Users {
    let mut u = store().clone();
    for user in &mut u.users {
        user.image = avatar_file(&user.avatar).filter(|p| p.is_file());
    }
    u
}

fn character_id(avatar: &str) -> Option<u64> {
    avatar.strip_prefix("char:")?.parse().ok()
}

/// The picture file of a character or own-picture avatar.
fn avatar_file(avatar: &str) -> Option<PathBuf> {
    if let Some(id) = character_id(avatar) {
        return Some(characters::picture(id));
    }
    let k = avatar.strip_prefix("pic:")?;
    k.chars().all(|c| c.is_ascii_hexdigit()).then(|| pictures::avatar(k))
}

/// A name: trimmed, 1 to `NAME_MAX` characters, and no one else's (letter case aside).
fn clean_name(name: &str, users: &[User], except: Option<&str>) -> Result<String, String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err("Type a name.".into());
    }
    if name.chars().count() > NAME_MAX {
        return Err(format!("A name can be up to {NAME_MAX} characters."));
    }
    if users.iter().any(|u| Some(u.id.as_str()) != except && u.name.to_lowercase() == name.to_lowercase()) {
        return Err(format!("There's already a user called {name}."));
    }
    Ok(name)
}

fn clean_avatar(avatar: &str) -> Result<String, String> {
    let a = avatar.trim();
    if a.is_empty() || a.len() > AVATAR_MAX || a.chars().any(|c| c.is_control()) {
        return Err("That avatar isn't one Padwake knows.".into());
    }
    if (a.starts_with("char:") || a.starts_with("pic:")) && !avatar_file(a).is_some_and(|p| p.is_file()) {
        return Err("That picture isn't on this PC any more. Pick it again.".into());
    }
    Ok(a.to_string())
}

/// Adds a user (not switching to them). Returns them.
pub fn add(name: &str, avatar: &str) -> Result<User, String> {
    let mut u = store();
    let user = User {
        id: new_id(&u.users),
        name: clean_name(name, &u.users, None)?,
        avatar: clean_avatar(avatar)?,
        created_at: now_ms(),
        ..User::default()
    };
    dir_of(&user.id);
    u.users.push(user.clone());
    save(&u);
    log(&format!("users: added {} ({})", user.name, user.id));
    Ok(user)
}

/// Renames a user or changes their avatar (a field left out stays). `avatar_name` names a character avatar.
pub fn edit(id: &str, name: Option<&str>, avatar: Option<&str>, avatar_name: Option<&str>) -> Result<User, String> {
    let mut guard = store();
    let u = &mut *guard;
    let name = name.map(|n| clean_name(n, &u.users, Some(id))).transpose()?;
    let avatar = avatar.map(clean_avatar).transpose()?;
    let user = u.users.iter_mut().find(|x| x.id == id).ok_or("That user isn't on this PC any more.")?;
    if let Some(n) = name {
        user.name = n;
    }
    if let Some(a) = avatar {
        let named = a.starts_with("char:") || a.starts_with("pic:");
        user.avatar_name = named.then(|| avatar_name.unwrap_or_default().chars().take(64).collect());
        user.avatar = a;
    }
    let user = user.clone();
    save(u);
    Ok(user)
}

/// Gives every `random` user a popular character nobody has yet. True if any changed. Needs AniList.
fn fill_random() -> Result<bool, String> {
    if !store().users.iter().any(|u| u.avatar == RANDOM) {
        return Ok(false);
    }
    let featured = characters::featured()?;
    let mut u = store();
    let mut seed = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos() as usize).unwrap_or(7);
    let mut any = false;
    for i in 0..u.users.len() {
        if u.users[i].avatar != RANDOM {
            continue;
        }
        let taken: Vec<String> = u.users.iter().map(|x| x.avatar.clone()).collect();
        let free: Vec<_> = featured.iter().filter(|c| !taken.contains(&format!("char:{}", c.id))).collect();
        let pool = if free.is_empty() { featured.iter().collect() } else { free };
        seed = seed.wrapping_mul(2_654_435_761).wrapping_add(i + 1);
        let c = pool[seed % pool.len()];
        u.users[i].avatar = format!("char:{}", c.id);
        u.users[i].avatar_name = Some(c.name.clone());
        log(&format!("users: {} is {} now", u.users[i].name, c.name));
        any = true;
    }
    if any {
        save(&u);
    }
    Ok(any)
}

/// `fill_random` in the background; the `users` event follows if anyone got a character.
fn fill_random_soon<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    let _ = std::thread::Builder::new().name("avatars".into()).spawn(move || match fill_random() {
        Ok(true) => {
            changed(&app);
        }
        Ok(false) => {}
        Err(e) => log(&format!("users: no random character yet: {e}")),
    });
}

/// Deletes a user and everything of theirs on this PC (their GitHub backup stays). Not the one playing now.
pub fn remove(id: &str) -> Result<(), String> {
    {
        let mut u = store();
        if u.current == id {
            return Err("Switch to another user first.".into());
        }
        let before = u.users.len();
        u.users.retain(|x| x.id != id);
        if u.users.len() == before {
            return Err("That user isn't on this PC any more.".into());
        }
        save(&u);
    }
    let d = crate::data::dir().join("users").join(id);
    if let Err(e) = std::fs::remove_dir_all(&d) {
        log(&format!("users: deleting {}: {e}", d.display()));
    }
    crate::library::drop_unkept();
    log(&format!("users: deleted {id}"));
    Ok(())
}

/// Makes `id` the one playing. Refused while a game runs: its session belongs to whoever started it.
pub fn switch<R: Runtime>(app: &AppHandle<R>, id: &str) -> Result<(), String> {
    if crate::games::is_running() {
        return Err("Close the game first. Its time goes to whoever started it.".into());
    }
    if current_id() == id {
        return Ok(());
    }
    if !ids().iter().any(|x| x == id) {
        return Err("That user isn't on this PC any more.".into());
    }
    // no backup runs meanwhile, so none reads one user's sign-in and writes the other's
    crate::cloud::while_switching(|| {
        let mut u = store();
        u.current = id.to_string();
        save(&u);
        drop(u);
        crate::library::set_user(id);
    });
    crate::library::log_event(id, crate::library::events::What::User {});
    crate::cloud::user_switched(app);
    log(&format!("users: switched to {id}"));
    Ok(())
}

fn changed<R: Runtime>(app: &AppHandle<R>) -> Users {
    let u = list();
    let _ = app.emit("users", u.clone());
    u
}

pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("users")
        .invoke_handler(tauri::generate_handler![
            commands::list,
            commands::add,
            commands::edit,
            commands::remove,
            commands::switch,
            commands::featured,
            commands::search,
            commands::pictures,
            commands::use_picture
        ])
        .setup(|app, _api| {
            fill_random_soon(app);
            Ok(())
        })
        .build()
}

mod commands {
    use super::characters::{self, Character};
    use super::pictures::{self, Picture};
    use super::{changed, User, Users};
    use tauri::{AppHandle, Runtime};

    /// The popular characters with their pictures (AniList; the pictures are kept in `data\avatars\`).
    #[tauri::command]
    pub async fn featured() -> Result<Vec<Character>, String> {
        tauri::async_runtime::spawn_blocking(characters::featured).await.map_err(|e| e.to_string())?
    }

    /// Your pictures (data\avatars\mine, Pictures, Downloads, Desktop), newest first, with thumbnails.
    #[tauri::command]
    pub async fn pictures() -> Vec<Picture> {
        tauri::async_runtime::spawn_blocking(pictures::list).await.unwrap_or_default()
    }

    /// Makes one of your pictures a user's avatar.
    #[tauri::command]
    pub async fn use_picture<R: Runtime>(app: AppHandle<R>, id: String, path: std::path::PathBuf) -> Result<User, String> {
        let (k, name) = tauri::async_runtime::spawn_blocking(move || pictures::take(&path)).await.map_err(|e| e.to_string())??;
        let user = super::edit(&id, None, Some(&format!("pic:{k}")), Some(&name))?;
        changed(&app);
        Ok(user)
    }

    /// Characters by name, with their pictures.
    #[tauri::command]
    pub async fn search(query: String) -> Result<Vec<Character>, String> {
        tauri::async_runtime::spawn_blocking(move || characters::search(&query)).await.map_err(|e| e.to_string())?
    }

    #[tauri::command]
    pub fn list() -> Users {
        super::list()
    }

    #[tauri::command]
    pub fn add<R: Runtime>(app: AppHandle<R>, name: String, avatar: String) -> Result<User, String> {
        let user = super::add(&name, &avatar)?;
        changed(&app);
        if user.avatar == super::RANDOM {
            super::fill_random_soon(&app);
        }
        Ok(user)
    }

    #[tauri::command]
    pub fn edit<R: Runtime>(
        app: AppHandle<R>,
        id: String,
        name: Option<String>,
        avatar: Option<String>,
        avatar_name: Option<String>,
    ) -> Result<User, String> {
        let user = super::edit(&id, name.as_deref(), avatar.as_deref(), avatar_name.as_deref())?;
        changed(&app);
        Ok(user)
    }

    #[tauri::command]
    pub fn remove<R: Runtime>(app: AppHandle<R>, id: String) -> Result<Users, String> {
        super::remove(&id)?;
        Ok(changed(&app))
    }

    #[tauri::command]
    pub async fn switch<R: Runtime>(app: AppHandle<R>, id: String) -> Result<Users, String> {
        // waits for a backup upload to finish, so off the main thread
        let a = app.clone();
        tauri::async_runtime::spawn_blocking(move || super::switch(&a, &id)).await.map_err(|e| e.to_string())??;
        Ok(changed(&app))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(id: &str, name: &str) -> User {
        User { id: id.into(), name: name.into(), avatar: "ember".into(), ..User::default() }
    }

    #[test]
    fn names_are_trimmed_short_and_unique() {
        let users = [user("a", "Madhav"), user("b", "Asha")];
        assert_eq!(clean_name("  Riya   Sen ", &users, None).unwrap(), "Riya Sen");
        assert!(clean_name("   ", &users, None).is_err());
        assert!(clean_name("madhav", &users, None).is_err(), "letter case aside");
        assert_eq!(clean_name("MADHAV", &users, Some("a")).unwrap(), "MADHAV", "renaming yourself is fine");
        assert!(clean_name(&"x".repeat(NAME_MAX + 1), &users, None).is_err());
        assert!(clean_name(&"é".repeat(NAME_MAX), &users, None).is_ok(), "characters, not bytes");
    }

    #[test]
    fn avatars_are_short_plain_strings() {
        assert_eq!(clean_avatar(" game:hades ").unwrap(), "game:hades");
        assert!(clean_avatar("").is_err());
        assert!(clean_avatar(&"a".repeat(AVATAR_MAX + 1)).is_err());
        assert!(clean_avatar("a\nb").is_err());
        assert!(clean_avatar("char:999999999").is_err(), "a character whose picture never downloaded");
        assert!(clean_avatar("char:nope").is_err());
        assert_eq!(character_id("char:422"), Some(422));
    }

    #[test]
    fn ids_are_unique_folder_names() {
        let a = new_id(&[]);
        assert!(a.starts_with('u') && a[1..].chars().all(|c| c.is_ascii_alphanumeric()));
        let b = new_id(&[user(&a, "A")]);
        assert_ne!(a, b);
        assert_eq!(base36(0), "0");
        assert_eq!(base36(36 * 36 + 35), "10z");
    }
}
