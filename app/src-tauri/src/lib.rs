//! Padwake's Rust host. Each area is its own Tauri plugin (`src/<area>/mod.rs`); the UI calls
//! `plugin:<area>|<command>` through `ui/host.js`. Later rows edit their area, not this file.

mod data;
mod games;
mod input;
mod library;
mod media;
mod settings;
mod startup;
mod system;

pub use library::cli;
pub use startup::watchdog_cli;

use tauri::{Manager, WebviewWindowBuilder};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if !startup::claim_single_instance() {
        return;
    }
    tauri::Builder::default()
        .plugin(input::plugin())
        .plugin(games::plugin())
        .plugin(library::plugin())
        .plugin(system::plugin())
        .plugin(settings::plugin())
        .plugin(startup::plugin())
        .plugin(media::plugin())
        .setup(|app| {
            // art and media are files in the data dir; the UI loads them through the asset protocol
            app.asset_protocol_scope().allow_directory(data::dir(), true)?;
            // the window is built here, not from the config, so WebView2's profile lands in the data dir, not %LOCALAPPDATA%
            let config = app
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == "main")
                .cloned()
                .ok_or("tauri.conf.json has no \"main\" window")?;
            WebviewWindowBuilder::from_config(app.handle(), &config)?
                .data_directory(data::dir().join("webview"))
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Padwake failed to start");
}
