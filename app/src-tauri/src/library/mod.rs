//! The library: one folder in `gamesRoot` = one game, as `extras/antcloud/sync.ps1` keeps Playnite's.
//! - A watcher thread checks the root every 30 s, never while a game runs. New folders join once nothing in them changed
//!   for 3 min (`PADWAKE_QUIET_SEC` overrides); removed folders leave, but a played game's id and playtime are
//!   kept for when its folder comes back.
//! - Each game is enriched once (`steam.rs`): Steam store info, SteamGridDB art, news, trailer and music.
//! - The list is saved in `library.json` in the data dir, with art and media in `library\<id>\`, and every change is
//!   sent to the page as the `library` event (the full list). Log: `library.log`.
//! - `library.json` keeps art and media paths relative to the data dir, so a copied or moved data dir keeps its art;
//!   in memory, `list` and events they are absolute.
//! - `padwake.exe --sync-once [--root <dir>]` runs one pass without a window; `--watch` keeps watching.

mod art;
mod scan;
mod steam;
mod trailer;

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime};
use steam::Http;
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Emitter, Runtime};

/// A library entry. Paths are absolute (art and media are relative only inside `library.json`); art and media are files in the data dir, which the UI turns into URLs with
/// `host.fileUrl`. Times are Unix milliseconds. `id` is stable: the folder's slug when it first joined.
#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Game {
    pub id: String,
    pub name: String,
    pub folder: PathBuf,
    pub exe: PathBuf,
    pub art: Art,
    pub tagline: Option<String>,
    /// plain text, paragraphs separated by a blank line
    pub description: Option<String>,
    pub screenshots: Vec<PathBuf>,
    pub trailer: Option<PathBuf>,
    pub music: Option<PathBuf>,
    pub news: Vec<News>,
    pub playtime_sec: u64,
    pub last_played: Option<u64>,
    pub added_at: Option<u64>,
    pub steam_app_id: Option<u64>,
    /// when store info and art last landed; `None` = still to fetch
    pub synced_at: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default)]
pub struct Art {
    /// square
    pub tile: Option<PathBuf>,
    /// 16:9, logo-free
    pub background: Option<PathBuf>,
    /// transparent PNG
    pub logo: Option<PathBuf>,
    /// portrait box art
    pub cover: Option<PathBuf>,
}

/// A Steam news item: plain text, `contents` about 300 characters.
#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default)]
pub struct News {
    pub title: String,
    pub url: String,
    pub date: u64,
    pub contents: String,
}

/// `library.json`: the games, plus played games whose folder is gone (only id, folder and playtime).
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Saved {
    games: Vec<Game>,
    kept: Vec<Game>,
}

static STATE: Mutex<Saved> = Mutex::new(Saved { games: Vec::new(), kept: Vec::new() });
/// Sends the list to the page (app) or prints it (`--watch`).
static NOTIFY: OnceLock<Box<dyn Fn(&[Game]) + Send + Sync>> = OnceLock::new();
/// One enrichment at a time (the watcher, or `refresh` from the page).
static ENRICHING: Mutex<()> = Mutex::new(());
/// Set by `rescan` to wake the watcher for a full pass now.
static WAKE: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());

const TICK: Duration = Duration::from_secs(30);

fn state() -> MutexGuard<'static, Saved> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("library")
        .invoke_handler(tauri::generate_handler![list, refresh, rescan])
        .setup(|app, _api| {
            load();
            let app = app.clone();
            let _ = NOTIFY.set(Box::new(move |games| {
                let _ = app.emit("library", games);
            }));
            std::thread::Builder::new().name("library".into()).spawn(|| {
                // below the UI: a first sync decodes and resizes images for minutes
                use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN};
                unsafe {
                    let _ = SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN);
                }
                watch(None)
            })?;
            Ok(())
        })
        .build()
}

/// One game by id.
pub fn get(id: &str) -> Option<Game> {
    state().games.iter().find(|g| g.id == id).cloned()
}

#[tauri::command]
fn list() -> Vec<Game> {
    state().games.clone()
}

/// Fetches one game's store info and art again. Resolves with the updated game (the `library` event follows too).
#[tauri::command]
async fn refresh(id: String) -> Result<Game, String> {
    tauri::async_runtime::spawn_blocking(move || {
        enrich_one(&id, &Http::new(), trailer::find_ffmpeg().as_deref())?;
        get(&id).ok_or_else(|| format!("no game {id}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Checks the games folder now instead of at the next 30 s tick. Returns the list as it is.
#[tauri::command]
fn rescan() -> Vec<Game> {
    *WAKE.0.lock().unwrap_or_else(|e| e.into_inner()) = true;
    WAKE.1.notify_all();
    list()
}

/// Row A5: a session of `secs` ended at `ended_at` (Unix ms). Adds it to the game's playtime, sets `lastPlayed` and
/// sends the list. False if there is no such game.
#[allow(dead_code)] // row A5 calls it
pub fn record_session(id: &str, secs: u64, ended_at: u64) -> bool {
    let mut s = state();
    let Some(g) = s.games.iter_mut().find(|g| g.id == id) else { return false };
    g.playtime_sec += secs;
    g.last_played = Some(ended_at);
    changed(&s);
    true
}

// ---------- saving ----------

fn library_file() -> PathBuf {
    crate::data::dir().join("library.json")
}

fn game_dir(id: &str) -> PathBuf {
    let dir = crate::data::dir().join("library").join(id);
    std::path::absolute(&dir).unwrap_or(dir)
}

/// The data dir as an absolute path: the base of the relative art and media paths in `library.json`.
fn data_root() -> PathBuf {
    let dir = crate::data::dir();
    std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf())
}

/// Every art and media path of a game (not its folder or exe, which are in the games folder).
fn media_paths(g: &mut Game) -> impl Iterator<Item = &mut PathBuf> {
    let a = &mut g.art;
    [&mut a.tile, &mut a.background, &mut a.logo, &mut a.cover, &mut g.trailer, &mut g.music]
        .into_iter()
        .flatten()
        .chain(g.screenshots.iter_mut())
}

/// For `library.json`: a path under `root` becomes relative to it (`library\<id>\tile.jpg`).
fn to_relative(p: &Path, root: &Path) -> PathBuf {
    p.strip_prefix(root).map(Path::to_path_buf).unwrap_or_else(|_| p.to_path_buf())
}

/// From `library.json`: a relative path joins `root`. An absolute one from another data dir (an older `library.json`
/// in a copied or moved data dir) keeps its `library\<id>\<file>` part, under `root`.
fn to_absolute(p: &Path, root: &Path) -> PathBuf {
    if p.is_relative() {
        return root.join(p);
    }
    if p.starts_with(root) {
        return p.to_path_buf();
    }
    let parts: Vec<_> = p.components().collect();
    match parts.len().checked_sub(3).map(|i| &parts[i..]) {
        Some([lib, id, file]) if lib.as_os_str().eq_ignore_ascii_case("library") => {
            root.join("library").join(id).join(file)
        }
        _ => p.to_path_buf(),
    }
}

fn load() {
    let mut saved = match std::fs::read_to_string(library_file()) {
        Ok(text) => serde_json::from_str::<Saved>(&text).unwrap_or_else(|e| {
            log(&format!("library.json is not valid ({e}); starting empty"));
            Saved::default()
        }),
        Err(_) => Saved::default(),
    };
    let root = data_root();
    for g in &mut saved.games {
        media_paths(g).for_each(|p| *p = to_absolute(p, &root));
    }
    *state() = saved;
}

/// Saves the library (art and media paths relative to the data dir) and sends it to the page (absolute).
fn changed(s: &Saved) {
    let root = data_root();
    let mut games = s.games.clone();
    for g in &mut games {
        media_paths(g).for_each(|p| *p = to_relative(p, &root));
    }
    match serde_json::to_vec_pretty(&Saved { games, kept: s.kept.clone() }) {
        Ok(text) => {
            if let Err(e) = write_file(&library_file(), &text) {
                log(&format!("saving library.json: {e}"));
            }
        }
        Err(e) => log(&format!("library.json: {e}")),
    }
    if let Some(notify) = NOTIFY.get() {
        notify(&s.games);
    }
}

/// Writes a file whole: to a temp name first, then renamed over the old one.
pub(crate) fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension(format!("{}.tmp", path.extension().and_then(|e| e.to_str()).unwrap_or("")));
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", path.display())
    })
}

fn log(msg: &str) {
    let line = format!("{} {msg}", timestamp(SystemTime::now()));
    eprintln!("library: {msg}");
    let path = crate::data::dir().join("library.log");
    // keep the log small: start over past 1 MB
    let append = std::fs::metadata(&path).map(|m| m.len() < 1 << 20).unwrap_or(true);
    let file = std::fs::OpenOptions::new().create(true).write(true).append(append).truncate(!append).open(&path);
    if let Ok(mut f) = file {
        let _ = writeln!(f, "{line}");
    }
}

/// `2026-10-28 14:05:09Z`
fn timestamp(t: SystemTime) -> String {
    let secs = scan::ms(t) / 1000;
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    // days since 1970-01-01 to a civil date (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

// ---------- the watcher ----------

fn quiet() -> Duration {
    let sec = std::env::var("PADWAKE_QUIET_SEC").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(180);
    Duration::from_secs(sec)
}

#[derive(Default)]
struct Watch {
    /// the folders at the last full pass
    seen: Option<Vec<PathBuf>>,
    /// a new folder is still being written: pass again next tick
    waiting: bool,
    /// logged "skipped" for the game that is running now
    skipping: bool,
    root_missing: bool,
    /// enrichment attempts this session (one per game, as sync.ps1)
    tried: HashSet<String>,
    no_exe: HashSet<PathBuf>,
    /// per game: what enrichment did, for `--sync-once`
    report: Vec<(String, Result<String, String>, Duration)>,
}

#[derive(Debug, PartialEq)]
enum Tick {
    Skipped,
    Idle,
    Passed,
}

/// One tick of the watcher. While a game runs it does nothing but log "skipped" once.
fn tick(w: &mut Watch, root: &Path, running: &dyn Fn() -> bool) -> Tick {
    if running() {
        if !w.skipping {
            log("scan skipped: a game is running");
            w.skipping = true;
        }
        return Tick::Skipped;
    }
    w.skipping = false;
    let Some(folders) = scan::folders(root) else {
        if !w.root_missing {
            log(&format!("games folder {} can't be read; the library stays as it is", root.display()));
            w.root_missing = true;
        }
        return Tick::Idle;
    };
    w.root_missing = false;
    if w.seen.as_ref() == Some(&folders) && !w.waiting {
        // nothing new on disk; finish enrichment a running game paused
        enrich_new(w, running);
        return Tick::Idle;
    }
    let (known, kept) = {
        let s = state();
        (s.games.clone(), s.kept.clone())
    };
    let p = scan::pass(&known, &kept, &folders, SystemTime::now(), quiet());
    apply(&p);
    for f in &p.no_exe {
        if w.no_exe.insert(f.clone()) {
            log(&format!("no game exe in {}", f.display()));
        }
    }
    if !p.waiting.is_empty() && !w.waiting {
        let names: Vec<_> = p.waiting.iter().filter_map(|f| f.file_name()).map(|n| n.to_string_lossy()).collect();
        log(&format!("waiting for {} to be quiet for {} s", names.join(", "), quiet().as_secs()));
    }
    w.seen = Some(folders);
    w.waiting = !p.waiting.is_empty();
    enrich_new(w, running);
    Tick::Passed
}

/// Puts a pass into the library: joins, leaves and exe changes, keeping anything that changed meanwhile (playtime).
fn apply(p: &scan::Pass) {
    if !p.changed() {
        return;
    }
    let mut s = state();
    let mut games = Vec::with_capacity(p.games.len());
    for g in &p.games {
        match s.games.iter().find(|c| c.id == g.id) {
            Some(cur) => games.push(Game { exe: g.exe.clone(), ..cur.clone() }),
            None => games.push(g.clone()),
        }
    }
    for g in &p.left {
        let cur = s.games.iter().find(|c| c.id == g.id).unwrap_or(g).clone();
        if cur.playtime_sec > 0 || cur.last_played.is_some() {
            let keep = Game {
                id: cur.id.clone(),
                name: cur.name.clone(),
                folder: cur.folder.clone(),
                playtime_sec: cur.playtime_sec,
                last_played: cur.last_played,
                ..Default::default()
            };
            s.kept.push(keep);
        }
        let _ = std::fs::remove_dir_all(game_dir(&g.id));
        log(&format!("left: {} ({})", cur.name, cur.folder.display()));
    }
    for id in &p.joined {
        if let Some(g) = games.iter().find(|g| &g.id == id) {
            log(&format!("joined: {} -> {}", g.id, g.exe.display()));
        }
    }
    for id in &p.exe_changed {
        if let Some(g) = games.iter().find(|g| &g.id == id) {
            log(&format!("exe for {} -> {}", g.id, g.exe.display()));
        }
    }
    s.kept.retain(|k| !games.iter().any(|g| g.id == k.id));
    s.games = games;
    changed(&s);
}

/// Enriches every game that still needs it and wasn't tried this session. Stops when a game starts.
fn enrich_new(w: &mut Watch, running: &dyn Fn() -> bool) {
    let todo: Vec<String> =
        state().games.iter().filter(|g| g.synced_at.is_none() && !w.tried.contains(&g.id)).map(|g| g.id.clone()).collect();
    if todo.is_empty() {
        return;
    }
    let http = Http::new();
    let ffmpeg = trailer::find_ffmpeg();
    if !http.has_sgdb_key() {
        log("no sgdb.key in the data dir: SteamGridDB art skipped");
    }
    if ffmpeg.is_none() {
        log("no ffmpeg (settings.ffmpeg, ffmpeg.exe beside the app, or PATH): trailers skipped");
    }
    for id in todo {
        if running() {
            log("enrichment paused: a game is running");
            return;
        }
        let t0 = Instant::now();
        let result = enrich_one(&id, &http, ffmpeg.as_deref());
        if !matches!(&result, Err(e) if e.starts_with("paused")) {
            w.tried.insert(id.clone());
        }
        w.report.push((id, result, t0.elapsed()));
    }
}

/// Fetches one game's store info and art and puts it in the library. Returns a one-line summary.
fn enrich_one(id: &str, http: &Http, ffmpeg: Option<&Path>) -> Result<String, String> {
    let _one = ENRICHING.lock().unwrap_or_else(|e| e.into_inner());
    let game = get(id).ok_or_else(|| format!("no game {id}"))?;
    let t0 = Instant::now();
    let dir = game_dir(id);
    let found = match steam::enrich(http, &game, &dir, ffmpeg, &crate::games::is_running) {
        Ok(f) => f,
        Err(e) => {
            log(&format!("{}: {e}", game.name));
            return Err(e);
        }
    };
    let mut s = state();
    let Some(g) = s.games.iter_mut().find(|g| g.id == id) else {
        // the folder left while we fetched
        drop(s);
        let _ = std::fs::remove_dir_all(&dir);
        return Err(format!("{id} left"));
    };
    g.name = found.name;
    g.steam_app_id = Some(found.steam_app_id);
    g.tagline = found.tagline;
    g.description = found.description;
    g.art = found.art;
    g.screenshots = found.screenshots;
    g.trailer = found.trailer;
    g.music = found.music;
    g.news = found.news;
    g.synced_at = Some(scan::ms(SystemTime::now()));
    let summary = summary(g);
    changed(&s);
    drop(s);
    log(&format!("{summary} in {:.1} s", t0.elapsed().as_secs_f64()));
    for p in &found.problems {
        log(&format!("  {id}: {p}"));
    }
    Ok(summary)
}

fn summary(g: &Game) -> String {
    let has = |p: &Option<PathBuf>, name: &str| if p.is_some() { name.to_string() } else { format!("-{name}") };
    format!(
        "{} \"{}\" steam {}: {} {} {} {} shots {} {} {} news {} tagline {}",
        g.id,
        g.name,
        g.steam_app_id.map_or("-".into(), |i| i.to_string()),
        has(&g.art.tile, "tile"),
        has(&g.art.background, "background"),
        has(&g.art.logo, "logo"),
        has(&g.art.cover, "cover"),
        g.screenshots.len(),
        has(&g.trailer, "trailer"),
        has(&g.music, "music"),
        g.news.len(),
        if g.tagline.is_some() { "yes" } else { "no" },
    )
}

/// The watcher loop: a tick every 30 s (or when `rescan` wakes it). `root` overrides `gamesRoot` (CLI).
fn watch(root: Option<PathBuf>) {
    let mut w = Watch::default();
    loop {
        let r = root.clone().unwrap_or_else(crate::data::games_root);
        tick(&mut w, &r, &crate::games::is_running);
        w.report.clear();
        let mut woken = WAKE.0.lock().unwrap_or_else(|e| e.into_inner());
        if !*woken {
            woken = WAKE.1.wait_timeout(woken, TICK).unwrap_or_else(|e| e.into_inner()).0;
        }
        if std::mem::take(&mut *woken) {
            // rescan: a full pass, and another try for games whose enrichment failed
            w.seen = None;
            w.tried.clear();
        }
    }
}

// ---------- the command line ----------

/// `--sync-once [--root <dir>]`: one pass and enrichment without a window, a summary, then exit.
/// `--watch [--root <dir>]`: the watcher in the console, printing each `library` event, until Ctrl+C.
/// Returns the exit code, or `None` when the arguments aren't a library command (start the app).
pub fn cli(args: &[String]) -> Option<i32> {
    let once = args.iter().any(|a| a == "--sync-once");
    if !once && !args.iter().any(|a| a == "--watch") {
        return None;
    }
    attach_console();
    let mut root = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--sync-once" | "--watch" => {}
            "--root" => root = it.next().map(PathBuf::from),
            other => {
                eprintln!("unknown argument {other}\nusage: padwake.exe --sync-once|--watch [--root <dir>]");
                return Some(2);
            }
        }
    }
    load();
    let root = root.unwrap_or_else(crate::data::games_root);
    println!("library: {} -> {}", root.display(), crate::data::dir().display());
    if !once {
        let _ = NOTIFY.set(Box::new(|games| {
            let ids: Vec<_> = games.iter().map(|g| g.id.as_str()).collect();
            println!("{} library event: {} games: {}", timestamp(SystemTime::now()), games.len(), ids.join(", "));
        }));
        watch(Some(root.clone()));
    }
    let t0 = Instant::now();
    let mut w = Watch::default();
    if tick(&mut w, &root, &|| false) == Tick::Idle {
        eprintln!("games folder {} can't be read", root.display());
        return Some(1);
    }
    let games = list();
    for (id, result, took) in &w.report {
        match result {
            Ok(s) => println!("{s} ({:.1} s)", took.as_secs_f64()),
            Err(e) => println!("{id}: {e} ({:.1} s)", took.as_secs_f64()),
        }
    }
    for g in games.iter().filter(|g| !w.report.iter().any(|(id, ..)| id == &g.id)) {
        println!("{} (already synced)", summary(g));
    }
    let matched = games.iter().filter(|g| g.steam_app_id.is_some()).count();
    println!(
        "{} games, {} matched on Steam, {} waiting to be quiet; took {:.1} s",
        games.len(),
        matched,
        if w.waiting { "some" } else { "none" },
        t0.elapsed().as_secs_f64()
    );
    Some(0)
}

/// Release builds have no console of their own: print into the one that started us, if any.
fn attach_console() {
    use windows::Win32::System::Console::{AttachConsole, GetStdHandle, ATTACH_PARENT_PROCESS, STD_OUTPUT_HANDLE};
    unsafe {
        if GetStdHandle(STD_OUTPUT_HANDLE).map_or(true, |h| h.is_invalid() || h.0.is_null()) {
            let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_scan_while_a_game_runs() {
        let root = std::env::temp_dir().join(format!("ch-lib-running-{}", std::process::id()));
        std::fs::create_dir_all(root.join("Hades")).unwrap();
        let mut w = Watch::default();
        assert_eq!(tick(&mut w, &root, &|| true), Tick::Skipped);
        assert!(w.skipping && w.seen.is_none(), "nothing was scanned");
        assert_eq!(tick(&mut w, &root, &|| true), Tick::Skipped);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sessions_add_playtime() {
        state().games.push(Game { id: "session-test".into(), playtime_sec: 60, ..Default::default() });
        assert!(record_session("session-test", 90, 1_793_196_309_000));
        let g = get("session-test").unwrap();
        assert_eq!((g.playtime_sec, g.last_played), (150, Some(1_793_196_309_000)));
        assert!(!record_session("no-such-game", 90, 0));
        state().games.retain(|g| g.id != "session-test");
    }

    #[test]
    fn art_paths_are_saved_relative_and_load_under_the_current_data_dir() {
        let root = Path::new(r"D:\Padwake\data");
        let tile = root.join(r"library\hades\tile.jpg");
        let rel = to_relative(&tile, root);
        assert_eq!(rel, Path::new(r"library\hades\tile.jpg"));
        assert_eq!(to_absolute(&rel, root), tile);
        // an absolute entry under this data dir stays as it is
        assert_eq!(to_absolute(&tile, root), tile);
        // an old absolute entry from a data dir that moved (antcloud deploy, renamed app folder) lands under this one
        let moved = Path::new(r"W:\tmp\ch-target-e2e\release\data\library\hades\shot2.jpg");
        assert_eq!(to_absolute(moved, root), root.join(r"library\hades\shot2.jpg"));
        // anything else is left alone
        let other = Path::new(r"C:\Pictures\x.jpg");
        assert_eq!((to_relative(other, root), to_absolute(other, root)), (other.to_path_buf(), other.to_path_buf()));
        // every art and media path of a game round-trips; its folder and exe are not touched
        let mut g = Game {
            folder: r"W:\games-test\Hades".into(),
            art: Art { tile: Some(tile.clone()), logo: Some(root.join(r"library\hades\logo.png")), ..Default::default() },
            screenshots: vec![root.join(r"library\hades\shot1.jpg")],
            music: Some(root.join(r"library\hades\music.m4a")),
            ..Default::default()
        };
        let before = format!("{g:?}");
        media_paths(&mut g).for_each(|p| *p = to_relative(p, root));
        assert_eq!(g.art.tile.as_deref(), Some(Path::new(r"library\hades\tile.jpg")));
        assert_eq!(g.screenshots[0], Path::new(r"library\hades\shot1.jpg"));
        assert_eq!(g.music.as_deref(), Some(Path::new(r"library\hades\music.m4a")));
        assert_eq!(g.folder, Path::new(r"W:\games-test\Hades"));
        media_paths(&mut g).for_each(|p| *p = to_absolute(p, root));
        assert_eq!(format!("{g:?}"), before);
    }

    #[test]
    fn timestamps() {
        assert_eq!(timestamp(SystemTime::UNIX_EPOCH), "1970-01-01 00:00:00Z");
        assert_eq!(timestamp(SystemTime::UNIX_EPOCH + Duration::from_secs(1_793_196_309)), "2026-10-28 14:05:09Z");
    }
}
