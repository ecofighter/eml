param([string]$Example = 'counter')
$repo = (Resolve-Path "$PSScriptRoot\..\..\..").Path
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public struct RECT { public int L, T, R, B; }
public struct PT { public int X, Y; }
public static class W {
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref PT p);
  [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr c);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
}
"@
[void][W]::SetThreadDpiAwarenessContext([IntPtr](-4))
$p = Start-Process -FilePath "$repo\target\debug\eml.exe" -ArgumentList "run tests/ui/run/gui/$Example.em" -WorkingDirectory $repo -PassThru
for ($i = 0; $i -lt 40 -and $p.MainWindowHandle -eq 0; $i++) { Start-Sleep -Milliseconds 250; $p.Refresh() }
Start-Sleep -Milliseconds 1000
$h = $p.MainWindowHandle
[void][W]::SetForegroundWindow($h); Start-Sleep -Milliseconds 300
$c = New-Object RECT; [void][W]::GetClientRect($h, [ref]$c)
$o = New-Object PT; [void][W]::ClientToScreen($h, [ref]$o)
"$Example client (physical, screen): x=$($o.X)..$($o.X+$c.R) y=$($o.Y)..$($o.Y+$c.B)"
$root = [System.Windows.Automation.AutomationElement]::FromHandle($h)
$all = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
foreach ($e in $all) {
  $t = $e.Current.ControlType.ProgrammaticName -replace 'ControlType.', ''
  if ($t -in 'Button','Text','Edit','CheckBox','ProgressBar','ComboBox') {
    $b = $e.Current.BoundingRectangle
    $over = ($b.Right -gt $o.X + $c.R) -or ($b.Bottom -gt $o.Y + $c.B)
    "  $t '$($e.Current.Name)' x=$($b.Left)..$($b.Right) y=$($b.Top)..$($b.Bottom)$(if ($over) {'  <-- outside client'})"
  }
}
$bmp = New-Object System.Drawing.Bitmap ($c.R + 40), ($c.B + 40)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($o.X - 20, $o.Y - 20, 0, 0, $bmp.Size)
$shot = Join-Path $env:TEMP "eml-gui-$Example.png"; $bmp.Save($shot); "  screenshot: $shot"
Stop-Process -Id $p.Id
