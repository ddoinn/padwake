//! A toast while the app sleeps (over a game: "Ready to play"; in mouse mode: "B to exit"). The main window is hidden
//! then, so the toast gets a small window of its own: transparent, topmost, click-through and never focused, sized to
//! the PS5 toast box (`design/ps5/1-home.md` §1.9). It loads `control-center/toast.html` (the same `toast.js` as in the
//! app) and is destroyed when the toast is done, so it costs nothing afterwards.
//! - Guide over the game copies the screen for Control Center's still: the toast is left out of that copy (else it
//!   would show dimmed under the scrims, below the clock), and goes back above the app once its window is up.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SetWindowDisplayAffinity, SetWindowPos, HWND_TOPMOST, SM_CXSCREEN, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
};

/// The toast box in 1920×1080 design pixels.
const BOX: (f64, f64, f64, f64) = (1236.0, 30.0, 652.0, 112.0);
/// In 250 ms, on 5.3 s, out 180 ms, plus the window's start-up.
const LIFE: Duration = Duration::from_millis(7000);

static SHOWN: AtomicU64 = AtomicU64::new(0);

/// Shows a toast above everything. `icon` is an image file (a game's tile) or `glyph:<name>` for one of the toast's own
/// icons. A newer toast replaces one still showing.
pub fn overlay_toast<R: Runtime>(app: &AppHandle<R>, title: &str, message: &str, icon: Option<PathBuf>) {
    let app = app.clone();
    let n = SHOWN.fetch_add(1, Ordering::SeqCst) + 1;
    let query = format!(
        "title={}&message={}&icon={}",
        enc(title),
        enc(message),
        enc(&icon.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default())
    );
    let title_log = format!("{title}: {message}");
    let spawned = std::thread::Builder::new().name("toast".into()).spawn(move || {
        for (label, old) in app.webview_windows() {
            if label.starts_with("toast-") {
                let _ = old.destroy();
            }
        }
        let t0 = std::time::Instant::now();
        // the primary screen in physical pixels (the app is DPI aware) and the main window's scale for logical units
        let width = f64::from(unsafe { GetSystemMetrics(SM_CXSCREEN) }.max(1));
        let scale = app.get_webview_window("main").and_then(|w| w.scale_factor().ok()).unwrap_or(1.0);
        let s = width / 1920.0;
        let logical = |physical: f64| physical / scale;
        let url = WebviewUrl::App(format!("control-center/toast.html?{query}").into());
        let built = WebviewWindowBuilder::new(&app, format!("toast-{n}"), url)
            .data_directory(crate::data::dir().join("webview"))
            .title("Padwake toast")
            .transparent(true)
            .no_redirection_bitmap(true)
            .decorations(false)
            .shadow(false)
            .resizable(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .focused(false)
            .focusable(false)
            .position(logical(BOX.0 * s), logical(BOX.1 * s))
            .inner_size(logical(BOX.2 * s), logical(BOX.3 * s))
            .build();
        // not click-through: tao's set_ignore_cursor_events makes the window layered without layered attributes,
        // and Windows then draws nothing. It never takes focus anyway (focusable false, shown without activating).
        let w = match built {
            Ok(w) => w,
            Err(e) => return super::log(&format!("toast window: {e}")),
        };
        super::log(&format!("toast: \"{}\" window up in {:.0} ms", title_log, t0.elapsed().as_secs_f64() * 1000.0));
        std::thread::sleep(LIFE);
        // only the toast this thread made: a newer one replaced it already
        if SHOWN.load(Ordering::SeqCst) == n {
            let _ = w.destroy();
        }
    });
    if let Err(e) = spawned {
        super::log(&format!("toast thread: {e}"));
    }
}

fn toasts<R: Runtime>(app: &AppHandle<R>) -> Vec<WebviewWindow<R>> {
    app.webview_windows().into_iter().filter(|(label, _)| label.starts_with("toast-")).map(|(_, w)| w).collect()
}

/// Runs `capture` (a copy of the screen) with any toast window left out of it; the toast stays on the monitor.
pub fn without_toasts<R: Runtime, T>(app: &AppHandle<R>, capture: impl FnOnce() -> T) -> T {
    let hwnds: Vec<_> = toasts(app).iter().filter_map(|w| w.hwnd().ok()).collect();
    if hwnds.is_empty() {
        return capture();
    }
    unsafe {
        for h in &hwnds {
            let _ = SetWindowDisplayAffinity(*h, WDA_EXCLUDEFROMCAPTURE);
        }
        // the next composed frame, which the copy reads, leaves the toast out
        let _ = DwmFlush();
    }
    let out = capture();
    for h in &hwnds {
        let _ = unsafe { SetWindowDisplayAffinity(*h, WDA_NONE) };
    }
    out
}

/// Puts a toast still showing back above the app's window, which just came up topmost over a game.
pub fn raise_toasts<R: Runtime>(app: &AppHandle<R>) {
    for h in toasts(app).iter().filter_map(|w| w.hwnd().ok()) {
        let _ = unsafe { SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };
    }
}

/// Percent-encodes a query value.
fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn query_values_are_encoded() {
        assert_eq!(super::enc(r"Ready to play. W:\a&b"), "Ready%20to%20play.%20W%3A%5Ca%26b");
        assert_eq!(super::enc("Café"), "Caf%C3%A9");
    }
}
