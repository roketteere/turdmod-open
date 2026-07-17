# Ollama pool dispatch.
# Sends a prompt to a chosen endpoint, returns the generated text + metadata.
#
# Example:
#   .\dispatch.ps1 -Endpoint wife-rig -Prompt "Hello" -Model qwen2.5-coder:7b
#
# Output: JSON object with { endpoint, model, response, eval_count,
#         eval_duration_ms, tokens_per_sec, total_duration_ms, error }

param(
    [Parameter(Mandatory = $true)]
    [string]$Endpoint,

    [Parameter(Mandatory = $true)]
    [string]$Prompt,

    [string]$Model = "qwen2.5-coder:7b",

    [int]$NumPredict = 512,
    [double]$Temperature = 0.2,
    [int]$TimeoutSec = 120,

    [string]$EndpointsFile = "$PSScriptRoot\endpoints.json"
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path $EndpointsFile)) {
    @{ error = "endpoints.json not found"; endpoint = $Endpoint } | ConvertTo-Json -Compress | Write-Output
    exit 1
}

$config = Get-Content $EndpointsFile -Raw -Encoding utf8 | ConvertFrom-Json
$ep = $config.endpoints | Where-Object { $_.name -eq $Endpoint }

if (-not $ep) {
    @{ error = "endpoint '$Endpoint' not found in registry"; endpoint = $Endpoint } | ConvertTo-Json -Compress | Write-Output
    exit 1
}

$body = @{
    model   = $Model
    prompt  = $Prompt
    stream  = $false
    options = @{
        num_predict = $NumPredict
        temperature = $Temperature
    }
} | ConvertTo-Json -Depth 5 -Compress

$start = Get-Date
try {
    $resp = Invoke-RestMethod -Uri "$($ep.url)/api/generate" `
                              -Method Post `
                              -ContentType "application/json" `
                              -Body $body `
                              -TimeoutSec $TimeoutSec `
                              -ErrorAction Stop

    $totalMs = [int]((Get-Date) - $start).TotalMilliseconds
    $evalMs = if ($resp.eval_duration) { [int]($resp.eval_duration / 1e6) } else { 0 }
    $tps = if ($evalMs -gt 0) { [math]::Round($resp.eval_count / ($evalMs / 1000.0), 1) } else { 0 }

    @{
        endpoint          = $Endpoint
        endpoint_url      = $ep.url
        model             = $Model
        response          = $resp.response
        eval_count        = $resp.eval_count
        eval_duration_ms  = $evalMs
        tokens_per_sec    = $tps
        total_duration_ms = $totalMs
        prompt_eval_count = $resp.prompt_eval_count
        done_reason       = $resp.done_reason
        error             = $null
    } | ConvertTo-Json -Depth 5 -Compress | Write-Output
}
catch {
    $totalMs = [int]((Get-Date) - $start).TotalMilliseconds
    @{
        endpoint          = $Endpoint
        endpoint_url      = $ep.url
        model             = $Model
        response          = $null
        total_duration_ms = $totalMs
        error             = $_.Exception.Message
    } | ConvertTo-Json -Depth 5 -Compress | Write-Output
    exit 2
}
