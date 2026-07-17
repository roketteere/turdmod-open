# Ollama pool health check.
# Reads endpoints.json, pings each endpoint's /api/tags, returns JSON status.
# Used by the Tauri Manager panel and by ad-hoc CLI diagnostics.
#
# Output is a single-line JSON object printed to stdout. Errors do not throw —
# they are reported per-endpoint with online=false and a reason field.

param(
    [string]$EndpointsFile = "$PSScriptRoot\endpoints.json",
    [int]$TimeoutSec = 3
)

$ErrorActionPreference = "SilentlyContinue"

if (-not (Test-Path $EndpointsFile)) {
    $err = @{ error = "endpoints.json not found at $EndpointsFile" } | ConvertTo-Json -Compress
    Write-Output $err
    exit 1
}

$config = Get-Content $EndpointsFile -Raw -Encoding utf8 | ConvertFrom-Json

$results = @()
foreach ($ep in $config.endpoints) {
    $start = Get-Date
    $online = $false
    $reason = $null
    $models = @()
    $version = $null

    try {
        $tagsResp = Invoke-RestMethod -Uri "$($ep.url)/api/tags" -TimeoutSec $TimeoutSec -ErrorAction Stop
        $online = $true
        $models = @($tagsResp.models | ForEach-Object { $_.name })
    }
    catch {
        $reason = $_.Exception.Message
    }

    if ($online) {
        try {
            $verResp = Invoke-RestMethod -Uri "$($ep.url)/api/version" -TimeoutSec $TimeoutSec -ErrorAction Stop
            $version = $verResp.version
        } catch {}
    }

    $latencyMs = [int]((Get-Date) - $start).TotalMilliseconds

    $results += [PSCustomObject]@{
        name        = $ep.name
        url         = $ep.url
        tier        = $ep.tier
        host        = $ep.host
        online      = $online
        latency_ms  = $latencyMs
        models      = $models
        model_count = $models.Count
        version     = $version
        reason      = $reason
        cost_per_hr = $ep.cost_per_hr
        tags        = $ep.tags
    }
}

@{
    checked_at = (Get-Date).ToString("o")
    endpoints  = $results
} | ConvertTo-Json -Depth 6 -Compress | Write-Output
