# Build + deploy the LOCAL/TEST bridge to the local SCUM server.
#
# ISOLATION MODEL (set up 2026-06-16 so parallel sessions never clobber each other):
#   - PRODUCTION bridge = C:\Development\RE-UE4SS\cppmods\TurdMODEngineBridge  (the OTHER
#     session owns this; it deploys to OVH). NEVER edit it for local dev.
#   - LOCAL/TEST bridge  = C:\Development\RE-UE4SS\cppmods\TurdMODEngineBridgeLocal  (OURS).
#     Own source + own DLL target. git branches there: local (dev) / dev (3rd) / production
#     (clean merge baseline). Work on `local`. This script builds + deploys it to LOCAL only.
#   - When the heli/physics work is production-ready: merge local -> the production bridge
#     (after the OVH fix lands) and deploy that to OVH.
#
# Usage:  pwsh/powershell -File scripts\deploy-local-bridge.ps1
$ErrorActionPreference = 'Stop'
$vc    = 'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat'
$cmake = 'C:\Program Files\CMake\bin\cmake.exe'
$build = 'C:\Development\RE-UE4SS\build'
$out   = "$build\Game__Shipping__Win64\bin\TurdMODEngineBridgeLocal.dll"
$dst   = 'C:\Program Files (x86)\Steam\steamapps\common\SCUM Server\SCUM\Binaries\Win64\UE4SS\Mods\TurdMODEngineBridge\dlls\main.dll'
$h     = @{ Authorization = 'Bearer $TURDMOD_ENGINE_TOKEN' }

Write-Host '[1/4] Building TurdMODEngineBridgeLocal...'
& cmd /c "`"$vc`" && `"$cmake`" --build `"$build`" --config Game__Shipping__Win64 --target TurdMODEngineBridgeLocal"
if (-not (Test-Path $out)) { throw "build output missing: $out" }

Write-Host '[2/4] Stopping SCUM + deploying DLL...'
try { Invoke-RestMethod http://localhost:9090/server/stop -Method Post -Headers $h -TimeoutSec 40 | Out-Null } catch {}
Start-Sleep 5
Get-Process SCUMServer -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 2
Copy-Item $out $dst -Force
Write-Host ("    deployed {0:N0} bytes -> main.dll" -f (Get-Item $dst).Length)

Write-Host '[3/4] Starting SCUM...'
$p = (Invoke-RestMethod http://localhost:9090/server/start -Method Post -Headers $h -TimeoutSec 40).pid
Write-Host "    SCUM pid=$p"

Write-Host '[4/4] Waiting for bridge...'
for ($i = 0; $i -lt 18; $i++) {
    try { Invoke-RestMethod http://localhost:9090/engine/rpc -Method Post -Headers $h -Body '{"method":"getOnlinePlayers"}' -ContentType 'application/json' -TimeoutSec 10 | Out-Null; Write-Host '    Bridge UP. Local heli bridge live.'; break }
    catch { Start-Sleep 8 }
}
