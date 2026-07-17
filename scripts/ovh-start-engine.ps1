#Requires -RunAsAdministrator
<#
.SYNOPSIS
    Start SCUMServer with TurdMOD engine on OVH production.
    Run on the OVH server via RDP (requires admin).
.DESCRIPTION
    1. Kills any running SCUMServer
    2. Starts SCUMServer SUSPENDED
    3. Injects turdmod_server_loader.dll + UE4SS.dll
    4. Resumes the process
    5. Writes pipe discovery file
#>

$ErrorActionPreference = 'Continue'
Write-Host "=== TurdMOD Engine Start (OVH Production) ===" -ForegroundColor Magenta

$binDir = "C:\SCUMServer\SCUM\Binaries\Win64"
$serverExe = "$binDir\SCUMServer.exe"
$ue4ssDll = "$binDir\UE4SS\UE4SS.dll"
$loaderDll = "$binDir\turdmod_server_loader.dll"

# Verify files exist
foreach ($f in @($serverExe, $ue4ssDll, $loaderDll)) {
    if (-not (Test-Path $f)) { Write-Host "MISSING: $f" -ForegroundColor Red; exit 1 }
}
Write-Host "[OK] All DLLs present" -ForegroundColor Green

# Kill existing
$null = cmd /c "taskkill /F /IM SCUMServer.exe 2>nul"
Start-Sleep -Seconds 3

Add-Type @"
using System;
using System.Runtime.InteropServices;
public class EngInj {
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

# Create SUSPENDED
Write-Host "[1] Starting SCUMServer SUSPENDED..." -ForegroundColor Cyan
$si = New-Object EngInj+STARTUPINFO
$si.cb = [System.Runtime.InteropServices.Marshal]::SizeOf($si)
$pi = New-Object EngInj+PROCESS_INFORMATION
$cmd = "`"$serverExe`" -log -port=7042 -QueryPort=7044 -NoBattlEye"

$ok = [EngInj]::CreateProcessW($serverExe, $cmd, [IntPtr]::Zero, [IntPtr]::Zero, $false, 4, [IntPtr]::Zero, $binDir, [ref]$si, [ref]$pi)
if (-not $ok) { Write-Host "CreateProcess FAILED: $([System.Runtime.InteropServices.Marshal]::GetLastWin32Error())" -ForegroundColor Red; exit 1 }
Write-Host "  PID: $($pi.dwProcessId)" -ForegroundColor Green

# Inject function
function Inject-DLL($hProc, $dllPath) {
    $k32 = [EngInj]::GetModuleHandleA("kernel32.dll")
    $loadLib = [EngInj]::GetProcAddress($k32, "LoadLibraryA")
    $dllBytes = [System.Text.Encoding]::ASCII.GetBytes($dllPath + [char]0)
    $remoteMem = [EngInj]::VirtualAllocEx($hProc, [IntPtr]::Zero, [uint32]$dllBytes.Length, 0x3000, 0x04)
    $w = [IntPtr]::Zero
    [EngInj]::WriteProcessMemory($hProc, $remoteMem, $dllBytes, [uint32]$dllBytes.Length, [ref]$w) | Out-Null
    $tid = [IntPtr]::Zero
    $hThread = [EngInj]::CreateRemoteThread($hProc, [IntPtr]::Zero, 0, $loadLib, $remoteMem, 0, [ref]$tid)
    [EngInj]::WaitForSingleObject($hThread, 15000) | Out-Null
    [EngInj]::CloseHandle($hThread) | Out-Null
}

# Inject loader first, then UE4SS
Write-Host "[2] Injecting DLLs..." -ForegroundColor Cyan
Inject-DLL $pi.hProcess $loaderDll
Write-Host "  turdmod_server_loader.dll injected" -ForegroundColor Green
Inject-DLL $pi.hProcess $ue4ssDll
Write-Host "  UE4SS.dll injected" -ForegroundColor Green

# Write discovery file
$discoDir = "$env:LOCALAPPDATA\TurdMOD\engine"
if (-not (Test-Path $discoDir)) { New-Item -ItemType Directory -Path $discoDir -Force | Out-Null }
$pipeName = "\\.\pipe\turdmod-engine-$($pi.dwProcessId)"
Set-Content "$discoDir\pipe.txt" $pipeName -Encoding utf8 -NoNewline
Write-Host "  Pipe discovery: $pipeName" -ForegroundColor Green

# Resume
Write-Host "[3] Resuming..." -ForegroundColor Cyan
[EngInj]::ResumeThread($pi.hThread) | Out-Null
[EngInj]::CloseHandle($pi.hThread) | Out-Null
[EngInj]::CloseHandle($pi.hProcess) | Out-Null

Write-Host "`n=== Server starting with TurdMOD Engine ===" -ForegroundColor Green
Write-Host "  PID: $($pi.dwProcessId)"
Write-Host "  Game: YOUR_SERVER_IP:7042"
Write-Host "  Pipe: $pipeName"
Write-Host "  Bypass: $(if (Test-Path 'C:\TurdMOD\pak_bypass.enabled') { 'ENABLED' } else { 'disabled' })"
Write-Host "`nMonitor: type C:\SCUMServer\SCUM\Binaries\Win64\UE4SS\UE4SS.log"
