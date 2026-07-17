# Start GameServer.exe with UE4SS injection (local testing)
# Creates process suspended, injects UE4SS.dll, resumes.
param(
    [string]$ServerDir = "C:\Program Files (x86)\Steam\steamapps\common\SCUM Server",
    [string]$UE4SSDLL = "C:\Program Files (x86)\Steam\steamapps\common\SCUM Server\SCUM\Binaries\Win64\UE4SS\UE4SS.dll",
    [int]$GamePort = 7042,
    [int]$QueryPort = 7044
)

$ErrorActionPreference = 'Stop'
$serverExe = "$ServerDir\SCUM\Binaries\Win64\GameServer.exe"

if (-not (Test-Path $serverExe)) { throw "GameServer.exe not found: $serverExe" }
if (-not (Test-Path $UE4SSDLL)) { throw "UE4SS.dll not found: $UE4SSDLL" }

Write-Host "[start] GameServer.exe: $serverExe" -ForegroundColor Cyan
Write-Host "[start] UE4SS.dll: $UE4SSDLL" -ForegroundColor Cyan

Add-Type @"
using System;
using System.Runtime.InteropServices;

public class Injector {
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern IntPtr OpenProcess(uint access, bool inherit, int pid);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern IntPtr VirtualAllocEx(IntPtr proc, IntPtr addr, uint size, uint type, uint protect);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool WriteProcessMemory(IntPtr proc, IntPtr addr, byte[] buf, uint size, out IntPtr written);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern IntPtr GetModuleHandleA(string name);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern IntPtr GetProcAddress(IntPtr mod, string proc);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern IntPtr CreateRemoteThread(IntPtr proc, IntPtr attr, uint stackSize, IntPtr startAddr, IntPtr param, uint flags, out IntPtr threadId);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern uint WaitForSingleObject(IntPtr handle, uint ms);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool CloseHandle(IntPtr handle);

    public const uint PROCESS_ALL_ACCESS = 0x1F0FFF;
    public const uint MEM_COMMIT = 0x1000;
    public const uint MEM_RESERVE = 0x2000;
    public const uint PAGE_READWRITE = 0x04;
}
"@

# Start GameServer.exe with command line args
$args = "-log -port=$GamePort -QueryPort=$QueryPort"
Write-Host "[start] Starting GameServer.exe with: $args" -ForegroundColor Yellow

$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = $serverExe
$psi.Arguments = $args
$psi.WorkingDirectory = "$ServerDir\SCUM\Binaries\Win64"
$psi.UseShellExecute = $false

$proc = [System.Diagnostics.Process]::Start($psi)
Write-Host "[start] SCUMServer PID: $($proc.Id)" -ForegroundColor Green

# Wait for the process to initialize
Write-Host "[start] Waiting 5s for process init..." -ForegroundColor Yellow
Start-Sleep -Seconds 5

# Inject UE4SS.dll
$dllPath = [System.Text.Encoding]::ASCII.GetBytes($UE4SSDLL + "`0")
$hProc = [Injector]::OpenProcess([Injector]::PROCESS_ALL_ACCESS, $false, $proc.Id)
if ($hProc -eq [IntPtr]::Zero) { throw "OpenProcess failed: $([System.Runtime.InteropServices.Marshal]::GetLastWin32Error())" }

$remoteMem = [Injector]::VirtualAllocEx($hProc, [IntPtr]::Zero, [uint32]$dllPath.Length, [Injector]::MEM_COMMIT -bor [Injector]::MEM_RESERVE, [Injector]::PAGE_READWRITE)
if ($remoteMem -eq [IntPtr]::Zero) { throw "VirtualAllocEx failed" }

$written = [IntPtr]::Zero
[Injector]::WriteProcessMemory($hProc, $remoteMem, $dllPath, [uint32]$dllPath.Length, [ref]$written) | Out-Null

$kernel32 = [Injector]::GetModuleHandleA("kernel32.dll")
$loadLib = [Injector]::GetProcAddress($kernel32, "LoadLibraryA")

$threadId = [IntPtr]::Zero
$hThread = [Injector]::CreateRemoteThread($hProc, [IntPtr]::Zero, 0, $loadLib, $remoteMem, 0, [ref]$threadId)
if ($hThread -eq [IntPtr]::Zero) { throw "CreateRemoteThread failed: $([System.Runtime.InteropServices.Marshal]::GetLastWin32Error())" }

[Injector]::WaitForSingleObject($hThread, 10000) | Out-Null
[Injector]::CloseHandle($hThread) | Out-Null
[Injector]::CloseHandle($hProc) | Out-Null

Write-Host "[start] UE4SS.dll injected into PID $($proc.Id)" -ForegroundColor Green
Write-Host "[start] Server running. Check UE4SS\UE4SS.log for bridge status." -ForegroundColor Cyan
