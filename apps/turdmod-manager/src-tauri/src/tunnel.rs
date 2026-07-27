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

/// First local port we can actually bind, starting at `preferred`.
///
/// @ctx: the local end used to be fixed. When something already holds it —
///   including an ORPHANED socket left behind by a dead ssh, which is exactly
///   what happened on the dev box — ssh exits 255 instantly and every remote
///   call fails with no explanation. Probing costs microseconds and removes a
///   whole class of "the Manager just doesn't work" reports.
fn free_local_port(preferred: u16) -> Option<u16> {
    use std::net::TcpListener;
    (0..20).find_map(|i| {
        let p = preferred.checked_add(i)?;
        TcpListener::bind(("127.0.0.1", p)).ok().map(|l| {
            drop(l);
            p
        })
    })
}

pub fn start(cfg: &TunnelConfig) -> Result<(), String> {
    stop();

    let port = free_local_port(cfg.local_port).ok_or_else(|| {
        format!(
            "no free local port near {} — something is holding that range. Close other TurdMOD \
             windows, or reboot if a dead process left the socket behind.",
            cfg.local_port
        )
    })?;

    // Forward to 127.0.0.1 (not "localhost") — avoids remote server resolving to ::1 where the service
    // (bound 0.0.0.0) isn't reachable. Verified working 2026-06-10.
    let fwd = format!("{}:127.0.0.1:{}", port, cfg.remote_port);
    let target = format!("{}@{}", cfg.ssh_user, cfg.ssh_host);
    let mut c = Command::new("ssh")
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

    // @inv: do NOT report success — or repoint remote.json — until ssh has
    //   actually stayed up. With ExitOnForwardFailure it dies in well under a
    //   second when the forward can't be established, and this used to return
    //   Ok() regardless: remote.json then advertised a live tunnel while every
    //   call silently failed, which reads to the user as "remote mode is broken".
    std::thread::sleep(std::time::Duration::from_millis(1200));
    match c.try_wait() {
        Ok(Some(status)) => {
            return Err(format!(
                "ssh tunnel to {} exited immediately ({}). Check the SSH key at {} and that the \
                 server is reachable.",
                cfg.ssh_host, status, cfg.ssh_key
            ))
        }
        Err(e) => return Err(format!("couldn't check the ssh tunnel: {e}")),
        Ok(None) => {}
    }

    *child().lock().unwrap() = Some(c);

    // repoint remote.json at the tunnel's local end, preserving the token
    let mut rc = crate::remote::RemoteConfig::load_raw().unwrap_or(crate::remote::RemoteConfig {
        host: "127.0.0.1".into(), port, token: String::new(), enabled: true,
    });
    rc.host = "127.0.0.1".into();
    rc.port = port;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// The bug this guards: a squatted local port made ssh exit instantly while
    /// start() still reported success.
    #[test]
    fn skips_a_port_that_is_already_taken() {
        let held = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let taken = held.local_addr().unwrap().port();
        let got = free_local_port(taken).expect("should find one nearby");
        assert_ne!(got, taken, "must not hand back a port we cannot bind");
        assert!(got > taken && got <= taken + 20);
    }

    #[test]
    fn uses_the_preferred_port_when_it_is_free() {
        let probe = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let p = probe.local_addr().unwrap().port();
        drop(probe); // now free
        assert_eq!(free_local_port(p), Some(p));
    }
}
