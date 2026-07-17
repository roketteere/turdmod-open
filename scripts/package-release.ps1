# Package TurdMOD Server Pack — bundles all server-side artifacts into a
# single zip that users download from turdmod.com. One zip, one config file,
# one command to start.
#
# Usage:  .\scripts\package-release.ps1                  # package all
#         .\scripts\package-release.ps1 -ServerOnly      # server pack only
#         .\scripts\package-release.ps1 -BuildFirst       # build then package
#
# Output: releases\TurdMOD-Server-Pack-<build>.zip
#         releases\TurdMOD-Manager-<build>.msi (if manager built)

param(
    [switch]$ServerOnly,
    [switch]$BuildFirst
)
$ErrorActionPreference = 'Stop'
$Repo = 'C:\Development\Claude\turdmod'
$ReleaseDir = Join-Path $Repo 'releases'
$StagingDir = Join-Path $Repo 'releases\.staging'

# Artifact paths
$BridgeDll  = 'C:\Development\RE-UE4SS\build\Game__Shipping__Win64\bin\TurdMODEngineBridge.dll'
$LoaderDll  = Join-Path $Repo 'apps\turdmod-server-loader\target\release\turdmod_server_loader.dll'
$ServiceExe = Join-Path $Repo 'apps\turdmod-service\target\release\turdmod-service.exe'
$UE4SSDll   = 'C:\Development\RE-UE4SS\build\Game__Shipping__Win64\bin\UE4SS.dll'

# Build first if requested
if ($BuildFirst) {
    Write-Host "[pack] Building all artifacts..."
    & "$Repo\scripts\build-engine.ps1"
    if ($LASTEXITCODE -ne 0) { Write-Host "[pack] Build failed" -ForegroundColor Red; exit 1 }
}

# Verify artifacts exist
$missing = @()
foreach ($a in @($BridgeDll, $LoaderDll, $ServiceExe)) {
    if (-not (Test-Path $a)) { $missing += $a }
}
if ($missing.Count -gt 0) {
    Write-Host "[pack] Missing artifacts:" -ForegroundColor Red
    $missing | ForEach-Object { Write-Host "  $_" -ForegroundColor Red }
    Write-Host "[pack] Run with -BuildFirst or build manually first."
    exit 1
}

# Get build info (use date — the service exe can't run --version outside the service context)
$buildId = Get-Date -Format 'yyyyMMdd'
$ts = Get-Date -Format 'yyyyMMdd-HHmm'
$packName = "TurdMOD-Server-Pack-$ts"

Write-Host "[pack] Packaging: $packName"

# Clean + create staging
if (Test-Path $StagingDir) { Remove-Item -Recurse -Force $StagingDir }
New-Item -ItemType Directory -Force $StagingDir | Out-Null
$stage = Join-Path $StagingDir $packName
New-Item -ItemType Directory -Force $stage | Out-Null

# ── Server Pack contents ──

# 1. Service exe
Copy-Item $ServiceExe (Join-Path $stage 'turdmod-service.exe')
Write-Host "  + turdmod-service.exe"

# 2. Bridge DLL (in the UE4SS mod structure)
$bridgeDest = Join-Path $stage 'UE4SS\Mods\TurdMODEngineBridge\dlls'
New-Item -ItemType Directory -Force $bridgeDest | Out-Null
Copy-Item $BridgeDll (Join-Path $bridgeDest 'main.dll')
# enabled.txt
Set-Content -Path (Join-Path $stage 'UE4SS\Mods\TurdMODEngineBridge\enabled.txt') -Value ''
Write-Host "  + UE4SS/Mods/TurdMODEngineBridge/dlls/main.dll"

# 3. Server loader DLL
Copy-Item $LoaderDll (Join-Path $stage 'turdmod_server_loader.dll')
Write-Host "  + turdmod_server_loader.dll"

# 4. UE4SS (if available)
if (Test-Path $UE4SSDll) {
    Copy-Item $UE4SSDll (Join-Path $stage 'UE4SS\UE4SS.dll')
    Write-Host "  + UE4SS/UE4SS.dll"
} else {
    Write-Host "  ~ UE4SS.dll not found (user must supply their own)" -ForegroundColor Yellow
}

# 5. Config template
$configTemplate = @'
{
  "port": 9090,
  "token": "CHANGE_ME_your_secret_bearer_token",
  "scum_exe": "C:\\SCUMServer\\SCUM\\Binaries\\Win64\\GameServer.exe",
  "inject_dlls": [
    "C:\\SCUMServer\\SCUM\\Binaries\\Win64\\UE4SS\\UE4SS.dll",
    "C:\\SCUMServer\\SCUM\\Binaries\\Win64\\turdmod_server_loader.dll"
  ],
  "auto_restart": true,
  "restart_interval_hours": 6,
  "restart_countdown_seconds": 60
}
'@
Set-Content -Encoding utf8 -Path (Join-Path $stage 'service.json.template') -Value $configTemplate
Write-Host "  + service.json.template"

# 6. Quick-start guide
$quickstart = @'
# TurdMOD Server Pack — Quick Start

## 1. Extract
Extract this zip to `C:\TurdMOD\` on your game server.

## 2. Install files
Copy the DLLs to your game server directory:
- `turdmod_server_loader.dll` → next to your game server exe (e.g., `GameServer.exe`)
- `UE4SS\` folder → into `Binaries\Win64\` (so UE4SS.dll is at `Binaries\Win64\UE4SS\UE4SS.dll`)
- The `UE4SS\Mods\TurdMODEngineBridge\` folder goes inside the UE4SS Mods directory

## 3. Configure
- Copy `service.json.template` to `service.json`
- Edit `service.json`:
  - Set `scum_exe` to the full path of your game server executable
  - Set `token` to a random secret string (this is your API password)
  - Adjust `inject_dlls` paths to match your server layout

## 4. Start
```
# Install as a Windows Service (runs on boot)
turdmod-service.exe install
net start TurdMODService

# OR run in console mode for testing
turdmod-service.exe --console
```

## 5. Start the game server
```
curl -X POST -H "Authorization: Bearer YOUR_TOKEN" http://localhost:9090/server/start
```

## 6. Verify
```
# Check service health
curl http://localhost:9090/health

# Check bridge is connected
curl -X POST -H "Authorization: Bearer YOUR_TOKEN" \
  -H "Content-Type: application/json" \
  -d "{\"method\":\"ping\"}" http://localhost:9090/engine/rpc
```

You should see `{"result":{"pong":true}}`.

## Next steps
- Download the **TurdMOD Manager** desktop app for a visual dashboard
- Visit https://turdmod.com/docs for full documentation
- Visit https://github.com/roketteere/turdmod-open for the source code

## Support
- GitHub Issues: https://github.com/roketteere/turdmod-open/issues
- Discord: https://discord.gg/turdmod
- Website: https://turdmod.com
'@
Set-Content -Encoding utf8 -Path (Join-Path $stage 'QUICK-START.md') -Value $quickstart
Write-Host "  + QUICK-START.md"

# 7. License
Copy-Item (Join-Path $Repo 'LICENSE.md') (Join-Path $stage 'LICENSE.md')
Copy-Item (Join-Path $Repo 'NOTICE.md') (Join-Path $stage 'NOTICE.md')
Write-Host "  + LICENSE.md, NOTICE.md"

# ── Create zip ──
New-Item -ItemType Directory -Force $ReleaseDir | Out-Null
$zipPath = Join-Path $ReleaseDir "$packName.zip"
if (Test-Path $zipPath) { Remove-Item $zipPath }
Compress-Archive -Path "$stage\*" -DestinationPath $zipPath -CompressionLevel Optimal
$sizeMB = [math]::Round((Get-Item $zipPath).Length / 1MB, 1)
Write-Host ""
Write-Host "[pack] Server Pack ready: $zipPath ($sizeMB MB)" -ForegroundColor Green

# Clean staging
Remove-Item -Recurse -Force $StagingDir

# ── Summary ──
Write-Host ""
Write-Host "[pack] Contents:"
Write-Host "  turdmod-service.exe           $('{0:N1}' -f ((Get-Item $ServiceExe).Length / 1MB)) MB"
Write-Host "  TurdMODEngineBridge (main.dll) $('{0:N1}' -f ((Get-Item $BridgeDll).Length / 1MB)) MB"
Write-Host "  turdmod_server_loader.dll     $('{0:N1}' -f ((Get-Item $LoaderDll).Length / 1MB)) MB"
if (Test-Path $UE4SSDll) {
    Write-Host "  UE4SS.dll                     $('{0:N1}' -f ((Get-Item $UE4SSDll).Length / 1MB)) MB"
}
Write-Host "  service.json.template"
Write-Host "  QUICK-START.md"
Write-Host "  LICENSE.md + NOTICE.md"
