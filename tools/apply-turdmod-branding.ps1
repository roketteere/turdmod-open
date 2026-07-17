<#
.SYNOPSIS
    Apply TurdMOD branding to a SCUM dedicated server's ServerSettings.ini.

.DESCRIPTION
    Backs up ServerSettings.ini, updates the native server-driven UI keys
    (ServerName / Description / BannerUrl / Playstyle / WelcomeMessage /
    MessageOfTheDay / Cooldown), and optionally restarts the server.

    Idempotent: re-running keeps applying the same values; the `.bak`
    backup is only created once (if missing) so the first pre-branding
    state is preserved.

    See docs/scum-internals/14-server-admin.md for the keys and what
    the SCUM client renders for each.

.PARAMETER ServerRoot
    SCUM dedicated-server install root. Defaults to the standard Steam
    location.

.PARAMETER Restart
    Pass to restart the server with the BE-off launch flag after applying.

.EXAMPLE
    .\tools\apply-turdmod-branding.ps1 -Restart
#>

param(
    [string]$ServerRoot = "C:\Program Files (x86)\Steam\steamapps\common\SCUM Server",
    [switch]$Restart
)

$ErrorActionPreference = "Stop"
$IniPath = Join-Path $ServerRoot "SCUM\Saved\Config\WindowsServer\ServerSettings.ini"
$BakPath = "$IniPath.turdmod-bak"

if (-not (Test-Path $IniPath)) { throw "ServerSettings.ini not found at $IniPath" }

# ---- branded values ----
# IMPORTANT: SCUM's UE4 FConfigFile parser truncates values at `|` and `//`.
# See docs/scum-internals/14-server-admin.md "INI-parser truncation rules".
# Substitute `|` with ` -- ` and avoid raw `//` (URL is double-quoted to
# survive; if SCUM strips the quotes we fall back to URL-encoding).
$brand = [ordered]@{
    "scum.ServerName"               = "TurdMOD Official -- PVE & PVP Zones"
    "scum.ServerDescription"        = "Flagship server for the TurdMOD modding ecosystem. KillFeed -- MySquad -- VehicleManager -- Guard"
    "scum.ServerBannerUrl"          = '"https://placehold.co/1200x300/0f172a/10b981/png?text=TurdMOD+Official"'
    "scum.ServerPlaystyle"          = "PVP"
    # `\n` literals as line-break attempt; if SCUM renders them as escapes
    # we'll switch to a single-line string with `--` separators.
    "scum.WelcomeMessage"           = "Welcome to TurdMOD Official.\nHouse Rules: be respectful, no cheating (Guard anti-cheat is live), PVE zones safe, PVP zones anything goes.\nCommands: /balance -- /market list -- /wiki -- /report\nDiscord: discord.gg/turdmod"
    "scum.MessageOfTheDay"          = "TurdMOD Official: type /balance for your wallet, /market list for trades, /report to flag cheaters. Discord: discord.gg/turdmod"
    "scum.MessageOfTheDayCooldown"  = "15.000000"
}

if (-not (Test-Path $BakPath)) {
    Copy-Item $IniPath $BakPath
    Write-Host "backed up ServerSettings.ini -> $($BakPath | Split-Path -Leaf)" -ForegroundColor Cyan
} else {
    Write-Host "backup already exists; not overwriting" -ForegroundColor DarkGray
}

# Read as ASCII to be safe; SCUM writes the file in plain ASCII.
$lines = Get-Content $IniPath
$applied = @{}
$out = New-Object System.Collections.Generic.List[string]
foreach ($line in $lines) {
    $matched = $false
    foreach ($key in $brand.Keys) {
        if ($line -match "^\s*$([regex]::Escape($key))\s*=") {
            $out.Add("$key=$($brand[$key])")
            $applied[$key] = $true
            $matched = $true
            break
        }
    }
    if (-not $matched) { $out.Add($line) }
}
# Append any keys that weren't already in the file
foreach ($key in $brand.Keys) {
    if (-not $applied.ContainsKey($key)) {
        $out.Add("$key=$($brand[$key])")
    }
}
Set-Content -Path $IniPath -Value $out -Encoding ASCII

Write-Host "applied TurdMOD branding to ServerSettings.ini:" -ForegroundColor Green
foreach ($key in $brand.Keys) {
    $val = $brand[$key]
    if ($val.Length -gt 80) { $val = $val.Substring(0,77) + "..." }
    Write-Host "  $key = $val"
}

if ($Restart) {
    Write-Host ""
    Write-Host "restarting server (BE off) ..." -ForegroundColor Cyan
    & (Join-Path $PSScriptRoot "scum-server-toggle.ps1") off -ServerRoot $ServerRoot
}
