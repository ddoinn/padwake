# Development

Windows x64, Node.js/npm, Rust with the MSVC toolchain, Visual Studio C++ Build Tools, and WebView2 are required.

## Build

From app/:

```powershell
npm ci
npx tauri build --no-bundle
```

The executable is in app/src-tauri/target/release, or CARGO_TARGET_DIR/release when configured.
Run with npx tauri dev for development. Debug builds default to W:\games-test; release builds default to C:\Games.
Use Settings > Library > Games Folder to choose your library.

## Verify

From app/src-tauri/: cargo test --lib --locked

From the repository root:

```powershell
node tools/app/ui-probe.mjs --query "scene=settings&settings=controllers/0/r/1" tools/app/checks/controller-test.json
node tools/app/ui-probe.mjs --query scene=home tools/app/checks/settings-dim-over-home.json
node tools/app/ui-probe.mjs --query scene=home tools/app/checks/cloud.json
node tools/app/ui-probe.mjs --query "scene=home&captures=9" tools/app/checks/gallery.json
node tools/app/ui-probe.mjs --query scene=home tools/app/checks/journal.json
```

The UI probe uses headless Edge and mock host data. Set EDGE to your Edge executable; its default path and the capture/input tools target the Windows development laptop.
The optional native test pad is enabled only with PADWAKE_TEST=1; leave it unset for normal runs.

## Portable files

Ship padwake.exe, LICENSE, NOTICE and setup instructions. Include the font license.
Trailers need ffmpeg.exe beside the app or on PATH; include its licenses and build/source provenance if bundling it.
Never include data/, browser profiles, API keys, logs, test libraries or reference recordings.
