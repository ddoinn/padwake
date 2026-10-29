//! Putting Home to sleep while a game runs, and waking it after. Any area that hides or shows the main window
//! (Control Center over a game, Guide hold) goes through these two, so the web view is never left suspended.

use tauri::{AppHandle, Manager, Runtime};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2_19, ICoreWebView2_3, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL, COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC,
};
use webview2_com::TrySuspendCompletedHandler;
use windows::core::Interface;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP};
use windows::Win32::UI::WindowsAndMessaging::{IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE};

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
