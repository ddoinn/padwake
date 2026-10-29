//! Windows itself: sound output and volume, pad batteries, display modes and HDR, network and Wi-Fi, power, and
//! "about". Settings (rows A9/A10) and Control Center (row A7) both use these through `host.system.*`.
//! Each part is a plain module that tests can call without Tauri; the commands here only wrap them.

mod audio;
mod display;
pub mod mouse;
mod network;
mod pads;
mod power;
mod toast;

use serde::Serialize;
use std::io::Write;
use std::path::PathBuf;
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{AppHandle, Emitter, Runtime};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

pub use audio::{AudioOutput, Volume};
pub use display::{Display, Mode};
pub use network::{Net, Wifi};
use pads::PadBattery;
pub use toast::{overlay_toast, raise_toasts, without_toasts};

pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("system")
        .invoke_handler(tauri::generate_handler![
            audio_outputs, set_audio_output, volume, set_volume, battery, power, about, display, try_display_mode, keep_display_mode, revert_display_mode, set_hdr, network, wifi_networks, wifi_connect, mouse_mode
        ])
        .build()
}

/// Appends a line to `data\padwake.log` (with Unix milliseconds) and to stderr.
pub fn log(line: &str) {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let text = format!("{ms} {line}");
    eprintln!("{text}");
    let path = crate::data::dir().join("padwake.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{text}");
    }
}

/// COM for this thread (multithreaded), once. Commands that use COM are async, so they run on the runtime's pool
/// threads, not on the window's STA thread; if a thread already has COM, the existing apartment is used.
fn com() {
    thread_local!(static INIT: () = {
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    });
    INIT.with(|_| {});
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

// ---------- sound ----------

#[tauri::command]
async fn audio_outputs() -> Result<Vec<AudioOutput>, String> {
    audio::outputs()
}

#[tauri::command]
async fn set_audio_output(id: String) -> Result<(), String> {
    audio::set_default(&id)
}

#[tauri::command]
async fn volume() -> Result<Volume, String> {
    audio::volume()
}

#[tauri::command]
async fn set_volume(level: f32) -> Result<(), String> {
    audio::set_volume(level)
}

// ---------- controllers ----------

/// Connected pads: battery, connection (wired, wireless, 2.4 GHz receiver) and name (`pads.rs`).
/// Async: asking an empty XInput slot is slow, so this stays off the window's thread.
#[tauri::command]
async fn battery() -> Vec<PadBattery> {
    pads::pads()
}

// ---------- power ----------

/// "sleep", "restart", "shutdown" or "exit" (to the desktop). With `PADWAKE_DRY_POWER=1` it only logs.
#[tauri::command]
async fn power<R: Runtime>(app: AppHandle<R>, action: String) -> Result<(), String> {
    power::run(&action, || app.exit(0))
}

// ---------- about ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct About {
    version: String,
    data_dir: PathBuf,
    exe: PathBuf,
}

#[tauri::command]
fn about<R: Runtime>(app: AppHandle<R>) -> About {
    About {
        version: app.package_info().version.to_string(),
        data_dir: crate::data::dir().to_path_buf(),
        exe: std::env::current_exe().unwrap_or_default(),
    }
}

// ---------- display ----------

#[tauri::command]
async fn display() -> Result<Display, String> {
    display::info()
}

fn emitter<R: Runtime>(app: &AppHandle<R>) -> display::Emit {
    let app = app.clone();
    std::sync::Arc::new(move |payload| {
        let _ = app.emit("display", payload);
    })
}

/// Applies a mode for a trial (not saved); it reverts after 15 s unless kept. Emits `display` {state: "trial"}.
#[tauri::command]
async fn try_display_mode<R: Runtime>(app: AppHandle<R>, width: u32, height: u32, hz: u32) -> Result<(), String> {
    display::try_mode(Mode { width, height, hz }, display::TRIAL, emitter(&app))
}

#[tauri::command]
async fn keep_display_mode<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    display::keep(&emitter(&app))
}

#[tauri::command]
async fn revert_display_mode<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    display::revert(&emitter(&app))
}

#[tauri::command]
async fn set_hdr(on: bool) -> Result<(), String> {
    display::set_hdr(on)
}

// ---------- network ----------

#[tauri::command]
async fn network() -> Result<Net, String> {
    network::status()
}

#[tauri::command]
async fn wifi_networks() -> Result<Vec<Wifi>, String> {
    network::wifi_list(true)
}

#[tauri::command]
async fn wifi_connect(ssid: String, password: String) -> Result<(), String> {
    network::connect(&ssid, &password)
}

// ---------- mouse mode (row A7) ----------

/// Starts or ends mouse mode (`mouse.rs`). Emits `mouse` {on}.
#[tauri::command]
async fn mouse_mode<R: Runtime>(app: AppHandle<R>, on: bool) {
    if on {
        mouse::start(&app);
    } else {
        mouse::stop(&app);
    }
}
