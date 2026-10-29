# Test library for the dev PC: one folder per game, each with a tiny stand-in exe that just waits 5 s and exits.
# The C:\Games watcher (extras/antcloud/sync.ps1, pointed here by games-root.txt) adds them with Steam art like real games.
# -Windowed games get a stand-in that behaves like a real game window instead: black, full screen, titled with the
# folder name; it closes on WM_CLOSE (its window's X) or Esc and exits by itself after 120 s.
# usage: make-test-games.ps1 [-Root W:\games-test] [-Names 'Hades','Celeste'] [-Windowed 'Stand-in Long']   (defaults below)
param([string]$Root = 'W:\games-test', [string[]]$Names = @(
  'Dark Souls Remastered', 'Ghost of Tsushima DC', 'Spider-man Remastered', 'Sekiro - Shadows Die Twice', 'Elden Ring',
  'God of War', 'Hades', 'Hollow Knight', 'Celeste', 'Red Dead Redemption 2', 'Cyberpunk 2077', 'Horizon Zero Dawn'),
  [string[]]$Windowed = @('Stand-in Long'))
$stub = Join-Path $Root '_stub.exe'
$window = Join-Path $Root '_stub-window.exe'
New-Item $Root -ItemType Directory -Force | Out-Null
if (-not (Test-Path $stub)) {
  Add-Type -OutputType ConsoleApplication -OutputAssembly $stub -TypeDefinition 'public static class P { public static void Main() { System.Threading.Thread.Sleep(5000); } }'
}
if (-not (Test-Path $window)) {
  Add-Type -OutputType WindowsApplication -OutputAssembly $window -ReferencedAssemblies System.Windows.Forms, System.Drawing -TypeDefinition @'
using System; using System.Drawing; using System.IO; using System.Windows.Forms;
public static class P {
  [STAThread] public static void Main() {
    string name = Path.GetFileName(Path.GetDirectoryName(Application.ExecutablePath));
    var f = new Form { Text = name, BackColor = Color.Black, FormBorderStyle = FormBorderStyle.None, KeyPreview = true,
      StartPosition = FormStartPosition.Manual, Bounds = Screen.PrimaryScreen.Bounds };
    f.Controls.Add(new Label { Text = name, ForeColor = Color.FromArgb(70, 70, 70), Dock = DockStyle.Fill,
      TextAlign = ContentAlignment.MiddleCenter, Font = new Font("Segoe UI", 18) });
    f.KeyDown += (s, e) => { if (e.KeyCode == Keys.Escape) f.Close(); };
    var t = new Timer { Interval = 120000 }; t.Tick += (s, e) => f.Close(); t.Start();
    Application.Run(f);
  }
}
'@
}
foreach ($n in $Names + $Windowed) {
  $d = Join-Path $Root $n
  if (Test-Path $d) { continue }
  New-Item $d -ItemType Directory | Out-Null
  Copy-Item $(if ($Windowed -contains $n) { $window } else { $stub }) (Join-Path $d (($n -replace '[^A-Za-z0-9]', '') + '.exe'))
  "made $n"
}
