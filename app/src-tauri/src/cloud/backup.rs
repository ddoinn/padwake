//! Backup to GitHub, free: the library's time and the journal (`library/portable.rs`) in one private gist on the
//! player's own account, so a new or reset PC gets them back.
//! - Sign-in is GitHub's device flow, made for TVs: the page shows a code, the player types it at github.com/login/device
//!   on a phone. It needs the client ID of a GitHub OAuth app with Device Flow on (`CLIENT_ID`; no secret is involved).
//!   The token (scope `gist` only) is kept with DPAPI in `data\github.token`: only this Windows user on this PC can
//!   read it.
//! - After sign-in the app looks for the gist ("Padwake backup"). If it holds games, the page offers a restore.
//! - Each library change (a session ends, a review) marks the backup due; 2 min after the last change, when no game
//!   runs, the gist is updated (setting `backupAuto`, on by default). Unchanged content isn't sent again. What is sent
//!   keeps the other PC's games and time (`portable::combine`), so declining a restore loses nothing.
//! - State: `data\backup.json` (login, gist id, last save and its content hash). Events: `backup` with the status.

use super::github::{self, Http, Poll};
use crate::library::portable::{self, Backup, Entry, Restored};
use crate::system::log;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Runtime};

/// The client ID of the "Padwake" GitHub OAuth app (Settings → Developer settings → OAuth Apps, "Enable Device
/// Flow" on). Public by design; `PADWAKE_GITHUB_CLIENT_ID` overrides it. Empty: backup isn't offered.
const CLIENT_ID: &str = "";
const DELAY: Duration = Duration::from_secs(120);

#[derive(Serialize, Clone, Copy, PartialEq, Debug, Default)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    #[default]
    Idle,
    SigningIn,
    Saving,
    Restoring,
    Error,
}

/// The device flow's code while sign-in waits.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Code {
    pub user_code: String,
    pub uri: String,
    pub expires_at: u64,
}

/// A backup found at sign-in, not yet restored.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    pub saved_at: u64,
    pub machine: String,
    pub games: usize,
}

/// The `backup` event and what `backup_status` returns.
#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// this build can sign in (it has a client ID)
    pub available: bool,
    pub signed_in: bool,
    pub login: Option<String>,
    pub state: State,
    pub code: Option<Code>,
    pub saved_at: Option<u64>,
    pub auto: bool,
    pub error: Option<String>,
    pub found: Option<Found>,
    /// the last restore's result
    pub restored: Option<Restored>,
}

/// `data\backup.json`.
#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase", default)]
struct Saved {
    login: Option<String>,
    gist_id: Option<String>,
    saved_at: Option<u64>,
    hash: Option<String>,
}

struct Live {
    saved: Saved,
    state: State,
    code: Option<Code>,
    error: Option<String>,
    found: Option<Found>,
    restored: Option<Restored>,
}

static LIVE: OnceLock<Mutex<Live>> = OnceLock::new();
/// The library changed at this time and isn't backed up yet.
static DUE: (Mutex<Option<Instant>>, Condvar) = (Mutex::new(None), Condvar::new());
/// Bumped to stop a sign-in that is waiting.
static SIGN_IN: AtomicU64 = AtomicU64::new(0);
/// One upload or restore at a time.
static BUSY: Mutex<()> = Mutex::new(());

fn live() -> MutexGuard<'static, Live> {
    LIVE.get_or_init(|| {
        let saved = std::fs::read(saved_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Mutex::new(Live { saved, state: State::Idle, code: None, error: None, found: None, restored: None })
    })
    .lock()
    .unwrap_or_else(|e| e.into_inner())
}

fn saved_path() -> PathBuf {
    crate::data::dir().join("backup.json")
}

fn token_path() -> PathBuf {
    crate::data::dir().join("github.token")
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn client_id() -> Option<String> {
    std::env::var("PADWAKE_GITHUB_CLIENT_ID").ok().or_else(|| Some(CLIENT_ID.to_string())).filter(|c| !c.is_empty())
}

fn auto() -> bool {
    crate::data::setting::<bool>("backupAuto").unwrap_or(true)
}

fn delay() -> Duration {
    std::env::var("PADWAKE_BACKUP_DELAY_SEC").ok().and_then(|v| v.parse().ok()).map(Duration::from_secs).unwrap_or(DELAY)
}

fn token() -> Option<String> {
    let sealed = std::fs::read(token_path()).ok()?;
    let plain = super::crypto::unprotect(&sealed).ok()?;
    String::from_utf8(plain).ok().filter(|t| !t.is_empty())
}

fn persist(saved: &Saved) {
    match serde_json::to_vec_pretty(saved) {
        Ok(b) => {
            if let Err(e) = crate::library::write_file(&saved_path(), &b) {
                log(&format!("backup: {e}"));
            }
        }
        Err(e) => log(&format!("backup: {e}")),
    }
}

pub fn status() -> Status {
    let l = live();
    Status {
        available: client_id().is_some(),
        signed_in: l.saved.login.is_some() && token_path().is_file(),
        login: l.saved.login.clone(),
        state: l.state,
        code: l.code.clone(),
        saved_at: l.saved.saved_at,
        auto: auto(),
        error: l.error.clone(),
        found: l.found.clone(),
        restored: l.restored.clone(),
    }
}

fn update<R: Runtime>(app: &AppHandle<R>, f: impl FnOnce(&mut Live)) -> Status {
    f(&mut live());
    let s = status();
    let _ = app.emit("backup", s.clone());
    s
}

/// A library change: the backup is due after the quiet time.
pub fn library_changed() {
    *DUE.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
    DUE.1.notify_all();
}

/// The automatic backups: one thread that sleeps until the library changes.
pub fn start<R: Runtime>(app: AppHandle<R>) {
    let _ = std::thread::Builder::new().name("backup".into()).spawn(move || loop {
        let since = {
            let mut due = DUE.0.lock().unwrap_or_else(|e| e.into_inner());
            while due.is_none() {
                due = DUE.1.wait(due).unwrap_or_else(|e| e.into_inner());
            }
            due.unwrap_or_else(Instant::now)
        };
        let wait = delay().saturating_sub(since.elapsed());
        if !wait.is_zero() {
            std::thread::sleep(wait);
            continue; // look again: another change may have moved the time
        }
        if crate::games::is_running() {
            std::thread::sleep(Duration::from_secs(60));
            continue;
        }
        *DUE.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
        if auto() && status().signed_in {
            if let Err(e) = save(&app) {
                log(&format!("backup: {e}"));
            }
        }
    });
}

/// Uploads the backup now (unchanged content isn't sent again). Returns the status.
pub fn save<R: Runtime>(app: &AppHandle<R>) -> Result<Status, String> {
    let _one = BUSY.lock().unwrap_or_else(|e| e.into_inner());
    let token = token().ok_or("Sign in to GitHub first.")?;
    let local = crate::library::backup_entries();
    let hash = super::crypto::sha256_hex(&serde_json::to_vec(&local).map_err(|e| e.to_string())?)?;
    let saved = live().saved.clone();
    if saved.gist_id.is_some() && saved.hash.as_deref() == Some(hash.as_str()) {
        return Ok(status());
    }
    update(app, |l| {
        l.state = State::Saving;
        l.error = None;
    });
    let result = upload(&token, saved.gist_id.clone(), local);
    match result {
        Ok((gist_id, at)) => {
            let saved = update(app, |l| {
                l.saved.gist_id = Some(gist_id);
                l.saved.saved_at = Some(at);
                l.saved.hash = Some(hash);
                l.state = State::Idle;
                persist(&l.saved);
            });
            log("backup: saved");
            Ok(saved)
        }
        Err(e) => Err(fail(app, e)),
    }
}

/// Sends `local` (combined with what is there) to the gist, creating it if needed. Returns the gist id and the time.
fn upload(token: &str, gist_id: Option<String>, local: Vec<Entry>) -> Result<(String, u64), String> {
    let http = Http::new();
    let mut id = match gist_id {
        Some(id) => Some(id),
        None => http.find_gist(token)?,
    };
    let remote = match &id {
        Some(g) => match http.read_gist(token, g)? {
            Some(text) => parse(&text).map(|b| b.games).unwrap_or_default(),
            None => {
                id = None; // deleted on GitHub: make a new one
                Vec::new()
            }
        },
        None => Vec::new(),
    };
    let at = now_ms();
    let backup = Backup {
        app: "padwake".into(),
        format: portable::FORMAT,
        saved_at: at,
        machine: std::env::var("COMPUTERNAME").unwrap_or_default(),
        games: portable::combine(local, &remote),
    };
    let text = serde_json::to_string_pretty(&backup).map_err(|e| e.to_string())?;
    if let Some(g) = &id {
        if http.update_gist(token, g, &text)? {
            return Ok((g.clone(), at));
        }
    }
    Ok((http.create_gist(token, &text)?, at))
}

fn parse(text: &str) -> Result<Backup, String> {
    let b: Backup = serde_json::from_str(text).map_err(|e| format!("the backup isn't readable: {e}"))?;
    if b.app != "padwake" {
        return Err("that gist isn't a Padwake backup".into());
    }
    Ok(b)
}

/// Records an error in the status; a token GitHub no longer accepts signs out.
fn fail<R: Runtime>(app: &AppHandle<R>, e: String) -> String {
    let signed_out = e == github::SIGNED_OUT;
    let msg = if signed_out { "GitHub signed this PC out. Sign in again.".to_string() } else { e };
    if signed_out {
        forget_account();
    }
    update(app, |l| {
        l.state = State::Error;
        l.error = Some(msg.clone());
    });
    msg
}

fn forget_account() {
    let _ = std::fs::remove_file(token_path());
    let _ = std::fs::remove_file(saved_path());
    let mut l = live();
    l.saved = Saved::default();
    l.found = None;
}

// ---------- sign-in ----------

/// Starts the device flow: the status carries the code to show. Waits for the player in the background; the
/// `backup` event says when it's done (signed in, maybe with `found`) or failed.
pub fn sign_in<R: Runtime>(app: &AppHandle<R>) -> Result<Status, String> {
    let client = client_id().ok_or("GitHub sign-in isn't set up in this version of Padwake.")?;
    let http = Http::new();
    let dc = http.device_code(&client)?;
    let run = SIGN_IN.fetch_add(1, Ordering::SeqCst) + 1;
    let s = update(app, |l| {
        l.state = State::SigningIn;
        l.error = None;
        l.code = Some(Code {
            user_code: dc.user_code.clone(),
            uri: dc.verification_uri.clone(),
            expires_at: now_ms() + dc.expires_in * 1000,
        });
    });
    let app = app.clone();
    std::thread::spawn(move || {
        let ends = Instant::now() + Duration::from_secs(dc.expires_in);
        let mut every = Duration::from_secs(dc.interval.max(1));
        let token = loop {
            std::thread::sleep(every);
            if SIGN_IN.load(Ordering::SeqCst) != run {
                return; // cancelled, or a newer sign-in
            }
            if Instant::now() > ends {
                break Err("The code expired. Sign in again for a new one.".to_string());
            }
            match http.poll_token(&client, &dc.device_code) {
                Poll::Token(t) => break Ok(t),
                Poll::Pending => {}
                Poll::SlowDown => every += Duration::from_secs(5),
                Poll::Failed(e) => break Err(e),
            }
        };
        if SIGN_IN.load(Ordering::SeqCst) != run {
            return;
        }
        match token.and_then(|t| signed_in(&http, &t)) {
            Ok((login, gist_id, found)) => {
                log(&format!("backup: signed in as {login}"));
                update(&app, |l| {
                    l.saved = Saved { login: Some(login), gist_id, ..Saved::default() };
                    l.state = State::Idle;
                    l.code = None;
                    l.found = found;
                    persist(&l.saved);
                });
                library_changed(); // the first backup (it keeps what the gist already has)
            }
            Err(e) => {
                let _ = std::fs::remove_file(token_path());
                update(&app, |l| {
                    l.state = State::Error;
                    l.code = None;
                    l.error = Some(e);
                });
            }
        }
    });
    Ok(s)
}

/// After the token: keeps it, and finds the account and its backup.
fn signed_in(http: &Http, token: &str) -> Result<(String, Option<String>, Option<Found>), String> {
    let login = http.login(token)?;
    let sealed = super::crypto::protect(token.as_bytes())?;
    crate::library::write_file(&token_path(), &sealed)?;
    let gist_id = http.find_gist(token)?;
    let found = match &gist_id {
        Some(id) => http.read_gist(token, id)?.and_then(|t| parse(&t).ok()).and_then(|b| {
            let games = b.games.iter().filter(|e| e.playtime_sec > 0 || e.last_played.is_some() || !e.journal.is_empty()).count();
            (games > 0).then(|| Found { saved_at: b.saved_at, machine: b.machine, games })
        }),
        None => None,
    };
    Ok((login, gist_id, found))
}

pub fn cancel_sign_in<R: Runtime>(app: &AppHandle<R>) -> Status {
    SIGN_IN.fetch_add(1, Ordering::SeqCst);
    update(app, |l| {
        if l.state == State::SigningIn {
            l.state = State::Idle;
        }
        l.code = None;
    })
}

pub fn sign_out<R: Runtime>(app: &AppHandle<R>) -> Status {
    SIGN_IN.fetch_add(1, Ordering::SeqCst);
    let _one = BUSY.lock().unwrap_or_else(|e| e.into_inner());
    forget_account();
    log("backup: signed out");
    update(app, |l| {
        l.state = State::Idle;
        l.code = None;
        l.error = None;
        l.restored = None;
    })
}

/// Merges the gist's backup into the library. Returns the status with `restored`.
pub fn restore<R: Runtime>(app: &AppHandle<R>) -> Result<Status, String> {
    let _one = BUSY.lock().unwrap_or_else(|e| e.into_inner());
    let token = token().ok_or("Sign in to GitHub first.")?;
    update(app, |l| {
        l.state = State::Restoring;
        l.error = None;
    });
    let http = Http::new();
    let result = (|| {
        let id = match live().saved.gist_id.clone() {
            Some(id) => id,
            None => http.find_gist(&token)?.ok_or("There's no backup on this GitHub account yet.")?,
        };
        let text = http.read_gist(&token, &id)?.ok_or("The backup gist was deleted on GitHub.")?;
        Ok::<_, String>((id, crate::library::restore(&parse(&text)?.games)))
    })();
    match result {
        Ok((id, r)) => Ok(update(app, |l| {
            l.saved.gist_id = Some(id);
            persist(&l.saved);
            l.state = State::Idle;
            l.found = None;
            l.restored = Some(r);
        })),
        Err(e) => Err(fail(app, e)),
    }
}

/// "Not now" on the restore offer.
pub fn dismiss_found<R: Runtime>(app: &AppHandle<R>) -> Status {
    update(app, |l| l.found = None)
}
