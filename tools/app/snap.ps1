# Full-screen capture at the screen's physical size (DPI aware: the Avita is 1920x1080 at 150%) to a PNG. Run in the
# desktop session. Prints the size and the foreground window's title.
#   snap.ps1 -Out shots\review\a1-home.png
param([string]$Out = 'W:\tmp\snap.png')
Add-Type -AssemblyName System.Drawing
Add-Type -Namespace App -Name Snap -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr c);
[DllImport("user32.dll")] public static extern IntPtr GetDC(IntPtr h);
[DllImport("gdi32.dll")] public static extern int GetDeviceCaps(IntPtr dc, int i);
[DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
[DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, System.Text.StringBuilder s, int n);
'@
[App.Snap]::SetProcessDpiAwarenessContext([IntPtr]-4) | Out-Null
$dc = [App.Snap]::GetDC([IntPtr]::Zero); $w = [App.Snap]::GetDeviceCaps($dc, 118); $h = [App.Snap]::GetDeviceCaps($dc, 117)
$b = New-Object System.Drawing.Bitmap $w, $h
$g = [System.Drawing.Graphics]::FromImage($b); $g.CopyFromScreen(0, 0, 0, 0, (New-Object System.Drawing.Size $w, $h)); $g.Dispose()
$Out = [IO.Path]::GetFullPath($Out); New-Item (Split-Path $Out) -ItemType Directory -Force | Out-Null
$b.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png); $b.Dispose()
$t = New-Object System.Text.StringBuilder 256; [App.Snap]::GetWindowText([App.Snap]::GetForegroundWindow(), $t, 256) | Out-Null
"$Out ${w}x$h, foreground: '$t'"
