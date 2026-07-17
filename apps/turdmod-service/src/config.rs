// Service config — loads C:\TurdMOD\service.json (or per-instance variant).
// Defines port, token, SCUM exe path, DLL paths, auto-restart policy.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_instance")]
    pub instance_id: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub token: String,
    pub scum_server_exe: String,
    #[serde(default)]
    pub scum_server_args: Vec<String>,
    #[serde(default)]
    pub inject_dlls: Vec<String>,
    #[serde(default = "default_true")]
    pub auto_restart: bool,
    #[serde(default = "default_restart_delay")]
    pub restart_delay_secs: u64,
    #[serde(default = "default_scumdb_path")]
    pub scumdb_path: String,
    #[serde(default)]
    pub scummap_api_url: Option<String>,
    #[serde(default)]
    pub scummap_api_token: Option<String>,
    #[serde(default)]
    pub scummap_map_id: Option<String>,
    /// Synthetic population floor for the map snapshot (/map/players): pad the REPORTED
    /// count + markers up to this many players with stable dummy entries. 0 = off.
    /// @inv: does NOT touch the operational player list — commands/welcome/spa/economy
    /// all key off the real getOnlinePlayers, so phantoms never break gameplay logic.
    #[serde(default)]
    pub phantom_population: usize,
}

fn default_instance() -> String { "default".into() }
fn default_port() -> u16 { 9090 }
fn default_true() -> bool { true }
fn default_restart_delay() -> u64 { 10 }
fn default_scumdb_path() -> String { r"C:\SCUMServer\SCUM\Saved\SaveFiles\SCUM.db".into() }

pub fn config_path(instance: &str) -> PathBuf {
    match instance {
        "default" => PathBuf::from(r"C:\TurdMOD\service.json"),
        id => PathBuf::from(format!(r"C:\TurdMOD\service-{}.json", id)),
    }
}

impl Config {
    pub fn load(instance: &str) -> Result<Self> {
        let path = config_path(instance);
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("read config: {}", path.display()))?;
        let mut cfg: Self = serde_json::from_str(&raw).context("parse config json")?;
        cfg.instance_id = instance.to_string();
        Ok(cfg)
    }

    pub fn load_or_default(instance: &str) -> Self {
        Self::load(instance).unwrap_or_else(|e| {
            tracing::warn!("config load failed ({}), using defaults", e);
            Self {
                instance_id: instance.to_string(),
                port: 9090,
                token: "changeme-alpha-token".into(),
                scum_server_exe: r"C:\SCUMServer\SCUM\Binaries\Win64\GameServer.exe".into(),
                scum_server_args: vec![
                    "-log".into(),
                    "-port=7042".into(),
                    "-QueryPort=7044".into(),
                    "-NoBattlEye".into(),
                ],
                inject_dlls: vec![
                    r"C:\SCUMServer\SCUM\Binaries\Win64\turdmod_server_loader.dll".into(),
                    r"C:\SCUMServer\SCUM\Binaries\Win64\UE4SS\UE4SS.dll".into(),
                ],
                auto_restart: true,
                restart_delay_secs: 10,
                scumdb_path: default_scumdb_path(),
                scummap_api_url: None,
                scummap_api_token: None,
                scummap_map_id: None,
                phantom_population: 0,
            }
        })
    }

    /// Re-read just `scum_server_args` from disk so live config edits (e.g. the
    /// BattlEye toggle via POST /server/battleye) apply on the NEXT launch without
    /// a full service restart. Falls back to the cached args if the file is gone.
    pub fn current_args(&self) -> Vec<String> {
        Self::load(&self.instance_id)
            .map(|c| c.scum_server_args)
            .unwrap_or_else(|_| self.scum_server_args.clone())
    }
}
