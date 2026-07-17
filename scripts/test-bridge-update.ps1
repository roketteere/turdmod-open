#Requires -RunAsAdministrator
<#
.SYNOPSIS
    Test the updated TurdMOD bridge against SCUM build 23396794 (Into the Wild).
    Run this AS ADMIN from PowerShell.
.DESCRIPTION
    1. Kills any running SCUMServer
    2. Cleans up the fake dwmapi.dll proxy
    3. Starts SCUMServer SUSPENDED
    4. Injects UE4SS.dll
    5. Resumes the process
    6. Monitors UE4SS.log for bridge status
    7. Tests the pipe with a getImageBase RPC
#>

$ErrorActionPreference = 'Continue'
Write-Host "=== TurdMOD Bridge Test (build 23396794) ===" -ForegroundColor Magenta

# Config
$binDir = "C:\Program Files (x86)\Steam\steamapps\common\SCUM Server\SCUM\Binaries\Win64"
$serverExe = "$binDir\GameServer.exe"
$ue4ssDll = "$binDir\UE4SS\UE4SS.dll"
$ue4ssLog = "$binDir\UE4SS\UE4SS.log"

# Step 0: Kill any existing server
Write-Host "[0] Cleaning up..." -ForegroundColor Yellow
$null = cmd /c "taskkill /F /IM GameServer.exe 2>nul"
Start-Sleep -Seconds 2

# Remove fake dwmapi.dll if present
$fakeDwmapi = "$binDir\dwmapi.dll"
if (Test-Path $fakeDwmapi) {
    Remove-Item $fakeDwmapi -Force -ErrorAction SilentlyContinue
    Write-Host "  Removed fake dwmapi.dll"
}

# Clear old log
if (Test-Path $ue4ssLog) { Remove-Item $ue4ssLog -Force }

# Step 1: Start server SUSPENDED
Write-Host "[1] Starting GameServer.exe SUSPENDED..." -ForegroundColor Cyan

Add-Type @"
using System;
using System.Runtime.InteropServices;
public class SuspInj {
    [StructLayout(LayoutKind.Sequential)]
    public struct STARTUPINFO {
        public int cb; public IntPtr lpReserved, lpDesktop, lpTitle;
        public int dwX, dwY, dwXSize, dwYSize, dwXCountChars, dwYCountChars, dwFillAttribute, dwFlags;
        public short wShowWindow, cbReserved2; public IntPtr lpReserved2, hStdInput, hStdOutput, hStdError;
    }
    [StructLayout(LayoutKind.Sequential)]
    public struct PROCESS_INFORMATION { public IntPtr hProcess, hThread; public int dwProcessId, dwThreadId; }
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    public static extern bool CreateProcessW(string app, string cmd, IntPtr pa, IntPtr ta, bool ih, uint flags, IntPtr env, string dir, ref STARTUPINFO si, out PROCESS_INFORMATION pi);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern IntPtr VirtualAllocEx(IntPtr p, IntPtr a, uint s, uint t, uint pr);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool WriteProcessMemory(IntPtr p, IntPtr a, byte[] b, uint s, out IntPtr w);
    [DllImport("kernel32.dll")] public static extern IntPtr GetModuleHandleA(string n);
    [DllImport("kernel32.dll")] public static extern IntPtr GetProcAddress(IntPtr m, string p);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern IntPtr CreateRemoteThread(IntPtr p, IntPtr a, uint ss, IntPtr sa, IntPtr pa, uint f, out IntPtr t);
    [DllImport("kernel32.dll")] public static extern uint WaitForSingleObject(IntPtr h, uint ms);
    [DllImport("kernel32.dll")] public static extern uint ResumeThread(IntPtr h);
    [DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr h);
}
"@

$si = New-Object SuspInj+STARTUPINFO
$si.cb = [System.Runtime.InteropServices.Marshal]::SizeOf($si)
$pi = New-Object SuspInj+PROCESS_INFORMATION
$cmd = "`"$serverExe`" -log -port=7042 -QueryPort=7044"

$ok = [SuspInj]::CreateProcessW($serverExe, $cmd, [IntPtr]::Zero, [IntPtr]::Zero, $false, 4, [IntPtr]::Zero, $binDir, [ref]$si, [ref]$pi)
if (-not $ok) { throw "CreateProcess failed: $([System.Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
Write-Host "  PID: $($pi.dwProcessId)" -ForegroundColor Green

# Step 2: Inject BOTH DLLs (loader first, then UE4SS)
Write-Host "[2] Injecting turdmod_server_loader.dll + UE4SS.dll..." -ForegroundColor Cyan
$k32 = [SuspInj]::GetModuleHandleA("kernel32.dll")
$loadLib = [SuspInj]::GetProcAddress($k32, "LoadLibraryA")

# Inject loader DLL first (bridge expects it to be present)
$loaderDll = "$binDir\turdmod_server_loader.dll"
if (Test-Path $loaderDll) {
    $loaderBytes = [System.Text.Encoding]::ASCII.GetBytes($loaderDll + [char]0)
    $remoteMem1 = [SuspInj]::VirtualAllocEx($pi.hProcess, [IntPtr]::Zero, [uint32]$loaderBytes.Length, 0x3000, 0x04)
    $w = [IntPtr]::Zero
    [SuspInj]::WriteProcessMemory($pi.hProcess, $remoteMem1, $loaderBytes, [uint32]$loaderBytes.Length, [ref]$w) | Out-Null
    $tid = [IntPtr]::Zero
    $hThread = [SuspInj]::CreateRemoteThread($pi.hProcess, [IntPtr]::Zero, 0, $loadLib, $remoteMem1, 0, [ref]$tid)
    [SuspInj]::WaitForSingleObject($hThread, 15000) | Out-Null
    [SuspInj]::CloseHandle($hThread) | Out-Null
    Write-Host "  turdmod_server_loader.dll injected" -ForegroundColor Green
} else {
    Write-Host "  WARNING: turdmod_server_loader.dll not found at $loaderDll" -ForegroundColor Yellow
}

# Inject UE4SS.dll (loads bridge mod)
$dllBytes = [System.Text.Encoding]::ASCII.GetBytes($ue4ssDll + [char]0)
$remoteMem2 = [SuspInj]::VirtualAllocEx($pi.hProcess, [IntPtr]::Zero, [uint32]$dllBytes.Length, 0x3000, 0x04)
$w = [IntPtr]::Zero
[SuspInj]::WriteProcessMemory($pi.hProcess, $remoteMem2, $dllBytes, [uint32]$dllBytes.Length, [ref]$w) | Out-Null
$tid = [IntPtr]::Zero
$hThread = [SuspInj]::CreateRemoteThread($pi.hProcess, [IntPtr]::Zero, 0, $loadLib, $remoteMem2, 0, [ref]$tid)
[SuspInj]::WaitForSingleObject($hThread, 15000) | Out-Null
[SuspInj]::CloseHandle($hThread) | Out-Null
Write-Host "  UE4SS.dll injected" -ForegroundColor Green

# Step 2b: Write pipe discovery file for Manager
$discoDir = "$env:LOCALAPPDATA\TurdMOD\engine"
if (-not (Test-Path $discoDir)) { New-Item -ItemType Directory -Path $discoDir -Force | Out-Null }
$pipeName = "\\.\pipe\turdmod-engine-$($pi.dwProcessId)"
Set-Content "$discoDir\pipe.txt" $pipeName -Encoding utf8 -NoNewline
Write-Host "  Discovery file: $pipeName" -ForegroundColor Green

# Step 3: Resume
Write-Host "[3] Resuming server..." -ForegroundColor Cyan
[SuspInj]::ResumeThread($pi.hThread) | Out-Null
[SuspInj]::CloseHandle($pi.hThread) | Out-Null
[SuspInj]::CloseHandle($pi.hProcess) | Out-Null

# Step 4: Monitor
Write-Host "[4] Monitoring (30s)..." -ForegroundColor Yellow
for ($i = 0; $i -lt 6; $i++) {
    Start-Sleep -Seconds 5
    $proc = Get-Process -Id $pi.dwProcessId -ErrorAction SilentlyContinue
    if ($proc) {
        Write-Host "  T+$((($i+1)*5))s: RAM $([math]::Round($proc.WorkingSet64/1MB))MB | CPU $([math]::Round($proc.CPU))s"
    } else {
        Write-Host "  SERVER CRASHED at T+$((($i+1)*5))s" -ForegroundColor Red
        break
    }
}

# Step 5: Check results
Write-Host "`n[5] Results:" -ForegroundColor Cyan
if (Test-Path $ue4ssLog) {
    Write-Host "--- UE4SS.log (bridge lines) ---" -ForegroundColor DarkGray
    Get-Content $ue4ssLog | Where-Object { $_ -match "BYPASS|Bridge|TurdMOD|pipe|handler|APPLIED|SKIP|FAIL|error" }
    Write-Host "--- end ---" -ForegroundColor DarkGray
} else {
    Write-Host "  No UE4SS.log!" -ForegroundColor Red
}

# Step 6: Pipe test
Write-Host "`n[6] Pipe test:" -ForegroundColor Cyan
try {
    $dynPipeName = "turdmod-engine-$($pi.dwProcessId)"
    $client = New-Object System.IO.Pipes.NamedPipeClientStream(".", $dynPipeName, [System.IO.Pipes.PipeDirection]::InOut)
    $client.Connect(5000)
    $msg = '{"handler":"getImageBase"}' + "`n"
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($msg)
    $client.Write($bytes, 0, $bytes.Length); $client.Flush()
    $buf = New-Object byte[] 4096
    $read = $client.Read($buf, 0, 4096)
    $resp = [System.Text.Encoding]::UTF8.GetString($buf, 0, $read)
    Write-Host "  PIPE OK: $resp" -ForegroundColor Green
    $client.Close()
} catch {
    Write-Host "  PIPE FAILED: $($_.Exception.Message)" -ForegroundColor Red
}

Write-Host "`n=== Test complete ===" -ForegroundColor Magenta
