# deploy.ps1 -- Phase C P1: stop SCUMServer, copy
# TurdMODHelloWorld_P.pak into the SCUM Server install's
# Content/Paks/ folder.
#
# Prerequisites:
#   1. scripts/helloworld-pak/cook.ps1 ran successfully
#      -> tmp/helloworld-pak/TurdMODHelloWorld_P.pak exists.
#   2. UAC will prompt once for the SCUMServer stop (Program Files
#      writes require elevated context to be safe).
#
# After deploy:
#   - Set $env:TURDMOD_PAK_BYPASS = '1' in the shell that will
#     spawn Manager (per feedback_env_var_propagation_to_manager).
#   - Restart Manager.
#   - Manager -> Engine -> Start.
#   - Modal expected (caller-aware v3 not yet shipped); leave it.

$ScriptName = "helloworld-deploy"
$RepoRoot   = Resolve-Path "$PSScriptRoot\..\.."
$Source     = "$RepoRoot\tmp\helloworld-pak\TurdMODHelloWorld_P.pak"
$ScumPaks   = 'C:\Program Files (x86)\Steam\steamapps\common\SCUM Server\SCUM\Content\Paks'
$Dest       = "$ScumPaks\TurdMODHelloWorld_P.pak"

if (-not (Test-Path $Source)) {
    Write-Host "[$ScriptName] MISSING $Source"
    Write-Host "[$ScriptName]   -> run scripts\helloworld-pak\cook.ps1 first"
    exit 1
}
if (-not (Test-Path $ScumPaks)) {
    Write-Host "[$ScriptName] MISSING $ScumPaks"
    Write-Host "[$ScriptName]   -> is SCUM Server installed at the expected Steam path?"
    exit 2
}

# Stop SCUMServer if running (elevated runas -- same pattern as
# Manager's engine_stop).
$scum = Get-Process SCUMServer -ErrorAction SilentlyContinue
if ($scum) {
    Write-Host "[$ScriptName] Stopping SCUMServer (pid $($scum.Id), elevated)..."
    Start-Process powershell `
        -ArgumentList '-NoProfile','-Command','Stop-Process -Name SCUMServer -Force' `
        -Verb RunAs -Wait -WindowStyle Hidden | Out-Null
    Start-Sleep -Seconds 2
    if (Get-Process SCUMServer -ErrorAction SilentlyContinue) {
        Write-Host "[$ScriptName] SCUMServer still running -- deploy aborted"
        exit 3
    }
}

Copy-Item -Path $Source -Destination $Dest -Force
if (-not (Test-Path $Dest)) {
    Write-Host "[$ScriptName] Copy FAILED -- verify Program Files write perms"
    exit 4
}
$destSize = (Get-Item $Dest).Length
Write-Host "[$ScriptName] Deployed: $Dest ($destSize bytes)"

Write-Host ""
Write-Host "[$ScriptName] Next:"
Write-Host "  1. In a shell that will spawn Manager:  `$env:TURDMOD_PAK_BYPASS = '1'"
Write-Host "  2. Restart Manager (e.g. pnpm tauri dev in apps/turdmod-manager)"
Write-Host "  3. Manager -> Engine -> Start. Modal expected; LEAVE IT (don't click OK)."
Write-Host "  4. Verify:  node tools\engine-rpc-test.mjs listClassInstances --pattern HelloWorld"
Write-Host "  5. Fire:    node tools\engine-rpc-test.mjs runHelloWorld --message hi"
exit 0
