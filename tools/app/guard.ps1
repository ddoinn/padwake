# Key-injection guard for tools that press keys on the dev laptop; dot-source it: . "$PSScriptRoot\guard.ps1"
# The dev laptop's built-in keyboard floods phantom input, so GetLastInputInfo is useless.
# humanwatch.ps1 hooks real input and writes W:\tools\ch\human.tick; Wait-Quiet holds each injected key until no
# person has touched the laptop for 60 s.
$HumanTick = 'W:\tools\ch\human.tick'

function Start-HumanWatch {
  $running = Get-CimInstance Win32_Process -Filter "Name='powershell.exe'" | Where-Object CommandLine -match 'humanwatch\.ps1'
  if ($running) { return }
  Start-Process powershell -WindowStyle Hidden -ArgumentList '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File',
    "$PSScriptRoot\humanwatch.ps1", '-Minutes', '240' | Out-Null
  for ($i = 0; $i -lt 50 -and $null -eq (Get-PersonIdle); $i++) { Start-Sleep -Milliseconds 100 }
}

# seconds since a person last used the laptop; $null if humanwatch isn't writing the tick file
function Get-PersonIdle {
  $t = "$(Get-Content $HumanTick -EA SilentlyContinue)".Split(' ')
  if ($t.Count -ne 2) { return $null }
  $now = [DateTime]::UtcNow.Ticks
  if (($now - [int64]$t[1]) / 1e7 -gt 3) { return $null }
  ($now - [int64]$t[0]) / 1e7
}

# waits until no person input for $Quiet s; $false if that didn't happen within $MaxWait s
function Wait-Quiet([int]$Quiet = 60, [int]$MaxWait = 900) {
  $deadline = (Get-Date).AddSeconds($MaxWait); $said = $false
  while ((Get-Date) -lt $deadline) {
    $idle = Get-PersonIdle
    if ($null -eq $idle) { Start-HumanWatch; $idle = Get-PersonIdle }
    if ($null -ne $idle -and $idle -ge $Quiet) { return $true }
    if (-not $said) { "waiting: someone used the laptop {0:N0} s ago" -f $idle | Write-Host; $said = $true }
    Start-Sleep -Seconds 2
  }
  $false
}
