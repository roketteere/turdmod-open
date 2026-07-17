# OVH Windows → SCUM Dedicated Server → TurdMOD Engine — install runbook

> First targeted at server `ns1018358.ip-15-204-47.us` (RISE-1 Xeon-E
> 2386G, HIL1, ordered 2026-05-17 PM). Steps generalize to any
> Windows Server 2022 box where the admin owns `Binaries/Win64/`.

This is the **end-to-end** install of a TurdMOD Engine-tier SCUM
server starting from a freshly-imaged Windows box. Run top to bottom;
each section ends with a verification step you should not skip.

**Time budget:** 45–90 min wall-clock (SCUM Steam download is the
biggest chunk — ~80 GB).

**Prereqs:**
- Windows Server 2022 Standard freshly installed on the OVH box
- Administrator RDP credentials from OVH
- Network access to the box (RDP on TCP 3389)
- The TurdMODEngineBridge.dll built locally and ready to copy
- The TurdMOD AES key from `scumdump.config.json` (or re-extract per
  build via AESDumpster)

## 0. First-touch RDP + Windows hardening

Open Microsoft Remote Desktop on your local box. Connect to the OVH
public IP using the credentials from OVH's delivery email.
**Change the Administrator password immediately on first login** —
OVH-provided initial passwords are emailed in plaintext.

```powershell
# Run in an elevated PowerShell on the OVH box.

# Set a new Administrator password (replace the placeholder).
$newPw = ConvertTo-SecureString 'YOUR_STRONG_NEW_PASSWORD_HERE' -AsPlainText -Force
Set-LocalUser -Name Administrator -Password $newPw

# Disable Windows Defender real-time scanning for the SCUM install
# directory only — Defender will quarantine UE4SS DLLs as RAT-like.
Add-MpPreference -ExclusionPath 'C:\SCUMServer'

# Enable RDP timeout (kicks idle sessions after 60 min).
Set-ItemProperty -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows NT\Terminal Services' `
  -Name MaxIdleTime -Value 3600000 -Type DWord -Force -ErrorAction SilentlyContinue
```

**Save the new Administrator password to `.secrets/credentials.md`
under a new "OVH Windows server" section before proceeding** — losing
it is a full reinstall.

**Optional but recommended:**

```powershell
# Set timezone to UTC so logs + SCUM event timestamps line up across
# admins in different timezones.
Set-TimeZone -Id 'UTC'

# Set the machine name (matches the OVH hostname so logs are easier
# to correlate).
Rename-Computer -NewName 'turdmod-scum-01' -Force
# Restart-Computer  # NOT YET — defer reboot to after Steam + UE4SS install
```

## 1. Install OpenSSH (lets Claude / scripts drive the server)

Windows Server 2022 ships with OpenSSH-Server as an optional feature.
Enable it so future deploys / re-runs of this runbook can happen via
Bash from anywhere instead of requiring RDP.

```powershell
# Install the OpenSSH server feature.
Add-WindowsCapability -Online -Name OpenSSH.Server~~~~0.0.1.0

# Start and persist the service.
Start-Service sshd
Set-Service -Name sshd -StartupType 'Automatic'

# Open SSH port (TCP 22) in Windows Firewall.
New-NetFirewallRule -Name 'OpenSSH-Server-In-TCP' -DisplayName 'OpenSSH Server (sshd)' `
  -Enabled True -Direction Inbound -Protocol TCP -Action Allow -LocalPort 22

# Add Joel's public key as an authorized SSH key. Append to
# %ProgramData%\ssh\administrators_authorized_keys (the special
# OpenSSH file for admin SSH logins on Windows Server).
$adminKey = 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBg8gFxc0RMEOqrwUByzaGtZZMfSpWvTO24BSxYhwBNS turdmod-ops@joel-windows'
$keyFile = "$env:ProgramData\ssh\administrators_authorized_keys"
Add-Content -Path $keyFile -Value $adminKey
# Lock down permissions per OpenSSH's strict-mode requirements.
icacls $keyFile /inheritance:r /grant 'Administrators:F' /grant 'SYSTEM:F'

# Default shell → PowerShell (better than cmd.exe over SSH).
New-ItemProperty -Path 'HKLM:\SOFTWARE\OpenSSH' -Name DefaultShell `
  -Value 'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe' -PropertyType String -Force
```

**Verify from your local box:**

```bash
# From local Bash / WSL / Git Bash:
ssh -i $HOME/.ssh/your_key Administrator@<OVH-PUBLIC-IP>
# Should land you at a Windows PowerShell prompt with no password.
```

## 2. Install SteamCMD

SteamCMD is Valve's command-line Steam client — required to install
SCUM Dedicated Server without a GUI Steam install.

```powershell
# Create directories.
New-Item -ItemType Directory -Force -Path 'C:\SteamCMD' | Out-Null
New-Item -ItemType Directory -Force -Path 'C:\SCUMServer' | Out-Null

# Download SteamCMD archive.
Invoke-WebRequest `
  -Uri 'https://steamcdn-a.akamaihd.net/client/installer/steamcmd.zip' `
  -OutFile 'C:\SteamCMD\steamcmd.zip'

# Extract.
Expand-Archive -Path 'C:\SteamCMD\steamcmd.zip' -DestinationPath 'C:\SteamCMD' -Force
Remove-Item 'C:\SteamCMD\steamcmd.zip'

# First run — SteamCMD self-updates and bootstraps.
& 'C:\SteamCMD\steamcmd.exe' +quit
```

**Verify:** `C:\SteamCMD\steamcmd.exe` runs without error and exits.

## 3. Install SCUM Dedicated Server

SCUM Dedicated Server is Steam app **3792580**. Anonymous install
works — no Steam login needed for dedicated server.

```powershell
# This will pull ~80 GB. Takes 15–60 min depending on OVH's network.
& 'C:\SteamCMD\steamcmd.exe' `
  +force_install_dir 'C:\SCUMServer' `
  +login anonymous `
  +app_update 3792580 validate `
  +quit
```

**Verify:**

```powershell
Test-Path 'C:\SCUMServer\SCUM\Binaries\Win64\GameServer.exe'
# Should output True.
Get-Item 'C:\SCUMServer\SCUM\Binaries\Win64\GameServer.exe' |
  Select-Object Length, LastWriteTime
# Length should be ~125 MB (current build).
```

## 4. First boot — generate default config files

GameServer.exe needs to run once to generate
`ServerSettings.ini`, `GameUserSettings.ini`, the `Saved/` tree,
etc. We'll start it, let it initialize, then stop it.

```powershell
# Start SCUMServer in a background job so we can stop it cleanly.
$scum = Start-Process -FilePath 'C:\SCUMServer\SCUM\Binaries\Win64\GameServer.exe' `
  -WorkingDirectory 'C:\SCUMServer\SCUM\Binaries\Win64' `
  -PassThru -WindowStyle Minimized

# Wait ~3 min for full initialization (loading paks is the slow part).
Start-Sleep -Seconds 180

# Stop it.
Stop-Process -Id $scum.Id -Force
```

**Verify:**

```powershell
Test-Path 'C:\SCUMServer\SCUM\Saved\Config\WindowsServer\ServerSettings.ini'
# True. File should be ~18 KB.
```

## 5. Configure SCUM (RCON + ServerSettings)

Generate an RCON password and patch `ServerSettings.ini` to enable
it. Use the same shape we used on the local box (see
`.secrets/credentials.md`, local-server section — the value lives there, never inline it in a doc).
For the OVH box, generate a NEW password.

```powershell
# Generate a fresh 20-char random RCON password.
$rconPw = -join ((1..20) | ForEach-Object {
  [char][int]((48..57) + (65..90) + (97..122) | Get-Random)
})
Write-Output "OVH RCON password (save to .secrets/credentials.md): $rconPw"

# Append RCON config to [General] section.
$ini = 'C:\SCUMServer\SCUM\Saved\Config\WindowsServer\ServerSettings.ini'
$rconBlock = @"

scum.RconEnabled=1
scum.RconPort=30016
scum.RconPassword=$rconPw
"@

# Find the [World] section start and inject before it (matches the
# pattern we used on G-Portal — keeps RCON inside [General]).
$content = Get-Content $ini -Raw
$patched = $content -replace '(\r?\n)\[World\]', "$rconBlock`$1[World]"
Set-Content -Path $ini -Value $patched -NoNewline
```

**Save the RCON password** to `.secrets/credentials.md` under a new
"OVH SCUM RCON" section before proceeding.

## 6. Install UE4SS

UE4SS = the loader that makes engine-tier modding possible. The
TurdMOD bridge is a UE4SS cppmod, so UE4SS must be present.

**Locally:** copy your existing UE4SS install + the
TurdMODEngineBridge bundle from
`C:\Program Files (x86)\Steam\steamapps\common\SCUM Server\SCUM\Binaries\Win64\UE4SS\`
(your working local install) to a portable bundle, then SCP/RDP-copy
to the OVH box.

```powershell
# On the OVH box (after copying the UE4SS folder over to
# C:\Transfer\UE4SS\):
$ue4ssTarget = 'C:\SCUMServer\SCUM\Binaries\Win64\UE4SS'
Copy-Item -Path 'C:\Transfer\UE4SS' -Destination $ue4ssTarget -Recurse -Force

# Verify the critical files landed:
Test-Path "$ue4ssTarget\UE4SS.dll"                              # True
Test-Path "$ue4ssTarget\UE4SS-settings.ini"                     # True
Test-Path "$ue4ssTarget\MemberVariableLayout.ini"               # True
Test-Path "$ue4ssTarget\VTableLayout.ini"                       # True
Test-Path "$ue4ssTarget\Mods\TurdMODEngineBridge\dlls\main.dll" # True
```

UE4SS loads via a proxy DLL or via the `turdmod-loader` injector.
For first install, use the proxy approach (simpler):

```powershell
# UE4SS standard install drops a dwmapi.dll proxy that GameServer.exe
# loads at startup (Windows looks for dwmapi.dll in the exe's
# directory first). The proxy then loads UE4SS.dll.
Test-Path 'C:\SCUMServer\SCUM\Binaries\Win64\dwmapi.dll'  # True
```

If that file is missing in your local UE4SS install, fetch the latest
UE4SS release from https://github.com/UE4SS-RE/RE-UE4SS/releases —
the `UE4SS_Signatures_v3.0.1.zip` (or current) drops the right proxy.

## 7. Configure Windows Firewall for SCUM

```powershell
# Game traffic (UDP).
New-NetFirewallRule -DisplayName 'SCUM Game' -Direction Inbound `
  -Protocol UDP -LocalPort 30002 -Action Allow

# Steam query (UDP).
New-NetFirewallRule -DisplayName 'SCUM Query' -Direction Inbound `
  -Protocol UDP -LocalPort 30015 -Action Allow

# RCON (TCP).
New-NetFirewallRule -DisplayName 'SCUM RCON' -Direction Inbound `
  -Protocol TCP -LocalPort 30016 -Action Allow
```

These match the ports our local server + G-Portal use, so Manager's
existing port assumptions Just Work.

## 8. First real boot

```powershell
# Start SCUMServer in the background. Tail UE4SS log to confirm
# the bridge initialized.
Start-Process -FilePath 'C:\SCUMServer\SCUM\Binaries\Win64\GameServer.exe' `
  -WorkingDirectory 'C:\SCUMServer\SCUM\Binaries\Win64' `
  -WindowStyle Minimized

# Wait ~3 min then tail UE4SS log.
Start-Sleep -Seconds 180
Get-Content 'C:\SCUMServer\SCUM\Binaries\Win64\UE4SS\UE4SS.log' -Tail 40
```

**Look for these lines in the UE4SS log:**

- `[PS] Found GUObjectArray: 0x...`  — UE4SS sigscan worked
- `[TurdMODEngineBridge] mirrored GUObjectArray @ 0x...` — bridge alive
- `[TurdMODEngineBridge] bridgeReady event emitted; ready` — handlers registered
- `[TurdMODEngineBridge] getOnlinePlayers: count=0` (heartbeat) — bridge healthy

If you see all four, the engine tier is alive on OVH. 🎉

## 9. Smoke test: scumdump phase-a against the OVH bridge

From your local box (not the OVH box), point scumdump at the OVH
bridge pipe. This requires routing the named pipe over SSH or
running scumdump on the OVH box itself.

**Path A: run scumdump on the OVH box** (simpler):

```powershell
# On OVH box:
# Copy scumdump tree from your local machine.
# Then in the scumdump directory:
npx tsx src/cli.ts detect
npx tsx src/cli.ts phase-a
```

Confirm `data/extracted/v<build>/{classes,enums,structs}.json` are
populated and counts roughly match local (~14,500 classes /
~1,700 enums / ~3,400 structs).

## 10. Add the OVH server to TurdMOD Admin

From your local TurdMOD Admin (the Manager you've been clicking),
add the OVH box as a remote engine-tier server. Engine-tier remote
support isn't wired in Manager yet (planned work) — for now, you
RDP into the OVH box and run the Manager there if you need its UI.

## 11. Lock down + auto-start

```powershell
# Wrap GameServer.exe in a Windows Service so it survives reboots.
# Use NSSM (https://nssm.cc/) or sc.exe — example with sc.exe:
sc.exe create SCUMServer `
  binPath= '"C:\SCUMServer\SCUM\Binaries\Win64\GameServer.exe"' `
  start= auto `
  DisplayName= 'SCUM Dedicated Server (TurdMOD)'
sc.exe description SCUMServer 'SCUM dedicated game server with UE4SS + TurdMODEngineBridge.'

# Set service to restart on crash.
sc.exe failure SCUMServer reset= 86400 actions= restart/60000/restart/60000/restart/60000
```

**Note:** running GameServer.exe via Windows Service has a known
quirk — UE4SS may not initialize fully because the service account
isn't an interactive session. If you hit issues, run GameServer.exe
under a scheduled task triggered at boot under the Administrator
account instead.

## Rollback

If anything in steps 4–8 goes wrong:

```powershell
# Stop SCUM.
Stop-Process -Name SCUMServer -Force -ErrorAction SilentlyContinue

# Wipe Saved/ and reinstall.
Remove-Item 'C:\SCUMServer\SCUM\Saved' -Recurse -Force
# Then re-run step 4 (first boot to regenerate config).
```

For a full nuke-and-pave, OVH Manager has a "Reinstall" button — same
flow as the initial install. Takes ~30 min.

## What's done at this point

| Capability | State |
|---|---|
| SCUM Dedicated Server installed | ✅ |
| Windows Firewall opened for SCUM ports | ✅ |
| RCON configured + tested | ✅ |
| UE4SS loading at SCUM startup | ✅ |
| TurdMODEngineBridge.dll loaded by UE4SS | ✅ |
| Named-pipe RPC alive | ✅ |
| scumdump phase-a smoke test passes | ✅ |
| OpenSSH server for remote drive | ✅ |
| Auto-start on reboot | ✅ |

You now have an Engine-tier TurdMOD server in production. Refer
players to the public IP + port 30002 to connect.

## Related

- `docs/architecture.md` — tier model + design
- `.secrets/credentials.md` — credentials runbook (RCON, FTP, SSH)
- `apps/turdmod-engine-bridge/README.md` — bridge build process
- `scumdump/PLAN.md` — extraction pipeline architecture
