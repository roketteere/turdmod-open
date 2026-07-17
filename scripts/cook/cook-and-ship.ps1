<#
.SYNOPSIS
    End-to-end cook + pak + deploy + verify pipeline.
.DESCRIPTION
    Wraps cook-uproject.ps1 and cook-and-deploy-bp-pak.ps1 into a single command.
    Cooks the project, then paks, deploys, and verifies the asset on the SCUM server.
.PARAMETER ProjectName
    Name of the project (e.g. "turdmod-helloworld-pak").
.PARAMETER AssetName
    Name of the asset class (e.g. "BPHelloWorld").
.PARAMETER PackagePath
    In-game package path (e.g. "/Game/TurdMOD/BPHelloWorld").
.PARAMETER ChunkNumber
    Chunk number for the pak file (e.g. 999).
.PARAMETER ScumServerRoot
    Root directory of the SCUM server installation.
.PARAMETER Iterative
    If set, passes -Iterative to the cook step for incremental cooking.
#>

param(
    [Parameter(Mandatory=$true)] [string]$ProjectName,
    [Parameter(Mandatory=$true)] [string]$AssetName,
    [Parameter(Mandatory=$true)] [string]$PackagePath,
    [Parameter(Mandatory=$true)] [int]   $ChunkNumber,
    [string]$ScumServerRoot = "C:\Program Files (x86)\Steam\steamapps\common\SCUM Server",
    [switch]$Iterative
)

$ScriptName = "cook-and-ship.ps1"
$Total = 2

function Step($n, $msg) { Write-Host ""; Write-Host "[$ScriptName] Step $n/$Total: $msg" -ForegroundColor Cyan }
function Fail($msg)     { Write-Host "[$ScriptName] FAIL: $msg" -ForegroundColor Red; exit 1 }
function Ok($msg)       { Write-Host "[$ScriptName]   ok: $msg" -ForegroundColor Green }

$ErrorActionPreference = "Stop"

# Banner
Write-Host ""
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "  Cook + Pak + Deploy + Verify Pipeline" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "  ProjectName:    $ProjectName" -ForegroundColor Gray
Write-Host "  AssetName:      $AssetName" -ForegroundColor Gray
Write-Host "  PackagePath:    $PackagePath" -ForegroundColor Gray
Write-Host "  ChunkNumber:    $ChunkNumber" -ForegroundColor Gray
Write-Host "  ScumServerRoot: $ScumServerRoot" -ForegroundColor Gray
Write-Host "  Iterative:      $($Iterative.IsPresent)" -ForegroundColor Gray
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

$StartTime = Get-Date

# Locate project root
$ProjectRoot = Resolve-Path "$PSScriptRoot\..\.."
$Uproject = "$ProjectRoot\apps\$ProjectName\$ProjectName.uproject"

if (-not (Test-Path -LiteralPath $Uproject -PathType Leaf)) {
    Fail "Uproject not found at '$Uproject'. Ensure the project directory exists under apps/$ProjectName."
}

# Stage A: Cook
Step 1 "Cook project '$ProjectName'"
$cookArgs = @{
    UprojectPath = $Uproject
}
if ($Iterative) {
    $cookArgs.Iterative = $true
}
& "$PSScriptRoot\cook-uproject.ps1" @cookArgs
if (-not $?) {
    Fail "Cook step failed with exit code $LASTEXITCODE."
}
Ok "Cook completed."

# Stage B: Pak, deploy, verify
Step 2 "Pak, deploy, and verify asset '$AssetName'"
& "$ProjectRoot\scripts\pak-probe\cook-and-deploy-bp-pak.ps1" `
    -ProjectName $ProjectName `
    -AssetName $AssetName `
    -PackagePath $PackagePath `
    -ChunkNumber $ChunkNumber `
    -ScumServerRoot $ScumServerRoot
if (-not $?) {
    Fail "Pak/deploy/verify step failed with exit code $LASTEXITCODE."
}
Ok "Pak, deploy, and verify completed."

$Elapsed = (Get-Date) - $StartTime
$ElapsedSeconds = [math]::Round($Elapsed.TotalSeconds, 1)
Write-Host ""
Write-Host "[$ScriptName] Pipeline completed in ${ElapsedSeconds}s." -ForegroundColor Green
exit 0