<#
.SYNOPSIS
    Cooks a UE 4.27 uproject for a target platform using UE4Editor-Cmd.exe headless.
.DESCRIPTION
    Drives the UE4Editor-Cmd.exe Cook command. Validates paths, streams output,
    enforces timeout, and detects common cook failures via log post-processing.
.PARAMETER UprojectPath
    Full path to the .uproject file.
.PARAMETER OutDir
    Output directory for cooked content. Defaults to <UprojectDir>/Saved/Cooked/<TargetPlatform>.
.PARAMETER TargetPlatform
    Target platform for cooking (default: WindowsServer).
.PARAMETER UEEditorCmd
    Path to UE4Editor-Cmd.exe (default: standard UE 4.27 install location).
.PARAMETER TimeoutMinutes
    Maximum cook duration in minutes (default: 15).
.PARAMETER Iterative
    If set, passes -iterate to the cook command for incremental cooking.
#>

param(
    [Parameter(Mandatory=$true)] [string]$UprojectPath,
    [string]$OutDir,
    [string]$TargetPlatform = "WindowsServer",
    [string]$UEEditorCmd = "C:\Program Files\Epic Games\UE_4.27\Engine\Binaries\Win64\UE4Editor-Cmd.exe",
    [int]$TimeoutMinutes = 15,
    [switch]$Iterative
)

$ScriptName = "cook-uproject.ps1"
$Total = 4

function Step($n, $msg) { Write-Host ""; Write-Host "[$ScriptName] Step $n/${Total}: $msg" -ForegroundColor Cyan }
function Fail($msg)     { Write-Host "[$ScriptName] FAIL: $msg" -ForegroundColor Red; exit 1 }
function Ok($msg)       { Write-Host "[$ScriptName]   ok: $msg" -ForegroundColor Green }

$ErrorActionPreference = "Stop"

# Step 1: Validate inputs
Step 1 "Validating inputs"

if (-not (Test-Path -LiteralPath $UprojectPath -PathType Leaf)) {
    Fail "UprojectPath '$UprojectPath' does not exist or is not a file."
}
if (-not ($UprojectPath -like "*.uproject")) {
    Fail "UprojectPath must end in .uproject."
}

if (-not (Test-Path -LiteralPath $UEEditorCmd -PathType Leaf)) {
    Write-Host "[$ScriptName] UE4Editor-Cmd.exe not found at '$UEEditorCmd'." -ForegroundColor Yellow
    Write-Host "[$ScriptName] Please install Unreal Engine 4.27 via Epic Games Launcher." -ForegroundColor Yellow
    Fail "UE4Editor-Cmd.exe missing."
}

$UprojectDir = Split-Path -Parent -Resolve $UprojectPath
if (-not $OutDir) {
    $OutDir = Join-Path -Path $UprojectDir -ChildPath "Saved\Cooked\$TargetPlatform"
}
if (-not (Test-Path -LiteralPath $OutDir -PathType Container)) {
    New-Item -ItemType Directory -Path $OutDir -Force | Out-Null
    Write-Host "[$ScriptName] Created output directory: $OutDir" -ForegroundColor Gray
}

Ok "Inputs validated."

# Step 2: Build command line
Step 2 "Building cook command line"

$CookArgs = @(
    "`"$UprojectPath`""
    "-run=Cook"
    "-targetplatform=$TargetPlatform"
    "-fileopenlog"
    "-ddc=DerivedDataBackendGraph"
    "-unversioned"
    "-compressed"
)
if ($Iterative) {
    $CookArgs += "-iterate"
}

$Timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
$LogDir = Join-Path -Path $UprojectDir -ChildPath "Saved\Logs"
if (-not (Test-Path -LiteralPath $LogDir -PathType Container)) {
    New-Item -ItemType Directory -Path $LogDir -Force | Out-Null
}
$LogFile = Join-Path -Path $LogDir -ChildPath "Cook-$TargetPlatform-$Timestamp.log"

$StartTime = Get-Date

Write-Host "[$ScriptName] Command: $UEEditorCmd $($CookArgs -join ' ')" -ForegroundColor Gray
Write-Host "[$ScriptName] Log file: $LogFile" -ForegroundColor Gray

# Step 3: Execute cook via Start-Process with redirected output
Step 3 "Executing cook (timeout: ${TimeoutMinutes}m)"

$procInfo = New-Object System.Diagnostics.ProcessStartInfo
$procInfo.FileName = $UEEditorCmd
$procInfo.Arguments = $CookArgs -join ' '
$procInfo.UseShellExecute = $false
$procInfo.RedirectStandardOutput = $true
$procInfo.RedirectStandardError = $true
$procInfo.CreateNoWindow = $true

$proc = New-Object System.Diagnostics.Process
$proc.StartInfo = $procInfo

# Collect output lines for log file and console streaming
$outputLines = New-Object System.Collections.ArrayList

# Register event handlers to stream output in real-time
$outputEvent = Register-ObjectEvent -InputObject $proc -EventName 'OutputDataReceived' -Action {
    $line = $EventArgs.Data
    if ($line -ne $null) {
        Write-Host "[cook]   $line"
        $eventSubscriber = $EventSubscriber
        $eventSubscriber.Action.OutputLines.Add($line) | Out-Null
    }
}
$errorEvent = Register-ObjectEvent -InputObject $proc -EventName 'ErrorDataReceived' -Action {
    $line = $EventArgs.Data
    if ($line -ne $null) {
        Write-Host "[cook]   $line"
        $eventSubscriber = $EventSubscriber
        $eventSubscriber.Action.OutputLines.Add($line) | Out-Null
    }
}

# Attach the output list to the event action's module scope via a workaround
# We'll store lines in a synchronized list accessible from the event
$syncOutput = [System.Collections.ArrayList]::Synchronized($outputLines)
$outputEvent.Action.OutputLines = $syncOutput
$errorEvent.Action.OutputLines = $syncOutput

$proc.Start() | Out-Null
$proc.BeginOutputReadLine()
$proc.BeginErrorReadLine()

$exited = $proc.WaitForExit($TimeoutMinutes * 60 * 1000)
if (-not $exited) {
    $proc.Kill()
    Unregister-Event -SourceIdentifier $outputEvent.Name -ErrorAction SilentlyContinue
    Unregister-Event -SourceIdentifier $errorEvent.Name -ErrorAction SilentlyContinue
    Write-Host "[$ScriptName] Cook timed out after ${TimeoutMinutes} minutes." -ForegroundColor Red
    Write-Host "[$ScriptName] Partial log at: $LogFile" -ForegroundColor Yellow
    exit 2
}

# Wait for async output to finish
$proc.WaitForExit()

# Unregister events
Unregister-Event -SourceIdentifier $outputEvent.Name -ErrorAction SilentlyContinue
Unregister-Event -SourceIdentifier $errorEvent.Name -ErrorAction SilentlyContinue

$exitCode = $proc.ExitCode
$proc.Dispose()

# Write full output to log file
$syncOutput | Out-File -FilePath $LogFile -Encoding utf8

$Elapsed = (Get-Date) - $StartTime
$ElapsedSeconds = [math]::Round($Elapsed.TotalSeconds, 1)

# Step 4: Post-process log for common failures
Step 4 "Checking cook results"

if ($exitCode -ne 0) {
    Write-Host "[$ScriptName] UE4Editor-Cmd.exe exited with code $exitCode." -ForegroundColor Yellow
}

# Read log for error patterns
$logContent = Get-Content -Path $LogFile -Raw -ErrorAction SilentlyContinue
if ($logContent) {
    if ($logContent -match "Error: Cooker Engine") {
        Fail "Cook failed: 'Error: Cooker Engine' detected in log. See $LogFile"
    }
    if ($logContent -match "Failed to load.*\.uasset") {
        Fail "Cook failed: 'Failed to load' a .uasset detected in log. See $LogFile"
    }
    if ($logContent -match "Plugin '.*' failed to load") {
        Fail "Cook failed: Plugin failed to load. See $LogFile"
    }
}

# Count cooked assets from log (heuristic: lines containing "Cooked" and "assets")
$assetCount = 0
if ($logContent) {
    $match = [regex]::Match($logContent, "Cooked (\d+) assets")
    if ($match.Success) {
        $assetCount = [int]$match.Groups[1].Value
    }
}

if ($exitCode -eq 0) {
    Ok "cooked $assetCount assets in ${ElapsedSeconds}s -> $OutDir"
    exit 0
} else {
    Fail "Cook failed with exit code $exitCode. See $LogFile"
}