# Key -> first moving frame on Home's tile row, measured on the real screen. Run in the desktop session with the app
# in front on Home. Each sample: wait until no person has used the laptop for 60 s (guard.ps1), inject Right or Left
# with SendInput, then BitBlt one line of the row (y 180, x 360-1900: the small tiles, not the ring) in a tight loop
# until its pixels change. Samples alternate Right/Left 600 ms apart, so the row ends where it started.
# Each BitBlt waits for the next composed frame, so times come in whole frames (16.7 ms at 60 Hz) and, without -Jitter,
# the key always lands just after a vsync: the worst phase. -Jitter waits a random 0-16.7 ms first, as a person's
# press lands anywhere in a frame.
#   latency.ps1 [-N 20] [-Jitter] [-Gap 600] [-Exe padwake]
#
# -Return: game exit -> Home on screen and answering. It finds the windowed stand-in's tile by name (-Game, default
# "Stand-in Long") in the app's library.json, in Home's order (last played first), goes there (LT to the first tile,
# then Right), saves <Out>\return-focus.png as proof of focus, then N times: Enter (launch), wait -Play ms, post
# WM_CLOSE to the game window, and time from WM_CLOSE until the app window is visible, until the row is drawn, and
# until the row moves after a Right injected as soon as it is drawn (then Left back). The game that ended comes back
# first and focused, so Right always has a tile to move to (the Library tile at least).
# The data dir is found from the app's WebView2 profile (`--user-data-dir=<data>\webview`); -Data overrides it.
#   latency.ps1 -Return [-N 5] [-Play 3000] [-Game 'Stand-in Long'] [-GameExe StandinLong] [-Data <dir>]
param([int]$N = 20, [switch]$Jitter, [int]$Gap = 600, [string]$Exe = 'padwake', [switch]$Return, [int]$Play = 3000,
  [string]$Game = 'Stand-in Long', [string]$GameExe = 'StandinLong', [string]$Data = '',
  [int]$Y = 180, [int]$X0 = 360, [int]$X1 = 1900, [string]$Out = 'W:\tmp\app-latency')
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\guard.ps1"
Add-Type -TypeDefinition @'
using System; using System.Diagnostics; using System.Runtime.InteropServices; using System.Threading;
public static class Lat {
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr c);
  [DllImport("user32.dll")] static extern IntPtr GetDC(IntPtr h);
  [DllImport("gdi32.dll")] static extern IntPtr CreateCompatibleDC(IntPtr dc);
  [DllImport("gdi32.dll")] static extern IntPtr CreateDIBSection(IntPtr dc, ref BMI bi, uint usage, out IntPtr bits, IntPtr sec, uint off);
  [DllImport("gdi32.dll")] static extern IntPtr SelectObject(IntPtr dc, IntPtr o);
  [DllImport("gdi32.dll")] static extern bool BitBlt(IntPtr d, int x, int y, int w, int h, IntPtr s, int sx, int sy, uint rop);
  [DllImport("user32.dll")] static extern uint SendInput(uint n, INPUT[] inputs, int size);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern void keybd_event(byte k, byte s, uint f, UIntPtr e);
  [StructLayout(LayoutKind.Sequential)] struct BMI { public uint size; public int w, h; public ushort planes, bpp; public uint comp, img; public int xp, yp; public uint used, imp; }
  [StructLayout(LayoutKind.Sequential)] struct KI { public ushort vk, scan; public uint flags, time; public IntPtr extra; }
  [StructLayout(LayoutKind.Sequential)] struct MI { public int dx, dy; public uint data, flags, time; public IntPtr extra; }
  [StructLayout(LayoutKind.Explicit)] struct U { [FieldOffset(0)] public MI mi; [FieldOffset(0)] public KI ki; }
  [StructLayout(LayoutKind.Sequential)] struct INPUT { public uint type; public U u; }

  static IntPtr screen, mem, bits; static int W, X, Y;
  public static void Init(int x0, int x1, int y) {
    X = x0; Y = y; W = x1 - x0; screen = GetDC(IntPtr.Zero); mem = CreateCompatibleDC(screen);
    var bi = new BMI { size = 40, w = W, h = -1, planes = 1, bpp = 32 };
    SelectObject(mem, CreateDIBSection(screen, ref bi, 0, out bits, IntPtr.Zero, 0));
  }
  public static byte[] Grab() { BitBlt(mem, 0, 0, W, 1, screen, X, Y, 0x00CC0020); var a = new byte[W * 4]; Marshal.Copy(bits, a, 0, a.Length); return a; }
  public static int Diff(byte[] a, byte[] b) {
    int n = 0;
    for (int i = 0; i < a.Length; i += 4) if (Math.Abs(a[i] - b[i]) > 24 || Math.Abs(a[i + 1] - b[i + 1]) > 24 || Math.Abs(a[i + 2] - b[i + 2]) > 24) n++;
    return n;
  }
  public static void Key(ushort vk) {
    var k = new INPUT[2];
    k[0].type = 1; k[0].u.ki.vk = vk; k[1].type = 1; k[1].u.ki.vk = vk; k[1].u.ki.flags = 2;
    SendInput(2, k, Marshal.SizeOf(typeof(INPUT)));
  }
  public static uint Foreground() { uint pid; GetWindowThreadProcessId(GetForegroundWindow(), out pid); return pid; }
  // {ms from the key to the first grab that differs from the line before it, grabs made, ms to the last grab that
  // didn't}: the row started moving between the two. ms = -1 if nothing moved
  static Random rng = new Random();
  public static double[] Trial(ushort vk, int timeoutMs, bool jitter) {
    var before = Grab();
    if (jitter) { double j = rng.NextDouble() * 16.7; var w = Stopwatch.StartNew(); while (w.Elapsed.TotalMilliseconds < j) { } }
    var sw = Stopwatch.StartNew(); Key(vk); int grabs = 0; double still = 0;
    while (sw.ElapsedMilliseconds < timeoutMs) {
      var g = Grab(); grabs++; double t = sw.Elapsed.TotalMilliseconds;
      if (Diff(before, g) > 8) return new double[] { t, grabs, still };
      still = t;
    }
    return new double[] { -1, grabs, still };
  }
  // after WM_CLOSE to the game: {window visible, row drawn (line differs from the game's), row moved after a Right} in ms
  public static double[] Return(IntPtr game, IntPtr app, int timeoutMs) {
    var black = Grab(); var sw = Stopwatch.StartNew(); PostMessage(game, 0x0010, IntPtr.Zero, IntPtr.Zero);
    double shown = -1, drawn = -1, moved = -1; byte[] home = null;
    while (sw.ElapsedMilliseconds < timeoutMs) {
      if (shown < 0 && IsWindowVisible(app)) shown = sw.Elapsed.TotalMilliseconds;
      var g = Grab();
      if (drawn < 0) { if (Diff(black, g) > 50) { drawn = sw.Elapsed.TotalMilliseconds; Thread.Sleep(0); home = Grab(); Key(0x27); } continue; }
      if (Diff(home, g) > 8) { moved = sw.Elapsed.TotalMilliseconds; break; }
    }
    return new double[] { shown, drawn, moved };
  }
}
'@
[Lat]::SetProcessDpiAwarenessContext([IntPtr]-4) | Out-Null
[Lat]::Init($X0, $X1, $Y)
New-Item $Out -ItemType Directory -Force | Out-Null

$app = Get-Process $Exe -EA SilentlyContinue | Where-Object MainWindowHandle -ne 0 | Select-Object -First 1
if (-not $app) { throw "$Exe is not running with a visible window" }
$hwnd = $app.MainWindowHandle
function Median($xs) { $s = @($xs | Sort-Object); if (-not $s.Count) { return $null }; if ($s.Count % 2) { $s[[int][Math]::Floor($s.Count / 2)] } else { ($s[$s.Count / 2 - 1] + $s[$s.Count / 2]) / 2 } }
function Ready {   # a quiet laptop and the app in front (F24 first: Windows lets a background app take focus only after input)
  if (-not (Wait-Quiet)) { throw 'someone kept using the laptop; stopped' }
  if ([Lat]::Foreground() -ne $app.Id) {
    [Lat]::keybd_event(0x87, 0, 0, [UIntPtr]::Zero); [Lat]::keybd_event(0x87, 0, 2, [UIntPtr]::Zero)
    [Lat]::SetForegroundWindow($hwnd) | Out-Null; Start-Sleep -Milliseconds 300
    if ([Lat]::Foreground() -ne $app.Id) { throw "$Exe is not in front; stopped" }
  }
}

if (-not $Return) {
  $rows = @()
  for ($i = 0; $i -lt $N; $i++) {
    Ready
    $vk = if ($i % 2) { 0x25 } else { 0x27 }
    $r = [Lat]::Trial($vk, 500, $Jitter.IsPresent)
    $rows += [pscustomobject]@{ n = $i + 1; key = if ($i % 2) { 'Left' } else { 'Right' }; ms = [Math]::Round($r[0], 1); notYetMs = [Math]::Round($r[2], 1); grabs = $r[1] }
    Start-Sleep -Milliseconds $Gap
  }
  $rows | Format-Table -AutoSize | Out-String | Write-Output
  $ok = @($rows | Where-Object ms -ge 0)
  'key -> first moving frame{5}: median {0} ms over {1} samples (min {2}, max {3}); {4} missed. The row was still on the grab before: median {6} ms' -f
    (Median $ok.ms), $ok.Count, ($ok.ms | Measure-Object -Minimum).Minimum, ($ok.ms | Measure-Object -Maximum).Maximum, ($rows.Count - $ok.Count),
    $(if ($Jitter) { ' (random phase)' } else { ' (key just after a vsync)' }), (Median $ok.notYetMs)
  return
}

# -Return
# the app's data dir: its WebView2 browser process (a child of the exe) runs with --user-data-dir=<data>\webview
$profileArg = '--user-data-dir="?([^"]+?)\\webview"?(\s|$)'
if (-not $Data) {
  $wv = Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" |
    Where-Object { $_.ParentProcessId -eq $app.Id -and $_.CommandLine -match $profileArg } | Select-Object -First 1
  if (-not $wv) { throw "can't find $Exe's WebView2 profile; pass -Data <data dir>" }
  $null = $wv.CommandLine -match $profileArg
  $Data = $Matches[1]
}
# Home's row: most recently played first, the rest in the library's order (a stable sort), then the Library tile
$games = @((Get-Content (Join-Path $Data 'library.json') -Raw -Encoding UTF8 | ConvertFrom-Json).games)
$rowItems = for ($i = 0; $i -lt $games.Count; $i++) {
  [pscustomobject]@{ i = $i; name = $games[$i].name; last = if ($games[$i].lastPlayed) { [double]$games[$i].lastPlayed } else { 0 } }
}
$order = @($rowItems | Sort-Object @{ Expression = 'last'; Descending = $true }, @{ Expression = 'i'; Ascending = $true })
$at = [array]::IndexOf([string[]]@($order.name), $Game)
if ($at -lt 0) { throw "no game named '$Game' in $Data\library.json" }
"'$Game' is tile $($at + 1) of $($order.Count) in $Data" | Write-Output
Ready
for ($i = 0; $i -le [Math]::Ceiling($order.Count / 7); $i++) { [Lat]::Key(0x21); Start-Sleep -Milliseconds 300 }   # LT: 7 tiles left
for ($i = 0; $i -lt $at; $i++) { [Lat]::Key(0x27); Start-Sleep -Milliseconds 250 }
Start-Sleep -Milliseconds 800
powershell -NoProfile -File "$PSScriptRoot\snap.ps1" -Out "$Out\return-focus.png" | Write-Output
$rows = @()
for ($i = 0; $i -lt $N; $i++) {
  Ready
  [Lat]::Key(0x0D)
  $gameProcess = $null
  for ($t = 0; $t -lt 100 -and -not $gameProcess; $t++) { Start-Sleep -Milliseconds 100; $gameProcess = Get-Process -EA SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 -and $_.MainWindowTitle -and $_.Path -like "*\$GameExe.exe" } | Select-Object -First 1 }
  if (-not $gameProcess) { throw "no $GameExe window after Enter: is the stand-in's tile focused? (see $Out\return-focus.png)" }
  Start-Sleep -Milliseconds $Play
  if ([Lat]::IsWindowVisible($hwnd)) { throw 'the app is still visible while the game runs' }
  $r = [Lat]::Return($gameProcess.MainWindowHandle, $hwnd, 3000)
  $rows += [pscustomobject]@{ n = $i + 1; visibleMs = [Math]::Round($r[0], 1); drawnMs = [Math]::Round($r[1], 1); answeredMs = [Math]::Round($r[2], 1) }
  Start-Sleep -Milliseconds $Gap
  Ready; [Lat]::Key(0x25); Start-Sleep -Milliseconds 1500
}
$rows | Format-Table -AutoSize | Out-String | Write-Output
$ok = @($rows | Where-Object answeredMs -ge 0)
'game exit (WM_CLOSE) -> visible median {0} ms, row drawn median {1} ms, answered a key median {2} ms (max {3}) over {4} samples' -f
  (Median $ok.visibleMs), (Median $ok.drawnMs), (Median $ok.answeredMs), ($ok.answeredMs | Measure-Object -Maximum).Maximum, $ok.Count
