# Deploy turdmod-service.exe to OVH. Run from local machine with SSH access to OVH.
# @inv: OVH has no git/toolchain — deploy = copy the prebuilt exe (build it locally first).
# @inv: default is EXE-ONLY. service.json on OVH holds the REAL bearer token; the repo
# template carries a placeholder ("changeme-alpha-token"). Pushing config would clobber the
# token and break the manager's auth — so -Config is opt-in and you must supply a real token.

param(
    [string]$RemoteHost = "YOUR_SERVER_IP",
    [string]$User = "admin",          # verified working SSH user (NOT "Administrator")
    [switch]$Config                   # also push config\service\service.json (see warning above)
)

$ErrorActionPreference = 'Stop'
$key    = "$env:USERPROFILE\.ssh\id_ed25519"
$svcExe = "C:\Development\Claude\turdmod\apps\turdmod-service\target\release\turdmod-service.exe"
$cfgSrc = "C:\Development\Claude\turdmod\config\service\service.json"
$ssh    = @("-i", $key, "-o", "ConnectTimeout=15", "${User}@${RemoteHost}")

Write-Host "=== Deploy turdmod-service.exe to $RemoteHost ===" -ForegroundColor Magenta

if (-not (Test-Path $svcExe)) {
    Write-Host "ERROR: build first — cargo build --release in apps/turdmod-service" -ForegroundColor Red
    exit 1
}

Write-Host "[1] Stopping remote service + any loose process so the exe unlocks..." -ForegroundColor Cyan
ssh @ssh "sc stop TurdMODService"
ssh @ssh "taskkill /F /IM turdmod-service.exe"
Start-Sleep -Seconds 3

Write-Host "[2] Copying binary..." -ForegroundColor Cyan
scp -i $key $svcExe "${User}@${RemoteHost}:C:\TurdMOD\turdmod-service.exe"
if ($Config) {
    $tok = (Get-Content $cfgSrc -Raw | ConvertFrom-Json).token
    if ($tok -eq "changeme-alpha-token") {
        Write-Host "  REFUSING to push service.json with placeholder token — set the real token first." -ForegroundColor Red
        exit 1
    }
    scp -i $key $cfgSrc "${User}@${RemoteHost}:C:\TurdMOD\service.json"
    Write-Host "  pushed service.json" -ForegroundColor Green
} else {
    Write-Host "  (config NOT pushed — preserving OVH's service.json + token; pass -Config to override)" -ForegroundColor DarkGray
}

Write-Host "[3] Re-installing + starting service (remote)..." -ForegroundColor Cyan
ssh @ssh @'
cd C:\TurdMOD
.\turdmod-service.exe --uninstall 2>$null
.\turdmod-service.exe --install
sc start TurdMODService
sc query TurdMODService
'@

Write-Host "`n[4] Verifying HTTP health + build SHA..." -ForegroundColor Cyan
Start-Sleep -Seconds 3
try {
    $resp = Invoke-RestMethod "http://${RemoteHost}:9090/health" -TimeoutSec 5
    Write-Host "  Service alive: $($resp.service) v$($resp.version) build=$($resp.build)" -ForegroundColor Green
} catch {
    Write-Host "  Health check failed — service may still be starting. Try: curl http://${RemoteHost}:9090/health" -ForegroundColor Yellow
}

Write-Host "`n=== Done ===  API: http://${RemoteHost}:9090" -ForegroundColor Green
