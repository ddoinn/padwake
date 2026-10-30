//! System software updates from GitHub Releases, free and without sign-in.
//! - A release is a tag `v<version>` with the zip for new installs, `padwake.exe` for this updater and
//!   `SHA256SUMS.txt` ("<hex>  <file>" lines, as `sha256sum` writes them). `tools/app/release.ps1` builds and
//!   publishes one. The repository must be public (anonymous API).
//! - The app checks 60 s after it starts and every 6 h after that, never while a game runs (setting `autoUpdate`, on
//!   by default; release builds only). A newer version is downloaded into `data\update\` and checked (size, "MZ",
//!   SHA-256); then it is "ready".
//! - A ready update installs the next time the app starts, or now with `install` (Settings → System). Installing
//!   renames the running exe to `padwake.old.exe` (Windows allows renaming a running exe, not deleting it),
//!   copies the new one in its place and starts it with `--update-from <pid>`, which waits for this process to end.
//!   The old exe stays as the way back until the next update.

use super::github::{self, Http};
use crate::system::log;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Runtime};

/// Where releases come from unless settings.json has `updateRepo`.
pub const REPO: &str = "ddoinn/padwake";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
const EXE_ASSET: &str = "padwake.exe";
const SHA_ASSET: &str = "SHA256SUMS.txt";
const EXE_LIMIT: u64 = 200 << 20;
const FIRST_CHECK: Duration = Duration::from_secs(60);
const EVERY: Duration = Duration::from_secs(6 * 3600);
pub const UPDATE_FROM: &str = "--update-from";
pub const UPDATED_FROM: &str = "--updated-from";

#[derive(Serialize, Clone, Copy, PartialEq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum State {
    #[default]
    Idle,
    Checking,
    Downloading,
    /// downloaded and checked: installs at the next start, or now
    Ready,
    /// this is the newest version
    Latest,
    Error,
}

/// The `update` event and what `update_status` returns.
#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub current: String,
    pub state: State,
    /// the newer version, while downloading or ready
    pub latest: Option<String>,
    /// the release notes (Markdown as written on GitHub)
    pub notes: Option<String>,
    /// 0..1 while downloading
    pub progress: Option<f32>,
    pub checked_at: Option<u64>,
    pub error: Option<String>,
    /// the version this start was updated from (once, right after an update)
    pub updated_from: Option<String>,
    pub auto: bool,
}

/// `data\update\ready.json`: the downloaded update.
#[derive(Serialize, Deserialize, Clone, Debug)]
struct Staged {
    version: String,
    file: String,
    sha256: String,
    notes: Option<String>,
}

static STATUS: Mutex<Option<Status>> = Mutex::new(None);

fn status_mut() -> MutexGuard<'static, Option<Status>> {
    STATUS.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn status() -> Status {
    let mut s = status_mut();
    let st = s.get_or_insert_with(|| {
        let staged = staged();
        Status {
            current: CURRENT.into(),
            state: if staged.is_some() { State::Ready } else { State::Idle },
            latest: staged.as_ref().map(|s| s.version.clone()),
            notes: staged.and_then(|s| s.notes),
            updated_from: arg_after(UPDATED_FROM),
            ..Default::default()
        }
    });
    st.auto = auto();
    st.clone()
}

fn set<R: Runtime>(app: &AppHandle<R>, f: impl FnOnce(&mut Status)) {
    let _ = status();
    let snapshot = {
        let mut s = status_mut();
        let st = s.as_mut().expect("status was just made");
        f(st);
        st.clone()
    };
    let _ = app.emit("update", snapshot);
}

fn auto() -> bool {
    crate::data::setting::<bool>("autoUpdate").unwrap_or(true)
}

fn repo() -> String {
    crate::data::setting::<String>("updateRepo").filter(|r| r.contains('/')).unwrap_or_else(|| REPO.into())
}

fn dir() -> PathBuf {
    crate::data::dir().join("update")
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn arg_after(flag: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1).cloned())
}

/// The automatic checks (release builds; `PADWAKE_UPDATE_FIRST_SEC` sets the first wait, for tests).
pub fn start<R: Runtime>(app: AppHandle<R>) {
    if cfg!(debug_assertions) && std::env::var_os("PADWAKE_UPDATE_FIRST_SEC").is_none() {
        return; // a dev build must never get a release exe put in its place
    }
    let first = std::env::var("PADWAKE_UPDATE_FIRST_SEC")
        .ok()
        .and_then(|v| v.parse().ok())
        .map(Duration::from_secs)
        .unwrap_or(FIRST_CHECK);
    let _ = std::thread::Builder::new().name("update".into()).spawn(move || {
        std::thread::sleep(first);
        loop {
            if auto() && !crate::games::is_running() {
                check(&app);
            }
            std::thread::sleep(EVERY);
        }
    });
}

/// Looks for a newer release and downloads it. Returns the status after.
pub fn check<R: Runtime>(app: &AppHandle<R>) -> Status {
    if matches!(status().state, State::Checking | State::Downloading) {
        return status();
    }
    set(app, |s| {
        s.state = State::Checking;
        s.error = None;
    });
    let result = fetch(app);
    set(app, |s| {
        s.checked_at = Some(now_ms());
        s.progress = None;
        match result {
            Ok(Some(st)) => {
                s.state = State::Ready;
                s.latest = Some(st.version);
                s.notes = st.notes;
            }
            Ok(None) => {
                s.state = State::Latest;
                s.latest = None;
                s.notes = None;
            }
            Err(e) => {
                log(&format!("update: {e}"));
                s.state = State::Error;
                s.error = Some(e);
            }
        }
    });
    status()
}

/// The newest release, downloaded and checked; `None` when this version is the newest.
fn fetch<R: Runtime>(app: &AppHandle<R>) -> Result<Option<Staged>, String> {
    let http = Http::new();
    let repo = repo();
    let release = http.latest_release(&repo)?;
    if !github::newer(&release.tag_name, CURRENT) {
        return Ok(None);
    }
    let version = release.tag_name.trim_start_matches(['v', 'V']).to_string();
    if let Some(st) = staged().filter(|s| s.version == version) {
        return Ok(Some(st));
    }
    let asset = |name: &str| release.assets.iter().find(|a| a.name.eq_ignore_ascii_case(name));
    let exe = asset(EXE_ASSET).ok_or_else(|| format!("release {} has no {EXE_ASSET}", release.tag_name))?;
    let sha = asset(SHA_ASSET).ok_or_else(|| format!("release {} has no {SHA_ASSET}", release.tag_name))?;
    let sha_text = String::from_utf8_lossy(&http.download(&sha.browser_download_url, 64 << 10, &|_, _| {})?).into_owned();
    let want = parse_sha(&sha_text).ok_or_else(|| format!("{SHA_ASSET} has no line for {EXE_ASSET}"))?;

    log(&format!("update: downloading {version} from {repo}"));
    set(app, |s| {
        s.state = State::Downloading;
        s.latest = Some(version.clone());
        s.progress = Some(0.0);
    });
    let last = Mutex::new(0u32);
    let total_hint = exe.size;
    let bytes = http.download(&exe.browser_download_url, EXE_LIMIT, &|done, total| {
        let total = if total > 0 { total } else { total_hint.max(1) };
        let pct = (done * 100 / total).min(100) as u32;
        let mut last = last.lock().unwrap_or_else(|e| e.into_inner());
        if pct >= *last + 5 {
            *last = pct;
            set(app, |s| s.progress = Some(pct as f32 / 100.0));
        }
    })?;
    check_exe(&bytes, &want)?;

    let file = format!("padwake-{version}.exe");
    let d = dir();
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
    crate::library::write_file(&d.join(&file), &bytes)?;
    let st = Staged { version, file, sha256: want, notes: release.body.filter(|b| !b.trim().is_empty()) };
    let json = serde_json::to_vec_pretty(&st).map_err(|e| e.to_string())?;
    crate::library::write_file(&d.join("ready.json"), &json)?;
    log(&format!("update: {} is ready", st.version));
    Ok(Some(st))
}

/// The SHA-256 of `padwake.exe` in a checksum file: its "<hex>  padwake.exe" line (`*` before the name marks
/// binary mode), or a lone hash.
fn parse_sha(text: &str) -> Option<String> {
    let is_hex = |h: &str| h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit());
    let mut lone = None;
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let Some(hex) = parts.next().filter(|h| is_hex(h)) else { continue };
        match parts.next().map(|n| n.trim_start_matches('*')) {
            Some(name) if name.eq_ignore_ascii_case(EXE_ASSET) => return Some(hex.to_ascii_lowercase()),
            None if lone.is_none() => lone = Some(hex.to_ascii_lowercase()),
            _ => {}
        }
    }
    lone
}

/// A Windows exe whose SHA-256 is `want`.
fn check_exe(bytes: &[u8], want: &str) -> Result<(), String> {
    if bytes.len() < 1024 || &bytes[..2] != b"MZ" {
        return Err("the download is not a Windows program".into());
    }
    let got = super::crypto::sha256_hex(bytes)?;
    if got != want {
        return Err(format!("the download's SHA-256 is {got}, the release says {want}"));
    }
    Ok(())
}

/// The downloaded update, if its file is there.
fn staged() -> Option<Staged> {
    staged_in(&dir())
}

fn staged_in(d: &Path) -> Option<Staged> {
    let text = std::fs::read(d.join("ready.json")).ok()?;
    let st: Staged = serde_json::from_slice(&text).ok()?;
    let ok = github::newer(&st.version, CURRENT) && d.join(&st.file).is_file();
    if !ok {
        let _ = std::fs::remove_dir_all(d);
        return None;
    }
    Some(st)
}

/// Puts the update staged in `d` in place of `exe`. Returns the version installed, `None` if there was none.
fn swap(d: &Path, exe: &Path) -> Result<Option<String>, String> {
    let Some(st) = staged_in(d) else { return Ok(None) };
    let src = d.join(&st.file);
    let result = (|| {
        let bytes = std::fs::read(&src).map_err(|e| format!("{}: {e}", src.display()))?;
        check_exe(&bytes, &st.sha256)?;
        let old = exe.with_file_name("padwake.old.exe");
        let _ = std::fs::remove_file(&old);
        std::fs::rename(exe, &old).map_err(|e| format!("moving the running exe aside: {e}"))?;
        if let Err(e) = std::fs::write(exe, &bytes) {
            let _ = std::fs::remove_file(exe);
            let _ = std::fs::rename(&old, exe);
            return Err(format!("writing the new exe: {e}"));
        }
        Ok(())
    })();
    // a failed install isn't retried at every start; the next check downloads it again
    let _ = std::fs::remove_dir_all(d);
    result.map(|()| Some(st.version))
}

/// Installs the staged update into this exe and starts it once this process has ended. True if it did: the caller
/// must exit now.
fn install_and_relaunch() -> Result<bool, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let Some(version) = swap(&dir(), &exe)? else { return Ok(false) };
    log(&format!("update: installed {version} over {CURRENT}; starting it"));
    let dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
    Command::new(&exe)
        .args([UPDATE_FROM, &std::process::id().to_string(), UPDATED_FROM, CURRENT])
        .current_dir(dir)
        .spawn()
        .map_err(|e| format!("starting the new version: {e}"))?;
    Ok(true)
}

/// At start, once this is the only instance: a staged update installs now. True if the caller must exit (the new
/// version is starting).
pub fn install_at_start() -> bool {
    if cfg!(debug_assertions) {
        return false;
    }
    match install_and_relaunch() {
        Ok(go) => go,
        Err(e) => {
            log(&format!("update: couldn't install at start: {e}"));
            false
        }
    }
}

/// "Update now": installs the ready update and restarts into it. Refused while a game runs.
pub fn install<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    if crate::games::is_running() {
        return Err("Close the game first.".into());
    }
    if !install_and_relaunch()? {
        return Err("No update is ready.".into());
    }
    app.exit(0);
    Ok(())
}

/// `--update-from <pid>`: the previous version is still closing; wait for it (up to 20 s) before taking the single
/// instance.
pub fn wait_for_previous() {
    let args: Vec<String> = std::env::args().collect();
    let Some(pid) = args.iter().position(|a| a == UPDATE_FROM).and_then(|i| args.get(i + 1)?.parse::<u32>().ok()) else {
        return;
    };
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
    unsafe {
        if let Ok(h) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            WaitForSingleObject(h, 20_000);
            let _ = CloseHandle(h);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha_files_with_or_without_a_name() {
        let h = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(parse_sha(h).as_deref(), Some(h));
        assert_eq!(parse_sha(&format!("{}  padwake.exe\r\n", h.to_uppercase())).as_deref(), Some(h));
        assert_eq!(parse_sha("not a hash"), None);
        assert_eq!(parse_sha(""), None);
        let zip = "0".repeat(64);
        let sums = format!("{zip}  padwake-0.2.0-windows-x64.zip\n{h} *padwake.exe\n");
        assert_eq!(parse_sha(&sums).as_deref(), Some(h), "the exe's line, not the zip's");
        assert_eq!(parse_sha(&format!("{zip}  padwake-0.2.0-windows-x64.zip\n")), None, "no line for the exe");
    }

    #[test]
    fn only_a_matching_exe_passes() {
        let mut exe = vec![0u8; 2048];
        exe[..2].copy_from_slice(b"MZ");
        let sha = super::super::crypto::sha256_hex(&exe).unwrap();
        assert!(check_exe(&exe, &sha).is_ok());
        let mut other = exe.clone();
        other[100] = 1;
        assert!(check_exe(&other, &sha).unwrap_err().contains("SHA-256"));
        assert!(check_exe(b"<html>not found</html>", &sha).unwrap_err().contains("not a Windows program"));
    }

    #[test]
    fn swap_replaces_a_running_exe_and_keeps_the_old_one() {
        // on W:, beside the test binary (never C:); a copy of ping.exe stands in for the running app
        let tmp = std::env::current_exe().unwrap().parent().unwrap().join(format!("ch-swap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let staged = tmp.join("update");
        std::fs::create_dir_all(&staged).unwrap();
        let exe = tmp.join("padwake.exe");
        std::fs::copy(r"C:\Windows\System32\PING.EXE", &exe).unwrap();
        let mut running = Command::new(&exe).args(["-n", "30", "127.0.0.1"]).stdout(std::process::Stdio::null()).spawn().unwrap();

        let mut new = vec![7u8; 4096];
        new[..2].copy_from_slice(b"MZ");
        std::fs::write(staged.join("padwake-99.0.0.exe"), &new).unwrap();
        let st = Staged {
            version: "99.0.0".into(),
            file: "padwake-99.0.0.exe".into(),
            sha256: super::super::crypto::sha256_hex(&new).unwrap(),
            notes: None,
        };
        std::fs::write(staged.join("ready.json"), serde_json::to_vec(&st).unwrap()).unwrap();

        assert_eq!(swap(&staged, &exe).unwrap().as_deref(), Some("99.0.0"));
        assert_eq!(std::fs::read(&exe).unwrap(), new);
        assert!(tmp.join("padwake.old.exe").is_file(), "the old exe stays as the way back");
        assert!(!staged.exists(), "the staged update is used up");
        assert_eq!(swap(&staged, &exe).unwrap(), None);

        let _ = running.kill();
        let _ = running.wait();
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn a_bad_staged_file_is_not_installed() {
        let tmp = std::env::current_exe().unwrap().parent().unwrap().join(format!("ch-swap-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let staged = tmp.join("update");
        std::fs::create_dir_all(&staged).unwrap();
        let exe = tmp.join("padwake.exe");
        std::fs::write(&exe, b"MZ the app").unwrap();
        std::fs::write(staged.join("x.exe"), b"MZ tampered").unwrap();
        let st = Staged { version: "99.0.0".into(), file: "x.exe".into(), sha256: "0".repeat(64), notes: None };
        std::fs::write(staged.join("ready.json"), serde_json::to_vec(&st).unwrap()).unwrap();
        assert!(swap(&staged, &exe).is_err());
        assert_eq!(std::fs::read(&exe).unwrap(), b"MZ the app", "the app is untouched");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
