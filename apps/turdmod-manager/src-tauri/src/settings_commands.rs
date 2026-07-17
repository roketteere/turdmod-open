// Tauri command surface for settings.
// The RCON password is never returned to the frontend; only `hasRconPassword`
// (bool) is surfaced so the UI can show "••••••" or "not set".

use serde::Serialize;
use tauri::{AppHandle, Runtime, State};
use tokio::net::TcpStream;
use tokio::time::{timeout, Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::settings::{self, ManagerSettings, SettingsPatch, SettingsState};

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

// ---------------------------------------------------------------------------
// Return type for settings_get_all
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPayload {
    pub scum_server_logs_dir: Option<String>,
    pub rcon_host: Option<String>,
    pub rcon_port: Option<u16>,
    pub discord_webhook: Option<String>,
    pub has_rcon_password: bool,
    pub wizard_completed: bool,
}

fn to_payload<R: tauri::Runtime>(app: &AppHandle<R>, s: &ManagerSettings) -> SettingsPayload {
    SettingsPayload {
        scum_server_logs_dir: s
            .scum_server_logs_dir
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned()),
        rcon_host: s.rcon_host.clone(),
        rcon_port: s.rcon_port,
        discord_webhook: s.discord_webhook.clone(),
        has_rcon_password: settings::has_rcon_password(app),
        wizard_completed: s.wizard_completed,
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeDetectResult {
    pub found: bool,
    pub version: Option<String>,
}

#[tauri::command(rename_all = "camelCase")]
pub async fn manager_detect_node() -> NodeDetectResult {
    let result = timeout(
        Duration::from_secs(5),
        tokio::process::Command::new("node")
            .arg("--version")
            .output(),
    )
    .await;

    match result {
        Ok(Ok(output)) if output.status.success() => {
            let v = String::from_utf8_lossy(&output.stdout).trim().to_string();
            NodeDetectResult {
                found: true,
                version: if v.is_empty() { None } else { Some(v) },
            }
        }
        _ => NodeDetectResult { found: false, version: None },
    }
}

// ---------------------------------------------------------------------------
// Test result types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogsDirTestResult {
    pub exists: bool,
    pub has_log_files: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RconTestResult {
    pub ok: bool,
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command(rename_all = "camelCase")]
pub fn settings_get_all<R: Runtime>(
    app: AppHandle<R>,
    _state: State<'_, SettingsState>,
) -> SettingsPayload {
    let s = settings::load_settings(&app);
    to_payload(&app, &s)
}

#[tauri::command(rename_all = "camelCase")]
pub fn settings_set_partial<R: Runtime>(
    app: AppHandle<R>,
    _state: State<'_, SettingsState>,
    patch: SettingsPatch,
) -> Result<SettingsPayload, String> {
    let updated = settings::apply_patch(&app, patch)?;
    Ok(to_payload(&app, &updated))
}

#[tauri::command(rename_all = "camelCase")]
pub fn settings_test_logs_dir(path: String) -> LogsDirTestResult {
    let p = std::path::Path::new(&path);
    if !p.exists() {
        return LogsDirTestResult {
            exists: false,
            has_log_files: false,
        };
    }
    let has_log_files = std::fs::read_dir(p)
        .map(|entries| {
            entries.filter_map(|e| e.ok()).any(|e| {
                e.path()
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext.eq_ignore_ascii_case("log"))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);
    LogsDirTestResult {
        exists: true,
        has_log_files,
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn settings_test_rcon(
    host: String,
    port: u16,
    password: String,
) -> RconTestResult {
    match probe_rcon_connection(&host, port, &password).await {
        Ok(()) => RconTestResult { ok: true, error: None },
        Err(e) => RconTestResult { ok: false, error: Some(e) },
    }
}

// ---------------------------------------------------------------------------
// Minimal RCON probe (mirrors server.rs's approach)
// ---------------------------------------------------------------------------

const RCON_AUTH: i32 = 3;
const RCON_EXEC: i32 = 2;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
const RCON_TIMEOUT: Duration = Duration::from_secs(5);

async fn probe_rcon_connection(host: &str, port: u16, password: &str) -> Result<(), String> {
    let addr = format!("{}:{}", host, port);
    let mut stream = timeout(CONNECT_TIMEOUT, TcpStream::connect(&addr))
        .await
        .map_err(|_| format!("connection timed out to {}", addr))?
        .map_err(|e| format!("connection failed: {}", e))?;

    write_rcon_packet(&mut stream, 1, RCON_AUTH, password.as_bytes())
        .await
        .map_err(|e| format!("write auth packet: {}", e))?;

    let resp = timeout(RCON_TIMEOUT, read_rcon_packet(&mut stream))
        .await
        .map_err(|_| "auth response timed out".to_string())?
        .map_err(|e| format!("read auth response: {}", e))?;

    if resp == -1 {
        return Err("RCON auth rejected (wrong password?)".to_string());
    }

    // Send a no-op command to confirm the connection fully works.
    write_rcon_packet(&mut stream, 2, RCON_EXEC, b"Echo TurdMODProbe")
        .await
        .map_err(|e| format!("write probe command: {}", e))?;

    timeout(RCON_TIMEOUT, read_rcon_packet(&mut stream))
        .await
        .map_err(|_| "probe response timed out".to_string())?
        .map_err(|e| format!("read probe response: {}", e))?;

    Ok(())
}

async fn write_rcon_packet(
    stream: &mut TcpStream,
    id: i32,
    kind: i32,
    body: &[u8],
) -> Result<(), String> {
    let size: i32 = (4 + 4 + body.len() + 2) as i32;
    let mut buf = Vec::with_capacity(size as usize + 4);
    buf.extend_from_slice(&size.to_le_bytes());
    buf.extend_from_slice(&id.to_le_bytes());
    buf.extend_from_slice(&kind.to_le_bytes());
    buf.extend_from_slice(body);
    buf.push(0);
    buf.push(0);
    stream
        .write_all(&buf)
        .await
        .map_err(|e| e.to_string())
}

/// Returns the packet id (or -1 for RCON auth failure).
async fn read_rcon_packet(stream: &mut TcpStream) -> Result<i32, String> {
    let mut size_buf = [0u8; 4];
    stream
        .read_exact(&mut size_buf)
        .await
        .map_err(|e| format!("read size: {}", e))?;
    let size = i32::from_le_bytes(size_buf);
    if !(10..=4096).contains(&size) {
        return Err(format!("rcon packet size out of range: {}", size));
    }
    let mut rest = vec![0u8; size as usize];
    stream
        .read_exact(&mut rest)
        .await
        .map_err(|e| format!("read body: {}", e))?;
    let id = i32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]);
    Ok(id)
}