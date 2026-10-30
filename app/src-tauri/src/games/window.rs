//! Putting Home to sleep while a game runs, and waking it after. Any area that hides or shows the main window
//! (Control Center over a game, Guide hold) goes through these two, so the web view is never left suspended.

use tauri::{AppHandle, Manager, Runtime};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2_19, ICoreWebView2_3, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL, COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC,
};
use webview2_com::TrySuspendCompletedHandler;
use windows::core::Interface;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
    SetForegroundWindow, ShowWindow, SW_RESTORE,
};

const VK_F24: u8 = 0x87;

/// Keeps the window above a running game while it shows over it (some games make their window topmost).
pub fn on_top<R: Runtime>(app: &AppHandle<R>, on: bool) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.set_always_on_top(on);
    }
}

/// Brings another app's window to the front (the game, on Resume), restoring it if it was minimized.
pub fn bring_to_front(hwnd: isize) {
    let h = HWND(hwnd as _);
    unsafe {
        if IsIconic(h).as_bool() {
            let _ = ShowWindow(h, SW_RESTORE);
        }
        f24();
        let _ = SetForegroundWindow(h);
    }
}

/// The foreground window, if it is another process's and covers its whole monitor (a full-screen game). The desktop
/// and the taskbar don't count.
pub fn other_fills_screen() -> Option<isize> {
    fills_screen(unsafe { GetForegroundWindow() }.0 as isize)
}

/// `hwnd`, if it is another process's visible window covering its whole monitor; not the desktop or the taskbar.
pub fn fills_screen(hwnd: isize) -> Option<isize> {
    unsafe {
        let h = HWND(hwnd as _);
        if h.0.is_null() || !IsWindowVisible(h).as_bool() || IsIconic(h).as_bool() {
            return None;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(h, Some(&mut pid));
        if pid == 0 || pid == GetCurrentProcessId() {
            return None;
        }
        let mut class = [0u16; 32];
        let n = GetClassNameW(h, &mut class) as usize;
        if SHELL_CLASSES.contains(&String::from_utf16_lossy(&class[..n]).as_str()) {
            return None;
        }
        let mut r = RECT::default();
        GetWindowRect(h, &mut r).ok()?;
        let mut m = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST), &mut m).as_bool() {
            return None;
        }
        covers(&r, &m.rcMonitor).then_some(h.0 as isize)
    }
}

/// The desktop's and the taskbar's windows, which are screen-sized but never a game.
const SHELL_CLASSES: [&str; 3] = ["Progman", "WorkerW", "Shell_TrayWnd"];

fn covers(w: &RECT, m: &RECT) -> bool {
    w.left <= m.left && w.top <= m.top && w.right >= m.right && w.bottom >= m.bottom
}

/// Windows lets a background app take the foreground only right after input; F24 is a key no app uses.
fn f24() {
    unsafe {
        keybd_event(VK_F24, 0, KEYBD_EVENT_FLAGS(0), 0);
        keybd_event(VK_F24, 0, KEYEVENTF_KEYUP, 0);
    }
}

/// Hides the window and suspends the web view: no script, timers or rendering, and memory trimmed.
pub fn sleep<R: Runtime>(app: &AppHandle<R>) {
    let Some(w) = app.get_webview_window("main") else { return };
    let _ = w.hide();
    let _ = w.with_webview(|wv| unsafe {
        let ctrl = wv.controller();
        // TrySuspend fails on a visible web view
        let _ = ctrl.SetIsVisible(false);
        let Ok(core) = ctrl.CoreWebView2() else { return };
        if let Ok(c) = core.cast::<ICoreWebView2_19>() {
            let _ = c.SetMemoryUsageTargetLevel(COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW);
        }
        if let Ok(c) = core.cast::<ICoreWebView2_3>() {
            let done = TrySuspendCompletedHandler::create(Box::new(|hr, ok| {
                if hr.is_err() || !ok {
                    eprintln!("web view did not suspend: {hr:?}");
                }
                Ok(())
            }));
            let _ = c.TrySuspend(&done);
        }
    });
}

/// Resumes the web view without showing the window, so the page is running by the time the window shows (Guide over
/// a game starts this before copying the screen).
pub fn resume_page<R: Runtime>(app: &AppHandle<R>) {
    let Some(w) = app.get_webview_window("main") else { return };
    let _ = w.with_webview(|wv| unsafe {
        let ctrl = wv.controller();
        if let Ok(core) = ctrl.CoreWebView2() {
            if let Ok(c) = core.cast::<ICoreWebView2_3>() {
                let _ = c.Resume();
            }
        }
        let _ = ctrl.SetIsVisible(true);
    });
}

/// Resumes the web view, shows the window and brings it to the foreground with keyboard focus in the page.
pub fn wake<R: Runtime>(app: &AppHandle<R>) {
    let Some(w) = app.get_webview_window("main") else { return };
    let _ = w.with_webview(|wv| unsafe {
        let ctrl = wv.controller();
        if let Ok(core) = ctrl.CoreWebView2() {
            if let Ok(c) = core.cast::<ICoreWebView2_3>() {
                let _ = c.Resume();
            }
            if let Ok(c) = core.cast::<ICoreWebView2_19>() {
                let _ = c.SetMemoryUsageTargetLevel(COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL);
            }
        }
        let _ = ctrl.SetIsVisible(true);
    });
    let _ = w.show();
    f24();
    // hwnd() waits for the main thread, so show() above has run by now
    if let Ok(h) = w.hwnd() {
        unsafe {
            let _ = SetForegroundWindow(h);
        }
    }
    let _ = w.set_focus();
    let _ = w.with_webview(|wv| unsafe {
        let _ = wv.controller().MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_fills_the_screen_only_when_it_covers_the_monitor() {
        let m = RECT { left: 0, top: 0, right: 1920, bottom: 1080 };
        assert!(covers(&m, &m)); // borderless or exclusive full screen
        assert!(covers(&RECT { left: -8, top: -8, right: 1928, bottom: 1088 }, &m)); // maximized, frame outside
        assert!(!covers(&RECT { left: 0, top: 0, right: 1920, bottom: 1040 }, &m)); // above the taskbar
        assert!(!covers(&RECT { left: 320, top: 180, right: 1600, bottom: 900 }, &m)); // a splash or launcher
        let second = RECT { left: 1920, top: 0, right: 4480, bottom: 1440 };
        assert!(covers(&second, &second) && !covers(&m, &second));
    }
}
