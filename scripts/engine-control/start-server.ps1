# Starts GameServer.exe via turdmod-launcher with the same args the Manager
# Engine page uses. Triggers a UAC elevation prompt because GameServer.exe
# has requireAdministrator in its manifest - answer Yes once and we're off.
# If this script is already running in an elevated session, no UAC prompt.

$ScriptName = "start-server"

$Launcher = "C:\Development\Claude\turdmod\apps\turdmod-loader\launcher\target\release\turdmod-launcher.exe"
$ScumServer = "C:\Program Files (x86)\Steam\steamapps\common\SCUM Server\SCUM\Binaries\Win64\GameServer.exe"
$UE4SS_DLL = "C:\Program Files (x86)\Steam\steamapps\common\SCUM Server\SCUM\Binaries\Win64\UE4SS\UE4SS.dll"
$LoaderDLL = "C:\Development\Claude\turdmod\apps\turdmod-server-loader\target\release\turdmod_server_loader.dll"

foreach ($pair in @(@($Launcher, "turdmod-launcher.exe"), @($ScumServer, "GameServer.exe"), @($UE4SS_DLL, "UE4SS.dll"))) {
    if (-not (Test-Path $pair[0])) {
        Write-Host "[$ScriptName] FAILED - $($pair[1]) missing at $($pair[0])"
        exit 1
    }
}

# Pre-flight: refuse to start if the server is already running.
$running = & tasklist /FI "IMAGENAME eq GameServer.exe" /NH 2>$null
if ($running -match "SCUMServer\.exe") {
    Write-Host "[$ScriptName] GameServer.exe ALREADY RUNNING. Run stop-server.ps1 first."
    exit 1
}

# Headless path: if the Task Scheduler task TurdMODBypassStart has been
# registered (one-time setup via register-headless-task.ps1), trigger it
# via `schtasks /run`. This runs the launcher at HIGHEST privilege based
# on the registered principal, with ZERO UAC prompts. If the task isn't
# registered, fall back to Start-Process -Verb RunAs (which DOES prompt).
$TaskName = "TurdMODBypassStart"
$taskCheck = & schtasks /Query /TN $TaskName /FO LIST 2>&1
$taskExists = $LASTEXITCODE -eq 0

if ($taskExists) {
    Write-Host "[$ScriptName] Task '$TaskName' registered -- triggering headless."
    & schtasks /Run /TN $TaskName 2>&1 | ForEach-Object { Write-Host "  $_" }
    if ($LASTEXITCODE -ne 0) {
        Write-Host "[$ScriptName] FAILED - schtasks /run exited with code $LASTEXITCODE"
        exit 1
    }
}
else {
    Write-Host "[$ScriptName] Task '$TaskName' NOT registered -- falling back to UAC prompt."
    Write-Host "[$ScriptName] (Run register-headless-task.ps1 once to switch to headless mode.)"

    # Build args matching engine.rs::spawn_engine. Each path-arg is
    # pre-quoted because Start-Process -ArgumentList joins with spaces
    # and ShellExecuteExW needs explicit quotes around paths containing
    # spaces ("Program Files (x86)").
    $q = '"'
    $launcherArgs = @(
        "--scum",       "$q$ScumServer$q",
        "--dll",        "$q$UE4SS_DLL$q",
        "--extra-dll",  "$q$LoaderDLL$q",
        "--skip-safety-check",
        # Everything after `--` is forwarded by the launcher to GameServer.exe.
        "--",
        "-nullrhi"
    )

    $proc = Start-Process -FilePath $Launcher -ArgumentList $launcherArgs -Verb RunAs -PassThru -Wait
    if ($proc.ExitCode -ne 0) {
        Write-Host "[$ScriptName] FAILED - launcher exited with code $($proc.ExitCode)"
        exit 1
    }
}

# Confirm GameServer.exe appeared in the process list.
for ($i = 0; $i -lt 20; $i++) {
    Start-Sleep -Milliseconds 500
    $check = & tasklist /FI "IMAGENAME eq GameServer.exe" /NH 2>$null
    if ($check -match "SCUMServer\.exe") {
        Write-Host "[$ScriptName] OK - GameServer.exe is running."
        exit 0
    }
}

Write-Host "[$ScriptName] WARN - launcher OK but GameServer.exe not in tasklist after 10s. May have crashed during boot. Check UE4SS.log and SCUM.log."
exit 0
