# Reliable engine build — bridge (C++/UE4SS) + loader (Rust) + service (Rust).
#
# WHY THIS EXISTS: on 2026-06-30 the SCUM update patch turned into hours of live
# compiler debugging because vcvars64.bat fails in non-interactive shells — it
# expands %ProgramFiles(x86)% (parens in the name don't survive shell layers) into
# a broken vswhere path ("vswhere not recognized" -> "\Microsoft was unexpected"),
# so INCLUDE/LIB never get set and cl.exe can't find <memory>. This script does NOT
# use vcvars. It locates VS via vswhere (full quoted path), auto-detects the newest
# MSVC toolset + Windows SDK, and sets INCLUDE/LIB/PATH explicitly — the exact env
# that finally built the bridge. Loader/service use plain `cargo` (no C++ env needed).
#
# Usage:  .\scripts\build-engine.ps1            # builds all three
#         .\scripts\build-engine.ps1 -Bridge    # just the bridge (or -Loader / -Service)
# Exit code is non-zero if any requested artifact fails to build (for the orchestrator).
param(
    [switch]$Bridge,
    [switch]$Loader,
    [switch]$Service
)
# NOTE: 'Continue', NOT 'Stop'. cargo/cmake write warnings + "Compiling..." progress
# to STDERR, and under 'Stop' PowerShell 5.1 wraps each stderr line in a terminating
# NativeCommandError even on exit 0. We judge success by $LASTEXITCODE + artifact
# presence, and use -ErrorAction Stop on the specific cmdlets that must fail loudly.
$ErrorActionPreference = 'Continue'
if (-not ($Bridge -or $Loader -or $Service)) { $Bridge = $Loader = $Service = $true }

$Repo  = 'C:\Development\Claude\turdmod'
$Ue4ss = 'C:\Development\RE-UE4SS'
$fail  = @()

function Info($m) { Write-Host "[build-engine] $m" -ForegroundColor Cyan }
function Ok($m)   { Write-Host "[build-engine] OK  $m" -ForegroundColor Green }
function Err($m)  { Write-Host "[build-engine] ERR $m" -ForegroundColor Red }

# --- Locate the C++ toolchain WITHOUT vcvars ---
# @inv: INCLUDE/LIB MUST match the exact cl.exe the cmake build is pinned to (in its
# CMakeCache), not merely the newest VS install — vswhere -latest can pick a different
# install (e.g. full VS 14.51) than the one the bridge was configured against (BuildTools
# 14.44), and mismatched headers fail the compile. So we derive MSVC from the cache.
function Get-MsvcEnv {
    $pf86 = ${env:ProgramFiles(x86)}; if (-not $pf86) { $pf86 = 'C:\Program Files (x86)' }

    # 1) MSVC: prefer the compiler the cmake cache is already configured with.
    $msvc = $null
    $cache = Join-Path $Ue4ss 'build\CMakeCache.txt'
    if (Test-Path $cache) {
        $m = Select-String -Path $cache -Pattern 'CMAKE_CXX_COMPILER:\w+=(.+cl\.exe)' | Select-Object -First 1
        if ($m) {
            $cl = ($m.Matches[0].Groups[1].Value.Trim()) -replace '/', '\'
            # ...\MSVC\<ver>\bin\Hostx64\x64\cl.exe  -> up 4 dirs = ...\MSVC\<ver>
            $msvc = Split-Path (Split-Path (Split-Path (Split-Path $cl)))
        }
    }
    # Fallback: vswhere -> newest MSVC toolset.
    if (-not $msvc -or -not (Test-Path $msvc)) {
        $vswhere = Join-Path $pf86 'Microsoft Visual Studio\Installer\vswhere.exe'
        if (-not (Test-Path $vswhere)) { throw "vswhere not found at $vswhere" }
        $vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        if (-not $vs) { throw "no VS install with C++ x64 tools found" }
        $msvc = (Get-ChildItem (Join-Path $vs 'VC\Tools\MSVC') -Directory | Sort-Object Name -Descending | Select-Object -First 1).FullName
    }
    if (-not $msvc -or -not (Test-Path $msvc)) { throw "could not resolve MSVC toolset" }

    # 2) Windows SDK: newest that is COMPLETE (both Include\<v>\ucrt AND Lib\<v>\ucrt\x64) —
    # filters out incomplete/insider SDKs (e.g. 10.0.28000) that break the link.
    $sdkRoot = Join-Path $pf86 'Windows Kits\10'
    $sdkv = (Get-ChildItem (Join-Path $sdkRoot 'Include') -Directory |
        Where-Object {
            (Test-Path (Join-Path $_.FullName 'ucrt')) -and
            (Test-Path (Join-Path $sdkRoot "Lib\$($_.Name)\ucrt\x64"))
        } | Sort-Object Name -Descending | Select-Object -First 1).Name
    if (-not $sdkv) { throw "no complete Windows SDK (Include+Lib ucrt) under $sdkRoot" }

    Info "MSVC $(Split-Path $msvc -Leaf) | Windows SDK $sdkv"
    [pscustomobject]@{ Msvc = $msvc; SdkRoot = $sdkRoot; Sdkv = $sdkv }
}

function Set-CppEnv($e) {
    $inc = @(
        (Join-Path $e.Msvc 'include'),
        (Join-Path $e.SdkRoot "Include\$($e.Sdkv)\ucrt"),
        (Join-Path $e.SdkRoot "Include\$($e.Sdkv)\shared"),
        (Join-Path $e.SdkRoot "Include\$($e.Sdkv)\um"),
        (Join-Path $e.SdkRoot "Include\$($e.Sdkv)\winrt"),
        (Join-Path $e.SdkRoot "Include\$($e.Sdkv)\cppwinrt")
    )
    $lib = @(
        (Join-Path $e.Msvc 'lib\x64'),
        (Join-Path $e.SdkRoot "Lib\$($e.Sdkv)\ucrt\x64"),
        (Join-Path $e.SdkRoot "Lib\$($e.Sdkv)\um\x64")
    )
    $env:INCLUDE = ($inc -join ';')
    $env:LIB     = ($lib -join ';')
    $env:PATH    = (Join-Path $e.Msvc 'bin\Hostx64\x64') + ';' + $env:PATH
    # Ensure cmake is reachable (system install or VS-bundled).
    if (-not (Get-Command cmake -ErrorAction SilentlyContinue)) {
        foreach ($c in @('C:\Program Files\CMake\bin', (Join-Path (Split-Path $e.Msvc -Parent) '..\..\..\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin'))) {
            if (Test-Path (Join-Path $c 'cmake.exe')) { $env:PATH = "$c;$env:PATH"; break }
        }
    }
}

# --- Bridge (C++ / UE4SS cmake) ---
if ($Bridge) {
    try {
        Info 'building bridge (TurdMODEngineBridge)...'
        Set-CppEnv (Get-MsvcEnv)
        # Sync canonical source into the wired-in copy (required — CLAUDE.md).
        $dllmain = Join-Path $Ue4ss 'cppmods\TurdMODEngineBridge\src\dllmain.cpp'
        Copy-Item (Join-Path $Repo 'apps\turdmod-engine-bridge\src\TurdMODEngineBridge.cpp') `
                  $dllmain -Force -ErrorAction Stop
        # Copy-Item preserves the source mtime (Jun-19) — touch it so cmake/ninja
        # actually rebuilds instead of no-op'ing against a newer .obj.
        (Get-Item $dllmain).LastWriteTime = Get-Date
        Push-Location $Ue4ss
        try {
            cmake --build build --target TurdMODEngineBridge --config Game__Shipping__Win64
            if ($LASTEXITCODE -ne 0) { throw "cmake exit $LASTEXITCODE" }
        } finally { Pop-Location }
        $dll = Join-Path $Ue4ss 'build\Game__Shipping__Win64\bin\TurdMODEngineBridge.dll'
        if (-not (Test-Path $dll)) { throw "artifact missing: $dll" }
        if ((Get-Item $dll).LastWriteTime -lt (Get-Date).AddMinutes(-5)) { throw "artifact not fresh (build no-op?): $dll" }
        $bt = (Get-Item $dll).LastWriteTime.ToString('HH:mm:ss')
        Ok "bridge -> $dll ($bt)"
    } catch { Err "bridge: $_"; $fail += 'bridge' }
}

# --- Loader + Service (Rust / cargo — no C++ env needed) ---
function Build-Cargo($name, $dir, $artifact) {
    try {
        Info "building $name..."
        Push-Location (Join-Path $Repo $dir)
        try {
            cargo build --release
            if ($LASTEXITCODE -ne 0) { throw "cargo exit $LASTEXITCODE" }
        } finally { Pop-Location }
        $a = Join-Path $Repo $artifact
        if (-not (Test-Path $a)) { throw "artifact missing: $a" }
        Ok "$name -> $a"
    } catch { Err "${name}: $_"; $script:fail += $name }
}
if ($Loader)  { Build-Cargo 'loader'  'apps\turdmod-server-loader' 'apps\turdmod-server-loader\target\release\turdmod_server_loader.dll' }
if ($Service) { Build-Cargo 'service' 'apps\turdmod-service'        'apps\turdmod-service\target\release\turdmod-service.exe' }

Write-Host ''
if ($fail.Count -gt 0) { Err "FAILED: $($fail -join ', ')"; exit 1 }
Ok 'all requested artifacts built'
exit 0
