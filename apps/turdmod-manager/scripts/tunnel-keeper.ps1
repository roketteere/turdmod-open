# Persistent OVH monitoring tunnel — runs at logon (scheduled task TurdMODTunnel) and respawns the
# SSH local-forward if it ever drops. Makes the manager's "OVH (remote)" monitoring survive reboots
# without depending on the app being open. Forwards local 9091 -> OVH 127.0.0.1:9090 (the turdmod-
# service). @dep remote.json must point at 127.0.0.1:9091. Runs as the logged-on user (needs the SSH
# key at %USERPROFILE%\.ssh\id_ed25519).

$key = "$env:USERPROFILE\.ssh\id_ed25519"
$ErrorActionPreference = 'Continue'
while ($true) {
    try {
        & ssh -N `
            -o StrictHostKeyChecking=accept-new `
            -o ExitOnForwardFailure=yes `
            -o ServerAliveInterval=30 `
            -o ServerAliveCountMax=3 `
            -L 9091:127.0.0.1:9090 `
            -i $key YOUR_SSH_USER@YOUR_SERVER_IP
    } catch {}
    # ssh exited (network blip / remote sshd restart / reboot) — pause then reconnect
    Start-Sleep -Seconds 10
}
