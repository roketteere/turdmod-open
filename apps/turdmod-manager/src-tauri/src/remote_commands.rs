// Tauri commands for remote service API (turdmod-service on OVH).

use crate::remote::{self, RemoteClient, RemoteConfig};
use crate::tunnel::{self, TunnelConfig};

#[tauri::command]
pub async fn remote_get_config() -> Result<Option<RemoteConfig>, String> {
    Ok(RemoteConfig::load())
}

#[tauri::command]
pub async fn remote_set_config(
    host: String,
    port: u16,
    token: String,
    enabled: bool,
) -> Result<(), String> {
    let cfg = RemoteConfig { host, port, token, enabled };
    remote::save_config(&cfg).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remote_health() -> Result<serde_json::Value, String> {
    let client = RemoteClient::from_config().map_err(|e| e.to_string())?;
    client.health().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remote_status() -> Result<serde_json::Value, String> {
    let client = RemoteClient::from_config().map_err(|e| e.to_string())?;
    client.status().await.map_err(|e| e.to_string())
}

// Server lifecycle — target-aware ("local" = local pipe, else OVH), like the
// monitor_* commands, so the Admin Map's host toggle drives which box we act on.
#[tauri::command]
pub async fn remote_server_start(target: String) -> Result<serde_json::Value, String> {
    client_for(&target)?.start().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remote_server_stop(target: String) -> Result<serde_json::Value, String> {
    client_for(&target)?.stop().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remote_server_restart(
    target: String,
    countdown: Option<u32>,
) -> Result<serde_json::Value, String> {
    client_for(&target)?.restart(countdown).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remote_server_status(target: String) -> Result<serde_json::Value, String> {
    client_for(&target)?.status().await.map_err(|e| e.to_string())
}

// Stage BattlEye on/off for the target host (applies on next start/restart).
#[tauri::command]
pub async fn remote_server_battleye(target: String, enabled: bool) -> Result<serde_json::Value, String> {
    client_for(&target)?.set_battleye(enabled).await.map_err(|e| e.to_string())
}

// Admin config files (AdminUsers.ini / ServerSettingsAdminUsers.ini) on the target host.
// Read raw .ini text and write it back via the service's allowlisted /admin/file endpoint.
#[tauri::command]
pub async fn remote_admin_file_get(target: String, name: String) -> Result<serde_json::Value, String> {
    client_for(&target)?.get_admin_file(&name).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remote_admin_file_set(target: String, name: String, contents: String) -> Result<serde_json::Value, String> {
    client_for(&target)?.write_admin_file(&name, &contents).await.map_err(|e| e.to_string())
}

// TurdMOD data JSON files (safe_zones / vehicle_ownership / repo_history / vehicle_durability)
// via the service's /data/file endpoint. Backs the Vehicle Manager + Safe Zones tabs.
#[tauri::command]
pub async fn remote_data_file_get(target: String, name: String) -> Result<serde_json::Value, String> {
    client_for(&target)?.get_data_file(&name).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remote_data_file_set(target: String, name: String, contents: String) -> Result<serde_json::Value, String> {
    client_for(&target)?.write_data_file(&name, &contents).await.map_err(|e| e.to_string())
}

// All server vehicles (id / sector / coords / owner / lock / parts) + manual repossession.
#[tauri::command]
pub async fn remote_vehicles_detail(target: String) -> Result<serde_json::Value, String> {
    client_for(&target)?.vehicles_detail().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remote_vehicles_repo(target: String, vehicles: serde_json::Value) -> Result<serde_json::Value, String> {
    client_for(&target)?.vehicles_repo(vehicles).await.map_err(|e| e.to_string())
}

// Permissions page — live tier ladder + per-player + per-command overrides.
#[tauri::command]
pub async fn remote_permissions_get(target: String) -> Result<serde_json::Value, String> {
    client_for(&target)?.permissions_get().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remote_permissions_set(target: String, state: serde_json::Value) -> Result<serde_json::Value, String> {
    client_for(&target)?.permissions_set(state).await.map_err(|e| e.to_string())
}

// Recurring restart schedule (get / set).
#[tauri::command]
pub async fn remote_server_schedule_get(target: String) -> Result<serde_json::Value, String> {
    client_for(&target)?.get_schedule().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remote_server_schedule_set(target: String, body: serde_json::Value) -> Result<serde_json::Value, String> {
    client_for(&target)?.set_schedule(body).await.map_err(|e| e.to_string())
}

// Elevated (dev-command) users (get / set).
#[tauri::command]
pub async fn remote_elevated_get(target: String) -> Result<serde_json::Value, String> {
    client_for(&target)?.get_elevated().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remote_elevated_set(target: String, body: serde_json::Value) -> Result<serde_json::Value, String> {
    client_for(&target)?.set_elevated(body).await.map_err(|e| e.to_string())
}

// Loot customization files (list / read / write Override).
#[tauri::command]
pub async fn remote_loot_files(target: String) -> Result<serde_json::Value, String> {
    client_for(&target)?.get_loot_files().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remote_loot_file_get(target: String, path: String) -> Result<serde_json::Value, String> {
    client_for(&target)?.get_loot_file(&path).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remote_loot_file_set(target: String, body: serde_json::Value) -> Result<serde_json::Value, String> {
    client_for(&target)?.set_loot_file(body).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remote_server_logs(lines: Option<usize>) -> Result<serde_json::Value, String> {
    let client = RemoteClient::from_config().map_err(|e| e.to_string())?;
    client.logs(lines.unwrap_or(100)).await.map_err(|e| e.to_string())
}

// Read a text file from OVH over SSH (reuses the tunnel's host/user/key). The
// service has no file endpoint, so config files (ServerSettings.ini) are read
// directly. Read-only by design — writing OVH config should go through the
// controlled stop→edit→start deploy, not a live overwrite.
#[tauri::command(rename_all = "camelCase")]
pub async fn read_remote_text_file(path: String) -> Result<crate::commands::ReadTextResult, String> {
    let cfg = TunnelConfig::load();
    let target = format!("{}@{}", cfg.ssh_user, cfg.ssh_host);
    // Path is single-quoted inside the -Command string; SCUM config paths carry
    // no spaces/quotes. '' escapes any literal single-quote defensively.
    let remote = format!(
        "powershell -NoProfile -Command \"if (Test-Path -LiteralPath '{p}') {{ Get-Content -LiteralPath '{p}' -Raw }}\"",
        p = path.replace('\'', "''"),
    );
    let out = tokio::process::Command::new("ssh")
        .args([
            "-o", "StrictHostKeyChecking=accept-new",
            "-o", "ConnectTimeout=15",
            "-i", &cfg.ssh_key,
            &target,
            &remote,
        ])
        .output()
        .await
        .map_err(|e| format!("spawn ssh: {}", e))?;
    if !out.status.success() {
        return Err(format!("ssh read failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let content = String::from_utf8_lossy(&out.stdout).to_string();
    let existed = !content.trim().is_empty();
    Ok(crate::commands::ReadTextResult { content, existed })
}

#[tauri::command]
pub async fn remote_engine_rpc(
    method: String,
    params: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let client = RemoteClient::from_config().map_err(|e| e.to_string())?;
    client.engine_rpc(&method, params).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remote_scumpilot_status() -> Result<serde_json::Value, String> {
    let client = RemoteClient::from_config().map_err(|e| e.to_string())?;
    client.scumpilot_status().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remote_scumpilot_pause() -> Result<serde_json::Value, String> {
    let client = RemoteClient::from_config().map_err(|e| e.to_string())?;
    client.scumpilot_pause().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remote_scumpilot_resume() -> Result<serde_json::Value, String> {
    let client = RemoteClient::from_config().map_err(|e| e.to_string())?;
    client.scumpilot_resume().await.map_err(|e| e.to_string())
}

// ─── Monitoring + control (Live page). `target` = "local" or "remote" (OVH). ───
fn client_for(target: &str) -> Result<RemoteClient, String> {
    RemoteClient::for_target(target).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn monitor_mods(target: String) -> Result<serde_json::Value, String> {
    client_for(&target)?.monitor_mods().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn monitor_activity(target: String, limit: Option<usize>) -> Result<serde_json::Value, String> {
    client_for(&target)?.monitor_activity(limit.unwrap_or(100)).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn monitor_system(target: String) -> Result<serde_json::Value, String> {
    client_for(&target)?.monitor_system().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn monitor_players(target: String) -> Result<serde_json::Value, String> {
    client_for(&target)?.monitor_players().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn control_mod(target: String, name: String, state: String) -> Result<serde_json::Value, String> {
    client_for(&target)?.control_mod(&name, &state).await.map_err(|e| e.to_string())
}

// ─── SCUM.db viewer (local or remote) ───
#[tauri::command]
pub async fn scumdb_tables(target: String) -> Result<serde_json::Value, String> {
    client_for(&target)?.scumdb_tables().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn scumdb_schema(target: String, table: String) -> Result<serde_json::Value, String> {
    client_for(&target)?.scumdb_schema(&table).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn scumdb_rows(target: String, table: String, limit: Option<usize>, offset: Option<usize>) -> Result<serde_json::Value, String> {
    client_for(&target)?.scumdb_rows(&table, limit.unwrap_or(100), offset.unwrap_or(0)).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn scumdb_player_card(target: String, steam: String) -> Result<serde_json::Value, String> {
    client_for(&target)?.player_card(&steam).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn scumdb_player_inventory(target: String, steam: String) -> Result<serde_json::Value, String> {
    client_for(&target)?.player_inventory(&steam).await.map_err(|e| e.to_string())
}

// Save-edit a player's skills (level/xp). The service stops SCUM, backs up scum.db, writes, restarts.
#[tauri::command]
pub async fn scumdb_set_skill(target: String, steam: String, skills: serde_json::Value) -> Result<serde_json::Value, String> {
    client_for(&target)?.set_skill(&steam, skills).await.map_err(|e| e.to_string())
}

// Save-edit a player's base attributes (body_simulation blob). Same safe cycle as set_skill.
#[tauri::command]
pub async fn scumdb_set_attribute(target: String, steam: String, attributes: serde_json::Value) -> Result<serde_json::Value, String> {
    client_for(&target)?.set_attribute(&steam, attributes).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn scumdb_map_vehicles(target: String) -> Result<serde_json::Value, String> {
    client_for(&target)?.map_vehicles().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn scumdb_last_positions(target: String) -> Result<serde_json::Value, String> {
    client_for(&target)?.last_positions().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn scumdb_map_zones(target: String) -> Result<serde_json::Value, String> {
    client_for(&target)?.map_zones().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn scumdb_traders(target: String) -> Result<serde_json::Value, String> {
    client_for(&target)?.scumdb_traders().await.map_err(|e| e.to_string())
}

// ─── SSH tunnel to OVH (so "remote" monitoring reaches its localhost:9090) ───
#[tauri::command]
pub async fn tunnel_get_config() -> Result<TunnelConfig, String> { Ok(TunnelConfig::load()) }

#[tauri::command]
pub async fn tunnel_set_config(ssh_host: String, ssh_user: String, ssh_key: String, local_port: u16, remote_port: u16) -> Result<(), String> {
    TunnelConfig { ssh_host, ssh_user, ssh_key, local_port, remote_port }.save()
}

#[tauri::command]
pub async fn tunnel_start() -> Result<(), String> { tunnel::start(&TunnelConfig::load()) }

// Ensure the OVH tunnel is up so "remote" controls/monitor reach OVH without a manual
// start. Probes the tunnel's local /health and only (re)starts if it's actually down —
// avoids needlessly killing an already-working tunnel (e.g. one this Manager didn't spawn).
// Returns true if it was already up, false if a fresh tunnel was started.
#[tauri::command]
pub async fn tunnel_ensure() -> Result<bool, String> {
    let cfg = TunnelConfig::load();
    let url = format!("http://127.0.0.1:{}/health", cfg.local_port);
    let up = reqwest::Client::new()
        .get(&url)
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false);
    if up {
        return Ok(true);
    }
    tunnel::start(&cfg)?;
    Ok(false)
}

#[tauri::command]
pub async fn tunnel_stop() -> Result<(), String> { tunnel::stop(); Ok(()) }

#[tauri::command]
pub async fn tunnel_status() -> Result<bool, String> { Ok(tunnel::is_running()) }
