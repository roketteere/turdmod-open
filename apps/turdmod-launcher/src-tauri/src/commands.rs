//! Tauri commands for the launcher.
//!
//! @dep: turdmod_launcher_core (launch, ServerTarget, paths) — shared with
//! the CLI launcher.
//! @dep: ../../turdmod-loader/src/runtime.rs (reads mods/enabled.json) and
//! ../../turdmod-loader/src/detect.rs (reads launch-mode.json) — the
//! on-disk contracts this backend writes.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use turdmod_launcher_core as core;

/// Where to fetch the server allowlist. Overridable for dev/staging via
/// TURDMOD_API_BASE (no hardcoded environment assumptions baked in).
fn api_base() -> String {
    std::env::var("TURDMOD_API_BASE").unwrap_or_else(|_| "https://turdmod.com".to_string())
}

// ---------------------------------------------------------------------------
// Servers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerDto {
    pub id: String,
    pub name: String,
    pub ip: String,
    pub port: u16,
    #[serde(rename = "battlEye", default)]
    pub battle_eye: bool,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ServersResponse {
    servers: Vec<ServerDto>,
}

fn servers_cache_path() -> Option<PathBuf> {
    core::turdmod_data_dir().map(|mut p| {
        p.push("servers-cache.json");
        p
    })
}

fn read_servers_cache() -> Vec<ServerDto> {
    let Some(path) = servers_cache_path() else {
        return Vec::new();
    };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Vec<ServerDto>>(&raw).ok())
        .unwrap_or_default()
}

fn write_servers_cache(servers: &[ServerDto]) {
    if let Some(dir) = core::turdmod_data_dir() {
        let _ = std::fs::create_dir_all(&dir);
    }
    if let (Some(path), Ok(json)) = (servers_cache_path(), serde_json::to_string_pretty(servers)) {
        let _ = std::fs::write(path, json);
    }
}

/// A joinable entry for the SCUM server on THIS box (the local turdmod-service
/// dev server on 127.0.0.1:7042). Dev-only: surfaced in debug builds so we can
/// test the modded client against local before the remote server. Never shipped to players.
/// Override the IP via TURDMOD_LOCAL_SERVER_IP (e.g. a LAN address) if needed.
fn local_dev_server() -> ServerDto {
    let ip = std::env::var("TURDMOD_LOCAL_SERVER_IP").unwrap_or_else(|_| "127.0.0.1".to_string());
    ServerDto {
        id: "local-dev".into(),
        name: "Local (dev box)".into(),
        ip,
        port: 7042,
        battle_eye: false,
        region: Some("Local".into()),
        description: Some("This machine's turdmod-service SCUM server. Dev-only — start it first (POST /server/start).".into()),
    }
}

/// Fetch the our-servers allowlist from turdmod.com. Only BE-off servers are
/// returned; on network failure, falls back to the last-good cache.
async fn fetch_remote_servers() -> Result<Vec<ServerDto>, String> {
    let url = format!("{}/api/servers", api_base());
    match reqwest::get(&url).await {
        Ok(resp) => match resp.json::<ServersResponse>().await {
            Ok(body) => {
                // Defense in depth: the endpoint already filters, but never
                // surface a BE-on server to the client.
                let servers: Vec<ServerDto> =
                    body.servers.into_iter().filter(|s| !s.battle_eye).collect();
                write_servers_cache(&servers);
                Ok(servers)
            }
            Err(e) => {
                let cached = read_servers_cache();
                if cached.is_empty() {
                    Err(format!("bad servers response: {e}"))
                } else {
                    Ok(cached)
                }
            }
        },
        Err(e) => {
            let cached = read_servers_cache();
            if cached.is_empty() {
                Err(format!("fetch servers failed ({e}) and no cache available"))
            } else {
                Ok(cached)
            }
        }
    }
}

/// Server list shown in the launcher. In DEBUG builds, the local dev box is
/// prepended so we can join it alongside the remote server (the remote cache never includes
/// it). In release, only the turdmod.com allowlist is returned.
#[tauri::command]
pub async fn launcher_list_servers() -> Result<Vec<ServerDto>, String> {
    let mut out: Vec<ServerDto> = Vec::new();
    if cfg!(debug_assertions) {
        out.push(local_dev_server());
    }
    match fetch_remote_servers().await {
        Ok(servers) => out.extend(servers),
        // In debug the local entry still stands; only error if we have nothing.
        Err(e) if out.is_empty() => return Err(e),
        Err(_) => {}
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Mods
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct ModDto {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
    pub enabled: bool,
}

fn enabled_json_path() -> Option<PathBuf> {
    core::mods_dir().map(|mut p| {
        p.push("enabled.json");
        p
    })
}

/// Returns the enabled-id set, or `None` if enabled.json is absent. `None`
/// means "all mods enabled" — matches the loader's back-compat behavior.
fn read_enabled() -> Option<Vec<String>> {
    let path = enabled_json_path()?;
    let raw = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let arr = v.get("enabled")?.as_array()?;
    Some(
        arr.iter()
            .filter_map(|e| e.as_str().map(|s| s.to_string()))
            .collect(),
    )
}

/// Reads an optional `turdmod.json` manifest from a mod folder for display
/// metadata. Tolerant: any missing field just falls back to the folder id.
fn read_manifest(dir: &std::path::Path) -> Option<serde_json::Value> {
    for name in ["turdmod.json", "manifest.json"] {
        let p = dir.join(name);
        if let Ok(raw) = std::fs::read_to_string(&p) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
                return Some(v);
            }
        }
    }
    None
}

/// Enumerate installed mods (folders under mods/ that contain main.lua),
/// merged with the current enabled state.
#[tauri::command]
pub fn launcher_list_mods() -> Result<Vec<ModDto>, String> {
    let Some(root) = core::mods_dir() else {
        return Ok(Vec::new());
    };
    let enabled = read_enabled();
    let is_enabled = |id: &str| match &enabled {
        Some(ids) => ids.iter().any(|e| e == id),
        None => true, // absent enabled.json ⇒ everything on (back-compat)
    };

    let mut out = Vec::new();
    let entries = match std::fs::read_dir(&root) {
        Ok(e) => e,
        Err(_) => return Ok(Vec::new()), // no mods dir yet — not an error
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if !path.join("main.lua").is_file() {
            continue;
        }
        let id = match path.file_name().and_then(|n| n.to_str()) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => continue,
        };
        let m = read_manifest(&path);
        let getstr = |key: &str| {
            m.as_ref()
                .and_then(|v| v.get(key))
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
        };
        out.push(ModDto {
            name: getstr("name").unwrap_or_else(|| id.clone()),
            version: getstr("version"),
            author: getstr("author"),
            description: getstr("description"),
            enabled: is_enabled(&id),
            id,
        });
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}

/// Persist the enabled-mod set to mods/enabled.json. The loader reads this
/// on next launch (absent file ⇒ all mods load).
#[tauri::command]
pub fn launcher_set_enabled_mods(ids: Vec<String>) -> Result<(), String> {
    let dir = core::mods_dir().ok_or("LOCALAPPDATA not set")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("create mods dir: {e}"))?;
    let path = enabled_json_path().ok_or("LOCALAPPDATA not set")?;
    let body = serde_json::json!({ "enabled": ids });
    let json = serde_json::to_string_pretty(&body).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| format!("write enabled.json: {e}"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Launch
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct LaunchResult {
    pub pid: u32,
    pub log: Vec<String>,
}

/// Resolve the loader DLL the launcher should inject. Prefers an explicit
/// TURDMOD_LOADER_DLL, else the lib's default resolution (next to the
/// launcher binary / dev target).
fn resolve_loader_dll() -> Result<PathBuf, String> {
    // 1) explicit env override (dev convenience)
    if let Ok(p) = std::env::var("TURDMOD_LOADER_DLL") {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Ok(pb);
        }
    }
    // 2) bundled resource: Tauri unpacks bundle.resources into a `resources/`
    // dir next to the exe. Check there for the packaged-install case.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for cand in [
                dir.join("resources").join("turdmod_loader.dll"),
                dir.join("turdmod_loader.dll"),
            ] {
                if cand.is_file() {
                    return Ok(cand);
                }
            }
        }
    }
    // 3) dev fallback: core's next-to-exe / one-up search.
    core::resolve_dll(None)
}

/// Launch SCUM into the selected BE-off server with the loader injected.
/// The allowlist gate lives in core::launch — passing a BE-on server is
/// rejected there, not just in the UI.
#[tauri::command]
pub async fn launcher_launch_modded(server_id: String) -> Result<LaunchResult, String> {
    // Re-fetch (cache-backed) so we always launch against the current,
    // verified-BE-off server record — never a stale UI value.
    let servers = launcher_list_servers().await?;
    let server = servers
        .into_iter()
        .find(|s| s.id == server_id)
        .ok_or_else(|| format!("server '{server_id}' not in allowlist"))?;

    let scum = core::resolve_scum(None)?;
    let dll = resolve_loader_dll()?;

    let opts = core::LaunchOptions {
        scum_exe: scum,
        dll,
        extra_dlls: Vec::new(),
        game_args: Vec::new(),
        server: Some(core::ServerTarget {
            id: server.id,
            name: server.name,
            ip: server.ip,
            port: server.port,
            battle_eye: server.battle_eye,
        }),
        skip_safety_check: false,
    };

    let mut log: Vec<String> = Vec::new();
    let pid = core::launch(&opts, &mut |line| log.push(line.to_string()))?;
    Ok(LaunchResult { pid, log })
}

/// Real join progress, read from SCUM's own client log. SCUM rotates the log
/// on each launch (old one is backed up), so the current SCUM.log is this
/// session's. We scan for verified milestone lines and report the furthest one
/// reached — so the launcher's loading beam tracks the ACTUAL server load, not
/// a fake timer. Markers confirmed live earlier this session.
#[derive(Debug, Serialize)]
pub struct JoinProgress {
    pub pct: u32,          // 0..100 for the beam fill
    pub label: String,     // human stage text
    pub done: bool,        // reached the world
    pub error: Option<String>,
}

fn scum_log_path() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    let mut p = PathBuf::from(local);
    p.push("SCUM");
    p.push("Saved");
    p.push("Logs");
    p.push("SCUM.log");
    Some(p)
}

#[tauri::command]
pub fn launcher_join_progress() -> JoinProgress {
    let text = scum_log_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default()
        .to_lowercase();

    // Error first — version mismatch is the known hard failure.
    if text.contains("neterrorwrongversion") {
        return JoinProgress {
            pct: 100,
            label: "Version mismatch".into(),
            done: false,
            error: Some(
                "Server/client build mismatch (NetErrorWrongVersion). Update SCUM or the server."
                    .into(),
            ),
        };
    }
    // Furthest real milestone wins. Lines verified live (2026-05-31).
    if text.contains("handlepossessedby") {
        return JoinProgress { pct: 100, label: "In world".into(), done: true, error: None };
    }
    if text.contains("welcomed by server") {
        return JoinProgress { pct: 92, label: "Entering world".into(), done: false, error: None };
    }
    if text.contains("game version:") {
        // Booted into the client; the connect handshake runs after this.
        return JoinProgress { pct: 60, label: "Connecting to server".into(), done: false, error: None };
    }
    // Process is up but the log hasn't hit the boot line yet.
    JoinProgress { pct: 25, label: "Booting SCUM".into(), done: false, error: None }
}

/// True if a process with `pid` is still running. The UI polls this on the
/// launched SCUM pid: when the GAME PROCESS exits, the launcher closes; just
/// disconnecting from a server leaves SCUM.exe running, so the pid stays
/// alive and the launcher stays open. Uses `tasklist /FI "PID eq <pid>"` —
/// no extra crates, matches the BE pre-flight's approach.
#[tauri::command]
pub fn launcher_pid_alive(pid: u32) -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    match std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        // tasklist prints the process row if found; "No tasks" (or empty) if
        // not. Confirm the pid actually appears in the output.
        Ok(o) => String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()),
        // Can't tell — report ALIVE so we never close the launcher on a
        // transient tasklist hiccup (fail-safe for "stays open").
        Err(_) => true,
    }
}
