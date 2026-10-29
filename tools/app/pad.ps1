# Drives the app's test pad (app/src-tauri/src/input/test_hook.rs): a virtual Xbox pad that acts exactly like a real
# one (Guide tap/hold timing, mouse mode, the page). The app must run with PADWAKE_TEST=1. One command per
# argument, sent in order:
#   guide tap | guide hold            Guide down for 120 ms / 800 ms
#   press <button> [ms]               a b x y lb rb lt rt menu view guide up down left right l3 r3 (default 80 ms)
#   pad {"rx":0.6,"a":true}           hold a state until the next pad command (sticks -1..1, y down); pad {} lets go
# Like an injected key, each command first waits for the key guard (no person for 60 s); -NoGuard skips that.
#   pad.ps1 'guide tap'
#   pad.ps1 'press right' 'press right' 'press a'
param([Parameter(Position = 0, ValueFromRemainingArguments)][string[]]$Commands, [switch]$NoGuard, [int]$Gap = 250)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\guard.ps1"
foreach ($c in $Commands) {
  if (-not $NoGuard -and -not (Wait-Quiet)) { throw 'someone is using the laptop' }
  $pipe = New-Object IO.Pipes.NamedPipeClientStream('.', 'padwake-test', [IO.Pipes.PipeDirection]::Out)
  $pipe.Connect(3000)
  $w = New-Object IO.StreamWriter($pipe)
  $w.WriteLine($c); $w.Flush(); $pipe.Dispose()
  Start-Sleep -Milliseconds $Gap
}
