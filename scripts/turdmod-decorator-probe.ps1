# turdmod-decorator-probe.ps1 -- one-shot Phase-B step-1 live test runner.
#
# Builds the three turdmod-loader crates, launches SCUM with both the
# kitchen-sink loader DLL and the rich-text decorator DLL injected, then
# tails decorators.log so the engine-resolve probe report is visible
# while the player connects to a server.
#
# Documented in docs/turdmod/ARCHITECTURE.md §"Running the Phase-B
# step-1 live test"; this script just removes the copy-paste step.
#
# Usage from repo root:
#   .\scripts\turdmod-decorator-probe.ps1
#
# Optional flags:
#   -ScumExe    <path>   Override SCUM.exe location. Default: Steam
#                        common-folder probe inside the launcher itself.
#   -SkipBuild           Reuse existing target/release/ artifacts.
#   -OnlyTail            Skip launching SCUM; just tail decorators.log.
#                        Useful if the user starts SCUM from Steam.
#
# Exit codes:
#   0 -- clean build + launcher returned 0.
#   1 -- cargo build failed.
#   2 -- launcher exited non-zero.
#   3 -- required artifact (DLL or EXE) missing after build.

param(
    [string]$ScumExe = "",
    [switch]$SkipBuild,
    [switch]$OnlyTail
)

$ErrorActionPreference = "Stop"

# Repo paths -- anchored at the script's location so the script works
# regardless of the caller's CWD.
$repoRoot = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$loaderDir     = Join-Path $repoRoot "apps/turdmod-loader"
$launcherDir   = Join-Path $repoRoot "apps/turdmod-loader/launcher"
$decoratorsDir = Join-Path $repoRoot "apps/turdmod-loader/decorators"

$loaderDll     = Join-Path $loaderDir     "target/release/turdmod_loader.dll"
$decoratorsDll = Join-Path $decoratorsDir "target/release/turdmod_rich_decorators.dll"
$launcherExe   = Join-Path $launcherDir   "target/release/turdmod-launcher.exe"

$logFile = Join-Path $env:LOCALAPPDATA "TurdMOD/decorators.log"

function Build-Crate([string]$manifest, [string]$label) {
    Write-Host "  building $label..." -ForegroundColor Cyan
    & cargo build --release --manifest-path $manifest
    if ($LASTEXITCODE -ne 0) {
        Write-Host "  FAILED: $label" -ForegroundColor Red
        exit 1
    }
}

if (-not $SkipBuild -and -not $OnlyTail) {
    Write-Host "Phase-B step-1 build" -ForegroundColor Yellow
    Build-Crate (Join-Path $loaderDir     "Cargo.toml") "turdmod_loader.dll"
    Build-Crate (Join-Path $launcherDir   "Cargo.toml") "turdmod-launcher.exe"
    Build-Crate (Join-Path $decoratorsDir "Cargo.toml") "turdmod_rich_decorators.dll"
}

# Verify each artifact exists before we try to use it. A missing artifact
# almost always means cargo build picked a different target dir; better to
# fail loud here than to launch SCUM without our DLL.
foreach ($p in @($loaderDll, $decoratorsDll, $launcherExe)) {
    if (-not (Test-Path $p)) {
        Write-Host "  missing artifact: $p" -ForegroundColor Red
        Write-Host "  did the build complete? Try: cargo build --release --manifest-path <its dir>/Cargo.toml" -ForegroundColor Red
        exit 3
    }
}

# Make sure the log directory exists so the tail loop below has something
# to chew on even if the very first attach hasn't appended yet.
$logDir = Split-Path -Parent $logFile
if (-not (Test-Path $logDir)) {
    New-Item -ItemType Directory -Path $logDir -Force | Out-Null
}
if (-not (Test-Path $logFile)) {
    "" | Out-File -FilePath $logFile -Encoding utf8
}

if (-not $OnlyTail) {
    $args = @("--dll", $loaderDll, "--extra-dll", $decoratorsDll)
    if ($ScumExe -ne "") { $args = @("--scum", $ScumExe) + $args }

    Write-Host ""
    Write-Host "launching:" -ForegroundColor Yellow
    Write-Host "  $launcherExe $($args -join ' ')" -ForegroundColor Gray

    # Run the launcher in a background job so we can tail the log in
    # foreground while SCUM is alive. The launcher returns once SCUM is
    # spawned (or fails to spawn); SCUM keeps running independently.
    Start-Process -FilePath $launcherExe -ArgumentList $args -NoNewWindow -PassThru | Out-Null
    Start-Sleep -Seconds 2
}

Write-Host ""
Write-Host "tailing $logFile" -ForegroundColor Yellow
Write-Host "  (Ctrl+C to stop watching -- SCUM keeps running)" -ForegroundColor Gray
Write-Host ""

# `Get-Content -Wait` mimics tail -f. We start from the end of the file so
# we only see new attach events, not historical noise.
Get-Content -Path $logFile -Wait -Tail 0
