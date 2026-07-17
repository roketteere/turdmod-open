#Requires -Version 5.1
<#
.SYNOPSIS
    Cook TurdMODLoader and pack into a SCUM-compatible .pak file.
#>

$ErrorActionPreference = 'Stop'

$repoRoot      = (Resolve-Path "$PSScriptRoot\..\..").Path
$appDir        = Join-Path $repoRoot 'apps\turdmod-loader-pak'
$uproject      = Join-Path $appDir 'TurdMODLoader.uproject'
$outDir        = Join-Path $repoRoot 'tmp\turdmod-loader'
$cookedRoot    = Join-Path $appDir 'Saved\Cooked\WindowsServer\TurdMODLoader'
$responseFile  = Join-Path $outDir 'pak-response.txt'
$pakFile       = Join-Path $outDir 'pakchunk998-WindowsServer.pak'
$sigFile       = Join-Path $outDir 'pakchunk998-WindowsServer.sig'

$ue4Editor     = 'C:\Program Files\Epic Games\UE_4.27\Engine\Binaries\Win64\UE4Editor-Cmd.exe'
$unrealPak     = 'C:\Program Files\Epic Games\UE_4.27\Engine\Binaries\Win64\UnrealPak.exe'
$scumPaksDir   = 'C:\Program Files (x86)\Steam\steamapps\common\SCUM Server\SCUM\Content\Paks'
$donorSig      = Join-Path $scumPaksDir 'pakchunk32-WindowsServer.sig'

if (-not (Test-Path $uproject)) { Write-Host "[ERROR] uproject not found: $uproject" -ForegroundColor Red; exit 1 }
if (-not (Test-Path $ue4Editor)) { Write-Host "[ERROR] UE4Editor-Cmd not found: $ue4Editor" -ForegroundColor Red; exit 1 }
if (-not (Test-Path $unrealPak)) { Write-Host "[ERROR] UnrealPak not found: $unrealPak" -ForegroundColor Red; exit 1 }

if (-not (Test-Path $outDir)) { New-Item -ItemType Directory -Path $outDir -Force | Out-Null }

# Step 1 — Cook
Write-Host "`n=== Step 1/4: Cooking TurdMODLoader for WindowsServer ===" -ForegroundColor Cyan
$cookProc = Start-Process -FilePath $ue4Editor `
    -ArgumentList "`"$uproject`"", '-run=cook', '-targetplatform=WindowsServer', '-unversioned', '-iterate', '-nullrhi' `
    -NoNewWindow -Wait -PassThru

if ($cookProc.ExitCode -ne 0) { Write-Host "[ERROR] Cook failed (exit $($cookProc.ExitCode))" -ForegroundColor Red; exit 1 }
Write-Host "[OK] Cook completed." -ForegroundColor Green

# Step 2 — Response file
Write-Host "`n=== Step 2/4: Generating UnrealPak response file ===" -ForegroundColor Cyan
$cookedContentDir = Join-Path $cookedRoot 'Content'
if (-not (Test-Path $cookedContentDir)) { Write-Host "[ERROR] No Content/ in cooked output" -ForegroundColor Red; exit 1 }

$cookedFiles = Get-ChildItem -Path $cookedContentDir -Recurse -File |
    Where-Object { $_.Extension -in '.uasset', '.uexp', '.ubulk' }
if ($cookedFiles.Count -eq 0) { Write-Host "[ERROR] No cooked assets found" -ForegroundColor Red; exit 1 }

Write-Host "  Found $($cookedFiles.Count) cooked asset file(s)"
$lines = @()
foreach ($f in $cookedFiles) {
    $rel = $f.FullName.Substring($cookedContentDir.Length).TrimStart('\') -replace '\\', '/'
    $mount = "../../../SCUM/Content/TurdMOD/Loader/$rel"
    $lines += "`"$($f.FullName)`" `"$mount`""
}
$lines | Out-File -FilePath $responseFile -Encoding utf8
Write-Host "[OK] Response file: $responseFile ($($lines.Count) entries)" -ForegroundColor Green

# Step 3 — Pack
Write-Host "`n=== Step 3/4: Packing ===" -ForegroundColor Cyan
if (Test-Path $pakFile) { Remove-Item -Path $pakFile -Force }
$pakProc = Start-Process -FilePath $unrealPak `
    -ArgumentList "`"$pakFile`"", "-Create=`"$responseFile`"", '-compress' `
    -NoNewWindow -Wait -PassThru
if ($pakProc.ExitCode -ne 0) { Write-Host "[ERROR] UnrealPak failed (exit $($pakProc.ExitCode))" -ForegroundColor Red; exit 1 }
$pakSize = (Get-Item $pakFile).Length
Write-Host "[OK] Pak: $pakFile ($([math]::Round($pakSize / 1KB, 1)) KB)" -ForegroundColor Green

# Step 4 — Sig sidecar
Write-Host "`n=== Step 4/4: Copying borrowed .sig ===" -ForegroundColor Cyan
if (Test-Path $donorSig) {
    Copy-Item -Path $donorSig -Destination $sigFile -Force
    Write-Host "[OK] Sig: $sigFile (borrowed from pakchunk32)" -ForegroundColor Green
} else {
    Write-Host "[WARN] Donor sig not found - deploy will fail without a .sig" -ForegroundColor Yellow
}

Write-Host "`n=== Cook complete ===" -ForegroundColor Green
Write-Host "  Pak: $pakFile"
Write-Host "  Sig: $sigFile"
