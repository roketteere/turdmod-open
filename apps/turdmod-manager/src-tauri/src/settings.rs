// JSON-file-backed settings store for the companion.
//
// ManagerSettings persists at <appdata>/settings.json.
// The RCON password is NEVER written to disk — it lives in the OS keychain
// under service="turdmod-manager", account="rcon-password", mirroring the
// pattern used by account.rs for the session token.

use std::path::PathBuf;

use keyring::Entry;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};

const KEYCHAIN_SERVICE: &str = "turdmod-manager";
const KEYCHAIN_ACCOUNT: &str = "rcon-password";

// ---------------------------------------------------------------------------
// InstallTarget
// ---------------------------------------------------------------------------

/// Which SCUM install the Manager is pointed at. Serializes as "game" / "server".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InstallTarget {
    Game,
    Server,
}

impl Default for InstallTarget {
    fn default() -> Self {
        InstallTarget::Server
    }
}

// ---------------------------------------------------------------------------
// Settings struct
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ManagerSettings {
    pub scum_server_logs_dir: Option<PathBuf>,
    pub rcon_host: Option<String>,
    pub rcon_port: Option<u16>,
    pub discord_webhook: Option<String>,
    #[serde(default)]
    pub active_target: InstallTarget,
    /// True once the first-run wizard has completed. Defaults to false
    /// so existing users without this key in settings.json see the wizard
    /// exactly once.
    #[serde(default)]
    pub wizard_completed: bool,
}

// ---------------------------------------------------------------------------
// Partial patch — sent from the frontend on Save
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    // `undefined` fields in JS arrive as missing JSON keys → None here,
    // meaning "no change". Use Option<Option<T>> to distinguish
    // "not provided" from "explicitly set to null".
    pub scum_server_logs_dir: Option<Option<String>>,
    pub rcon_host: Option<Option<String>>,
    pub rcon_port: Option<Option<u16>>,
    pub discord_webhook: Option<Option<String>>,
    // Empty string = clear the keychain entry.
    pub rcon_password: Option<String>,
    // None = no change; Some(target) = update.
    pub active_target: Option<InstallTarget>,
    // Some(true) marks the wizard done; Some(false) re-shows it.
    pub wizard_completed: Option<bool>,
}

// ---------------------------------------------------------------------------
// Managed state (currently just a marker; the real state is on disk/keychain)
// ---------------------------------------------------------------------------

pub struct SettingsState;

// ---------------------------------------------------------------------------
// File helpers
// ---------------------------------------------------------------------------

fn settings_path<R: Runtime>(app: &AppHandle<R>) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("settings.json")
}

pub fn load_settings<R: Runtime>(app: &AppHandle<R>) -> ManagerSettings {
    let path = settings_path(app);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_settings<R: Runtime>(
    app: &AppHandle<R>,
    settings: &ManagerSettings,
) -> Result<(), String> {
    let path = settings_path(app);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create settings dir: {}", e))?;
    }
    let json = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("serialize settings: {}", e))?;
    std::fs::write(&path, json)
        .map_err(|e| format!("write settings: {}", e))?;
    Ok(())
}

pub fn apply_patch<R: Runtime>(
    app: &AppHandle<R>,
    patch: SettingsPatch,
) -> Result<ManagerSettings, String> {
    let mut settings = load_settings(app);

    if let Some(v) = patch.scum_server_logs_dir {
        settings.scum_server_logs_dir = v.map(PathBuf::from);
    }
    if let Some(v) = patch.rcon_host {
        settings.rcon_host = v;
    }
    if let Some(v) = patch.rcon_port {
        settings.rcon_port = v;
    }
    if let Some(v) = patch.discord_webhook {
        settings.discord_webhook = v;
    }
    if let Some(ref pw) = patch.rcon_password {
        if pw.is_empty() {
            clear_rcon_password();
        } else {
            set_rcon_password(pw)?;
        }
    }
    if let Some(target) = patch.active_target {
        settings.active_target = target;
    }
    if let Some(v) = patch.wizard_completed {
        settings.wizard_completed = v;
    }

    save_settings(app, &settings)?;
    Ok(settings)
}

// ---------------------------------------------------------------------------
// Keychain helpers
// ---------------------------------------------------------------------------

fn rcon_entry() -> Result<Entry, String> {
    Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)
        .map_err(|e| format!("keychain entry: {}", e))
}

pub fn get_rcon_password<R: Runtime>(_app: &AppHandle<R>) -> Option<String> {
    let entry = rcon_entry().ok()?;
    match entry.get_password() {
        Ok(s) if s.trim().is_empty() => None,
        Ok(s) => Some(s),
        Err(keyring::Error::NoEntry) => None,
        Err(_) => None,
    }
}

pub fn has_rcon_password<R: Runtime>(app: &AppHandle<R>) -> bool {
    get_rcon_password(app).is_some()
}

pub fn set_rcon_password(password: &str) -> Result<(), String> {
    rcon_entry()?
        .set_password(password)
        .map_err(|e| format!("keychain write: {}", e))
}

pub fn clear_rcon_password() {
    if let Ok(entry) = rcon_entry() {
        let _ = entry.delete_credential();
    }
}