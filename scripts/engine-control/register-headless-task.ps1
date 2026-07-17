# One-time setup: register a Windows Task Scheduler task that runs
# turdmod-launcher.exe with the right args at HIGHEST privilege. After
# this is registered, start-server.ps1 can trigger the task via
# `schtasks /run /tn TurdMODBypassStart` with ZERO UAC prompts -- the
# task runs elevated based on its registered privilege level.
#
# This script MUST be run from an elevated PowerShell. It will UAC-prompt
# itself if not already elevated (Start-Process -Verb RunAs auto-relaunch).
#
# Run once per machine. Re-run if the launcher / dll paths change.

$ScriptName = "register-headless-task"
$TaskName = "TurdMODBypassStart"

# Are we elevated? If not, re-launch self with elevation.
$identity = [System.Security.Principal.WindowsIdentity]::GetCurrent()
$principal = New-Object System.Security.Principal.WindowsPrincipal($identity)
$isAdmin = $principal.IsInRole([System.Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Host "[$ScriptName] Not elevated -- re-launching self with UAC..."
    Start-Process -FilePath "powershell.exe" -ArgumentList "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $PSCommandPath -Verb RunAs -Wait
    exit $LASTEXITCODE
}

Write-Host "[$ScriptName] Running elevated. Registering task '$TaskName'..."

$Launcher = "C:\Development\Claude\turdmod\apps\turdmod-loader\launcher\target\release\turdmod-launcher.exe"
$ScumServer = "C:\Program Files (x86)\Steam\steamapps\common\SCUM Server\SCUM\Binaries\Win64\SCUMServer.exe"
$UE4SS_DLL = "C:\Program Files (x86)\Steam\steamapps\common\SCUM Server\SCUM\Binaries\Win64\UE4SS\UE4SS.dll"
$LoaderDLL = "C:\Development\Claude\turdmod\apps\turdmod-server-loader\target\release\turdmod_server_loader.dll"

foreach ($pair in @(@($Launcher, "turdmod-launcher.exe"), @($ScumServer, "SCUMServer.exe"), @($UE4SS_DLL, "UE4SS.dll"))) {
    if (-not (Test-Path $pair[0])) {
        Write-Host "[$ScriptName] FAILED - $($pair[1]) missing at $($pair[0])"
        exit 1
    }
}

# Build the argument string. Wrap each space-containing path in double quotes.
# Everything AFTER `--` is forwarded by turdmod-launcher to SCUMServer.exe
# (per the launcher's main.rs:53 contract). Use that to pass -nullrhi which
# forces SCUMServer into true headless mode (no RHI graphics init) -- the
# fix for the GPU-driver-TDR crashes that killed boots ~3:30 in.
$argString = "--scum `"$ScumServer`" --dll `"$UE4SS_DLL`" --extra-dll `"$LoaderDLL`" --skip-safety-check -- -nullrhi"

# Action: run turdmod-launcher with the args.
$action = New-ScheduledTaskAction -Execute $Launcher -Argument $argString

# Principal: run as the current interactive user, but at HIGHEST privilege.
# This avoids needing a saved password while still giving the launcher
# the admin token it needs for spawning elevated SCUMServer.exe.
$principal_obj = New-ScheduledTaskPrincipal `
    -UserId "$env:USERDOMAIN\$env:USERNAME" `
    -LogonType Interactive `
    -RunLevel Highest

# Settings: allow on-demand, no time limits (server runs indefinitely),
# don't start if AC is unplugged (laptops only -- desktop ignores).
$settings = New-ScheduledTaskSettingsSet `
    -AllowStartIfOnBatteries `
    -DontStopIfGoingOnBatteries `
    -ExecutionTimeLimit (New-TimeSpan -Seconds 0) `
    -StartWhenAvailable

# Trigger: NONE. The task is only fired on-demand via `schtasks /run`.
# (Register-ScheduledTask requires at least one trigger arg, so we
# pass an empty array via a workaround.)
try {
    Register-ScheduledTask `
        -TaskName $TaskName `
        -Action $action `
        -Principal $principal_obj `
        -Settings $settings `
        -Force `
        -ErrorAction Stop | Out-Null
    Write-Host "[$ScriptName] OK -- task '$TaskName' registered."
    Write-Host "  Action: $Launcher $argString"
    Write-Host ""
    Write-Host "Use it via: schtasks /run /tn $TaskName"
    Write-Host "  (no UAC prompt; task runs at Highest privilege based on registration)"
    exit 0
}
catch {
    Write-Host "[$ScriptName] FAILED - Register-ScheduledTask: $_"
    exit 1
}
