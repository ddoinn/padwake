# Publish a Padwake update on GitHub Releases (cloud area, src-tauri/src/cloud/update.rs). Every installed app checks
# the repository's latest release, downloads padwake.exe, checks it against SHA256SUMS.txt and installs it the next
# time it starts. Free: GitHub hosts the files. The repository must be public (the apps ask GitHub without signing in).
#   powershell -File tools/app/release.ps1 -Version 0.2.0 -NotesFile notes.md
#   -Notes "- ..."           release notes inline instead (Markdown; the app shows the first 4 lines)
#   -Repo owner/name         another repository (default: the one the apps check, update::REPO)
#   -Target W:\tmp\...       the cargo target dir to build in
#   -Base <zip>              the previous portable zip to build on (default: the one in the latest release)
#   -DryRun                  build and pack into dist\release-<v>; publish nothing, keep the version
# A release has three assets:
# - padwake-<v>-windows-x64.zip for new installs: the previous zip with the new padwake.exe, build.txt and NOTICE
#   version line; ffmpeg.exe, README.md and the licenses folder carry over unchanged
# - padwake.exe for the updater, and SHA256SUMS.txt with both hashes
# It sets the version in app/src-tauri/Cargo.toml (commit that afterwards) and creates the release with gh, which must
# be signed in as an account that can publish to the repository.
param(
  [Parameter(Mandatory)][string]$Version,
  [string]$Notes = '',
  [string]$NotesFile = '',
  [string]$Repo = 'ddoinn/padwake',
  [string]$Target = 'W:\tmp\ch-target-release',
  [string]$Base = '',
  [switch]$DryRun
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
$root = Split-Path (Split-Path $PSScriptRoot)
if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw "version must look like 0.2.0, not '$Version'" }

# the version lives in Cargo.toml only (tauri.conf.json has none, so Tauri reads Cargo's)
$toml = "$root\app\src-tauri\Cargo.toml"
$text = [IO.File]::ReadAllText($toml)
$lock = "$root\app\src-tauri\Cargo.lock"
$lockText = [IO.File]::ReadAllText($lock)
# a dry run or a failure puts both files back as they were
function Restore { [IO.File]::WriteAllText($toml, $text); [IO.File]::WriteAllText($lock, $lockText) }
$old = [regex]::Match($text, '(?m)^version = "([^"]+)"').Groups[1].Value
if ([version]$Version -le [version]$old) { throw "version $Version is not newer than $old" }

# the previous portable zip first, so a missing one fails before the 4 min build
$out = "$root\dist\release-$Version"
$name = "padwake-$Version-windows-x64.zip"
$zip = "$out\$name"
New-Item $out -ItemType Directory -Force | Out-Null
if ($Base) { Copy-Item $Base $zip -Force }
else {
  $prev = "$out\previous"
  New-Item $prev -ItemType Directory -Force | Out-Null
  gh release download --repo $Repo --pattern 'padwake-*-windows-x64.zip' --dir $prev --clobber
  if ($LASTEXITCODE) { throw "no portable zip in the latest release of $Repo (pass -Base <zip>)" }
  Move-Item (Get-ChildItem $prev -Filter *.zip | Select-Object -First 1).FullName $zip -Force
  Remove-Item $prev -Recurse -Force
}

[IO.File]::WriteAllText($toml, [regex]::Replace($text, '(?m)^version = "[^"]+"', "version = `"$Version`"", 1))
"version $old -> $Version"
$env:RUSTUP_HOME = 'W:\tools\rust\rustup'; $env:CARGO_HOME = 'W:\tools\rust\cargo'
$env:PATH = "W:\tools\rust\cargo\bin;$env:PATH"; $env:CARGO_TARGET_DIR = $Target
Push-Location "$root\app"
try { npx tauri build --no-bundle; if ($LASTEXITCODE) { throw "build failed ($LASTEXITCODE)" } }
catch { Restore; throw }
finally { Pop-Location }

Copy-Item "$Target\release\padwake.exe" "$out\padwake.exe" -Force
$rev = git -C $root rev-parse HEAD
$utf8 = New-Object Text.UTF8Encoding $false
# unpack the previous zip, swap the files, and pack it again with "/" in entry names (0.1.0's zip has "\", which
# only Windows reads as folders)
$stage = "$out\stage"
try {
  if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
  [IO.Compression.ZipFile]::ExtractToDirectory($zip, $stage)
  $dir = (Get-ChildItem $stage -Recurse -Filter padwake.exe | Select-Object -First 1).DirectoryName
  if (-not $dir) { throw "$zip has no padwake.exe" }
  Copy-Item "$out\padwake.exe" "$dir\padwake.exe" -Force
  [IO.File]::WriteAllText("$dir\build.txt", "Padwake $Version`nSource commit: $rev`nWindows x64 portable release`n", $utf8)
  if (Test-Path "$dir\NOTICE") {
    $notice = [IO.File]::ReadAllText("$dir\NOTICE")
    [IO.File]::WriteAllText("$dir\NOTICE", ([regex]'(?m)^Padwake \d+\.\d+\.\d+').Replace($notice, "Padwake $Version", 1), $utf8)
  }
  Remove-Item $zip -Force
  $top = Split-Path $dir -Leaf
  $a = [IO.Compression.ZipFile]::Open($zip, 'Create')
  try {
    foreach ($f in Get-ChildItem $dir -Recurse -File) {
      $entry = "$top/" + $f.FullName.Substring($dir.Length + 1).Replace('\', '/')
      [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile($a, $f.FullName, $entry, 'Optimal')
    }
  }
  finally { $a.Dispose() }
  Remove-Item $stage -Recurse -Force
}
catch { Restore; throw }

$sums = foreach ($f in @($name, 'padwake.exe')) { '{0}  {1}' -f (Get-FileHash "$out\$f" -Algorithm SHA256).Hash.ToLower(), $f }
[IO.File]::WriteAllText("$out\SHA256SUMS.txt", (($sums -join "`n") + "`n"), $utf8)
if ($NotesFile) { Copy-Item $NotesFile "$out\notes.md" -Force }
else { [IO.File]::WriteAllText("$out\notes.md", $(if ($Notes) { $Notes } else { "Padwake $Version" }), $utf8) }
foreach ($f in @($name, 'padwake.exe')) { '{0,-32} {1,8:N1} MB' -f $f, ((Get-Item "$out\$f").Length / 1MB) }
Get-Content "$out\SHA256SUMS.txt"

if ($DryRun) {
  Restore
  "dry run: would publish v$Version to $Repo from $out (Cargo.toml and Cargo.lock are back at $old)"
  return
}
gh release create "v$Version" $zip "$out\padwake.exe" "$out\SHA256SUMS.txt" --repo $Repo --title "Padwake $Version" --notes-file "$out\notes.md"
if ($LASTEXITCODE) { throw "gh release create failed ($LASTEXITCODE)" }
"published v$Version to $Repo. Commit the version: git commit -am 'Release $Version' (Cargo.toml and Cargo.lock)"
