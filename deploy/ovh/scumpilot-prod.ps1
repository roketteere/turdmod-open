# TurdMOD scumpilot production supervisor — runs as SYSTEM via scheduled task.
# Keeps companion (real SCUM logs -> SSE) + runner (DeepSeek god-admin) alive.
# Reads secrets/config from the machine environment (set once by go-live.ps1).
$ErrorActionPreference='Continue'
$cb='C:\TurdMOD\companion-bundle'; $sb='C:\TurdMOD\scumpilot-bundle'
$reallogs='C:\SCUMServer\SCUM\Saved\SaveFiles\Logs'
$rd= Join-Path $sb 'runs\prod'; $logdir= Join-Path $sb 'logs'
New-Item -ItemType Directory -Force $rd,(Join-Path $sb 'memory'),$logdir | Out-Null
function M([string]$n){ [Environment]::GetEnvironmentVariable($n,'Machine') }
$env:LOCALAPPDATA='C:\Windows\System32\config\systemprofile\AppData\Local'
$env:TURDMOD_COMPANION_PORT='53990'
$env:DEEPSEEK_API_KEY=M 'DEEPSEEK_API_KEY'
$env:GOD_ADMIN_ENABLED='true'; $env:STORYTELLER_ENABLED='false'; $env:LLM_PROVIDER='deepseek'
$env:ENGINE_BRIDGE_ENABLED='true'
$env:ENGINE_BRIDGE_DISCOVERY_PATH='C:\Windows\System32\config\systemprofile\AppData\Local\TurdMOD\engine\pipe.txt'
$env:ENGINE_BRIDGE_CONNECT_RETRIES='30'; $env:REFRESH_ADMIN_CATALOG='true'
$env:OWNER_STEAM_ID='YOUR_STEAM_ID_1'; $env:OWNER_DISPLAY_NAME='YOUR_OWNER_NAME'
$env:RUN_DIR=$rd; $env:MEMORY_DIR=(Join-Path $sb 'memory'); $env:RUNNER_TICK_HZ='0.1'
# HTTP control (so the web manager / service proxy can drive the brain) + consent->Discord.
$env:SCUMPILOT_HTTP_PORT=(M 'SCUMPILOT_HTTP_PORT'); $env:SCUMPILOT_HTTP_TOKEN=(M 'SCUMPILOT_HTTP_TOKEN')
$env:DISCORD_CONSENT_WEBHOOK=(M 'DISCORD_CONSENT_WEBHOOK')
$comp=$null; $run=$null
while($true){
  if($null -eq $comp -or $comp.HasExited){
    $comp = Start-Process node -ArgumentList (Join-Path $cb 'companion.mjs'),'--logs-dir',$reallogs -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $logdir 'companion.out') -RedirectStandardError (Join-Path $logdir 'companion.err')
    Start-Sleep 8
  }
  if($null -eq $run -or $run.HasExited){
    $run = Start-Process node -ArgumentList (Join-Path $sb 'scumpilot-runner.mjs') -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $logdir 'runner.out') -RedirectStandardError (Join-Path $logdir 'runner.err')
  }
  Start-Sleep 20
}
