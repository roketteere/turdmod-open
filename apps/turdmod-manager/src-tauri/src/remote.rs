// Remote service HTTP client — talks to turdmod-service on remote server (or any host).
// Config loaded from %LOCALAPPDATA%\TurdMOD\remote.json.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteConfig {
    pub host: String,
    pub port: u16,
    pub token: String,
    #[serde(default)]
    pub enabled: bool,
}

impl RemoteConfig {
    pub fn load() -> Option<Self> {
        let path = config_path();
        let raw = std::fs::read_to_string(&path).ok()?;
        // strip a UTF-8 BOM (PowerShell's Set-Content -Encoding utf8 adds one; serde chokes on it)
        let cfg: Self = serde_json::from_str(raw.trim_start_matches('\u{feff}')).ok()?;
        if cfg.enabled { Some(cfg) } else { None }
    }

    /// Load ignoring `enabled` (so the tunnel can repoint host/port without losing the token).
    pub fn load_raw() -> Option<Self> {
        let raw = std::fs::read_to_string(config_path()).ok()?;
        serde_json::from_str(raw.trim_start_matches('\u{feff}')).ok()
    }

    pub fn base_url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }
}

fn config_path() -> PathBuf {
    let appdata = std::env::var("LOCALAPPDATA")
        .unwrap_or_else(|_| r"C:\Users\Default\AppData\Local".into());
    PathBuf::from(appdata).join("TurdMOD").join("remote.json")
}

pub struct RemoteClient {
    cfg: RemoteConfig,
    http: reqwest::Client,
}

impl RemoteClient {
    pub fn new(cfg: RemoteConfig) -> Self {
        Self {
            cfg,
            http: reqwest::Client::new(),
        }
    }

    pub fn from_config() -> Result<Self> {
        let cfg = RemoteConfig::load().context("remote.json not found or disabled")?;
        Ok(Self::new(cfg))
    }

    /// Build a client for a named server: "local" reads C:\TurdMOD\service.json (localhost); any
    /// other value uses the configured remote (remote server, remote.json). The control plane for both boxes.
    pub fn for_target(target: &str) -> Result<Self> {
        if target == "local" {
            let raw = std::fs::read_to_string(r"C:\TurdMOD\service.json")
                .context("local service.json not found")?;
            let v: serde_json::Value = serde_json::from_str(&raw).context("parse local service.json")?;
            let port = v.get("port").and_then(|p| p.as_u64()).unwrap_or(9090) as u16;
            let token = v.get("token").and_then(|t| t.as_str()).unwrap_or_default().to_string();
            Ok(Self::new(RemoteConfig { host: "127.0.0.1".into(), port, token, enabled: true }))
        } else {
            Self::from_config()
        }
    }

    pub async fn monitor_mods(&self) -> Result<serde_json::Value> { self.authed_get("/monitor/mods").await }
    pub async fn monitor_activity(&self, limit: usize) -> Result<serde_json::Value> {
        self.authed_get(&format!("/monitor/activity?limit={}", limit)).await
    }
    pub async fn monitor_system(&self) -> Result<serde_json::Value> { self.authed_get("/monitor/system").await }
    pub async fn monitor_players(&self) -> Result<serde_json::Value> { self.authed_get("/monitor/players").await }
    // SCUM.db viewer — list tables, per-table columns, paged rows. Read-only.
    pub async fn scumdb_tables(&self) -> Result<serde_json::Value> { self.authed_get("/scumdb/tables").await }
    pub async fn scumdb_schema(&self, table: &str) -> Result<serde_json::Value> {
        self.authed_get(&format!("/scumdb/schema?table={}", table)).await
    }
    pub async fn scumdb_rows(&self, table: &str, limit: usize, offset: usize) -> Result<serde_json::Value> {
        self.authed_get(&format!("/scumdb/rows?table={}&limit={}&offset={}", table, limit, offset)).await
    }
    pub async fn player_card(&self, steam: &str) -> Result<serde_json::Value> {
        self.authed_get(&format!("/scumdb/player-card?steam={}", steam)).await
    }
    pub async fn player_inventory(&self, steam: &str) -> Result<serde_json::Value> {
        self.authed_get(&format!("/scumdb/player-inventory?steam={}", steam)).await
    }
    pub async fn map_vehicles(&self) -> Result<serde_json::Value> {
        self.authed_get("/scumdb/map-vehicles").await
    }
    pub async fn last_positions(&self) -> Result<serde_json::Value> {
        self.authed_get("/scumdb/last-positions").await
    }
    pub async fn map_zones(&self) -> Result<serde_json::Value> {
        self.authed_get("/scumdb/map-zones").await
    }
    pub async fn scumdb_traders(&self) -> Result<serde_json::Value> {
        self.authed_get("/scumdb/traders").await
    }
    pub async fn control_mod(&self, name: &str, state: &str) -> Result<serde_json::Value> {
        self.authed_post("/control/mod", Some(serde_json::json!({ "name": name, "state": state }))).await
    }

    // Admin config files (AdminUsers.ini, ServerSettingsAdminUsers.ini, …) via the
    // service's /admin/file endpoint (allowlisted server-side). Read raw text + write back.
    pub async fn get_admin_file(&self, name: &str) -> Result<serde_json::Value> {
        self.authed_get(&format!("/admin/file?name={}", name)).await
    }
    pub async fn write_admin_file(&self, name: &str, contents: &str) -> Result<serde_json::Value> {
        self.authed_post("/admin/file", Some(serde_json::json!({ "name": name, "contents": contents }))).await
    }
    // TurdMOD data files (C:\TurdMOD\data\*.json) — backs the Vehicle Manager + Safe Zones tabs.
    pub async fn get_data_file(&self, name: &str) -> Result<serde_json::Value> {
        self.authed_get(&format!("/data/file?name={}", name)).await
    }
    pub async fn write_data_file(&self, name: &str, contents: &str) -> Result<serde_json::Value> {
        self.authed_post("/data/file", Some(serde_json::json!({ "name": name, "contents": contents }))).await
    }
    pub async fn vehicles_detail(&self) -> Result<serde_json::Value> {
        self.authed_get("/scumdb/vehicles-detail").await
    }
    pub async fn vehicles_repo(&self, vehicles: serde_json::Value) -> Result<serde_json::Value> {
        self.authed_post("/vehicles/repo", Some(serde_json::json!({ "vehicles": vehicles }))).await
    }
    // Permissions — read/write the live SharedPerms (tiers + per-command overrides).
    pub async fn permissions_get(&self) -> Result<serde_json::Value> {
        self.authed_get("/permissions").await
    }
    pub async fn permissions_set(&self, state: serde_json::Value) -> Result<serde_json::Value> {
        self.authed_post("/permissions", Some(state)).await
    }

    // Save-edit a player's skill levels/XP (stop → backup → DB write → restart, server-side).
    pub async fn set_skill(&self, steam: &str, skills: serde_json::Value) -> Result<serde_json::Value> {
        self.authed_post("/scumdb/set-skill", Some(serde_json::json!({ "steam": steam, "skills": skills }))).await
    }

    // Save-edit a player's base attributes (body_simulation blob); same safe cycle.
    pub async fn set_attribute(&self, steam: &str, attributes: serde_json::Value) -> Result<serde_json::Value> {
        self.authed_post("/scumdb/set-attribute", Some(serde_json::json!({ "steam": steam, "attributes": attributes }))).await
    }

    pub async fn health(&self) -> Result<serde_json::Value> {
        let resp = self.http
            .get(format!("{}/health", self.cfg.base_url()))
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
            .context("health request")?;
        resp.json().await.context("health json")
    }

    pub async fn status(&self) -> Result<serde_json::Value> {
        self.authed_get("/status").await
    }

    pub async fn start(&self) -> Result<serde_json::Value> {
        self.authed_post("/server/start", None).await
    }

    pub async fn stop(&self) -> Result<serde_json::Value> {
        self.authed_post("/server/stop", None).await
    }

    pub async fn restart(&self, countdown: Option<u32>) -> Result<serde_json::Value> {
        let path = match countdown {
            Some(c) if c > 0 => format!("/server/restart?countdown={}", c),
            _ => "/server/restart".to_string(),
        };
        self.authed_post(&path, None).await
    }

    pub async fn logs(&self, lines: usize) -> Result<serde_json::Value> {
        self.authed_get(&format!("/server/logs?lines={}", lines)).await
    }

    /// Stage BattlEye on/off (applies on the next start/restart).
    pub async fn set_battleye(&self, enabled: bool) -> Result<serde_json::Value> {
        self.authed_post("/server/battleye", Some(serde_json::json!({ "enabled": enabled }))).await
    }

    /// Get / set the recurring restart schedule.
    pub async fn get_schedule(&self) -> Result<serde_json::Value> {
        self.authed_get("/server/schedule").await
    }
    pub async fn set_schedule(&self, body: serde_json::Value) -> Result<serde_json::Value> {
        self.authed_post("/server/schedule", Some(body)).await
    }

    /// Get / set the elevated (dev-command) users for SCUM.db `elevated_users`.
    pub async fn get_elevated(&self) -> Result<serde_json::Value> {
        self.authed_get("/scumdb/elevated").await
    }
    pub async fn set_elevated(&self, body: serde_json::Value) -> Result<serde_json::Value> {
        self.authed_post("/scumdb/elevated", Some(body)).await
    }

    /// Loot customization files: list / read (Default or Override) / write (Override only).
    pub async fn get_loot_files(&self) -> Result<serde_json::Value> {
        self.authed_get("/loot/files").await
    }
    pub async fn get_loot_file(&self, path: &str) -> Result<serde_json::Value> {
        self.authed_get(&format!("/loot/file?path={}", path)).await
    }
    pub async fn set_loot_file(&self, body: serde_json::Value) -> Result<serde_json::Value> {
        self.authed_post("/loot/file", Some(body)).await
    }

    pub async fn engine_rpc(&self, method: &str, params: Option<serde_json::Value>) -> Result<serde_json::Value> {
        let body = serde_json::json!({ "method": method, "params": params });
        let resp = self.authed_post("/engine/rpc", Some(body)).await?;
        if let Some(err) = resp.get("error") {
            anyhow::bail!("rpc error: {}", err);
        }
        Ok(resp.get("result").cloned().unwrap_or(serde_json::Value::Null))
    }

    // scumpilot (AI admin) control — proxied by the service at /scumpilot/*.
    pub async fn scumpilot_status(&self) -> Result<serde_json::Value> {
        self.authed_get("/scumpilot/status").await
    }
    pub async fn scumpilot_pause(&self) -> Result<serde_json::Value> {
        self.authed_post("/scumpilot/pause", None).await
    }
    pub async fn scumpilot_resume(&self) -> Result<serde_json::Value> {
        self.authed_post("/scumpilot/resume", None).await
    }

    async fn authed_get(&self, path: &str) -> Result<serde_json::Value> {
        let resp = self.http
            .get(format!("{}{}", self.cfg.base_url(), path))
            .bearer_auth(&self.cfg.token)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .context("request failed")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("HTTP {} — {}", status, body);
        }
        resp.json().await.context("json parse")
    }

    async fn authed_post(&self, path: &str, body: Option<serde_json::Value>) -> Result<serde_json::Value> {
        let mut req = self.http
            .post(format!("{}{}", self.cfg.base_url(), path))
            .bearer_auth(&self.cfg.token)
            .timeout(std::time::Duration::from_secs(60));

        if let Some(b) = body {
            req = req.json(&b);
        }

        let resp = req.send().await.context("request failed")?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("HTTP {} — {}", status, body);
        }
        resp.json().await.context("json parse")
    }
}

pub fn save_config(cfg: &RemoteConfig) -> Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let json = serde_json::to_string_pretty(cfg)?;
    std::fs::write(&path, json).context("write remote.json")?;
    Ok(())
}
