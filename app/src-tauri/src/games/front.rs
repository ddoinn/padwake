//! Which window is in front: a WinEvent hook on foreground changes, on its own thread. Windows calls it only when the
//! foreground window changes (an OS event, no polling), so it costs nothing while a game plays.
//! - After alt-tab, the window picked sometimes gets no event of its own (the switcher's is the last one). So each
//!   event arms a one-shot timer that reads the foreground window once, 500 ms later, and reports it if it changed.
//!   Only an event arms it: still nothing runs while nobody switches windows.

use std::sync::atomic::{AtomicIsize, AtomicUsize, Ordering};
use std::sync::OnceLock;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetClassNameW, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, KillTimer, SetTimer,
    TranslateMessage, EVENT_SYSTEM_FOREGROUND, MSG, WINEVENT_OUTOFCONTEXT, WM_TIMER,
};

/// The window last reported, and the re-read timer armed (0: none).
static LAST: AtomicIsize = AtomicIsize::new(0);
static TIMER: AtomicUsize = AtomicUsize::new(0);
const RECHECK_MS: u32 = 500;

type OnFront = Box<dyn Fn(isize, u32) + Send + Sync>;
static ON_FRONT: OnceLock<OnFront> = OnceLock::new();

/// Calls `f(hwnd, pid)` each time a window comes to the front (this app's too). Once per app.
pub fn start(f: impl Fn(isize, u32) + Send + Sync + 'static) {
    if ON_FRONT.set(Box::new(f)).is_err() {
        return;
    }
    let spawned = std::thread::Builder::new().name("foreground".into()).spawn(|| unsafe {
        let hook = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(changed),
            0,
            0,
            // our own windows too: Padwake clicked back in front of another app, over the game
            WINEVENT_OUTOFCONTEXT,
        );
        if hook.is_invalid() {
            eprintln!("foreground hook failed");
            return;
        }
        // out-of-context hooks are delivered through this thread's message queue
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_TIMER && msg.hwnd.0.is_null() {
                recheck();
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
    if let Err(e) = spawned {
        eprintln!("foreground thread: {e}");
    }
}

/// Windows' own windows that come to the front in passing: the alt-tab switcher (it takes the foreground as it opens
/// and closes; the window picked after it gets none when it is the one that was in front), the taskbar and the desktop.
const PASSING: [&str; 8] = [
    "ForegroundStaging",
    "XamlExplorerHostIslandWindow",
    "MultitaskingViewFrame",
    "TaskSwitcherWnd",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "Progman",
    "WorkerW",
];

/// True for a window of the shell that comes to the front in passing (`PASSING`), by its class (never its title).
pub fn in_passing(hwnd: isize) -> bool {
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(HWND(hwnd as _), &mut buf) }.max(0) as usize;
    passing_class(&String::from_utf16_lossy(&buf[..n]))
}

fn passing_class(class: &str) -> bool {
    PASSING.contains(&class)
}

unsafe extern "system" fn changed(_: HWINEVENTHOOK, _event: u32, hwnd: HWND, _: i32, _: i32, _: u32, _: u32) {
    if hwnd.0.is_null() {
        return;
    }
    report(hwnd);
    // on this thread: its message loop gets the WM_TIMER (a thread timer, no window)
    unsafe {
        let old = TIMER.swap(0, Ordering::Relaxed);
        if old != 0 {
            let _ = KillTimer(None, old);
        }
        TIMER.store(SetTimer(None, 0, RECHECK_MS, None), Ordering::Relaxed);
    }
}

/// The re-read after an event: the window in front now, if it isn't the one last reported.
fn recheck() {
    unsafe {
        let old = TIMER.swap(0, Ordering::Relaxed);
        if old != 0 {
            let _ = KillTimer(None, old);
        }
        let hwnd = GetForegroundWindow();
        if !hwnd.0.is_null() && hwnd.0 as isize != LAST.load(Ordering::Relaxed) {
            report(hwnd);
        }
    }
}

fn report(hwnd: HWND) {
    LAST.store(hwnd.0 as isize, Ordering::Relaxed);
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if let (Some(f), true) = (ON_FRONT.get(), pid != 0) {
        f(hwnd.0 as isize, pid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shell_comes_to_the_front_only_in_passing() {
        for class in ["ForegroundStaging", "XamlExplorerHostIslandWindow", "Shell_TrayWnd", "Progman", "WorkerW"] {
            assert!(passing_class(class), "{class}");
        }
        // apps, Explorer's own file windows included, are another app in front
        for class in ["Notepad", "Chrome_WidgetWin_1", "CabinetWClass", "WindowsForms10.Window.8.app.0.141b42a_r6_ad1", ""] {
            assert!(!passing_class(class), "{class}");
        }
    }
}
