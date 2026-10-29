//! The pad: XInput for all four slots, including Guide. Emits `input` (pad state on change) and
//! `guide` (`{kind: "tap" | "hold"}`). The page does repeat, acceleration and focus (row A3).
//! Mouse mode (`system::mouse`) and Guide over a running game (`games::guide`) see the pad first (row A7, A5).
//! `PADWAKE_TEST=1` adds a virtual pad driven through a named pipe (`test_hook.rs`).

mod test_hook;
mod xinput;

pub use xinput::Pad;

use tauri::plugin::{Builder, TauriPlugin};
use tauri::Runtime;

pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("input")
        .invoke_handler(tauri::generate_handler![rumble])
        .setup(|app, _api| {
            test_hook::start();
            xinput::start(app.clone());
            Ok(())
        })
        .build()
}

/// A rumble tick on the pad used last: `strength` 0..1 for `ms` milliseconds. Off when the `rumble` setting is false.
/// The page sends one per move; async, so it runs off the main thread and never delays a frame.
#[tauri::command]
async fn rumble(strength: f32, ms: u32) {
    if crate::data::setting::<bool>("rumble").unwrap_or(true) {
        xinput::rumble(strength, ms);
    }
}
