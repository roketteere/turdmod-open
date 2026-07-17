// SSH tunnel to remote server — the manager spawns `ssh -N -L <local>:localhost:<remote> admin@<host>` so the
// Live Monitor's "remote" target reaches remote server's localhost-bound turdmod-service (9090) WITHOUT
// exposing it to the internet (Joel's choice: tunnel, not open firewall). On start it repoints
// remote.json at 127.0.0.1:<local> (preserving the token) so /monitor/* "just works" over the tunnel.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelConfig {
    #[serde(default = "d_host")] pub ssh_host: String,
    #[serde(default = "d_user")] pub ssh_user: String,
    #[serde(default = "d_key")]  pub ssh_key: String,
    #[serde(default = "d_local")] pub local_port: u16,
    #[serde(default = "d_remote")] pub remote_port: u16,
}

fn d_host() -> String { "YOUR_SERVER_IP".into() }
fn d_user() -> String { "admin".into() }
fn d_key() -> String {
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    format!("{}\\.ssh\\id_ed25519", home)
}
fn d_local() -> u16 { 9091 }
fn d_remote() -> u16 { 9090 }

impl Default for TunnelConfig {
    fn default() -> Self {
        TunnelConfig { ssh_host: d_host(), ssh_user: d_user(), ssh_key: d_key(), local_port: d_local(), remote_port: d_remote() }
    }
}

fn cfg_path() -> PathBuf {
    let appdata = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| r"C:\Users\Default\AppData\Local".into());
    PathBuf::from(appdata).join("TurdMOD").join("tunnel.json")
}

impl TunnelConfig {
    pub fn load() -> Self {
        std::fs::read_to_string(cfg_path()).ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) -> Result<(), String> {
        let p = cfg_path();
        if let Some(parent) = p.parent() { std::fs::create_dir_all(parent).ok(); }
        std::fs::write(&p, serde_json::to_string_pretty(self).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
    }
}

static CHILD: OnceLock<Mutex<Option<Child>>> = OnceLock::new();
fn child() -> &'static Mutex<Option<Child>> { CHILD.get_or_init(|| Mutex::new(None)) }

pub fn start(cfg: &TunnelConfig) -> Result<(), String> {
    stop();
    // Forward to 127.0.0.1 (not "localhost") — avoids remote server resolving to ::1 where the service
    // (bound 0.0.0.0) isn't reachable. Verified working 2026-06-10.
    let fwd = format!("{}:127.0.0.1:{}", cfg.local_port, cfg.remote_port);
    let target = format!("{}@{}", cfg.ssh_user, cfg.ssh_host);
    let c = Command::new("ssh")
        .args([
            "-N",
            "-o", "StrictHostKeyChecking=accept-new",
            "-o", "ExitOnForwardFailure=yes",
            "-o", "ServerAliveInterval=30",
            "-L", &fwd,
            "-i", &cfg.ssh_key,
            &target,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn ssh: {}", e))?;
    *child().lock().unwrap() = Some(c);

    // repoint remote.json at the tunnel's local end, preserving the token
    let mut rc = crate::remote::RemoteConfig::load_raw().unwrap_or(crate::remote::RemoteConfig {
        host: "127.0.0.1".into(), port: cfg.local_port, token: String::new(), enabled: true,
    });
    rc.host = "127.0.0.1".into();
    rc.port = cfg.local_port;
    rc.enabled = true;
    let _ = crate::remote::save_config(&rc);
    Ok(())
}

pub fn stop() {
    if let Ok(mut g) = child().lock() {
        if let Some(mut c) = g.take() { let _ = c.kill(); }
    }
}

pub fn is_running() -> bool {
    if let Ok(mut g) = child().lock() {
        if let Some(c) = g.as_mut() {
            match c.try_wait() {
                Ok(None) => return true,   // still running
                _ => { *g = None; }         // exited or errored
            }
        }
    }
    false
}
