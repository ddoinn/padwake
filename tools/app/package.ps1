# Package the release exe and optional ffmpeg into dist\padwake. Keep personal data out of the package.
# usage: powershell -File tools/app/package.ps1 [-Build] [-Target W:\tmp\ch-target-release]
param([switch]$Build, [string]$Target = 'W:\tmp\ch-target-release')
$ErrorActionPreference = 'Stop'
$repo = Split-Path (Split-Path $PSScriptRoot)
if ($Build) {
  $env:RUSTUP_HOME = 'W:\tools\rust\rustup'; $env:CARGO_HOME = 'W:\tools\rust\cargo'
  $env:PATH = "W:\tools\rust\cargo\bin;$env:PATH"; $env:CARGO_TARGET_DIR = $Target
  Push-Location "$repo\app"
  try { npx tauri build --no-bundle; if ($LASTEXITCODE) { throw "build failed ($LASTEXITCODE)" } } finally { Pop-Location }
}
$exe = "$Target\release\padwake.exe"
if (-not (Test-Path $exe)) { throw "no build at $exe (run with -Build)" }
$out = "$repo\dist\padwake"
New-Item $out -ItemType Directory -Force | Out-Null
Copy-Item $exe $out -Force
# LGPL build, shipped as its own exe; the library looks for ffmpeg.exe beside padwake.exe
$ff = 'W:\tools\ffmpeg-dl\x\ffmpeg-master-latest-win64-lgpl\bin\ffmpeg.exe'
if ((Test-Path $ff) -and -not (Test-Path "$out\ffmpeg.exe")) { Copy-Item $ff $out }
$rev = git -C $repo rev-parse --short HEAD
$dirty = if (git -C $repo status --porcelain -- app) { ' +local changes' } else { '' }
Set-Content "$out\build.txt" "padwake $rev$dirty, built $((Get-Item $exe).LastWriteTime.ToString('s'))" -Encoding ASCII
Get-ChildItem $out | ForEach-Object { '{0,-18} {1,8:N1} MB' -f $_.Name, ($_.Length / 1MB) }
Get-Content "$out\build.txt"
