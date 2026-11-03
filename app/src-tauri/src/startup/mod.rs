//! Start-up and the living room: one instance, "Start with Windows" (the logon task, `task.rs`), and at each start
//! the window in front with input (`keep_in_front`), Game Bar's claim on Guide turned off (`gamebar.rs`) and the crash
//! watchdog (`watchdog.rs`). The page asks `info` whether this start is a restart after a crash.

mod gamebar;
mod task;
mod watchdog;

pub use watchdog::cli as watchdog_cli;

use serde::Serialize;
use std::time::Duration;
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{AppHandle, Manager, RunEvent, Runtime};
use windows::core::w;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, GetForegroundWindow, SetForegroundWindow};

pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("startup")
        .invoke_handler(tauri::generate_handler![with_windows, set_with_windows, info, note])
        .on_event(|app, event| {
            if let RunEvent::Ready = event {
                on_start(app);
            }
        })
        .build()
}

/// True if the logon task "Padwake" exists.
#[tauri::command]
async fn with_windows() -> bool {
    task::exists()
}

/// Adds or removes the logon task; returns whether it exists now.
#[tauri::command]
async fn set_with_windows(on: bool) -> Result<bool, String> {
    task::set(on)
}

#[derive(Serialize)]
struct Info {
    /// started again in the middle of a sitting: the page skips the light show
    restarted: bool,
}

/// A PC that came on less than this ago still shows the start screen, whatever restarted the app: a crash while the
/// stream connects, or an update installed at boot, would hide it before anyone had seen it.
const JUST_ON: Duration = Duration::from_secs(10 * 60);

/// Skip the start screen? Only if the app restarted mid-sitting: "Update and Restart" in Settings, or a crash once the
/// PC has been on a while.
fn skip_start(crashed: bool, updated: bool, updated_at_start: bool, up: Duration) -> bool {
    (updated && !updated_at_start) || (crashed && up >= JUST_ON)
}

fn skip() -> bool {
    let has = |flag: &str| std::env::args().any(|a| a == flag);
    let up = Duration::from_millis(unsafe { GetTickCount64() });
    skip_start(restarted(), has(crate::cloud::UPDATE_FROM), has(crate::cloud::UPDATED_AT_START), up)
}

#[tauri::command]
fn info() -> Info {
    Info { restarted: skip() }
}

/// A line from the start screen for the log (what dismissed it, and when).
#[tauri::command]
fn note(text: String) {
    crate::system::log(&format!("start screen: {text}"));
}

pub fn restarted() -> bool {
    std::env::args().any(|a| a == watchdog::RESTARTED)
}

/// True for the first instance. A second one brings the first to the front and should exit.
pub fn claim_single_instance() -> bool {
    unsafe {
        // the handle stays open for the life of the process, which is what holds the claim
        let first = match CreateMutexW(None, true, w!("Local\\Padwake.Instance")) {
            Ok(_) => GetLastError() != ERROR_ALREADY_EXISTS,
            Err(_) => true,
        };
        if !first {
            // only raise it: a hidden one is asleep behind a game and must be woken by its own process
            if let Ok(h) = FindWindowW(None, w!("Padwake")) {
                let _ = SetForegroundWindow(h);
            }
        }
        first
    }
}

/// The main window exists: start the watchdog, release Guide from Game Bar, and get the window in front.
fn on_start<R: Runtime>(app: &AppHandle<R>) {
    crate::system::log(&format!(
        "startup: pid {}{}{}, PC on for {} s, start screen {}",
        std::process::id(),
        if restarted() { ", restarted after a crash" } else { "" },
        if std::env::args().any(|a| a == crate::cloud::UPDATE_FROM) { ", just updated" } else { "" },
        unsafe { GetTickCount64() } / 1000,
        if skip() { "skipped" } else { "shown" },
    ));
    watchdog::spawn();
    std::thread::spawn(gamebar::release_guide);
    let app = app.clone();
    std::thread::spawn(move || keep_in_front(&app));
}

/// Windows lets an app started by a task (or by the watchdog) take the foreground only right after input, and other
/// sign-in apps (store launchers, chat) take it while they start. So for the first 15 s, whenever the window is
/// shown but not in front, `games::wake` brings it back the way it does after a game: F24 (a key no app uses), then
/// SetForegroundWindow and keyboard focus in the page. A window asleep behind a game is left alone.
fn keep_in_front<R: Runtime>(app: &AppHandle<R>) {
    for wait in [0, 500, 1500, 3000, 5000, 5000] {
        std::thread::sleep(Duration::from_millis(wait));
        let Some(w) = app.get_webview_window("main") else { return };
        if crate::games::is_running() || !w.is_visible().unwrap_or(false) {
            return;
        }
        let Ok(h) = w.hwnd() else { return };
        if unsafe { GetForegroundWindow() } == h {
            continue;
        }
        crate::games::wake(app);
        let now = unsafe { GetForegroundWindow() } == h;
        crate::system::log(&format!("startup: window brought to the front ({})", if now { "done" } else { "refused" }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_start_screen_goes_only_for_a_restart_mid_sitting() {
        let (just_on, later) = (Duration::from_secs(90), Duration::from_secs(3600));
        assert!(!skip_start(false, false, false, just_on), "an ordinary start");
        assert!(!skip_start(false, false, false, later), "started by hand on a PC that has been on");
        assert!(!skip_start(true, false, false, just_on), "a crash while the PC comes on and the stream connects");
        assert!(skip_start(true, false, false, later), "a crash mid-sitting goes straight back to Home");
        assert!(!skip_start(false, true, true, just_on), "an update installed as the PC came on");
        assert!(!skip_start(false, true, true, later), "an update installed at a later start");
        assert!(skip_start(false, true, false, later), "Update and Restart in Settings");
    }
}
