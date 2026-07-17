// Tauri commands for the soft-RCON admin surface.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime};

use crate::admin_files::{self, validate_filename};
use crate::server_commands::{get_profile, read_secret, ssh_secret_key};

const SCUM_SECTION: &str = "[/Script/Scum.ScumGameMode]";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerSettingsForm {
    pub server_name: Option<String>,
    pub server_description: Option<String>,
    pub server_password: Option<String>,
    pub max_players: Option<String>,
    pub server_playstyle: Option<String>,
    pub message_of_the_day: Option<String>,
    pub enable_whitelist: Option<String>,
    pub enable_battl_eye: Option<String>,
    pub allow_first_person: Option<String>,
    pub allow_third_person: Option<String>,
    pub allow_crosshair: Option<String>,
    pub enable_new_player_protection: Option<String>,
    pub new_player_protection_duration: Option<String>,
    pub allow_voting: Option<String>,
    pub day_cycle_speed_multiplier: Option<String>,
    pub nighttime_speed_multiplier: Option<String>,
    pub economy_multiplier: Option<String>,
    pub respawn_time: Option<String>,
    pub xp_multiplier: Option<String>,
    pub raw_ini: String,
}

// (field_name, ini_key) pairs — used to translate frontend patches into INI writes.
const SETTINGS_KEYS: &[(&str, &str)] = &[
    ("serverName", "scum.ServerName"),
    ("serverDescription", "scum.ServerDescription"),
    ("serverPassword", "scum.ServerPassword"),
    ("maxPlayers", "scum.MaxPlayers"),
    ("serverPlaystyle", "scum.ServerPlaystyle"),
    ("messageOfTheDay", "scum.MessageOfTheDay"),
    ("enableWhitelist", "scum.bEnableWhiteList"),
    ("enableBattlEye", "scum.bEnableBattlEye"),
    ("allowFirstPerson", "scum.bAllowFirstPersonView"),
    ("allowThirdPerson", "scum.bAllowThirdPersonView"),
    ("allowCrosshair", "scum.bAllowCrosshair"),
    ("enableNewPlayerProtection", "scum.bEnableNewPlayerProtection"),
    ("newPlayerProtectionDuration", "scum.NewPlayerProtectionDuration"),
    ("allowVoting", "scum.bAllowVoting"),
    ("dayCycleSpeedMultiplier", "scum.DayCycleSpeedMultiplier"),
    ("nighttimeSpeedMultiplier", "scum.NighttimeSpeedMultiplier"),
    ("economyMultiplier", "scum.EconomyMultiplier"),
    ("respawnTime", "scum.RespawnTime"),
    ("xpMultiplier", "scum.XpMultiplier"),
];

fn parse_settings_form(raw: &str) -> ServerSettingsForm {
    let g = |k: &str| admin_files::ini_get(raw, SCUM_SECTION, k);
    ServerSettingsForm {
        server_name: g("scum.ServerName"),
        server_description: g("scum.ServerDescription"),
        server_password: g("scum.ServerPassword"),
        max_players: g("scum.MaxPlayers"),
        server_playstyle: g("scum.ServerPlaystyle"),
        message_of_the_day: g("scum.MessageOfTheDay"),
        enable_whitelist: g("scum.bEnableWhiteList"),
        enable_battl_eye: g("scum.bEnableBattlEye"),
        allow_first_person: g("scum.bAllowFirstPersonView"),
        allow_third_person: g("scum.bAllowThirdPersonView"),
        allow_crosshair: g("scum.bAllowCrosshair"),
        enable_new_player_protection: g("scum.bEnableNewPlayerProtection"),
        new_player_protection_duration: g("scum.NewPlayerProtectionDuration"),
        allow_voting: g("scum.bAllowVoting"),
        day_cycle_speed_multiplier: g("scum.DayCycleSpeedMultiplier"),
        nighttime_speed_multiplier: g("scum.NighttimeSpeedMultiplier"),
        economy_multiplier: g("scum.EconomyMultiplier"),
        respawn_time: g("scum.RespawnTime"),
        xp_multiplier: g("scum.XpMultiplier"),
        raw_ini: raw.to_string(),
    }
}

pub type SettingsPatch = std::collections::HashMap<String, String>;

fn apply_patch(raw: &str, patch: &SettingsPatch) -> String {
    let mut text = raw.to_string();
    for (field, value) in patch {
        if let Some((_, ini_key)) = SETTINGS_KEYS.iter().find(|(f, _)| *f == field.as_str()) {
            text = admin_files::ini_set(&text, SCUM_SECTION, ini_key, value);
        }
        // Unknown fields silently ignored — frontend only sends fields it
        // knows about, and we only write what we can validate.
    }
    text
}

#[tauri::command(rename_all = "camelCase")]
pub async fn manager_server_read_admin_file<R: Runtime>(
    app: AppHandle<R>,
    server_id: String,
    filename: String,
) -> Result<String, String> {
    validate_filename(&filename).map_err(|e| e.to_string())?;
    let profile = get_profile(&app, &server_id)?;
    let secret = read_secret(&app, &ssh_secret_key(&server_id));
    admin_files::download_admin_file(&profile, secret.as_deref(), &filename)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn manager_server_write_admin_file<R: Runtime>(
    app: AppHandle<R>,
    server_id: String,
    filename: String,
    contents: String,
) -> Result<(), String> {
    validate_filename(&filename).map_err(|e| e.to_string())?;
    let profile = get_profile(&app, &server_id)?;
    let secret = read_secret(&app, &ssh_secret_key(&server_id));
    admin_files::upload_admin_file(&profile, secret.as_deref(), &filename, &contents)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn manager_server_parse_server_settings<R: Runtime>(
    app: AppHandle<R>,
    server_id: String,
) -> Result<ServerSettingsForm, String> {
    let profile = get_profile(&app, &server_id)?;
    let secret = read_secret(&app, &ssh_secret_key(&server_id));
    let raw = admin_files::download_admin_file(&profile, secret.as_deref(), "ServerSettings.ini")
        .await
        .map_err(|e| e.to_string())?;
    Ok(parse_settings_form(&raw))
}

#[tauri::command(rename_all = "camelCase")]
pub async fn manager_server_save_server_settings_partial<R: Runtime>(
    app: AppHandle<R>,
    server_id: String,
    patch: SettingsPatch,
) -> Result<(), String> {
    let profile = get_profile(&app, &server_id)?;
    let secret = read_secret(&app, &ssh_secret_key(&server_id));

    let raw = admin_files::download_admin_file(&profile, secret.as_deref(), "ServerSettings.ini")
        .await
        .map_err(|e| e.to_string())?;
    let updated = apply_patch(&raw, &patch);
    admin_files::upload_admin_file(&profile, secret.as_deref(), "ServerSettings.ini", &updated)
        .await
        .map_err(|e| e.to_string())
}
