# v3.1 vanilla-baseline test - single-command verification that the
# new file-flag gate correctly disables the pak-bypass when no flag
# file is present, allowing vanilla SCUMServer to boot clean.
#
# Run from PowerShell:
#   .\scripts\v3-1-vanilla-baseline-test.ps1
#
# What it does:
#  1. Confirms v3.1 DLL is built (errors if missing).
#  2. Confirms C:\TurdMOD\pak_bypass.enabled is ABSENT (errors if
#     present - the test only proves the disabled-default path).
#  3. Stops any running SCUMServer (force-kills, dismisses any modal).
#  4. Deploys the new DLL via elevated copy.
#  5. Launches via turdmod-launcher.exe.
#  6. Watches UE4SS.log for the "NOT INSTALLED -- no opt-in flag" line
#     AND the bridge-ready event.
#  7. Watches SCUMServer process RAM. >1500 MB at any point = vanilla
#     init is proceeding past pak load.
#  8. Reports PASS / FAIL with a short reason.
#
# IMPORTANT: pure ASCII only - PowerShell 5.1 lexer chokes on
# em-dashes, Unicode arrows, stars, emojis, checkmarks (file is read
# as Windows-1252 by default; UTF-8 multi-byte chars corrupt string
# boundaries and produce confusing parse errors elsewhere in the file).

$ErrorActionPreference = 'Stop'

$dll      = 'C:\Development\RE-UE4SS\build\Game__Shipping__Win64\bin\TurdMODEngineBridge.dll'
$dst      = 'C:\Program Files (x86)\Steam\steamapps\common\SCUM Server\SCUM\Binaries\Win64\UE4SS\Mods\TurdMODEngineBridge\dlls\main.dll'
$ue4ssLog = 'C:\Program Files (x86)\Steam\steamapps\common\SCUM Server\SCUM\Binaries\Win64\UE4SS\UE4SS.log'
$scumLog  = 'C:\Program Files (x86)\Steam\steamapps\common\SCUM Server\SCUM\Saved\Logs\SCUM.log'
$flag     = 'C:\TurdMOD\pak_bypass.enabled'
$pipeFile = "$env:LOCALAPPDATA\TurdMOD\engine\pipe.txt"
$launcher = 'C:\Development\Claude\turdmod\apps\turdmod-loader\launcher\target\release\turdmod-launcher.exe'

Write-Host '=== v3.1 vanilla-baseline test ===' -ForegroundColor Cyan

# 1. Built DLL present
if (-not (Test-Path $dll)) {
    Write-Host '[FAIL] v3.1 DLL not built. Run tmp\build-bridge.cmd first.' -ForegroundColor Red
    exit 1
}
$dllInfo = Get-Item $dll
Write-Host ('  v3.1 DLL: {0} ({1} bytes)' -f $dllInfo.LastWriteTime, $dllInfo.Length)

# 2. Flag absent (this script ONLY tests the disabled path)
if (Test-Path $flag) {
    Write-Host "[FAIL] $flag exists. Delete it first - this script tests the disabled-default path." -ForegroundColor Red
    exit 1
}
Write-Host '  flag absent: OK (testing disabled-default path)'

# 3. Stop SCUMServer (force-kill dismisses any modal)
Write-Host "`nStopping any running SCUMServer (also dismisses modal)..."
$cleanupScript = @'
$ErrorActionPreference = 'SilentlyContinue'
Get-Process SCUMServer | Stop-Process -Force
Get-Process turdmod-launcher | Stop-Process -Force
Start-Sleep -Seconds 1
[System.IO.File]::Copy(
  'C:\Development\RE-UE4SS\build\Game__Shipping__Win64\bin\TurdMODEngineBridge.dll',
  'C:\Program Files (x86)\Steam\steamapps\common\SCUM Server\SCUM\Binaries\Win64\UE4SS\Mods\TurdMODEngineBridge\dlls\main.dll',
  $true)
'@
$tmpCleanup = 'C:\Development\Claude\turdmod\tmp\v3-1-deploy.ps1'
New-Item -ItemType Directory -Path (Split-Path $tmpCleanup) -Force -ErrorAction SilentlyContinue | Out-Null
Set-Content -Path $tmpCleanup -Value $cleanupScript -Encoding ASCII
Start-Process powershell -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File',$tmpCleanup -Verb RunAs -Wait -WindowStyle Hidden
Start-Sleep -Seconds 2

# 4. Verify deploy
$dstInfo = Get-Item $dst -ErrorAction SilentlyContinue
if (-not $dstInfo -or $dstInfo.Length -ne $dllInfo.Length) {
    Write-Host '[FAIL] deploy did not land - check UAC approval.' -ForegroundColor Red
    exit 1
}
Write-Host ('  deployed: {0}' -f $dstInfo.LastWriteTime)

# 5. Launch
Write-Host "`nLaunching SCUMServer with v3.1 bridge..."
Remove-Item $pipeFile -Force -ErrorAction SilentlyContinue
$argString = "--scum `"C:\Program Files (x86)\Steam\steamapps\common\SCUM Server\SCUM\Binaries\Win64\SCUMServer.exe`" --dll `"C:\Program Files (x86)\Steam\steamapps\common\SCUM Server\SCUM\Binaries\Win64\UE4SS\UE4SS.dll`" --extra-dll `"C:\Development\Claude\turdmod\apps\turdmod-server-loader\target\release\turdmod_server_loader.dll`""
Start-Process -FilePath $launcher -ArgumentList $argString -Verb RunAs -PassThru | Out-Null
Write-Host '  ** APPROVE UAC PROMPT NOW **' -ForegroundColor Yellow

# 6+7. Watch for boot + verdict signals
Write-Host "`nWatching for verdict signals (max 3 min)..."
$maxRam = 0
$bypassDisabledSeen = $false
$bypassOverrideSeen = $false
$bridgeReadySeen = $false
$fatalSeen = $false
for ($i = 1; $i -le 36; $i++) {
    Start-Sleep -Seconds 5
    $scum = Get-Process SCUMServer -ErrorAction SilentlyContinue
    if ($scum) {
        $ram = [math]::Round($scum.WorkingSet64 / 1MB, 0)
        if ($ram -gt $maxRam) { $maxRam = $ram }

        # Scan UE4SS.log for signals
        if (Test-Path $ue4ssLog) {
            $content = Get-Content $ue4ssLog -Raw -ErrorAction SilentlyContinue
            if ($content -match '\[BYPASS\] pak validator hook NOT INSTALLED') { $bypassDisabledSeen = $true }
            if ($content -match '\[BYPASS v3\] HOOK #')                       { $bypassOverrideSeen = $true }
            if ($content -match 'bridgeReady event emitted')                  { $bridgeReadySeen = $true }
        }
        # Scan SCUM.log for fatals THIS RUN ONLY (last 200 lines)
        if (Test-Path $scumLog) {
            $tail = Get-Content $scumLog -Tail 200 -ErrorAction SilentlyContinue
            if ($tail -match 'Critical error|Fatal error.*BP_GameEventBorder') { $fatalSeen = $true }
        }

        Write-Host ('  +{0}s pid={1} ram={2}MB  flags: disabled={3} override={4} ready={5} fatal={6}' -f `
            ($i * 5), $scum.Id, $ram, $bypassDisabledSeen, $bypassOverrideSeen, $bridgeReadySeen, $fatalSeen)

        if ($ram -gt 1500) {
            Write-Host "`n  STAR RAM > 1.5 GB - vanilla pak load is succeeding!" -ForegroundColor Green
            break
        }
    } else {
        Write-Host ('  +{0}s SCUMServer not running yet' -f ($i * 5))
        if ($i -gt 6) { break }
    }
}

# 8. Verdict
Write-Host "`n=== VERDICT ===" -ForegroundColor Cyan
$pass = $true
$reasons = New-Object System.Collections.ArrayList

if (-not $bypassDisabledSeen) { [void]$reasons.Add('NO [BYPASS] pak validator NOT INSTALLED line - v3.1 gate may not be working'); $pass = $false }
if ($bypassOverrideSeen)      { [void]$reasons.Add('[BYPASS v3] HOOK # lines present - bypass DID install despite no flag (bug)'); $pass = $false }
if (-not $bridgeReadySeen)    { [void]$reasons.Add('bridge never reached bridgeReady - process startup broken'); $pass = $false }
if ($fatalSeen)               { [void]$reasons.Add('SCUM.log has SuperStruct fatal error - pak load still breaks (drift candidate 1 or 3)'); $pass = $false }
if ($maxRam -lt 1500)         { [void]$reasons.Add("max RAM only $maxRam MB - never reached vanilla init threshold (1.5 GB)"); $pass = $false }

if ($pass) {
    Write-Host 'PASS - v3.1 file-flag gate is working. Vanilla baseline reproduced.' -ForegroundColor Green
    Write-Host "   Max RAM: $maxRam MB"
    Write-Host '   Next step (separate test): create C:\TurdMOD\pak_bypass.enabled + deploy probe pak.'
    exit 0
} else {
    Write-Host 'FAIL - see reasons below' -ForegroundColor Red
    foreach ($r in $reasons) { Write-Host "   - $r" -ForegroundColor Red }
    Write-Host "`n   max RAM: $maxRam MB"
    Write-Host "   bypass disabled seen: $bypassDisabledSeen"
    Write-Host "   bypass override seen: $bypassOverrideSeen"
    Write-Host "   bridge ready seen:    $bridgeReadySeen"
    Write-Host "   fatal seen:           $fatalSeen"
    exit 1
}
