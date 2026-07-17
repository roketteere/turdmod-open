# TurdMOD AI auto-admin GO-LIVE - operator runs this ONCE on the OVH box.
# Credential-free: pass Discord secrets as params; the scumpilot HTTP-control
# token is auto-generated if absent. Idempotent. Run from an elevated shell.
param(
  [string]$DiscordFeedWebhook = "",
  [string]$DiscordConsentWebhook = "",
  [switch]$EnableOverlay
)
$ErrorActionPreference='Stop'
function SetM($n,$v){ [Environment]::SetEnvironmentVariable($n,$v,'Machine'); Write-Host ("  set " + $n) }

Write-Host "== 1/4  environment =="
if (-not [Environment]::GetEnvironmentVariable('SCUMPILOT_HTTP_TOKEN','Machine')) {
  SetM 'SCUMPILOT_HTTP_TOKEN' ([Guid]::NewGuid().ToString('N'))
}
SetM 'SCUMPILOT_HTTP_PORT' '54330'
if ($EnableOverlay) { SetM 'OVERLAY_ENABLED' '1' }
if ($DiscordFeedWebhook)    { SetM 'DISCORD_FEED_WEBHOOK' $DiscordFeedWebhook }
if ($DiscordConsentWebhook) { SetM 'DISCORD_CONSENT_WEBHOOK' $DiscordConsentWebhook }
if (-not [Environment]::GetEnvironmentVariable('DEEPSEEK_API_KEY','Machine')) {
  Write-Warning "DEEPSEEK_API_KEY not set on machine - set it with setx, or scumpilot cannot reach its brain."
}

Write-Host "== 2/4  swap turdmod-service.exe (prestige/factions/overlay) =="
$staged='C:\TurdMOD\staging\turdmod-service-new.exe'; $live='C:\TurdMOD\turdmod-service.exe'
if (Test-Path $staged) {
  Stop-Service TurdMODService -ErrorAction SilentlyContinue; Start-Sleep 3
  Copy-Item $live "$live.prev" -Force -ErrorAction SilentlyContinue
  Copy-Item $staged $live -Force
  Start-Service TurdMODService
  Write-Host "  swapped + restarted TurdMODService (rollback: turdmod-service.exe.prev)"
} else { Write-Warning "  staged binary not found at $staged - skipping swap" }

Write-Host "== 3/4  install + start scumpilot 24/7 task =="
schtasks /create /tn "TurdMOD-scumpilot" /tr "powershell -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File C:\TurdMOD\scumpilot-bundle\scumpilot-prod.ps1" /sc onstart /ru SYSTEM /rl HIGHEST /f | Out-Null
schtasks /run /tn "TurdMOD-scumpilot" | Out-Null
Write-Host "  scumpilot task installed + started"

Write-Host "== 4/4  Discord bot (manual, optional) =="
Write-Host "  Kill feed: set DISCORD_FEED_WEBHOOK then run: node C:\TurdMOD\feed-bundle\live-feed.mjs"
Write-Host "  Gateway:   run live-gateway with DISCORD_BOT_TOKEN/CLIENT_ID/GUILD_ID/RELAY_CHANNEL_ID + TURDMOD_ENGINE_RPC_URL/TOKEN"
Write-Host ""
Write-Host "DONE. In-game: type  #pilot pause  to halt the AI. Web manager: Admin then Pilot (AI)."
Write-Host "Rollback service: Stop-Service TurdMODService; copy turdmod-service.exe.prev over turdmod-service.exe; Start-Service TurdMODService."
