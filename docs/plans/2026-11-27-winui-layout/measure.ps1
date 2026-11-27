param([string]$Example = 'counter')
$repo = (Resolve-Path "$PSScriptRoot\..\..\..").Path
Add-Type @"
using System; using System.Runtime.InteropServices;
public struct RECT { public int L, T, R, B; }
public static class W {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr c);
  [DllImport("user32.dll")] public static extern IntPtr GetWindowDpiAwarenessContext(IntPtr h);
  [DllImport("user32.dll")] public static extern int GetAwarenessFromDpiAwarenessContext(IntPtr c);
  [DllImport("user32.dll")] public static extern IntPtr MonitorFromWindow(IntPtr h, uint f);
  [DllImport("shcore.dll")] public static extern int GetDpiForMonitor(IntPtr m, int t, out uint x, out uint y);
}
"@
[void][W]::SetThreadDpiAwarenessContext([IntPtr](-4))
$p = Start-Process -FilePath "$repo\target\debug\eml.exe" -ArgumentList "run tests/ui/run/gui/$Example.em" -WorkingDirectory $repo -PassThru
for ($i = 0; $i -lt 40 -and $p.MainWindowHandle -eq 0; $i++) { Start-Sleep -Milliseconds 250; $p.Refresh() }
Start-Sleep -Milliseconds 500
$h = $p.MainWindowHandle
$r = New-Object RECT; $c = New-Object RECT
[void][W]::GetWindowRect($h, [ref]$r); [void][W]::GetClientRect($h, [ref]$c)
$dpi = [W]::GetDpiForWindow($h)
$aw = [W]::GetAwarenessFromDpiAwarenessContext([W]::GetWindowDpiAwarenessContext($h))
$mx = 0; $my = 0; [void][W]::GetDpiForMonitor([W]::MonitorFromWindow($h, 2), 0, [ref]$mx, [ref]$my)
"$Example awareness=$aw windowDpi=$dpi monitorDpi=$mx window=$($r.R-$r.L)x$($r.B-$r.T) client=$($c.R)x$($c.B)"
Stop-Process -Id $p.Id
