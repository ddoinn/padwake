// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // `--watchdog <pid>`: the crash watchdog, no window (see startup/watchdog.rs)
    if let Some(code) = padwake_lib::watchdog_cli(&args) {
        std::process::exit(code);
    }
    // `--sync-once` / `--watch`: the library without a window (see library/mod.rs)
    if let Some(code) = padwake_lib::cli(&args) {
        std::process::exit(code);
    }
    padwake_lib::run()
}
