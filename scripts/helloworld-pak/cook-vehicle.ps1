# cook-vehicle.ps1 — cook the BP project (WindowsServer) and pak it PRESERVING real
# package paths, so a vehicle BP authored at /Game/ConZ_Files/Vehicles/... mounts at
# that exact path in-game. This is REQUIRED for native "Vehicle" primary-asset
# registration: UConZAssetManager scans /Game/ConZ_Files/Vehicles/ at boot and reads
# the asset's BAKED package name — a pak file-remap to a different path does NOT relocate
# the asset for the registry. (Contrast cook.ps1, which remaps everything to /Game/TurdMOD/.)
#
# @inv in-pak path "../../../SCUM/Content/<rel>" == mount "/Game/<rel>" (SCUM maps /Game/ -> SCUM/Content/).
# Output: tmp/helloworld-pak/TurdMODVehicle_P.pak  (deploy via deploy-vehicle.ps1)

$ScriptName   = "vehicle-cook"
$RepoRoot     = Resolve-Path "$PSScriptRoot\..\.."
$ProjectDir   = "$RepoRoot\apps\turdmod-helloworld-pak"
$UProject     = "$ProjectDir\turdmod-helloworld-pak.uproject"
$EngineRoot   = "C:\Program Files\Epic Games\UE_4.27"
$UE4EditorCmd = "$EngineRoot\Engine\Binaries\Win64\UE4Editor-Cmd.exe"
$UnrealPak    = "$EngineRoot\Engine\Binaries\Win64\UnrealPak.exe"
$CookRoot     = "$ProjectDir\Saved\Cooked\WindowsServer\turdmod-helloworld-pak"
$CookOutDir   = "$CookRoot\Content"
$TempDir      = "$RepoRoot\tmp\helloworld-pak"
$ResponseFile = "$TempDir\response-vehicle.txt"
$OutputPak    = "$TempDir\TurdMODVehicle_P.pak"

foreach ($req in @($UE4EditorCmd, $UnrealPak, $UProject)) {
    if (-not (Test-Path $req)) { Write-Host "[$ScriptName] MISSING required path: $req"; exit 1 }
}
New-Item -ItemType Directory -Path $TempDir -Force | Out-Null

Write-Host "[$ScriptName] Cooking project for WindowsServer..."
$cookArgs = @("`"$UProject`"","-run=cook","-targetplatform=WindowsServer","-unversioned","-iterate","-nullrhi")
$cookProc = Start-Process -FilePath $UE4EditorCmd -ArgumentList $cookArgs -NoNewWindow -Wait -PassThru `
    -RedirectStandardOutput "$TempDir\cook-vehicle.stdout.log" -RedirectStandardError "$TempDir\cook-vehicle.stderr.log"
# Cook exit-1 is cosmetic (_Del cleanup); success = Content output present.
if (-not (Test-Path $CookOutDir)) {
    Write-Host "[$ScriptName] Cook produced no Content at $CookOutDir (exit $($cookProc.ExitCode)). See cook-vehicle.stderr.log"; exit 3
}
$cookFiles = Get-ChildItem -Path $CookOutDir -Recurse -File
Write-Host "[$ScriptName] Cook OK -- $($cookFiles.Count) file(s)"

# Response file: mirror the cooked Content tree 1:1 into ../../../SCUM/Content/<rel> (preserve real paths).
$lines = New-Object System.Collections.Generic.List[string]
foreach ($f in $cookFiles) {
    $rel = $f.FullName.Substring($CookOutDir.Length + 1).Replace('\','/')
    $inPak = "../../../SCUM/Content/$rel"
    $lines.Add("`"$($f.FullName)`" `"$inPak`"")
}
# Inject the MERGED registry (base game + BPC_TurdJeep) at SCUM/AssetRegistry.bin so our
# higher-priority pak SHADOWS the baked registry -> the AssetManager registers our vehicle.
$merged = "$RepoRoot\tmp\merged-registry\AssetRegistry.bin"
if (Test-Path $merged) { $lines.Add("`"$merged`" `"../../../SCUM/AssetRegistry.bin`""); Write-Host "[$ScriptName] + MERGED AssetRegistry.bin (shadows base)" }
else { Write-Host "[$ScriptName] !! merged registry MISSING - run TurdMODMergeRegistry first" }
[System.IO.File]::WriteAllLines($ResponseFile, $lines, [System.Text.UTF8Encoding]::new($false))
Write-Host "[$ScriptName] Response file: $($lines.Count) entries -> $ResponseFile"

if (Test-Path $OutputPak) { Remove-Item $OutputPak -Force }
Write-Host "[$ScriptName] Building pak..."
$pakOutput = & $UnrealPak $OutputPak "-Create=$ResponseFile" -compress 2>&1
$pakOutput | Out-File "$TempDir\unrealpak-vehicle.log" -Encoding UTF8
if (-not (Test-Path $OutputPak)) { Write-Host "[$ScriptName] UnrealPak FAILED. See unrealpak-vehicle.log"; exit 4 }
Write-Host "[$ScriptName] OK -- TurdMODVehicle_P.pak ($((Get-Item $OutputPak).Length) bytes)"
Write-Host "[$ScriptName] Next: scripts\helloworld-pak\deploy-vehicle.ps1"
exit 0
