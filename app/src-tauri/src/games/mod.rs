//! Game sessions (rows A1, A5): launch, track, Guide over a running game, back to it, Close Game, and playtime.
//! One session at a time. `start` is the API Media (row A12) reuses for Edge app windows (`Kind::Media`).
//! - Launch: the command runs in a job object (`session.rs`); the app sleeps (hidden, web view suspended) meanwhile.
//! - Tracking is an OS wait on the job, then on processes run from the game's folder (launcher stubs). A foreground hook
//!   (`front.rs`) notes the game's window when it first comes to the front: "Ready to play".
//! - Guide while the game is in front: the pad thread asks `guide` before the page. A tap uses a still of the screen
//!   (`backdrop.rs`, Control Center's backdrop) copied as Guide went down (`guide_down`: a tap is only known at the
//!   release, and the copy takes 45-95 ms), a hold doesn't; both send `game` {state: "over", show}, wake the window
//!   above the game and then send the `guide` event, so the page handles Guide as always. After a tap the window waits
//!   (up to 200 ms) for the page's `over_ready`: the still decoded and on its stage, so the first frame shows the game.
//! - `resume` sleeps the app and brings the game's window to the front; `close` asks the game to close and ends it
//!   after 5 s. When the session ends: playtime and last played (`library::record_session`), then Home.

mod backdrop;
mod front;
pub mod session;
mod window;

pub use window::{sleep, wake};

use serde::Serialize;
use session::Tracked;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{AppHandle, Emitter, Runtime};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, ASFW_ANY};

/// Close Game asks first, then ends what is left after this long.
pub const CLOSE_GRACE: Duration = Duration::from_secs(5);

#[derive(Serialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Game,
    #[allow(dead_code)] // row A12 starts Media sessions
    Media,
}

/// Who is in front while a session runs: the game (the app sleeps) or the app (Home or Control Center over the game).
#[derive(Serialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Front {
    Game,
    App,
}

struct Live {
    kind: Kind,
    id: String,
    name: String,
    started: Instant,
    started_at: u64,
    tracked: Arc<Tracked>,
    front: Front,
    /// the session's window, last seen in front
    window: Option<isize>,
    ready: bool,
}

static LIVE: Mutex<Option<Live>> = Mutex::new(None);
/// Set by the page (`over_ready`) once the still is decoded and on its stage: the window may show.
static OVER_READY: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());
/// Guide tap over a game shows the window at the latest this long after the still was taken.
const OVER_READY_WAIT: Duration = Duration::from_millis(200);
/// The screen copied as Guide went down over the game, when, and how long the copy took. A tap uses it up to this old.
static PRESS_STILL: Mutex<Option<(Instant, Result<Duration, String>)>> = Mutex::new(None);
const PRESS_STILL_FRESH: Duration = Duration::from_millis(1000);

fn live() -> MutexGuard<'static, Option<Live>> {
    LIVE.lock().unwrap_or_else(|e| e.into_inner())
}

/// The `game` event. `state`: "launching", "running", "ready" (its window came to the front), "over" (the app shows
/// over it after Guide; `show` is "control-center" or "home") or "exited" (with `front`).
#[derive(Serialize, Clone)]
struct GameEvent<'a> {
    id: &'a str,
    kind: Kind,
    state: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    show: Option<&'a str>,
    /// on "exited": who was in front as it ended ("app": Home was already showing over it)
    #[serde(skip_serializing_if = "Option::is_none")]
    front: Option<Front>,
}

fn emit<R: Runtime>(app: &AppHandle<R>, id: &str, kind: Kind, state: &str, show: Option<&str>) {
    let _ = app.emit("game", GameEvent { id, kind, state, show, front: None });
}

/// What `games|session` returns.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub kind: Kind,
    pub name: String,
    pub front: Front,
    pub started_at: u64,
    pub ready: bool,
}

pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("games")
        .invoke_handler(tauri::generate_handler![launch, close, resume, session, backdrop, over_ready])
        .setup(|app, _api| {
            let app = app.clone();
            front::start(move |hwnd, pid| came_to_front(&app, hwnd, pid));
            Ok(())
        })
        .build()
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn log(line: &str) {
    crate::system::log(&format!("game: {line}"));
}

// ---------- the session API (Media reuses it) ----------

/// Starts a session: spawns `cmd` (this sets its creation flags) in a job, sleeps the app, and tracks it until no
/// process of it remains; `folder` also counts processes run from that folder. Emits `game` events with `id` and `kind`.
/// Errors if a session already runs.
pub fn start<R: Runtime>(
    app: &AppHandle<R>,
    kind: Kind,
    id: &str,
    name: &str,
    mut cmd: Command,
    folder: Option<PathBuf>,
) -> Result<(), String> {
    let tracked = {
        let mut live = live();
        if let Some(other) = live.as_ref() {
            return Err(format!("{} is already running", other.name));
        }
        emit(app, id, kind, "launching", None);
        let tracked = match Tracked::spawn(&mut cmd, folder) {
            Ok(t) => Arc::new(t),
            Err(e) => {
                emit(app, id, kind, "exited", None);
                log(&format!("{id} failed to start: {e}"));
                return Err(e);
            }
        };
        // the player just pressed A in our window, so we may hand the foreground on: to the game, or to whatever a
        // launcher stub starts
        unsafe {
            let _ = AllowSetForegroundWindow(ASFW_ANY);
        }
        *live = Some(Live {
            kind,
            id: id.to_string(),
            name: name.to_string(),
            started: Instant::now(),
            started_at: now_ms(),
            tracked: tracked.clone(),
            front: Front::Game,
            window: None,
            ready: false,
        });
        tracked
    };
    log(&format!("{id} started (pid {}, folder {:?})", tracked.pid, tracked.folder));
    emit(app, id, kind, "running", None);
    sleep(app);

    let handle = app.clone();
    let waiter = std::thread::Builder::new().name("session-wait".into()).spawn(move || {
        tracked.wait();
        end(&handle);
    });
    if let Err(e) = waiter {
        // without a waiter nobody would bring Home back, so bring it back now
        end(app);
        return Err(format!("session wait thread: {e}"));
    }
    Ok(())
}

/// True while a session runs (the library doesn't scan then).
pub fn is_running() -> bool {
    live().is_some()
}

/// The running session, if any.
pub fn current() -> Option<Session> {
    live().as_ref().map(|l| Session {
        id: l.id.clone(),
        kind: l.kind,
        name: l.name.clone(),
        front: l.front,
        started_at: l.started_at,
        ready: l.ready,
    })
}

/// Back to the session: the app sleeps and the session's window comes to the front.
pub fn back_to_session<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let (tracked, hint) = {
        let mut live = live();
        let l = live.as_mut().ok_or("nothing is running")?;
        l.front = Front::Game;
        (l.tracked.clone(), l.window)
    };
    window::on_top(app, false);
    // we are in front now, so we may give the foreground away; then hide
    if let Some(w) = tracked.main_window(hint) {
        window::bring_to_front(w);
    }
    sleep(app);
    // the session may have ended meanwhile: its waiter woke Home before the sleep above, and with no session Guide
    // couldn't bring it back. (Holding LIVE across this instead could deadlock: sleep waits on the main thread.)
    if !is_running() {
        log("the session ended while resuming: Home comes back");
        wake(app);
    }
    Ok(())
}

/// Closes the running session `id` (or any, if `id` is None): WM_CLOSE to its windows, then after `CLOSE_GRACE` ends
/// the processes left in its job or run from its folder. Blocks for up to the grace time. The waiter then ends the
/// session as for any exit.
pub fn close_session(id: Option<&str>) -> Result<session::Closed, String> {
    let tracked = {
        let live = live();
        let l = live.as_ref().ok_or("nothing is running")?;
        if id.is_some_and(|id| id != l.id) {
            return Err(format!("{} is not running", id.unwrap_or_default()));
        }
        l.tracked.clone()
    };
    let closed = tracked.close(CLOSE_GRACE);
    log(&format!("close: asked {} window(s), {}; ended {:?}", closed.windows, if closed.asked { "it closed" } else { "it stayed" }, closed.ended));
    Ok(closed)
}

/// Mouse mode starts: the app sleeps so another window can be used. With a session, its window comes back to the front.
pub fn to_background<R: Runtime>(app: &AppHandle<R>) {
    if back_to_session(app).is_err() {
        window::on_top(app, false);
        sleep(app);
    }
}

/// Mouse mode ended: without a session the app comes back; with one, the game stays in front.
pub fn from_background<R: Runtime>(app: &AppHandle<R>) {
    if !is_running() {
        wake(app);
    }
}

/// Guide went down (pad thread). While the session's window is in front, the page starts resuming (the window stays
/// hidden) and the screen is copied now, so a tap, known only at the release, finds the still ready. A hold drops it.
pub fn guide_down<R: Runtime>(app: &AppHandle<R>) {
    if !matches!(live().as_ref(), Some(l) if l.front == Front::Game) {
        return;
    }
    window::resume_page(app);
    let took = crate::system::without_toasts(app, backdrop::capture);
    *PRESS_STILL.lock().unwrap_or_else(|e| e.into_inner()) = Some((Instant::now(), took));
}

/// Guide from the pad thread, before the page gets it. While a session's window is in front this wakes the app above
/// it and returns true; the page then gets `game` {state: "over"} and the `guide` event itself.
pub fn guide<R: Runtime>(app: &AppHandle<R>, kind: &str) -> bool {
    let t0 = Instant::now();
    let (id, skind) = {
        let mut live = live();
        let Some(l) = live.as_mut() else { return false };
        if l.front != Front::Game {
            return false;
        }
        l.front = Front::App;
        (l.id.clone(), l.kind)
    };
    // the page starts resuming (the window stays hidden) while the screen is copied, and hears "over" before the
    // window shows, so it fetches the still meanwhile; Guide's event itself comes once the window is up
    window::resume_page(app);
    let pressed = PRESS_STILL.lock().unwrap_or_else(|e| e.into_inner()).take().filter(|(at, _)| at.elapsed() < PRESS_STILL_FRESH);
    let at_press = pressed.is_some();
    let took = match (kind, pressed) {
        ("tap", Some((_, took))) => took,
        ("tap", None) => crate::system::without_toasts(app, backdrop::capture),
        _ => Ok(Duration::ZERO),
    };
    let show = if kind == "tap" { "control-center" } else { "home" };
    *OVER_READY.0.lock().unwrap_or_else(|e| e.into_inner()) = false;
    emit(app, &id, skind, "over", Some(show));
    // a tap: the page decodes the still (half size) before the window shows, so its first frame is the game, not black
    let mut waited = Duration::ZERO;
    if kind == "tap" && took.is_ok() {
        let t1 = Instant::now();
        let ready = OVER_READY.0.lock().unwrap_or_else(|e| e.into_inner());
        let _ = OVER_READY.1.wait_timeout_while(ready, OVER_READY_WAIT, |r| !*r);
        waited = t1.elapsed();
    }
    // a tap's page has the still up already: Control Center may start opening while the window shows
    if kind == "tap" {
        let _ = app.emit("guide", serde_json::json!({ "kind": kind }));
    }
    window::on_top(app, true);
    wake(app);
    // a "Ready to play" toast still showing stays above the app, at its spot (design/ps5/1-home.md §1.9)
    crate::system::raise_toasts(app);
    if kind != "tap" {
        let _ = app.emit("guide", serde_json::json!({ "kind": kind }));
    }
    match took {
        Ok(c) => log(&format!(
            "guide {kind} over {id}: still {:.1} ms{}, page ready after {:.1} ms more, awake {:.1} ms",
            c.as_secs_f64() * 1000.0,
            if at_press { " (at the press)" } else { "" },
            waited.as_secs_f64() * 1000.0,
            t0.elapsed().as_secs_f64() * 1000.0
        )),
        Err(e) => log(&format!("guide {kind} over {id}: no still ({e})")),
    }
    true
}

/// The session ended: playtime and last played for a game, then the app comes back and Home follows `exited`.
fn end<R: Runtime>(app: &AppHandle<R>) {
    let Some(l) = live().take() else { return };
    let secs = l.started.elapsed().as_secs_f64().round() as u64;
    if l.kind == Kind::Game && !crate::library::record_session(&l.id, secs, now_ms()) {
        log(&format!("{} is no longer in the library; {secs} s not recorded", l.id));
    }
    log(&format!("{} ended after {secs} s", l.id));
    backdrop::clear();
    window::on_top(app, false);
    wake(app);
    let _ = app.emit("game", GameEvent { id: &l.id, kind: l.kind, state: "exited", show: None, front: Some(l.front) });
}

/// A window of another process came to the front. The session's first one means the game is up: "Ready to play".
fn came_to_front<R: Runtime>(app: &AppHandle<R>, hwnd: isize, pid: u32) {
    if pid == unsafe { GetCurrentProcessId() } {
        return;
    }
    let ready = {
        let mut live = live();
        let Some(l) = live.as_mut() else { return };
        if !l.tracked.contains(pid) {
            return;
        }
        l.window = Some(hwnd);
        if l.ready {
            return;
        }
        l.ready = true;
        (l.id.clone(), l.kind, l.name.clone(), l.front)
    };
    let (id, kind, name, front) = ready;
    log(&format!("{id} is up (window {hwnd:#x}, pid {pid})"));
    emit(app, &id, kind, "ready", None);
    if front == Front::Game {
        // the app is asleep behind the game: the toast goes in a small window of its own above it
        let icon = crate::library::get(&id).and_then(|g| g.art.tile);
        crate::system::overlay_toast(app, &name, "Ready to play.", icon);
    }
}

// ---------- commands ----------

/// Launches a library game: its exe, in its folder, tracked with that folder.
#[tauri::command]
async fn launch<R: Runtime>(app: AppHandle<R>, id: String) -> Result<(), String> {
    let game = crate::library::get(&id).ok_or_else(|| format!("no game {id}"))?;
    let mut cmd = Command::new(&game.exe);
    cmd.current_dir(&game.folder);
    start(&app, Kind::Game, &id, &game.name, cmd, Some(game.folder.clone()))
}

/// Close Game: resolves once the game was asked and, if it stayed, ended (up to 5 s). `game` "exited" follows.
#[tauri::command]
async fn close(id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || close_session(Some(&id)).map(|_| ()))
        .await
        .map_err(|e| e.to_string())?
}

/// Back to the running game (Resume, or Back to game in Control Center).
#[tauri::command]
async fn resume<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    back_to_session(&app)
}

/// The page has the still on screen (Guide tap over a game): the window may show now.
#[tauri::command]
fn over_ready() {
    *OVER_READY.0.lock().unwrap_or_else(|e| e.into_inner()) = true;
    OVER_READY.1.notify_all();
}

#[tauri::command]
fn session() -> Option<Session> {
    current()
}

/// The still taken as Guide was pressed over the game, as a BMP: `size` "small" (half size, sent first) or "full".
#[tauri::command]
async fn backdrop(size: Option<String>) -> Result<tauri::ipc::Response, String> {
    let small = size.as_deref() == Some("small");
    let still = tauri::async_runtime::spawn_blocking(move || backdrop::still(small)).await.map_err(|e| e.to_string())?;
    still.map(tauri::ipc::Response::new).ok_or_else(|| "no still".into())
}
