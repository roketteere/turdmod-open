# cook.ps1 -- Phase C P1 Hello World pak: cook the BP project for
# WindowsServer then wrap the cooked content as TurdMODHelloWorld_P.pak.
#
# Prerequisites:
#   1. UE 4.27.2 installed at C:/Program Files/Epic Games/UE_4.27/.
#   2. The Editor has been opened against the project at least once
#      so UBT has generated VS files and compiled HelloWorldPak module.
#   3. BP_HelloWorld asset exists under Content/ (Joel-driven step
#      per docs/runbooks/uproject-cook.md).
#
# Output: tmp/helloworld-pak/TurdMODHelloWorld_P.pak (suffix `_P`
# matters per the 2026-05-19 Q2 probe-pak learning -- sets mount
# priority; SCUM ignores paks that lack the `_P` suffix).
#
# Mirrors scripts/pak-probe/build-probe-pak.ps1 for UnrealPak
# response-file invocation.

$ScriptName  = "helloworld-cook"
$RepoRoot    = Resolve-Path "$PSScriptRoot\..\.."
$ProjectDir  = "$RepoRoot\apps\turdmod-helloworld-pak"
$UProject    = "$ProjectDir\turdmod-helloworld-pak.uproject"
$EngineRoot  = "C:\Program Files\Epic Games\UE_4.27"
$UE4EditorCmd = "$EngineRoot\Engine\Binaries\Win64\UE4Editor-Cmd.exe"
$UnrealPak    = "$EngineRoot\Engine\Binaries\Win64\UnrealPak.exe"
# Cook output goes under <project-name>/Content/ -- UE names this
# folder after the .uproject filename, NOT the C++ module name.
# Our .uproject is turdmod-helloworld-pak.uproject so the dir is
# 'turdmod-helloworld-pak', not 'HelloWorldPak'.
$CookOutDir  = "$ProjectDir\Saved\Cooked\WindowsServer\turdmod-helloworld-pak\Content"
$TempDir     = "$RepoRoot\tmp\helloworld-pak"
$ResponseFile = "$TempDir\response.txt"
$OutputPak   = "$TempDir\TurdMODHelloWorld_P.pak"

# Verify prerequisites
foreach ($req in @($UE4EditorCmd, $UnrealPak, $UProject)) {
    if (-not (Test-Path $req)) {
        Write-Host "[$ScriptName] MISSING required path: $req"
        exit 1
    }
}

New-Item -ItemType Directory -Path $TempDir -Force | Out-Null

# Step 1 -- cook the project for WindowsServer target.
# -unversioned strips object-version metadata so cooked assets work
# across minor SCUM patches (best-effort; verify on each SCUM update).
# -iterate skips assets that haven't changed since the last cook.
Write-Host "[$ScriptName] Cooking project for WindowsServer..."
$cookArgs = @(
    "`"$UProject`"",
    "-run=cook",
    "-targetplatform=WindowsServer",
    "-unversioned",
    "-iterate",
    "-nullrhi"
)
$cookProc = Start-Process -FilePath $UE4EditorCmd `
    -ArgumentList $cookArgs `
    -NoNewWindow -Wait -PassThru `
    -RedirectStandardOutput "$TempDir\cook.stdout.log" `
    -RedirectStandardError  "$TempDir\cook.stderr.log"
if ($cookProc.ExitCode -ne 0) {
    Write-Host "[$ScriptName] Cook FAILED -- exit $($cookProc.ExitCode). See $TempDir\cook.stderr.log"
    exit 2
}
if (-not (Test-Path $CookOutDir)) {
    Write-Host "[$ScriptName] Cook produced no Content output at $CookOutDir"
    Write-Host "[$ScriptName]   -> did you create BP_HelloWorld in the Editor first?"
    exit 3
}
$cookFiles = Get-ChildItem -Path $CookOutDir -Recurse -File
Write-Host "[$ScriptName] Cook OK -- $($cookFiles.Count) file(s) in $CookOutDir"

# Step 2 -- build the UnrealPak response file.
# Each line is: "<absolute source>" "<in-pak path>" [flags]
# The in-pak path mirrors SCUM's pak content layout
# (../../../SCUM/Content/...) so SCUM's asset registry finds the
# assets at the expected relative paths.
$lines = New-Object System.Collections.Generic.List[string]
foreach ($f in $cookFiles) {
    $rel = $f.FullName.Substring($CookOutDir.Length + 1).Replace('\', '/')
    $inPak = "../../../SCUM/Content/TurdMOD/HelloWorld/$rel"
    $lines.Add("`"$($f.FullName)`" `"$inPak`"")
}
[System.IO.File]::WriteAllLines($ResponseFile, $lines, [System.Text.UTF8Encoding]::new($false))
Write-Host "[$ScriptName] Wrote response file with $($lines.Count) entries -> $ResponseFile"

# Step 3 -- pack via UnrealPak.
if (Test-Path $OutputPak) { Remove-Item $OutputPak -Force }
Write-Host "[$ScriptName] Building pak..."
$pakOutput = & $UnrealPak $OutputPak "-Create=$ResponseFile" -compress 2>&1
$pakOutput | Out-File "$TempDir\unrealpak.log" -Encoding UTF8
if (-not (Test-Path $OutputPak)) {
    Write-Host "[$ScriptName] UnrealPak FAILED -- pak not produced. See $TempDir\unrealpak.log"
    exit 4
}
$pakSize = (Get-Item $OutputPak).Length
Write-Host "[$ScriptName] OK -- TurdMODHelloWorld_P.pak ($pakSize bytes)"
Write-Host "[$ScriptName] Next: scripts\helloworld-pak\deploy.ps1"
exit 0
