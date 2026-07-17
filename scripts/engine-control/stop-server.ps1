# Stops SCUMServer.exe. Cross-elevation safe: uses taskkill /F which works
# even when the calling user cannot OpenProcess across an elevation boundary.
# Returns 0 if stopped (or already-stopped), 1 if taskkill fails outright.

$ScriptName = "stop-server"

# Probe first via tasklist (Get-Process is unreliable across elevation).
$running = & tasklist /FI "IMAGENAME eq SCUMServer.exe" /NH 2>$null
if (-not ($running -match "SCUMServer\.exe")) {
    Write-Host "[$ScriptName] SCUMServer.exe not running."
    exit 0
}

Write-Host "[$ScriptName] SCUMServer.exe is running. Calling taskkill /F..."
& taskkill /F /IM SCUMServer.exe 2>&1 | ForEach-Object { Write-Host "  $_" }

# Wait up to 10 seconds for the process to actually exit.
for ($i = 0; $i -lt 20; $i++) {
    Start-Sleep -Milliseconds 500
    $check = & tasklist /FI "IMAGENAME eq SCUMServer.exe" /NH 2>$null
    if (-not ($check -match "SCUMServer\.exe")) {
        Write-Host "[$ScriptName] STOPPED."
        exit 0
    }
}

Write-Host "[$ScriptName] FAILED - process still alive after 10s. May need to retry from elevated shell."
exit 1
