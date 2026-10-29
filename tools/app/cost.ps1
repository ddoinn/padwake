# What the app costs while it runs: CPU of the main exe plus every msedgewebview2.exe whose parent chain leads to it,
# averaged over -Sec seconds as % of the whole machine (all logical cores), and the GPU engine utilisation of the same
# processes from the "GPU Engine" counters, sampled every second. Games the app started are not counted.
#   cost.ps1 [-Sec 60] [-Exe padwake]
param([int]$Sec = 60, [string]$Exe = 'padwake')
$ErrorActionPreference = 'Stop'

$procs = Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, Name, CreationDate, CommandLine
# the crash watchdog (`--watchdog <pid>`, row A11) is the same exe: it counts, but the app is the other one
$watchdogs = @($procs | Where-Object { $_.Name -eq "$Exe.exe" -and $_.CommandLine -match '--watchdog' })
$main = @($procs | Where-Object { $_.Name -eq "$Exe.exe" -and $_.CommandLine -notmatch '--watchdog' })
if ($main.Count -ne 1) { throw "expected one $Exe.exe, found $($main.Count)" }
$ids = [Collections.Generic.HashSet[uint32]]::new(); [void]$ids.Add($main[0].ProcessId)
foreach ($w in $watchdogs) { [void]$ids.Add($w.ProcessId) }
do {   # a child counts only if it started after its parent (a pid can be reused)
  $added = $false
  foreach ($p in $procs) {
    if ($p.Name -ne 'msedgewebview2.exe' -or $ids.Contains($p.ProcessId) -or -not $ids.Contains($p.ParentProcessId)) { continue }
    $parent = $procs | Where-Object ProcessId -eq $p.ParentProcessId
    if ($parent.CreationDate -le $p.CreationDate) { [void]$ids.Add($p.ProcessId); $added = $true }
  }
} while ($added)

$before = @{}
foreach ($id in $ids) { $p = Get-Process -Id $id -EA SilentlyContinue; if ($p) { $before[$id] = $p } }
$cpu0 = @{}; foreach ($id in $before.Keys) { $cpu0[$id] = $before[$id].TotalProcessorTime.TotalMilliseconds }
$sw = [Diagnostics.Stopwatch]::StartNew()
# Utilization Percentage is cooked over each 1 s interval, so the samples cover the whole window
$paths = $before.Keys | ForEach-Object { "\GPU Engine(pid_$($_)_*)\Utilization Percentage" }
$gpu = @(Get-Counter -Counter $paths -SampleInterval 1 -MaxSamples $Sec -EA SilentlyContinue | ForEach-Object {
  ($_.CounterSamples | Measure-Object CookedValue -Sum).Sum })
$elapsed = $sw.Elapsed.TotalMilliseconds
$cores = [Environment]::ProcessorCount

$rows = foreach ($id in $before.Keys) {
  $p = $before[$id]; $p.Refresh()
  $ms = if ($p.HasExited) { $null } else { $p.TotalProcessorTime.TotalMilliseconds - $cpu0[$id] }
  [pscustomobject]@{ pid = $id; name = $p.ProcessName; cpuMs = $ms; pctOfPC = if ($null -ne $ms) { [Math]::Round($ms / $elapsed / $cores * 100, 4) } }
}
$rows | Sort-Object cpuMs -Descending | Format-Table -AutoSize | Out-String | Write-Output
$total = ($rows | Measure-Object cpuMs -Sum).Sum
$gpuAvg = if ($gpu.Count) { ($gpu | Measure-Object -Average).Average } else { 0 }
$gpuMax = if ($gpu.Count) { ($gpu | Measure-Object -Maximum).Maximum } else { 0 }
'{0} processes over {1:N1} s: CPU {2:N1} ms = {3:N4}% of the PC ({4} logical cores) = {5:N3}% of one core; GPU avg {6:N3}% max {7:N3}% ({8} samples)' -f
  $rows.Count, ($elapsed / 1000), $total, ($total / $elapsed / $cores * 100), $cores, ($total / $elapsed * 100), $gpuAvg, $gpuMax, $gpu.Count
