# Verify local + OVH run the EXACT same engine build, BattlEye OFF on both.
# Compares SHA256 of the 4 artifacts and the running service build-SHA + BE flag.
# @inv: no in-binary version stamp on the DLLs — SHA256 is the source of truth there;
# the service additionally reports GIT_SHA via /health (added with build.rs).

param(
    [string]$RemoteHost = "YOUR_SERVER_IP",
    [string]$User = "admin",
    [string]$LocalServerDir = "C:\Program Files (x86)\Steam\steamapps\common\SCUM Server\SCUM\Binaries\Win64",
    [int]$LocalPort = 9090
)

$ErrorActionPreference = 'Continue'
$key = "$env:USERPROFILE\.ssh\id_ed25519"
$ssh = @("-i", $key, "-o", "ConnectTimeout=15", "${User}@${RemoteHost}")

function RHash($path) { (ssh @ssh "powershell -NoProfile -Command `"(Get-FileHash '$path' -Algorithm SHA256).Hash`"" 2>$null | Out-String).Trim() }
function LHash($path) { if (Test-Path $path) { (Get-FileHash $path -Algorithm SHA256).Hash } else { "MISSING" } }

# artifact: local path | remote path
$svcLocal = "C:\Development\Claude\turdmod\apps\turdmod-service\target\release\turdmod-service.exe"
$artifacts = @(
    @{ name = "bridge main.dll"; l = "$LocalServerDir\UE4SS\Mods\TurdMODEngineBridge\dlls\main.dll"; r = "C:\SCUMServer\SCUM\Binaries\Win64\UE4SS\Mods\TurdMODEngineBridge\dlls\main.dll" },
    @{ name = "loader.dll";      l = "$LocalServerDir\turdmod_server_loader.dll";                     r = "C:\SCUMServer\SCUM\Binaries\Win64\turdmod_server_loader.dll" },
    @{ name = "UE4SS.dll";       l = "$LocalServerDir\UE4SS\UE4SS.dll";                               r = "C:\SCUMServer\SCUM\Binaries\Win64\UE4SS\UE4SS.dll" },
    @{ name = "turdmod-service"; l = "$svcLocal";                                                      r = "C:\TurdMOD\turdmod-service.exe" }
)

Write-Host "=== Engine build parity: local vs $RemoteHost ===" -ForegroundColor Magenta
$allMatch = $true
foreach ($a in $artifacts) {
    $lh = LHash $a.l; $rh = RHash $a.r
    $ok = ($lh -eq $rh) -and ($lh -ne "MISSING") -and ($rh)
    if (-not $ok) { $allMatch = $false }
    $tag = if ($ok) { "MATCH " } else { "DIFFER" }
    $col = if ($ok) { "Green" } else { "Red" }
    $lhS = if ($lh -and $lh.Length -ge 12) { $lh.Substring(0,12) } else { $lh }
    $rhS = if ($rh -and $rh.Length -ge 12) { $rh.Substring(0,12) } else { "??" }
    Write-Host ("  [{0}] {1,-18} L={2}  R={3}" -f $tag, $a.name, $lhS, $rhS) -ForegroundColor $col
}

Write-Host "`n--- running service build SHA + BattlEye flag ---" -ForegroundColor Cyan
try { $loc = Invoke-RestMethod "http://localhost:${LocalPort}/health" -TimeoutSec 4; Write-Host "  local  build=$($loc.build)" } catch { Write-Host "  local  /health unreachable" -ForegroundColor Yellow }
try { $rem = Invoke-RestMethod "http://${RemoteHost}:9090/health" -TimeoutSec 5; Write-Host "  OVH    build=$($rem.build)" } catch { Write-Host "  OVH    /health unreachable" -ForegroundColor Yellow }

# BattlEye must be OFF on both: check the running SCUMServer command lines
function BEoff($where, $cmd) { if ($cmd -match "-NoBattlEye") { Write-Host "  $where BattlEye: OFF (-NoBattlEye)" -ForegroundColor Green } else { Write-Host "  $where BattlEye: NOT confirmed off!" -ForegroundColor Red } }
$locCmd = (Get-CimInstance Win32_Process -Filter "name='SCUMServer.exe'" -ErrorAction SilentlyContinue).CommandLine
BEoff "local" $locCmd
# wmic is more robust than nested-quoted Get-CimInstance over ssh
$remCmd = (ssh @ssh 'wmic process where "name=''SCUMServer.exe''" get commandline' | Out-String)
BEoff "OVH  " $remCmd

Write-Host ""
if ($allMatch) {
    Write-Host "PARITY OK -- all 4 artifacts identical." -ForegroundColor Green
} else {
    Write-Host "PARITY MISMATCH -- redeploy the DIFFER'd artifacts (deploy-service.ps1 / deploy-engine.ps1)." -ForegroundColor Red
}
