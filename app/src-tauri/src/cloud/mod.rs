//! Online, free, through GitHub: system software updates from the project's releases (`update.rs`) and a backup of
//! the library's time and journal in a private gist on the player's account (`backup.rs`). Settings → System and
//! Settings → Saved Data use these through `host.cloud.*`. Events: `update` and `backup`, each with its status.

mod backup;
mod crypto;
mod github;
mod update;

pub use backup::library_changed;
pub use update::{install_at_start, wait_for_previous, UPDATE_FROM};

use tauri::plugin::{Builder, TauriPlugin};
use tauri::{AppHandle, Runtime};

pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("cloud")
        .invoke_handler(tauri::generate_handler![
            update_status, update_check, update_install, backup_status, backup_sign_in, backup_cancel_sign_in, backup_sign_out, backup_now, backup_restore, backup_dismiss
        ])
        .setup(|app, _api| {
            update::start(app.clone());
            backup::start(app.clone());
            if backup::status().signed_in {
                library_changed(); // anything changed after the last backup goes up once the app has settled
            }
            Ok(())
        })
        .build()
}

// ---------- updates ----------

#[tauri::command]
fn update_status() -> update::Status {
    update::status()
}

/// Looks for a newer version now and downloads it. Resolves with the status after.
#[tauri::command]
async fn update_check<R: Runtime>(app: AppHandle<R>) -> update::Status {
    tauri::async_runtime::spawn_blocking(move || update::check(&app)).await.unwrap_or_else(|_| update::status())
}

/// Installs the ready update and restarts into it (the app exits).
#[tauri::command]
async fn update_install<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    update::install(&app)
}

// ---------- backup ----------

#[tauri::command]
fn backup_status() -> backup::Status {
    backup::status()
}

#[tauri::command]
async fn backup_sign_in<R: Runtime>(app: AppHandle<R>) -> Result<backup::Status, String> {
    tauri::async_runtime::spawn_blocking(move || backup::sign_in(&app)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
fn backup_cancel_sign_in<R: Runtime>(app: AppHandle<R>) -> backup::Status {
    backup::cancel_sign_in(&app)
}

#[tauri::command]
async fn backup_sign_out<R: Runtime>(app: AppHandle<R>) -> backup::Status {
    tauri::async_runtime::spawn_blocking(move || backup::sign_out(&app)).await.unwrap_or_else(|_| backup::status())
}

#[tauri::command]
async fn backup_now<R: Runtime>(app: AppHandle<R>) -> Result<backup::Status, String> {
    tauri::async_runtime::spawn_blocking(move || backup::save(&app)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn backup_restore<R: Runtime>(app: AppHandle<R>) -> Result<backup::Status, String> {
    tauri::async_runtime::spawn_blocking(move || backup::restore(&app)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
fn backup_dismiss<R: Runtime>(app: AppHandle<R>) -> backup::Status {
    backup::dismiss_found(&app)
}
