#Requires -Version 5.1
<#
.SYNOPSIS
    Cook TurdMODLoader pak, then deploy to SCUMServer.
#>

$ErrorActionPreference = 'Stop'
$scriptDir = $PSScriptRoot

Write-Host "=== TurdMODLoader — Cook & Deploy ===" -ForegroundColor Magenta

& (Join-Path $scriptDir 'cook.ps1')
if ($LASTEXITCODE -ne 0) { Write-Host "[ERROR] Cook failed. Aborting deploy." -ForegroundColor Red; exit 1 }

& (Join-Path $scriptDir 'deploy.ps1')
if ($LASTEXITCODE -ne 0) { Write-Host "[ERROR] Deploy failed." -ForegroundColor Red; exit 1 }

Write-Host "`n=== TurdMODLoader — Cook & Deploy COMPLETE ===" -ForegroundColor Green
