# Deploy the engine DLLs (bridge + loader + UE4SS) to OVH. Run from local with SSH to OVH.
# @inv: OVH has no toolchain — deploy = copy prebuilt DLLs. The SCUMServer process LOCKS the
# injected DLLs, so we stop it first, copy, then restart (restart re-injects per service.json).
# @inv: only copy a DLL if it actually changed (SHA differs) — pass -Force to copy regardless.

param(
    [string]$RemoteHost = "YOUR_SERVER_IP",
    [string]$User = "admin",
    [string]$Token,                   # service bearer token — if given, restart the engine via the API after copy
    [switch]$Force
)

$ErrorActionPreference = 'Stop'
$key = "$env:USERPROFILE\.ssh\id_ed25519"
$ssh = @("-i", $key, "-o", "ConnectTimeout=15", "${User}@${RemoteHost}")

# local source -> remote dest  (dest paths per the OVH C:\SCUMServer layout)
$reUe4ss = "C:\Development\RE-UE4SS\build\Game__Shipping__Win64\bin"
$loader  = "C:\Development\Claude\turdmod\apps\turdmod-server-loader\target\release\turdmod_server_loader.dll"
$map = @(
    @{ src = "$reUe4ss\TurdMODEngineBridge.dll"; dst = "C:\SCUMServer\SCUM\Binaries\Win64\UE4SS\Mods\TurdMODEngineBridge\dlls\main.dll" },
    @{ src = "$loader";                          dst = "C:\SCUMServer\SCUM\Binaries\Win64\turdmod_server_loader.dll" },
    @{ src = "$reUe4ss\UE4SS.dll";               dst = "C:\SCUMServer\SCUM\Binaries\Win64\UE4SS\UE4SS.dll" }
)

Write-Host "=== Deploy engine DLLs to $RemoteHost ===" -ForegroundColor Magenta

# Preflight: all local artifacts present
foreach ($m in $map) {
    if (-not (Test-Path $m.src)) { Write-Host "MISSING local artifact: $($m.src) — build it first." -ForegroundColor Red; exit 1 }
}

# Decide which DLLs differ (skip identical ones unless -Force)
$toCopy = @()
foreach ($m in $map) {
    $localHash = (Get-FileHash $m.src -Algorithm SHA256).Hash
    $remoteHash = (ssh @ssh "powershell -NoProfile -Command `"(Get-FileHash '$($m.dst)' -Algorithm SHA256).Hash`"" 2>$null | Out-String).Trim()
    if ($Force -or ($localHash -ne $remoteHash)) {
        Write-Host ("  CHANGED: {0}`n    local={1}`n    remote={2}" -f (Split-Path $m.dst -Leaf), $localHash, $remoteHash) -ForegroundColor Yellow
        $toCopy += $m
    } else {
        Write-Host ("  same:    {0}" -f (Split-Path $m.dst -Leaf)) -ForegroundColor DarkGray
    }
}
if ($toCopy.Count -eq 0) { Write-Host "All DLLs already in sync. Nothing to do." -ForegroundColor Green; exit 0 }

Write-Host "[1] Stopping SCUMServer on OVH (unlock DLLs)..." -ForegroundColor Cyan
ssh @ssh "taskkill /F /IM GameServer.exe 2>nul & ping -n 3 127.0.0.1 >nul"

Write-Host "[2] Copying $($toCopy.Count) DLL(s)..." -ForegroundColor Cyan
foreach ($m in $toCopy) {
    ssh @ssh "if not exist `"$(Split-Path $m.dst -Parent)`" md `"$(Split-Path $m.dst -Parent)`""
    scp -i $key $m.src "${User}@${RemoteHost}:$($m.dst)"
}

if ($Token) {
    Write-Host "[3] Restarting engine via API..." -ForegroundColor Cyan
    Invoke-RestMethod "http://${RemoteHost}:9090/server/restart" -Method Post -Headers @{ Authorization = "Bearer $Token" } -TimeoutSec 30 | Out-Null
    Write-Host "  restart requested" -ForegroundColor Green
} else {
    Write-Host "[3] DLLs copied. Restart the engine to load them (POST /server/restart, or via the manager)." -ForegroundColor Yellow
}
Write-Host "=== Done ===" -ForegroundColor Green
