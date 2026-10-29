//! The crash watchdog. Once the app owns the single instance it starts `padwake.exe --watchdog <pid>`. That
//! process starts a copy of itself with `--detached` and exits at once, so the watchdog leaves the app's process tree
//! (Task Manager's End task on the app ends its whole tree). The copy opens the app's process and waits on it with
//! `WaitForSingleObject`: no polling, no CPU. When the app ends, `should_restart` decides from its exit code, how long
//! it ran and whether Windows is shutting down:
//! - a clean exit (Exit to desktop, Alt+F4: code 0) ends the watchdog too;
//! - a crash or a kill (any other code) starts the app again with `--restarted`, unless it died in its first
//!   `MIN_UPTIME` (a crash at start would loop) or Windows is signing out or shutting down.
//!
//! The restarted app owns the instance (the dead one's mutex went with it) and starts its own watchdog.
//! Release builds only; `PADWAKE_NO_WATCHDOG=1` turns it off.

use crate::system::log;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{
    GetExitCodeProcess, GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, WaitForSingleObject,
    CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS, INFINITE, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE,
};
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_SHUTTINGDOWN};

pub const FLAG: &str = "--watchdog";
pub const RESTARTED: &str = "--restarted";
const DETACHED: &str = "--detached";
/// A crash sooner than this after start isn't retried.
pub const MIN_UPTIME: Duration = Duration::from_secs(5);

pub fn enabled() -> bool {
    !cfg!(debug_assertions) && std::env::var("PADWAKE_NO_WATCHDOG").map_or(true, |v| v != "1")
}

/// Restart the app? `code` is its exit code, `ran` how long it ran.
pub fn should_restart(code: u32, ran: Duration, shutting_down: bool) -> bool {
    code != 0 && ran >= MIN_UPTIME && !shutting_down
}

fn start(args: &[&str]) -> std::io::Result<u32> {
    let exe = std::env::current_exe()?;
    let dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
    let child = Command::new(&exe)
        .args(args)
        .current_dir(dir)
        .creation_flags((DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP).0)
        .spawn()?;
    Ok(child.id())
}

/// Called by the app once it owns the single instance.
pub fn spawn() {
    if !enabled() {
        log("watchdog: off (debug build or PADWAKE_NO_WATCHDOG=1)");
        return;
    }
    let pid = std::process::id().to_string();
    if let Err(e) = start(&[FLAG, &pid]) {
        log(&format!("watchdog: couldn't start: {e}"));
    }
}

/// `--watchdog <pid> [--detached]`: `Some(exit code)` if these are watchdog arguments.
pub fn cli(args: &[String]) -> Option<i32> {
    let i = args.iter().position(|a| a == FLAG)?;
    let Some(pid) = args.get(i + 1).and_then(|p| p.parse::<u32>().ok()) else {
        eprintln!("usage: padwake.exe --watchdog <pid>");
        return Some(2);
    };
    if !args.iter().any(|a| a == DETACHED) {
        // out of the app's process tree: the copy is nobody's child once this one exits
        return Some(match start(&[FLAG, &pid.to_string(), DETACHED]) {
            Ok(_) => 0,
            Err(e) => {
                log(&format!("watchdog: couldn't start: {e}"));
                1
            }
        });
    }
    Some(watch(pid))
}

fn watch(pid: u32) -> i32 {
    let Some((code, ran)) = wait_for(pid) else {
        log(&format!("watchdog: app {pid} isn't running"));
        return 1;
    };
    let shutting_down = unsafe { GetSystemMetrics(SM_SHUTTINGDOWN) } != 0;
    let why = format!("app {pid} ended with code {code:#x} after {:.1} s", ran.as_secs_f64());
    if !should_restart(code, ran, shutting_down) {
        let reason = if code == 0 {
            "a clean exit"
        } else if shutting_down {
            "Windows is shutting down"
        } else {
            "it crashed at start"
        };
        log(&format!("watchdog: {why}; not restarting ({reason})"));
        return 0;
    }
    match start(&[RESTARTED]) {
        Ok(new) => log(&format!("watchdog: {why}; restarted as {new}")),
        Err(e) => log(&format!("watchdog: {why}; restart failed: {e}")),
    }
    0
}

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// Waits for process `pid` to end (an OS wait); its exit code and how long it ran. `None` if it isn't this program.
fn wait_for(pid: u32) -> Option<(u32, Duration)> {
    let h = Handle(unsafe { OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?);
    // the same exe, not a process that took over a freed pid
    let me = std::env::current_exe().ok()?;
    if !same_file(&image(h.0)?, &me) {
        return None;
    }
    if unsafe { WaitForSingleObject(h.0, INFINITE) } != WAIT_OBJECT_0 {
        return None;
    }
    let mut code = 0u32;
    unsafe { GetExitCodeProcess(h.0, &mut code) }.ok()?;
    let (mut created, mut exited, mut kernel, mut user) = Default::default();
    unsafe { GetProcessTimes(h.0, &mut created, &mut exited, &mut kernel, &mut user) }.ok()?;
    Some((code, Duration::from_nanos(ticks(exited).saturating_sub(ticks(created)) * 100)))
}

fn ticks(t: FILETIME) -> u64 {
    (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime)
}

fn image(h: HANDLE) -> Option<PathBuf> {
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    unsafe { QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) }.ok()?;
    Some(PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])))
}

fn same_file(a: &Path, b: &Path) -> bool {
    a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_exit_ends_the_watchdog_and_a_crash_restarts() {
        let long = Duration::from_secs(600);
        assert!(!should_restart(0, long, false), "Exit to desktop / Alt+F4 (code 0) is clean");
        assert!(should_restart(1, long, false), "taskkill /F and Task Manager's End task give code 1");
        assert!(should_restart(0xC000_0005, long, false), "an access violation");
        assert!(should_restart(0xC000_0409, long, false), "a Rust panic (abort, fast fail)");
        assert!(!should_restart(1, Duration::from_secs(2), false), "a crash at start would loop");
        assert!(should_restart(1, MIN_UPTIME, false));
        assert!(!should_restart(1, long, true), "Windows is signing out or shutting down");
    }

    #[test]
    fn arguments() {
        let a = |s: &str| s.split(' ').map(String::from).collect::<Vec<_>>();
        assert_eq!(cli(&a("--sync-once")), None);
        assert_eq!(cli(&a("--restarted")), None);
        assert_eq!(cli(&a("--watchdog nope")), Some(2));
    }

    /// The wait reports the real exit code of a process of this exe: the test binary itself, run once with arguments
    /// that end it cleanly (`--list`, code 0) and once with a bad flag (code 101).
    #[test]
    fn wait_reports_the_exit_code_of_this_exe() {
        use std::process::Stdio;
        let me = std::env::current_exe().unwrap();
        let run = |arg: &str| {
            // the Child keeps a handle open, so the pid stays this process even if it has already ended
            let mut child = Command::new(&me)
                .args([arg, "no_test_has_this_name"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let got = wait_for(child.id()).expect("waits on a process of this exe");
            assert_eq!(Some(got.0 as i32), child.wait().unwrap().code());
            got
        };
        let (clean, ran) = run("--list");
        assert_eq!(clean, 0);
        assert!(ran < Duration::from_secs(30));
        assert_ne!(run("--no-such-flag").0, 0);
        // another program is refused (the pid could have been reused), and so is a pid that isn't running
        let mut cmd = Command::new("cmd").args(["/c", "exit 3"]).spawn().unwrap();
        assert!(wait_for(cmd.id()).is_none());
        let _ = cmd.wait();
        assert!(wait_for(u32::MAX - 3).is_none());
    }
}
