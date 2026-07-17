Add-WindowsCapability -Online -Name OpenSSH.Server~~~~0.0.1.0
Start-Service sshd
Set-Service -Name sshd -StartupType Automatic
$sshDir = "C:\Users\admin\.ssh"
New-Item -ItemType Directory -Path $sshDir -Force
"ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBg8gFxc0RMEOqrwUByzaGtZZMfSpWvTO24BSxYhwBNS turdmod-ops@joel-windows" | Out-File -Encoding utf8 "$sshDir\authorized_keys" -Force
$adminKeys = "C:\ProgramData\ssh\administrators_authorized_keys"
"ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBg8gFxc0RMEOqrwUByzaGtZZMfSpWvTO24BSxYhwBNS turdmod-ops@joel-windows" | Out-File -Encoding utf8 $adminKeys -Force
icacls $adminKeys /inheritance:r /grant "Administrators:F" /grant "SYSTEM:F"
New-NetFirewallRule -DisplayName "SSH" -Direction Inbound -Protocol TCP -LocalPort 22 -Action Allow -ErrorAction SilentlyContinue
$sshdConfig = "C:\ProgramData\ssh\sshd_config"
(Get-Content $sshdConfig) -replace '#PasswordAuthentication yes','PasswordAuthentication no' | Set-Content $sshdConfig
Restart-Service sshd
Write-Host "SSH ready - key-only auth for admin" -ForegroundColor Green
