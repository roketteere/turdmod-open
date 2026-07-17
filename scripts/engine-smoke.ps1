# engine-smoke.ps1 — end-to-end smoke test for the TurdMOD engine stack.
#
# What this does (in order):
#   1. Sanity-checks that all build artifacts exist.
#   2. Copies UE4SS.dll + TurdMODEngineBridge.dll into the target SCUMServer
#      install (canonical UE4SS layout).
#   3. Writes a minimal UE4SS-settings.ini if one doesn't exist.
#   4. Tails ue4ss.log, server-loader.log, and companion stdout in parallel.
#   5. Launches GameServer.exe via turdmod-launcher with both DLLs injected.
#
# Usage:
#   .\scripts\engine-smoke.ps1 `
#       -ServerInstall  "D:\SteamLibrary\steamapps\common\SCUM Server" `
#       [-Profile        "Game__Shipping__Win64"] `
#       [-SkipLaunch]    # only install, don't actually start SCUMServer
#       [-NotepadTarget] # inject into notepad.exe instead (no real SCUMServer needed)
#
# Stop with Ctrl+C — the tail jobs and the launched process die with the script.

[CmdletBinding()]
param(
    [Parameter(Mandatory = $false)]
    [string]$ServerInstall,

    [string]$Profile = "Game__Shipping__Win64",

    [switch]$SkipLaunch,

    [switch]$NotepadTarget
)

$ErrorActionPreference = "Stop"

# --- Paths (build artifacts produced by the toolchain) ---
$RepoRoot       = Split-Path -Parent $PSScriptRoot
$Ue4ssBuildBin  = "C:\Development\RE-UE4SS\build\$Profile\bin"
$Ue4ssDll       = Join-Path $Ue4ssBuildBin "UE4SS.dll"
$BridgeDll      = Join-Path $Ue4ssBuildBin "TurdMODEngineBridge.dll"
$LoaderDll      = Join-Path $RepoRoot "apps\turdmod-server-loader\target\release\turdmod_server_loader.dll"
$LauncherExe    = Join-Path $RepoRoot "apps\turdmod-loader\launcher\target\release\turdmod-launcher.exe"

function Assert-Exists($path, $label) {
    if (-not (Test-Path $path)) {
        throw "$label missing: $path"
    }
}

Write-Host "==> Verifying build artifacts" -ForegroundColor Cyan
Assert-Exists $Ue4ssDll   "UE4SS.dll"
Assert-Exists $BridgeDll  "TurdMODEngineBridge.dll"
Assert-Exists $LoaderDll  "turdmod_server_loader.dll"
Assert-Exists $LauncherExe "turdmod-launcher.exe"

# --- Install DLLs into SCUMServer's UE4SS folder ---
if ($ServerInstall) {
    Assert-Exists $ServerInstall "SCUMServer install"

    $Win64       = Join-Path $ServerInstall "SCUM\Binaries\Win64"
    $Ue4ssDir    = Join-Path $Win64 "UE4SS"
    $ModsDir     = Join-Path $Ue4ssDir "Mods"
    $BridgeMod   = Join-Path $ModsDir "TurdMODEngineBridge"
    $BridgeDlls  = Join-Path $BridgeMod "dlls"

    if (-not (Test-Path $Win64)) {
        throw "SCUMServer Win64 dir not found: $Win64 (is this really a SCUMServer install?)"
    }

    Write-Host "==> Installing UE4SS + bridge into $Win64" -ForegroundColor Cyan
    New-Item -ItemType Directory -Force -Path $Ue4ssDir   | Out-Null
    New-Item -ItemType Directory -Force -Path $ModsDir    | Out-Null
    New-Item -ItemType Directory -Force -Path $BridgeMod  | Out-Null
    New-Item -ItemType Directory -Force -Path $BridgeDlls | Out-Null

    Copy-Item -Force $Ue4ssDll  -Destination (Join-Path $Ue4ssDir "UE4SS.dll")
    Copy-Item -Force $BridgeDll -Destination (Join-Path $BridgeDlls "main.dll")

    # UE4SS-settings.ini — only write if missing so user tweaks survive.
    $SettingsIni = Join-Path $Ue4ssDir "UE4SS-settings.ini"
    if (-not (Test-Path $SettingsIni)) {
        @"
[Debug]
ConsoleEnabled = 1
GuiConsoleEnabled = 0
GuiConsoleVisible = 0
"@ | Set-Content -Path $SettingsIni -Encoding utf8
        Write-Host "    wrote default UE4SS-settings.ini" -ForegroundColor DarkGray
    }

    # Mods enable list — append TurdMODEngineBridge=1 if missing.
    $ModsTxt = Join-Path $ModsDir "mods.txt"
    $ModLine = "TurdMODEngineBridge : 1"
    if (-not (Test-Path $ModsTxt) -or
        -not (Select-String -Path $ModsTxt -Pattern "TurdMODEngineBridge" -Quiet -ErrorAction SilentlyContinue))
    {
        Add-Content -Path $ModsTxt -Value $ModLine
        Write-Host "    appended TurdMODEngineBridge to mods.txt" -ForegroundColor DarkGray
    }

    Write-Host "    UE4SS.dll                 -> $Ue4ssDir" -ForegroundColor DarkGray
    Write-Host "    TurdMODEngineBridge.dll   -> $BridgeDlls\main.dll" -ForegroundColor DarkGray
}
else {
    Write-Host "==> -ServerInstall not given; skipping DLL install step" -ForegroundColor Yellow
}

if ($SkipLaunch) {
    Write-Host "==> -SkipLaunch set; install done, exiting" -ForegroundColor Green
    exit 0
}

# --- Tail logs in background jobs ---
function Start-LogTail($path, $label, $color) {
    Start-Job -Name "tail-$label" -ScriptBlock {
        param($p, $l, $c)
        while (-not (Test-Path $p)) { Start-Sleep -Milliseconds 500 }
        Get-Content -Path $p -Wait -Tail 0 | ForEach-Object {
            Write-Host "[$l] $_"
        }
    } -ArgumentList $path, $label, $color | Out-Null
}

$LoaderLog   = Join-Path $env:PROGRAMDATA "TurdMOD\server-loader.log"
$Ue4ssLog    = if ($ServerInstall) { Join-Path $ServerInstall "SCUM\Binaries\Win64\UE4SS\ue4ss.log" } else { $null }

Write-Host "==> Starting log tails" -ForegroundColor Cyan
Start-LogTail $LoaderLog "loader" "Yellow"
if ($Ue4ssLog) { Start-LogTail $Ue4ssLog "ue4ss" "Cyan" }

# --- Launch ---
$LaunchTarget = if ($NotepadTarget) {
    Write-Host "==> Injecting into notepad.exe (smoke mode, no SCUMServer)" -ForegroundColor Magenta
    $env:TURDMOD_FORCE_TEST = "1"
    "C:\Windows\System32\notepad.exe"
} elseif ($ServerInstall) {
    Join-Path $ServerInstall "SCUM\Binaries\Win64\GameServer.exe"
} else {
    throw "Need either -ServerInstall <path> or -NotepadTarget"
}

Assert-Exists $LaunchTarget "launch target"

Write-Host "==> Invoking launcher" -ForegroundColor Cyan
Write-Host "    --scum      $LaunchTarget"      -ForegroundColor DarkGray
Write-Host "    --dll       $Ue4ssDll"           -ForegroundColor DarkGray
Write-Host "    --extra-dll $LoaderDll"          -ForegroundColor DarkGray

# Prefer the installed UE4SS.dll (alongside its Mods folder), not the build
# artifact — UE4SS resolves Mods/ relative to its own DLL location.
$Ue4ssDllToInject = if ($ServerInstall -and -not $NotepadTarget) {
    Join-Path $ServerInstall "SCUM\Binaries\Win64\UE4SS\UE4SS.dll"
} else { $Ue4ssDll }
if (-not (Test-Path $Ue4ssDllToInject)) {
    throw "UE4SS.dll missing at injection target: $Ue4ssDllToInject"
}

$launchArgs = @(
    "--scum",      $LaunchTarget,
    "--dll",       $Ue4ssDllToInject,
    "--extra-dll", $LoaderDll,
    "--skip-safety-check"
)

try {
    & $LauncherExe @launchArgs
}
finally {
    Write-Host "==> Cleaning up tail jobs" -ForegroundColor Cyan
    Get-Job -Name "tail-*" -ErrorAction SilentlyContinue | Remove-Job -Force
}
