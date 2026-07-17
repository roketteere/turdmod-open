#Requires -RunAsAdministrator
# Install turdmod-service as a LOCAL Windows service (runs as SYSTEM so DLL injection
# works), BattlEye OFF, matching the OVH deployment exactly. Run elevated.
# @inv: local server is the RE box — for BE-ON pak-bypass work use start-server.ps1 instead;
# this is the BE-off "resting"/parity posture so the manager drives local like OVH over HTTP.
param([switch]$NoServerStart)

$ErrorActionPreference = 'Continue'
$log = "C:\TurdMOD\install-local.log"
New-Item -ItemType Directory -Force "C:\TurdMOD" | Out-Null
Start-Transcript -Path $log -Force | Out-Null

$rel = "C:\Development\Claude\turdmod\apps\turdmod-service\target\release\turdmod-service.exe"
$cfg = "C:\Development\Claude\turdmod\config\service\service-local.json"
$dst = "C:\TurdMOD\turdmod-service.exe"

try {
    if (-not (Test-Path $rel)) { throw "build first: cargo build --release in apps/turdmod-service" }

    Write-Host "[1] Stopping existing server + service..."
    cmd /c "taskkill /F /IM SCUMServer.exe 2>nul" | Out-Null
    & sc.exe stop TurdMODService | Out-Null
    cmd /c "taskkill /F /IM turdmod-service.exe 2>nul" | Out-Null
    Start-Sleep -Seconds 3

    Write-Host "[2] Copying release exe + local config to C:\TurdMOD..."
    Copy-Item $rel $dst -Force
    Copy-Item $cfg "C:\TurdMOD\service.json" -Force

    Write-Host "[3] (Re)installing Windows service..."
    & $dst --uninstall  2>&1 | Out-Null
    Start-Sleep -Seconds 1
    & $dst --install
    Start-Sleep -Seconds 2
    & sc.exe start TurdMODService
    Start-Sleep -Seconds 5

    Write-Host "[4] Health:"
    try { Invoke-RestMethod "http://localhost:9090/health" -TimeoutSec 6 | ConvertTo-Json -Compress | Write-Host }
    catch { Write-Host "  /health not ready: $_" }

    if (-not $NoServerStart) {
        Write-Host "[5] Launching SCUMServer BE-off (via service /server/start)..."
        $tok = (Get-Content "C:\TurdMOD\service.json" -Raw | ConvertFrom-Json).token
        try { Invoke-RestMethod "http://localhost:9090/server/start" -Method Post -Headers @{ Authorization = "Bearer $tok" } -TimeoutSec 40 | ConvertTo-Json -Compress | Write-Host }
        catch { Write-Host "  /server/start failed: $_" }
    }
    Write-Host "[OK] local service install complete."
}
catch { Write-Host "[ERROR] $_" }
finally { Stop-Transcript | Out-Null }
