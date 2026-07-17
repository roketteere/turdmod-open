# TurdMOD update -> patch -> deploy orchestrator (the "one command").
#
# Runs on the LOCAL BUILD BOX (it has the C++/Rust toolchain + SSH to OVH; OVH has
# no toolchain). Executes the full runbook Joel set — server DOWN until confirmed:
#
#   preflight -> final DB snapshot -> stop SCUM + disable scheduler -> steamcmd update
#   -> build bridge+loader+service (build-engine.ps1) -> push artifacts (with backups)
#   -> start SCUM -> AUTO-CONFIRM (service/7042/pipe/ping/RPC) ->
#        success: re-enable scheduler, notify OK
#        failure: STOP SCUM (stay down), leave scheduler off, notify FAILURE, exit 1
#
# Discord-notifies each stage. Idempotent-ish; fail-safe (a broken patch leaves the
# server DOWN + alerted, never up-and-broken). A full game-version rollback isn't
# possible (steamcmd won't downgrade, DB schema migrates), so failure = down + alert
# for a human to take over — with the pre-update DB snapshot + .bak artifacts on hand.
#
# Config (secrets, NOT in the repo): C:\TurdMOD\orchestrator.config.json
#   { "host","user","keyPath","token","webhook","scumcmd" }
#
# Usage:  .\scripts\update-patch-deploy.ps1                 # full run
#         .\scripts\update-patch-deploy.ps1 -SkipUpdate     # patch only (steamcmd already ran)
#         .\scripts\update-patch-deploy.ps1 -SkipBuild      # use existing artifacts
#         .\scripts\update-patch-deploy.ps1 -DryRun         # log actions, skip destructive ones
param(
    [string]$ConfigPath = 'C:\TurdMOD\orchestrator.config.json',
    [switch]$SkipBuild,
    [switch]$SkipUpdate,
    [switch]$DryRun
)
$ErrorActionPreference = 'Continue'   # native stderr is not fatal; we check exit codes
$Repo = 'C:\Development\Claude\turdmod'
$LogFile = 'C:\TurdMOD\orchestrator.log'
$script:Stage = 'init'

if (-not (Test-Path $ConfigPath)) { Write-Host "config not found: $ConfigPath" -ForegroundColor Red; exit 2 }
$cfg     = Get-Content $ConfigPath -Raw | ConvertFrom-Json
$OvhHost = $cfg.host
$User    = $cfg.user
$Key     = $cfg.keyPath
$Token   = $cfg.token
$Webhook = $cfg.webhook
$Api     = "http://${OvhHost}:9090"

# Remote (OVH) paths.
$SaveDir      = 'C:\SCUMServer\SCUM\Saved\SaveFiles'
$SnapDir      = 'C:\TurdMOD\prewipe-snapshot'
$BridgeRemote = 'C:\SCUMServer\SCUM\Binaries\Win64\UE4SS\Mods\TurdMODEngineBridge\dlls\main.dll'
$LoaderRemote = 'C:\SCUMServer\SCUM\Binaries\Win64\turdmod_server_loader.dll'
$SchedFile    = 'C:\TurdMOD\restart-schedule.json'
# Local artifacts (produced by build-engine.ps1).
$BridgeLocal  = 'C:\Development\RE-UE4SS\build\Game__Shipping__Win64\bin\TurdMODEngineBridge.dll'
$LoaderLocal  = "$Repo\apps\turdmod-server-loader\target\release\turdmod_server_loader.dll"

# ── helpers ──────────────────────────────────────────────────────────────────
function Log([string]$m) {
    $line = "$([DateTime]::Now.ToString('HH:mm:ss')) [$script:Stage] $m"
    Write-Host "[orch] $line" -ForegroundColor Cyan
    try { $line | Out-File -Append -Encoding utf8 $LogFile } catch {}
}
function Notify([string]$emoji, [string]$msg) {
    if ($DryRun) { Log "(notify suppressed in dryrun) $emoji $msg"; return }
    if (-not $Webhook) { return }
    $body = @{ username = 'TurdMOD Orchestrator'; content = "$emoji **[$script:Stage]** $msg" } | ConvertTo-Json
    try { Invoke-RestMethod -Uri $Webhook -Method Post -Body $body -ContentType 'application/json' -TimeoutSec 15 | Out-Null } catch {}
}
# @brk: ssh.exe/scp.exe (NOT bare `ssh`) — bare `ssh` is case-insensitively the same token as
#   the `Ssh` function and can resolve back to it (recursion -> empty output). Explicit .exe
#   always binds the application.
function Ssh([string]$cmd) { & ssh.exe -i $Key -o ConnectTimeout=20 -o StrictHostKeyChecking=accept-new "${User}@${OvhHost}" $cmd 2>$null }
# @brk: remote PowerShell MUST go through SshPs, never a quoted -Command via Ssh. Local-PS
#   native-arg passing shreds embedded double-quotes on the way to ssh, so any inline
#   `powershell -Command "..."` arrives mangled (silently produced 0-file snapshots +
#   un-toggled scheduler on the first real run). -EncodedCommand is pure base64 — no quote
#   survives-the-shell problem across the PS -> ssh -> remote-cmd -> PS layers.
function SshPs([string]$ps) {
    $enc = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($ps))
    Ssh "powershell -NoProfile -EncodedCommand $enc"
}
function Scp([string]$local, [string]$remote) { & scp.exe -i $Key -o ConnectTimeout=30 $local "${User}@${OvhHost}:$remote" 2>$null; return ($LASTEXITCODE -eq 0) }
function ApiPost([string]$path, [int]$timeout = 30) {
    try { return Invoke-RestMethod -Uri "$Api$path" -Method Post -Headers @{ Authorization = "Bearer $Token" } -TimeoutSec $timeout } catch { return $null }
}
# @brk: toggle the restart scheduler through the API, NEVER by editing restart-schedule.json
#   directly. PowerShell `Set-Content -Encoding utf8` (5.1) prepends a UTF-8 BOM, and the
#   Rust service's serde_json::from_str fails on a leading BOM -> load() silently returns the
#   default (enabled=false) -> scheduler never fires. The API's save() writes clean JSON.
function ApiPostBody([string]$path, [string]$json, [int]$timeout = 15) {
    try { return Invoke-RestMethod -Uri "$Api$path" -Method Post -Headers @{ Authorization = "Bearer $Token" } -Body $json -ContentType 'application/json' -TimeoutSec $timeout } catch { return $null }
}
function ApiGet([string]$path, [int]$timeout = 10) {
    try { return Invoke-RestMethod -Uri "$Api$path" -Headers @{ Authorization = "Bearer $Token" } -TimeoutSec $timeout } catch { return $null }
}
function EngineRpc([string]$method, [int]$timeout = 20) {
    $b = @{ method = $method } | ConvertTo-Json
    try { return Invoke-RestMethod -Uri "$Api/engine/rpc" -Method Post -Headers @{ Authorization = "Bearer $Token" } -Body $b -ContentType 'application/json' -TimeoutSec $timeout } catch { return $null }
}
function Fail([string]$msg) {
    Log "FAILURE: $msg"
    Notify ':x:' "$msg`n-> stopping SCUM, leaving it DOWN + scheduler off for manual recovery. Pre-update DB snapshot + .bak artifacts are on OVH."
    # Honor 'down until working': ensure SCUM is stopped, scheduler stays disabled.
    if (-not $DryRun) { ApiPost '/server/stop' 20 | Out-Null }
    exit 1
}
# Poll for a healthy engine: SCUM up + 7042 bound + bridge pong + a real RPC. Returns $true
# on confirm, $false after ~3.5 min (10x20s). Idempotent — safe to call again after a restart.
function Confirm-Engine([int]$attempts = 10) {
    for ($i = 0; $i -lt $attempts; $i++) {
        Start-Sleep 20
        $st = ApiGet '/status'
        if (-not $st -or -not $st.server_running) { Log "attempt $($i+1): SCUM not up yet"; continue }
        # @brk: -join to a scalar BEFORE -notmatch. Ssh returns a multi-LINE array, and
        #   `$array -notmatch ':7042'` returns every non-matching line (non-empty = always
        #   truthy) — it NEVER confirms even when 7042 is bound. Collapsing makes it boolean.
        $seven = (Ssh 'netstat -an -p UDP') -join "`n"
        if ($seven -notmatch ':7042') { Log "attempt $($i+1): 7042 not bound yet"; continue }
        $pong = EngineRpc 'ping'
        if (-not $pong -or -not $pong.result.pong) { Log "attempt $($i+1): bridge pipe not answering yet"; continue }
        $players = EngineRpc 'getOnlinePlayers'
        if ($null -eq $players -or $null -eq $players.result) { Log "attempt $($i+1): RPC (getOnlinePlayers) not ready"; continue }
        Log "CONFIRMED: SCUM up (pid $($st.pid)), 7042 bound, bridge pong, RPC online=$($players.result.count)"
        return $true
    }
    return $false
}

Log "=== orchestrator start (DryRun=$DryRun SkipUpdate=$SkipUpdate SkipBuild=$SkipBuild) ==="
Notify ':gear:' "Update pipeline starting on $OvhHost (dryrun=$DryRun)."

# ── 1. preflight ─────────────────────────────────────────────────────────────
$script:Stage = 'preflight'
$ping = Ssh 'echo ok'
if ($ping -notmatch 'ok') { Fail "SSH to $OvhHost failed" }
$st = ApiGet '/status'
if (-not $st) { Fail "service API $Api unreachable" }
Log "SSH ok; service alive; SCUM running=$($st.server_running) build=$($st.build)"

# ── 2. final pre-update DB snapshot ──────────────────────────────────────────
$script:Stage = 'snapshot'
if ($DryRun) { Log 'DRYRUN skip snapshot' }
else {
    $ps = @"
Remove-Item '$SnapDir' -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force '$SnapDir' | Out-Null
Copy-Item @('$SaveDir\SCUM.db','$SaveDir\SCUM.db-wal','$SaveDir\SCUM.db-shm') '$SnapDir' -Force -ErrorAction SilentlyContinue
(Get-ChildItem '$SnapDir' -File | Measure-Object).Count
"@
    $ok = (SshPs $ps | Select-Object -Last 1)
    Log "snapshot -> $SnapDir ($ok files)"
    if ("$ok" -notmatch '[1-9]') { Fail 'snapshot produced no files' }
}
Notify ':floppy_disk:' 'Pre-update DB snapshot captured.'

# ── 3. stop SCUM + disable restart scheduler ─────────────────────────────────
$script:Stage = 'stop'
if ($DryRun) { Log 'DRYRUN skip stop' }
else {
    ApiPostBody '/server/schedule' '{"enabled":false}' | Out-Null
    ApiPost '/server/stop' 25 | Out-Null
    $gone = $false
    for ($i = 0; $i -lt 12; $i++) {
        Start-Sleep 3
        # SshPs (quote-free) not `tasklist /FI "..."` — embedded quotes mangle through PS->ssh and
        #   could false-read "gone". Get-Process emits 'SCUMServer' only while the proc is alive.
        if ((SshPs "(Get-Process SCUMServer -ErrorAction SilentlyContinue | Out-String)") -notmatch 'SCUMServer') { $gone = $true; break }
    }
    if (-not $gone) { Fail 'SCUM did not stop' }
    Log 'SCUM stopped; restart scheduler disabled'
}

# ── 4. steamcmd update ───────────────────────────────────────────────────────
$script:Stage = 'update'
if ($SkipUpdate -or $DryRun) { Log 'skip steamcmd update' }
else {
    Notify ':arrow_down:' 'Running steamcmd update (this takes a few minutes)...'
    $sc = $cfg.scumcmd; if (-not $sc) { $sc = 'C:\SteamCMD\steamcmd.exe' }
    Ssh "$sc +force_install_dir C:\SCUMServer +login anonymous +app_update 3792580 validate +quit" | Out-Null
    $bid = SshPs "(Select-String -Path 'C:\SCUMServer\steamapps\appmanifest_3792580.acf' -Pattern 'buildid').Line"
    Log "steamcmd done; installed manifest: $($bid -replace '\s+',' ')"
}

# ── 5. build bridge + loader + service ───────────────────────────────────────
$script:Stage = 'build'
if ($SkipBuild) { Log 'skip build (using existing artifacts)' }
else {
    & "$Repo\scripts\build-engine.ps1" -Bridge -Loader | Out-Host
    if ($LASTEXITCODE -ne 0) { Fail 'build-engine.ps1 failed' }
    Log 'bridge + loader built'
}
foreach ($a in @($BridgeLocal, $LoaderLocal)) { if (-not (Test-Path $a)) { Fail "artifact missing: $a" } }
Notify ':hammer_and_wrench:' 'Bridge + loader built for the new build.'

# ── 6. push artifacts (with backups) ─────────────────────────────────────────
$script:Stage = 'push'
if ($DryRun) { Log 'DRYRUN skip push' }
else {
    $tag = 'bak-' + ([int][double]::Parse((Get-Date -UFormat %s)))
    SshPs "Copy-Item '$BridgeRemote' '$BridgeRemote.$tag' -Force -EA SilentlyContinue; Copy-Item '$LoaderRemote' '$LoaderRemote.$tag' -Force -EA SilentlyContinue" | Out-Null
    if (-not (Scp $BridgeLocal $BridgeRemote)) { Fail 'scp bridge failed' }
    if (-not (Scp $LoaderLocal $LoaderRemote)) { Fail 'scp loader failed' }
    Log "pushed bridge + loader (backups tagged .$tag)"
}
Notify ':outbox_tray:' 'Patched DLLs pushed to OVH.'

# ── 7. start SCUM ────────────────────────────────────────────────────────────
$script:Stage = 'start'
if ($DryRun) { Log 'DRYRUN skip start'; Notify ':white_check_mark:' 'DryRun complete (no destructive actions taken).'; exit 0 }
$r = ApiPost '/server/start' 30
if (-not $r) { Fail 'server start call failed' }
Log "start issued (pid $($r.pid))"

# ── 8. AUTO-CONFIRM (server up + engine working) ─────────────────────────────
$script:Stage = 'confirm'
$confirmed = Confirm-Engine
if (-not $confirmed) {
    # @brk: the engine bridge can wedge on the FIRST boot after a steamcmd update
    #   (ERROR_PIPE_BUSY 231 / RPC timeout — transient game-thread gate contention, NOT a
    #   code regression); a single clean restart clears it (verified live 2026-07-08, the
    #   24107648 patch). Try ONE restart + re-confirm before declaring failure, so an
    #   unattended auto-patch doesn't false-Fail (and stop) a server that's actually healthy.
    Log 'confirm exhausted — one clean restart, then re-confirm (first-boot bridge-wedge recovery)'
    Notify ':arrows_counterclockwise:' 'Engine did not confirm; one clean restart, then re-confirm before giving up.'
    ApiPost '/server/restart?countdown=0' 30 | Out-Null
    $confirmed = Confirm-Engine
}
if (-not $confirmed) { Fail 'engine did not confirm healthy even after a restart retry (bridge/RPC/pipe/7042)' }

# ── 9. success: re-enable scheduler ──────────────────────────────────────────
$script:Stage = 'finalize'
ApiPostBody '/server/schedule' '{"enabled":true}' | Out-Null
Log 'restart scheduler re-enabled'
$fin = ApiGet '/status'
Notify ':white_check_mark:' "Update complete + modded engine confirmed working. Server UP. build=$($fin.build)"
Log '=== orchestrator success ==='
exit 0
