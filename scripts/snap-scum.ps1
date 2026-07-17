# Capture the SCUM window to a PNG. Brings the window to the foreground
# first so the capture is unblocked by overlapping windows. Returns the
# PNG path on stdout. Caller is expected to feed the path back to Claude
# via the Read tool for visual verification.
#
# Usage:
#   pwsh C:\tmp\snap-scum.ps1                      # autonames in C:\tmp
#   pwsh C:\tmp\snap-scum.ps1 -Out C:\tmp\foo.png  # explicit output path

param(
    [string]$Out = ""
)

if (-not $Out) {
    $ts = Get-Date -Format "yyyyMMdd_HHmmss"
    $Out = "C:\tmp\scum_$ts.png"
}

Add-Type @"
using System;
using System.Runtime.InteropServices;
public class W {
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr hWnd);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L; public int T; public int R; public int B; }
}
"@

$proc = Get-Process -Name SCUM -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $proc) {
    Write-Error "SCUM process not running"
    exit 2
}
$hwnd = $proc.MainWindowHandle
if ($hwnd -eq [IntPtr]::Zero) {
    Write-Error "SCUM has no main window handle yet (may still be loading)"
    exit 3
}

# 9 = SW_RESTORE (un-minimize if needed)
if ([W]::IsIconic($hwnd)) { [W]::ShowWindow($hwnd, 9) | Out-Null }
[W]::SetForegroundWindow($hwnd) | Out-Null
Start-Sleep -Milliseconds 700

$rect = New-Object W+RECT
[W]::GetWindowRect($hwnd, [ref]$rect) | Out-Null
$w = $rect.R - $rect.L
$h = $rect.B - $rect.T
if ($w -lt 100 -or $h -lt 100) {
    Write-Error "SCUM window rect implausible: ${w}x${h}"
    exit 4
}

Add-Type -AssemblyName System.Drawing
$bmp = New-Object System.Drawing.Bitmap $w, $h
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($rect.L, $rect.T, 0, 0, [System.Drawing.Size]::new($w, $h))
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose()
$bmp.Dispose()

"snap=$Out size=$((Get-Item $Out).Length) rect=${w}x${h}"
