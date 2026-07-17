# SCUM update detector — the reliable, build-agnostic replacement for the old
# Sentinel updater (which can't run on OVH: it read the local Steam appmanifest
# and relied on a client-app canary that doesn't exist on a headless server).
#
# Polls the PUBLIC SteamCMD API for the published build id of the SCUM server
# (3792580) + client (513710). On a change vs the ledger it (1) writes an
# escalation file for the future patch orchestrator to consume and (2) fires a
# Discord alert so Joel knows the instant an update ships — no more stumbling
# on it hours late. Detect + alert only; it does NOT touch the server.
#
# Deploy: scp to C:\TurdMOD\scum-update-detector.ps1 on OVH, drop a
# C:\TurdMOD\detector.config.json with {"webhook":"<discord url>"}, register a
# scheduled task to run it every ~10 min. Safe to run repeatedly (idempotent).
param(
    [string]$ConfigPath = 'C:\TurdMOD\detector.config.json',
    # -AutoPatch: on a detected change, launch update-patch-deploy.ps1 (must sit next to this
    #   script — i.e. run the REPO copy on the BUILD BOX, which has the toolchain; the OVH copy
    #   has no orchestrator beside it and simply can't, which is the safe default there).
    # -PatchDryRun: pass -DryRun to the orchestrator (for testing the wiring without deploying).
    [switch]$AutoPatch,
    [switch]$PatchDryRun
)
$ErrorActionPreference = 'Stop'

$Apps          = [ordered]@{ '3792580' = 'server'; '513710' = 'client' }
$SentinelDir   = 'C:\TurdMOD\sentinel'
$LedgerPath    = Join-Path $SentinelDir 'build-ledger.json'
$EscalationDir = Join-Path $SentinelDir 'escalations'
$LogPath       = Join-Path $SentinelDir 'detector.log'

New-Item -ItemType Directory -Force $SentinelDir, $EscalationDir | Out-Null

function Write-Log([string]$m) {
    "$([DateTime]::UtcNow.ToString('s'))Z  $m" | Out-File -Append -Encoding utf8 $LogPath
}

# Webhook: prefer the config file, fall back to env var. Never hardcoded (no secret in the repo).
$Webhook = $env:TURDMOD_DISCORD_WEBHOOK
if (Test-Path $ConfigPath) {
    try { $Webhook = (Get-Content $ConfigPath -Raw | ConvertFrom-Json).webhook } catch {}
}

function Get-PublishedBuild([string]$appid) {
    try {
        $r = Invoke-RestMethod -Uri "https://api.steamcmd.net/v1/info/$appid" -TimeoutSec 25
        return [string]$r.data.$appid.depots.branches.public.buildid
    } catch {
        return $null
    }
}

# Fetch live published builds. If the API is unreachable, do NOTHING (never
# false-alarm, never crash the scheduled task).
$live = [ordered]@{}
foreach ($id in $Apps.Keys) {
    $b = Get-PublishedBuild $id
    if ($b) { $live[$id] = $b }
}
if ($live.Count -eq 0) { Write-Log 'steamcmd API unreachable — skipping this run'; exit 0 }

# Load ledger (last-known builds).
$first = -not (Test-Path $LedgerPath)
$ledger = $null
if (-not $first) { try { $ledger = Get-Content $LedgerPath -Raw | ConvertFrom-Json } catch { $first = $true } }

# Detect changes.
$changes = @()
if (-not $first) {
    foreach ($id in $live.Keys) {
        $old = $ledger.$id
        $new = $live[$id]
        if ($old -and $old -ne $new) {
            $changes += [pscustomobject]@{ app = $Apps[$id]; appid = $id; old = [string]$old; new = $new }
        }
    }
}

# Persist the new ledger.
$obj = [ordered]@{}
foreach ($id in $live.Keys) { $obj[$id] = $live[$id] }
($obj | ConvertTo-Json) | Set-Content -Encoding utf8 $LedgerPath

if ($first) {
    Write-Log ("baseline recorded: " + (($live.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join ' '))
    exit 0
}

if ($changes.Count -eq 0) {
    Write-Log ("no change: " + (($live.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join ' '))
    exit 0
}

# --- Change detected: escalate + alert ---
$ts = [int][double]::Parse((Get-Date -UFormat %s))
foreach ($c in $changes) {
    $esc = @{ at = $ts; kind = 'scum_update'; app = $c.app; appid = $c.appid; old = $c.old; new = $c.new } | ConvertTo-Json
    $esc | Set-Content -Encoding utf8 (Join-Path $EscalationDir "$ts-$($c.app)-$($c.new).json")
}
$descLines = $changes | ForEach-Object { "**$($_.app)** ``$($_.old)`` -> ``$($_.new)``" }
$desc = $descLines -join "`n"
Write-Log "CHANGE detected: $($desc -replace '[`*]','')"

if ($Webhook) {
    $tail = if ($AutoPatch) { 'Auto-patch is ARMED - launching the pipeline now.' } else { 'Run the patch pipeline: scripts\update-patch-deploy.ps1' }
    $content = ":rotating_light: **SCUM update published on Steam**`n$desc`n`n$tail"
    $payload = @{ content = $content; username = 'TurdMOD Sentinel' } | ConvertTo-Json
    try {
        Invoke-RestMethod -Uri $Webhook -Method Post -Body $payload -ContentType 'application/json' -TimeoutSec 20 | Out-Null
        Write-Log 'discord alert sent'
    } catch {
        Write-Log "discord alert FAILED: $($_.Exception.Message)"
    }
} else {
    Write-Log 'no webhook configured (config.webhook / TURDMOD_DISCORD_WEBHOOK) — escalation written, no alert sent'
}

# --- Auto-patch: launch the orchestrator (build box only) ---
# The ledger has already advanced to the new build above, so a failed patch will NOT auto-retry
# on the next tick (it alerted; a human takes over) - intentional (no loop-bouncing prod).
if ($AutoPatch) {
    $orch = Join-Path $PSScriptRoot 'update-patch-deploy.ps1'
    if (-not (Test-Path $orch)) {
        Write-Log "AUTO-PATCH skipped: orchestrator not found at $orch (not the build box?)"
    }
    else {
        $lock = Join-Path $SentinelDir 'autopatch.lock'
        if (Test-Path $lock) {
            Write-Log "AUTO-PATCH skipped: lock present - a patch is already running"
        }
        else {
            Set-Content -Encoding utf8 -Path $lock -Value ([DateTime]::UtcNow.ToString('s'))
            try {
                if ($PatchDryRun) {
                    Write-Log 'AUTO-PATCH: launching orchestrator (DryRun)'
                    & $orch -DryRun
                }
                else {
                    Write-Log 'AUTO-PATCH: launching orchestrator'
                    & $orch
                }
                Write-Log "AUTO-PATCH: orchestrator exit $LASTEXITCODE"
            }
            finally {
                Remove-Item $lock -Force -ErrorAction SilentlyContinue
            }
        }
    }
}
