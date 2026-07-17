#Requires -Version 5.1
$ErrorActionPreference = 'Stop'

$repoRoot    = (Resolve-Path "$PSScriptRoot\..\..").Path
$sourceDir   = Join-Path $repoRoot 'tmp\turdmod-loader'
$sourcePak   = Join-Path $sourceDir 'pakchunk998-WindowsServer.pak'
$sourceSig   = Join-Path $sourceDir 'pakchunk998-WindowsServer.sig'
$scumPaksDir = 'C:\Program Files (x86)\Steam\steamapps\common\SCUM Server\SCUM\Content\Paks'
$destPak     = Join-Path $scumPaksDir 'pakchunk998-WindowsServer.pak'
$destSig     = Join-Path $scumPaksDir 'pakchunk998-WindowsServer.sig'
$bypassFlag  = 'C:\TurdMOD\pak_bypass.enabled'

if (-not (Test-Path $sourcePak)) { Write-Host '[ERROR] Pak not found - run cook.ps1 first' -ForegroundColor Red; exit 1 }
if (-not (Test-Path $sourceSig)) { Write-Host '[ERROR] Sig not found - run cook.ps1 first' -ForegroundColor Red; exit 1 }

$scumProc = $null
try { $scumProc = Get-Process -Name 'SCUMServer' -ErrorAction Stop } catch {}
if ($scumProc) {
    Write-Host "[WARN] SCUMServer.exe is running - files may be locked" -ForegroundColor Yellow
}

Write-Host '=== Deploying pak + sig ===' -ForegroundColor Cyan
Copy-Item -Path $sourcePak -Destination $destPak -Force
Write-Host "  Pak deployed: $destPak"
Copy-Item -Path $sourceSig -Destination $destSig -Force
Write-Host "  Sig deployed: $destSig"

$bypassDir = Split-Path $bypassFlag -Parent
if (-not (Test-Path $bypassDir)) { New-Item -ItemType Directory -Path $bypassDir -Force | Out-Null }
if (-not (Test-Path $bypassFlag)) { New-Item -ItemType File -Path $bypassFlag -Force | Out-Null }
Write-Host "  Bypass flag: $bypassFlag"

Write-Host '=== Deploy complete ===' -ForegroundColor Green
