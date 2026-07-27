# Package TurdMOD Server Pack — bundles all server-side artifacts into a
# single zip that users download from turdmod.com. One zip, one config file,
# one command to start.
#
# Usage:  .\scripts\package-release.ps1                  # package all
#         .\scripts\package-release.ps1 -ServerOnly      # server pack only
#         .\scripts\package-release.ps1 -BuildFirst       # build then package
#
#         .\scripts\package-release.ps1 -WithSetup       # also build + bundle TurdMOD Setup
#
# Output: releases\TurdMOD-Server-Pack-<build>.zip
#         releases\TurdMOD-Manager-<build>.msi (if manager built)
#         releases\TurdMOD-Setup-<build>.exe (with -WithSetup)
#
# @inv: TurdMOD-Setup.exe ships INSIDE the Server Pack zip, at the zip root.
#       install_local.rs::find_artifacts_dir looks next to the running exe for
#       turdmod-service.exe — that adjacency is what makes "extract and run
#       Setup" work with zero configuration. Don't move it into a subfolder.

param(
    [switch]$ServerOnly,
    [switch]$BuildFirst,
    # Build TurdMOD Setup (Tauri) and include it. Needs Rust + MSVC + pnpm.
    [switch]$WithSetup
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

# TurdMOD Setup — the guided installer. Built separately because it needs the
# Node toolchain as well as Rust, and most repackages don't change it.
$SetupDir = Join-Path $Repo 'apps\turdmod-setup'
$SetupExe = Join-Path $SetupDir 'src-tauri\target\release\TurdMOD-Setup.exe'
if ($WithSetup) {
    Write-Host "[pack] Building TurdMOD Setup..."
    Push-Location $SetupDir
    pnpm install | Out-Null
    pnpm tauri build
    $rc = $LASTEXITCODE
    Pop-Location
    if ($rc -ne 0) { Write-Host "[pack] TurdMOD Setup build failed" -ForegroundColor Red; exit 1 }
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

# 4b. TurdMOD Setup — at the zip root, next to turdmod-service.exe (see @inv above)
if (Test-Path $SetupExe) {
    Copy-Item $SetupExe (Join-Path $stage 'TurdMOD-Setup.exe')
    Write-Host "  + TurdMOD-Setup.exe"
} else {
    Write-Host "  ~ TurdMOD-Setup.exe not found (run with -WithSetup to build it)" -ForegroundColor Yellow
}

# 5. Config template
# @dep: apps/turdmod-service/src/config.rs::Config — key names must match it
# exactly. scum_server_exe has no serde default, so a wrong name makes the
# service fail to parse the whole file. inject_dlls is loader-then-UE4SS.
$configTemplate = @'
{
  "port": 9090,
  "token": "CHANGE_ME_your_secret_bearer_token",
  "scum_server_exe": "C:\\SCUMServer\\SCUM\\Binaries\\Win64\\GameServer.exe",
  "scum_server_args": ["-log", "-port=7042", "-QueryPort=7044"],
  "inject_dlls": [
    "C:\\SCUMServer\\SCUM\\Binaries\\Win64\\turdmod_server_loader.dll",
    "C:\\SCUMServer\\SCUM\\Binaries\\Win64\\UE4SS\\UE4SS.dll"
  ],
  "auto_restart": true,
  "restart_delay_secs": 10,
  "scumdb_path": "C:\\SCUMServer\\SCUM\\Saved\\SaveFiles\\SCUM.db"
}
'@
Set-Content -Encoding utf8 -Path (Join-Path $stage 'service.json.template') -Value $configTemplate
Write-Host "  + service.json.template"

# 6. Quick-start guide
$quickstart = @'
# TurdMOD Server Pack — Quick Start

## The easy way (recommended)

1. Extract this zip to `C:\TurdMOD\` **on the machine that runs your game server**.
2. Right-click `TurdMOD-Setup.exe` and choose **Run as administrator**.
3. Answer one question, click through, done.

TurdMOD Setup finds your server, tells you honestly what your hosting can run,
writes the config, installs the service, and checks that it actually worked. If
anything fails it tells you the specific fix. It also has a built-in AI
assistant that can do the whole install for you — bring your own API key
(Claude, ChatGPT, Gemini, DeepSeek) or point it at free local Ollama.

Stop your SCUM server before running it — files can't be replaced while in use.

Everything below is the manual path, for people who'd rather do it by hand.

---

# Manual install

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
  - Set `scum_server_exe` to the full path of your game server executable
  - Set `owner_steam_ids` to your Steam64 ID(s) and `owner_name` to your in-game name.
    Owner-only mods (god mode, safe zones, teleport, spa, warzone...) match against these —
    leave them blank and those mods will ignore everyone, including you.
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

# 6b. VERSION.json — what this pack IS. Setup records it at install time so it
# can later compare against /releases/latest.json and offer an update.
# @dep: apps/turdmod-setup/src-tauri/src/update.rs reads both; key names must match.
$bridgeStamp = (Get-Item $BridgeDll).LastWriteTime.ToString('yyyy-MM-dd')
$versionJson = @"
{
  "build": "$ts",
  "released": "$(Get-Date -Format 'yyyy-MM-dd')",
  "engine_built": "$bridgeStamp",
  "pack": "TurdMOD-Server-Pack-latest.zip",
  "setup": "TurdMOD-Setup-latest.exe"
}
"@
Set-Content -Encoding utf8 -Path (Join-Path $stage 'VERSION.json') -Value $versionJson
Write-Host "  + VERSION.json ($ts)"

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

# latest.json — what turdmod.com advertises as current. Setup polls this.
# @inv: must be uploaded alongside the artifacts or the update check reports a
#   stale build forever. upload-release.ps1 pushes it.
$latest = Join-Path $ReleaseDir 'latest.json'
Set-Content -Encoding utf8 -Path $latest -Value $versionJson
Write-Host "[pack] latest.json written ($ts)"

# Also stage Setup as a standalone download (turdmod.com/downloads links it directly)
if (Test-Path $SetupExe) {
    Copy-Item $SetupExe (Join-Path $ReleaseDir "TurdMOD-Setup-$ts.exe")
    Copy-Item $SetupExe (Join-Path $ReleaseDir 'TurdMOD-Setup-latest.exe')
    Write-Host "[pack] TurdMOD Setup ready: releases\TurdMOD-Setup-latest.exe" -ForegroundColor Green
}

# Clean staging
Remove-Item -Recurse -Force $StagingDir

# ── Summary ──
Write-Host ""
Write-Host "[pack] Contents:"
if (Test-Path $SetupExe) {
    Write-Host "  TurdMOD-Setup.exe             $('{0:N1}' -f ((Get-Item $SetupExe).Length / 1MB)) MB"
}
Write-Host "  turdmod-service.exe           $('{0:N1}' -f ((Get-Item $ServiceExe).Length / 1MB)) MB"
Write-Host "  TurdMODEngineBridge (main.dll) $('{0:N1}' -f ((Get-Item $BridgeDll).Length / 1MB)) MB"
Write-Host "  turdmod_server_loader.dll     $('{0:N1}' -f ((Get-Item $LoaderDll).Length / 1MB)) MB"
if (Test-Path $UE4SSDll) {
    Write-Host "  UE4SS.dll                     $('{0:N1}' -f ((Get-Item $UE4SSDll).Length / 1MB)) MB"
}
Write-Host "  service.json.template"
Write-Host "  QUICK-START.md"
Write-Host "  LICENSE.md + NOTICE.md"
