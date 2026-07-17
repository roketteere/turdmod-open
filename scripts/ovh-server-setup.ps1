#Requires -Version 5.1
<#
.SYNOPSIS
    Set up a fresh Windows Server 2022 (OVH RISE-1) for TurdMOD + SCUM.
.DESCRIPTION
    Run this script via RDP on the OVH server (YOUR_SERVER_IP).
    It installs: SteamCMD, SCUM Dedicated Server, Node.js, Git,
    and prepares for TurdMOD engine deployment.

    Run as Administrator.
#>

$ErrorActionPreference = 'Stop'
Write-Host "=== TurdMOD OVH Server Setup ===" -ForegroundColor Magenta
Write-Host "  Server: YOUR_SERVER_IP (RISE-1 Hillsboro)"
Write-Host "  OS: Windows Server 2022"
Write-Host ""

# Step 1 — Create directories
Write-Host "--- Step 1: Creating directories ---" -ForegroundColor Cyan
$dirs = @(
    "C:\TurdMOD",
    "C:\SteamCMD",
    "C:\SCUMServer",
    "C:\TurdMOD\engine",
    "C:\TurdMOD\mods",
    "C:\TurdMOD\backups"
)
foreach ($d in $dirs) {
    if (-not (Test-Path $d)) { New-Item -ItemType Directory -Path $d -Force | Out-Null }
    Write-Host "  $d"
}

# Step 2 — Install SteamCMD
Write-Host "`n--- Step 2: Installing SteamCMD ---" -ForegroundColor Cyan
if (-not (Test-Path "C:\SteamCMD\steamcmd.exe")) {
    Write-Host "  Downloading SteamCMD..."
    Invoke-WebRequest -Uri "https://steamcdn-a.akamaihd.net/client/installer/steamcmd.zip" -OutFile "C:\TurdMOD\steamcmd.zip"
    Expand-Archive -Path "C:\TurdMOD\steamcmd.zip" -DestinationPath "C:\SteamCMD" -Force
    Remove-Item "C:\TurdMOD\steamcmd.zip"
    Write-Host "  SteamCMD installed at C:\SteamCMD"
} else {
    Write-Host "  SteamCMD already installed"
}

# Step 3 — Install SCUM Dedicated Server
Write-Host "`n--- Step 3: Installing SCUM Dedicated Server ---" -ForegroundColor Cyan
Write-Host "  This will download ~15 GB. May take 10-30 minutes."
# @inv: SCUM Dedicated Server app id = 3792580 (NOT the old 1077840 — that id is
# unowned and returns "No subscription"). Requires a Steam account that owns SCUM
# (login `roketteere`, cached on the box) — anonymous also returns "No subscription".
Write-Host "  App ID: 3792580 (SCUM Dedicated Server), login roketteere (owns SCUM)"
$steamArgs = "+force_install_dir C:\SCUMServer +login roketteere +app_update 3792580 validate +quit"
Start-Process -FilePath "C:\SteamCMD\steamcmd.exe" -ArgumentList $steamArgs -NoNewWindow -Wait
Write-Host "  SCUM Server installed at C:\SCUMServer"

# Step 4 — Install Node.js LTS
Write-Host "`n--- Step 4: Installing Node.js 20 LTS ---" -ForegroundColor Cyan
if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
    Write-Host "  Downloading Node.js 20 LTS installer..."
    Invoke-WebRequest -Uri "https://nodejs.org/dist/v20.18.1/node-v20.18.1-x64.msi" -OutFile "C:\TurdMOD\node-installer.msi"
    Start-Process msiexec.exe -ArgumentList "/i C:\TurdMOD\node-installer.msi /quiet /norestart" -Wait
    Remove-Item "C:\TurdMOD\node-installer.msi"
    Write-Host "  Node.js installed. Restart terminal to refresh PATH."
} else {
    Write-Host "  Node.js already installed: $(node --version)"
}

# Step 5 — Install Git
Write-Host "`n--- Step 5: Installing Git ---" -ForegroundColor Cyan
if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
    Write-Host "  Downloading Git..."
    Invoke-WebRequest -Uri "https://github.com/git-for-windows/git/releases/download/v2.47.1.windows.1/Git-2.47.1-64-bit.exe" -OutFile "C:\TurdMOD\git-installer.exe"
    Start-Process -FilePath "C:\TurdMOD\git-installer.exe" -ArgumentList "/VERYSILENT /NORESTART" -Wait
    Remove-Item "C:\TurdMOD\git-installer.exe"
    Write-Host "  Git installed."
} else {
    Write-Host "  Git already installed: $(git --version)"
}

# Step 6 — Open firewall ports
Write-Host "`n--- Step 6: Opening firewall ports ---" -ForegroundColor Cyan
$rules = @(
    @{ Name="SCUM Game Port"; Port=7042; Protocol="UDP" },
    @{ Name="SCUM Query Port"; Port=7044; Protocol="UDP" },
    @{ Name="SCUM RCON"; Port=7048; Protocol="TCP" },
    @{ Name="SCUM Steam Port"; Port=27015; Protocol="UDP" }
)
foreach ($r in $rules) {
    $existing = Get-NetFirewallRule -DisplayName $r.Name -ErrorAction SilentlyContinue
    if (-not $existing) {
        New-NetFirewallRule -DisplayName $r.Name -Direction Inbound -Protocol $r.Protocol -LocalPort $r.Port -Action Allow | Out-Null
        Write-Host "  Opened $($r.Name): $($r.Protocol) $($r.Port)"
    } else {
        Write-Host "  Already open: $($r.Name)"
    }
}

# Step 7 — Create SCUM server config
Write-Host "`n--- Step 7: Server config ---" -ForegroundColor Cyan
$configDir = "C:\SCUMServer\SCUM\Saved\Config\WindowsServer"
if (-not (Test-Path $configDir)) { New-Item -ItemType Directory -Path $configDir -Force | Out-Null }

# Step 8 — Enable OpenSSH Server
Write-Host "`n--- Step 8: Enabling OpenSSH Server ---" -ForegroundColor Cyan
$sshCapability = Get-WindowsCapability -Online | Where-Object Name -like 'OpenSSH.Server*'
if ($sshCapability.State -ne 'Installed') {
    Add-WindowsCapability -Online -Name $sshCapability.Name
    Start-Service sshd
    Set-Service -Name sshd -StartupType Automatic
    New-NetFirewallRule -DisplayName "OpenSSH Server" -Direction Inbound -Protocol TCP -LocalPort 22 -Action Allow -ErrorAction SilentlyContinue | Out-Null
    Write-Host "  OpenSSH Server installed and started"
} else {
    Write-Host "  OpenSSH Server already installed"
    Start-Service sshd -ErrorAction SilentlyContinue
}

# Step 9 — Create pak bypass flag
Write-Host "`n--- Step 9: Pak bypass flag ---" -ForegroundColor Cyan
if (-not (Test-Path "C:\TurdMOD\pak_bypass.enabled")) {
    New-Item -ItemType File -Path "C:\TurdMOD\pak_bypass.enabled" -Force | Out-Null
}
Write-Host "  C:\TurdMOD\pak_bypass.enabled exists"

# Summary
Write-Host "`n=== Setup Complete ===" -ForegroundColor Green
Write-Host "  SteamCMD: C:\SteamCMD"
Write-Host "  SCUM Server: C:\SCUMServer"
Write-Host "  TurdMOD: C:\TurdMOD"
Write-Host ""
Write-Host "Next steps:" -ForegroundColor Yellow
Write-Host "  1. Clone turdmod repo: git clone https://github.com/roketteere/turdmod C:\TurdMOD\repo"
Write-Host "  2. Deploy bridge DLL (from Manager or manually)"
Write-Host "  3. Deploy companion mods"
Write-Host "  4. Start SCUM server"
Write-Host "  5. Test from game client"
