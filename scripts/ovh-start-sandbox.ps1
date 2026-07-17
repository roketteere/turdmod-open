# Start the sandbox SCUM server instance on OVH
# Uses the same binary as production but different ports + save directory
# Run from the OVH server as admin

$binDir = "C:\SCUMServer\SCUM\Binaries\Win64"
$saveDir = "C:\SCUMServer-Sandbox\SCUM\Saved"

# Create save dir if missing
if (-not (Test-Path $saveDir)) {
    New-Item -ItemType Directory -Path "$saveDir\Config\WindowsServer" -Force | Out-Null
}

# Start with isolated save dir + different ports + no BattlEye
Start-Process -FilePath "$binDir\SCUMServer.exe" -ArgumentList @(
    "-log",
    "-port=7052",
    "-QueryPort=7054",
    "-NoBattlEye",
    "-SaveDir=`"$saveDir`""
) -WorkingDirectory $binDir

Write-Host "Sandbox server starting on port 7052 (6 players, admin testing)"
Write-Host "Connect: YOUR_SERVER_IP:7052"
