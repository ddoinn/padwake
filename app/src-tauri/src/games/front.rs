//! Which window is in front: a WinEvent hook on foreground changes, on its own thread. Windows calls it only when the
//! foreground window changes (an OS event, no polling), so it costs nothing while a game plays.

use std::sync::OnceLock;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, GetWindowThreadProcessId, TranslateMessage, EVENT_SYSTEM_FOREGROUND, MSG,
    WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
};

type OnFront = Box<dyn Fn(isize, u32) + Send + Sync>;
static ON_FRONT: OnceLock<OnFront> = OnceLock::new();

/// Calls `f(hwnd, pid)` each time another process's window comes to the front. Once per app.
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
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        if hook.is_invalid() {
            eprintln!("foreground hook failed");
            return;
        }
        // out-of-context hooks are delivered through this thread's message queue
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
    if let Err(e) = spawned {
        eprintln!("foreground thread: {e}");
    }
}

unsafe extern "system" fn changed(_: HWINEVENTHOOK, _event: u32, hwnd: HWND, _: i32, _: i32, _: u32, _: u32) {
    if hwnd.0.is_null() {
        return;
    }
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if let (Some(f), true) = (ON_FRONT.get(), pid != 0) {
        f(hwnd.0 as isize, pid);
    }
}
