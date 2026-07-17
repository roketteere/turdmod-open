# Phase C end-to-end test - exercises the v3.1 file-flag bypass with
# the HelloWorld probe pak deployed, then calls runHelloWorld via the
# bridge RPC and reports whether the pak is mountable, the BP class
# is reflection-visible, and the UFunction dispatches.
#
# Preconditions (script verifies):
#  - v3.1 DLL deployed in SCUM Mods dir
#  - HelloWorld pak built at tmp/helloworld-pak/TurdMODHelloWorld_P.pak
#
# Run from PowerShell:
#   .\scripts\phase-c-end-to-end.ps1
#
# IMPORTANT: pure ASCII only (PowerShell 5.1 lexer is picky).

$ErrorActionPreference = 'Stop'

$paksDir       = 'C:\Program Files (x86)\Steam\steamapps\common\SCUM Server\SCUM\Content\Paks'
$builtPak      = 'C:\Development\Claude\turdmod\tmp\helloworld-pak\TurdMODHelloWorld_P.pak'
$deployedPak   = Join-Path $paksDir 'TurdMODHelloWorld_P.pak'
$deployedSig   = Join-Path $paksDir 'TurdMODHelloWorld_P.sig'
$flag          = 'C:\TurdMOD\pak_bypass.enabled'
$flagDir       = 'C:\TurdMOD'
$pipeFile      = "$env:LOCALAPPDATA\TurdMOD\engine\pipe.txt"
$launcher      = 'C:\Development\Claude\turdmod\apps\turdmod-loader\launcher\target\release\turdmod-launcher.exe'
$ue4ssLog      = 'C:\Program Files (x86)\Steam\steamapps\common\SCUM Server\SCUM\Binaries\Win64\UE4SS\UE4SS.log'
$scumLog       = 'C:\Program Files (x86)\Steam\steamapps\common\SCUM Server\SCUM\Saved\Logs\SCUM.log'
$rpcTest       = 'C:\Development\Claude\turdmod\tools\engine-rpc-test.mjs'

Write-Host '=== Phase C end-to-end test ===' -ForegroundColor Cyan

# 0. Preflight
if (-not (Test-Path $builtPak)) {
    Write-Host "[FAIL] $builtPak missing - run scripts\helloworld-pak\cook.ps1 first." -ForegroundColor Red
    exit 1
}
$pakInfo = Get-Item $builtPak
Write-Host ('  built pak: {0} ({1} bytes)' -f $pakInfo.LastWriteTime, $pakInfo.Length)

if (-not (Test-Path $rpcTest)) {
    Write-Host "[FAIL] $rpcTest missing." -ForegroundColor Red
    exit 1
}

# 1. Prepare elevated cleanup + deploy + flag-set in one batch
Write-Host "`nPreparing: stop SCUMServer, create flag, deploy pak + sig..."
$tmpSig = "$env:TEMP\TurdMODHelloWorld_P.sig"
Copy-Item -Path $builtPak -Destination $tmpSig -Force

$setupScript = @'
$ErrorActionPreference = 'SilentlyContinue'
# Stop any running SCUM + launcher
Get-Process SCUMServer | Stop-Process -Force
Get-Process turdmod-launcher | Stop-Process -Force
Start-Sleep -Seconds 2

# Create flag dir + flag file (opt-in to v3.1 bypass)
if (-not (Test-Path 'C:\TurdMOD')) { New-Item -ItemType Directory -Path 'C:\TurdMOD' -Force | Out-Null }
New-Item -ItemType File -Path 'C:\TurdMOD\pak_bypass.enabled' -Force | Out-Null

# Deploy pak + sig (sig is a copy of pak bytes - satisfies the
# "sig file exists" check; per-block crypto fails but v3 overrides
# those failures)
[System.IO.File]::Copy(
  'C:\Development\Claude\turdmod\tmp\helloworld-pak\TurdMODHelloWorld_P.pak',
  'C:\Program Files (x86)\Steam\steamapps\common\SCUM Server\SCUM\Content\Paks\TurdMODHelloWorld_P.pak',
  $true)
[System.IO.File]::Copy(
  "$env:TEMP\TurdMODHelloWorld_P.sig",
  'C:\Program Files (x86)\Steam\steamapps\common\SCUM Server\SCUM\Content\Paks\TurdMODHelloWorld_P.sig',
  $true)
'@
$tmpSetup = 'C:\Development\Claude\turdmod\tmp\phase-c-setup.ps1'
New-Item -ItemType Directory -Path (Split-Path $tmpSetup) -Force -ErrorAction SilentlyContinue | Out-Null
Set-Content -Path $tmpSetup -Value $setupScript -Encoding ASCII
Start-Process powershell -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File',$tmpSetup -Verb RunAs -Wait -WindowStyle Hidden
Start-Sleep -Seconds 2

# 2. Verify setup
if (-not (Test-Path $flag)) { Write-Host '[FAIL] flag not created' -ForegroundColor Red; exit 1 }
if (-not (Test-Path $deployedPak)) { Write-Host '[FAIL] pak not deployed' -ForegroundColor Red; exit 1 }
if (-not (Test-Path $deployedSig)) { Write-Host '[FAIL] sig not deployed' -ForegroundColor Red; exit 1 }
Write-Host '  flag: OK    pak: OK    sig: OK'

# 3. Launch
Write-Host "`nLaunching SCUMServer with v3.1 bridge + probe pak + flag..."
Remove-Item $pipeFile -Force -ErrorAction SilentlyContinue
$argString = "--scum `"C:\Program Files (x86)\Steam\steamapps\common\SCUM Server\SCUM\Binaries\Win64\GameServer.exe`" --dll `"C:\Program Files (x86)\Steam\steamapps\common\SCUM Server\SCUM\Binaries\Win64\UE4SS\UE4SS.dll`" --extra-dll `"C:\Development\Claude\turdmod\apps\turdmod-server-loader\target\release\turdmod_server_loader.dll`""
Start-Process -FilePath $launcher -ArgumentList $argString -Verb RunAs -PassThru | Out-Null
Write-Host '  ** APPROVE UAC PROMPT NOW **' -ForegroundColor Yellow

# 4. Watch for boot
Write-Host "`nWatching for boot (max 3 min, target RAM > 4 GB)..."
$bypassActive = $false
$pakMounted = $false
$integrityError = $false
$fatalSeen = $false
$maxRam = 0
for ($i = 1; $i -le 36; $i++) {
    Start-Sleep -Seconds 5
    $scum = Get-Process SCUMServer -ErrorAction SilentlyContinue
    if ($scum) {
        $ram = [math]::Round($scum.WorkingSet64 / 1MB, 0)
        if ($ram -gt $maxRam) { $maxRam = $ram }
        if (Test-Path $ue4ssLog) {
            $c = Get-Content $ue4ssLog -Raw -ErrorAction SilentlyContinue
            if ($c -match 'v3\.1 ACTIVE') { $bypassActive = $true }
        }
        if (Test-Path $scumLog) {
            $tail = Get-Content $scumLog -Tail 300 -ErrorAction SilentlyContinue
            if ($tail -match 'Mounting pak file.*TurdMODHelloWorld') { $pakMounted = $true }
            if ($tail -match 'integrity compromised') { $integrityError = $true }
            if ($tail -match 'Critical error|Fatal error.*BP_GameEventBorder') { $fatalSeen = $true }
        }
        Write-Host ('  +{0}s ram={1}MB bypass={2} mounted={3} integErr={4} fatal={5}' -f `
            ($i * 5), $ram, $bypassActive, $pakMounted, $integrityError, $fatalSeen)
        if ($ram -gt 4000) { Write-Host '  ** boot complete **'; break }
        if ($fatalSeen) { Write-Host '  ** fatal detected - aborting wait **' -ForegroundColor Red; break }
    } else {
        Write-Host ('  +{0}s SCUMServer not running yet' -f ($i * 5))
        if ($i -gt 6) { break }
    }
}

if ($maxRam -lt 4000) {
    Write-Host "`nFAIL - SCUM did not reach 4 GB. maxRam=$maxRam MB. bypass=$bypassActive mounted=$pakMounted integErr=$integrityError fatal=$fatalSeen" -ForegroundColor Red
    exit 1
}

# 5. RPC tests
Write-Host "`n=== RPC test 1: listClassInstances HelloWorld ===" -ForegroundColor Cyan
$listResult = & node $rpcTest listClassInstances --pattern HelloWorld 2>&1 | Out-String
$listResult | ForEach-Object { "  $_" }
$matchesLine = ($listResult -split "`n" | Where-Object { $_ -match '"matches":\s*(\d+)' } | Select-Object -First 1)
$listMatches = 0
if ($matchesLine -match '"matches":\s*(\d+)') { $listMatches = [int]$Matches[1] }

Write-Host "`n=== RPC test 2: runHelloWorld --message 'phase c works' ===" -ForegroundColor Cyan
$runResult = & node $rpcTest runHelloWorld --message 'phase c works' 2>&1 | Out-String
$runResult | ForEach-Object { "  $_" }
$runOk = ($runResult -match '"ok":\s*true')

Write-Host "`n=== VERDICT ===" -ForegroundColor Cyan
Write-Host "  bypass v3.1 ACTIVE seen:   $bypassActive"
Write-Host "  pak mounted:               $pakMounted"
Write-Host "  integrity error fired:     $integrityError"
Write-Host "  SuperStruct fatal seen:    $fatalSeen"
Write-Host "  max RAM reached:           $maxRam MB"
Write-Host "  listClassInstances matches: $listMatches"
Write-Host "  runHelloWorld ok:          $runOk"

if ($bypassActive -and $pakMounted -and (-not $integrityError) -and (-not $fatalSeen) -and ($listMatches -gt 0) -and $runOk) {
    Write-Host "`nPASS - Phase C end-to-end works!" -ForegroundColor Green
    exit 0
} else {
    Write-Host "`nFAIL - one or more verification gates did not pass" -ForegroundColor Red
    exit 1
}
