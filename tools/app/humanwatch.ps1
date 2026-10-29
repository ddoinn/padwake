# Person detector for the dev laptop (guard.ps1 starts it in the desktop session). Low-level hooks count all mouse
# input, real key input (not injected keys, not the broken built-in key: vk 255, 30/s), pad packet changes, and
# Deskflow entering or leaving this screen (the Mac's input arrives injected). Writes
# "<last person input UTC ticks> <now UTC ticks>" to W:\tools\ch\human.tick every 200 ms.
# The hooks get a thread that does nothing else: Windows holds every key and mouse event until a low-level hook
# returns, so a hook sharing a thread with the file work below delays all input (and skews tools/app/latency.ps1).
param([int]$Minutes = 60)
Add-Type -ReferencedAssemblies System.Windows.Forms @'
using System; using System.Runtime.InteropServices; using System.Threading; using System.Windows.Forms;
public class Hw {
  delegate IntPtr P(int c, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] static extern IntPtr SetWindowsHookEx(int id, P f, IntPtr m, uint t);
  [DllImport("user32.dll")] static extern IntPtr CallNextHookEx(IntPtr h, int c, IntPtr w, IntPtr l);
  [DllImport("kernel32.dll")] static extern IntPtr GetModuleHandle(string n);
  public static long Last = DateTime.UtcNow.Ticks; static P mp, kp;   // a 64-bit long: reads and writes are atomic
  static IntPtr M(int c, IntPtr w, IntPtr l) { if (c >= 0) Last = DateTime.UtcNow.Ticks; return CallNextHookEx(IntPtr.Zero, c, w, l); }
  static IntPtr K(int c, IntPtr w, IntPtr l) { if (c >= 0 && (Marshal.ReadInt32(l, 8) & 0x10) == 0 && Marshal.ReadInt32(l, 0) != 255) Last = DateTime.UtcNow.Ticks; return CallNextHookEx(IntPtr.Zero, c, w, l); }
  [DllImport("xinput1_4.dll")] static extern uint XInputGetState(uint i, byte[] s); static uint[] pk = new uint[4];
  public static void Pads() { for (uint i = 0; i < 4; i++) { var b = new byte[16]; if (XInputGetState(i, b) == 0) { uint n = BitConverter.ToUInt32(b, 0); if (pk[i] != 0 && n != pk[i]) Last = DateTime.UtcNow.Ticks; pk[i] = n; } else pk[i] = 0; } }
  public static void Hook() {
    var t = new Thread(() => { mp = M; kp = K; var h = GetModuleHandle(null); SetWindowsHookEx(14, mp, h, 0); SetWindowsHookEx(13, kp, h, 0); Application.Run(); });
    t.IsBackground = true; t.Start();
  }
}
'@
[Hw]::Hook()
$t = New-Object Windows.Forms.Timer; $t.Interval = 200; $end = (Get-Date).AddMinutes($Minutes)
$log = 'C:\ProgramData\Deskflow\deskflow-daemon.log'; $script:len = [long](Get-Item $log -EA 0).Length
$t.Add_Tick({ $n = (Get-Item $log -EA 0).Length; if ($n -lt $script:len) { $script:len = 0 }
  if ($n -gt $script:len) { $fs = [IO.File]::Open($log, 'Open', 'Read', 'ReadWrite'); $fs.Seek($script:len, 'Begin') | Out-Null; $b = New-Object byte[] ($n - $script:len); $fs.Read($b, 0, $b.Length) | Out-Null; $fs.Close(); $script:len = $n
    if ([Text.Encoding]::UTF8.GetString($b) -match 'entering computer|leaving computer') { [Hw]::Last = [DateTime]::UtcNow.Ticks } }
  [Hw]::Pads(); Set-Content W:\tools\ch\human.tick "$([Hw]::Last) $([DateTime]::UtcNow.Ticks)"; if ((Get-Date) -gt $end) { [Windows.Forms.Application]::Exit() } })
$t.Start(); [Windows.Forms.Application]::Run()
