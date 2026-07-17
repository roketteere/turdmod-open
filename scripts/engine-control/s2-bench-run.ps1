# S2 bench run - load a REAL custom asset on the local bench end-to-end.
# Prereqs (verified primed 2026-06-13): 6/6 bridge deployed to the Steam UE4SS
# Mods dir, C:\TurdMOD\pak_bypass.enabled present, TurdMODHelloWorld_P.pak + .sig
# staged in Content\Paks, turdmod-service up on :9090.
#
# Launch path: service /server/start (injects loader + UE4SS + -nullrhi per
# service.json AND owns the engine-RPC pipe). The 6/6 bypass applies regardless
# of launcher because the bridge installs the byte-patches at init.
#
# @brk: if the deployed bridge is NOT 6/6 (or the flag is off), the staged pak
#   fails sig validation and SCUM calls LogExit:Exiting ~1s into boot - this
#   script detects that and aborts loud. See reference_ovh_leftover_pak_bootkill.
# @inv: ASCII ONLY - PowerShell 5.1 mangles non-ASCII (em-dash/box-draw) and
#   the parse breaks. Do not reintroduce unicode punctuation.
#
# Usage:  .\s2-bench-run.ps1     (best on a fresh reboot = clean GPU, no TDR)

$ErrorActionPreference = 'Stop'
$Port   = 9090
$Token  = 'local-dev-token'
$Base   = "http://127.0.0.1:$Port"
$Hdr    = @{ Authorization = "Bearer $Token" }
$Steam  = 'C:\Program Files (x86)\Steam\steamapps\common\SCUM Server\SCUM'
$ScumLog= "$Steam\Saved\Logs\SCUM.log"
$Ue4Log = "$Steam\Binaries\Win64\UE4SS\UE4SS.log"
$PkgPath= '/Game/TurdMOD/BPHelloWorld'

function Say($m){ Write-Host "[s2] $m" }
function Rpc($method,$params){
  $body = @{ method=$method; params=$params } | ConvertTo-Json -Compress
  Invoke-RestMethod -Uri "$Base/engine/rpc" -Method Post -Headers $Hdr -ContentType 'application/json' -Body $body -TimeoutSec 30
}

# 0. Pre-flight
Say 'pre-flight'
$built = 'C:\Development\RE-UE4SS\build\Game__Shipping__Win64\bin\TurdMODEngineBridge.dll'
$dep   = "$Steam\Binaries\Win64\UE4SS\Mods\TurdMODEngineBridge\dlls\main.dll"
if(-not (Test-Path 'C:\TurdMOD\pak_bypass.enabled')){ throw 'flag C:\TurdMOD\pak_bypass.enabled MISSING' }
if(-not (Test-Path "$Steam\Content\Paks\TurdMODHelloWorld_P.pak")){ throw 'custom pak NOT staged in Content\Paks' }
if(-not (Test-Path $dep)){ throw 'bridge main.dll NOT deployed' }
if((Get-FileHash $dep).Hash -ne (Get-FileHash $built).Hash){ throw 'deployed bridge != built 6/6 bridge - redeploy first' }
try { $h = Invoke-RestMethod "$Base/health" -TimeoutSec 5 } catch { throw "service not up on :$Port" }
Say "  OK - 6/6 bridge live, flag+pak staged, service build $($h.build)"

# 1. Restart SCUM via the service (fresh pipe handle, loader injected)
Say 'stopping SCUM (if up)'
try { Invoke-RestMethod "$Base/server/stop" -Method Post -Headers $Hdr -TimeoutSec 30 | Out-Null } catch {}
Start-Sleep 4
Say 'starting SCUM (service injects loader + UE4SS + -nullrhi)'
$start = Invoke-RestMethod "$Base/server/start" -Method Post -Headers $Hdr -TimeoutSec 30
Say "  /server/start -> $($start | ConvertTo-Json -Compress)"

# 2. Wait for a REAL world boot (multi-GB) OR detect a pak boot-crash
Say 'waiting for world boot (multi-GB working set; abort on LogExit)'
$booted = $false
for($i=0; $i -lt 60; $i++){
  Start-Sleep 5
  $p = Get-Process SCUMServer -ErrorAction SilentlyContinue
  if(-not $p){
    $tail = Get-Content $ScumLog -Tail 6 -ErrorAction SilentlyContinue
    if($tail -match 'integrity compromised|signing mismatch'){
      throw ("BYPASS FAILED - pak sig rejected, SCUM exited on boot. tail: " + ($tail -join ' | '))
    }
    throw ("SCUM gone during boot (not a pak-sig exit). tail: " + ($tail -join ' | '))
  }
  $mb = [math]::Round($p.WorkingSet64/1MB)
  if($i % 3 -eq 0){ Say ("  t=" + [int]($i*5) + "s  ws=" + $mb + "MB") }
  if($p.WorkingSet64 -gt 2GB){ $booted=$true; Say ("  BOOTED - ws=" + $mb + "MB"); break }
}
if(-not $booted){ throw 'world did not reach multi-GB within ~5min - boot stalled' }

# 3. Confirm bypass applied + pak mounted (UE4SS log)
Say 'bypass markers in UE4SS.log:'
Select-String -Path $Ue4Log -Pattern '\[BYPASS' -ErrorAction SilentlyContinue |
  Select-Object -Last 12 | ForEach-Object { Write-Host "    $($_.Line)" }

# 4. THE S2 TEST: loadAsset then runHelloWorld
Start-Sleep 8
Say "loadAsset $PkgPath"
$la = Rpc 'loadAsset' @{ packagePath = $PkgPath }
Say "  -> $($la | ConvertTo-Json -Compress)"

Say 'runHelloWorld'
$rh = Rpc 'runHelloWorld' @{}
Say "  -> $($rh | ConvertTo-Json -Compress)"

# 5. Verdict
$okLoad = ($la.result.ok -eq $true) -or ($la.ok -eq $true)
$okRun  = ($rh.result.ok -eq $true) -or ($rh.ok -eq $true)
Write-Host ''
if($okLoad -and $okRun){
  Write-Host '[s2] ===== PASS - custom asset loaded + BP dispatched server-side =====' -ForegroundColor Green
} elseif($okLoad){
  Write-Host '[s2] PARTIAL - loadAsset ok but runHelloWorld did not confirm. See output above.' -ForegroundColor Yellow
} else {
  Write-Host '[s2] FAIL - loadAsset did not return ok. See output + UE4SS.log [loadAsset] lines.' -ForegroundColor Red
}
