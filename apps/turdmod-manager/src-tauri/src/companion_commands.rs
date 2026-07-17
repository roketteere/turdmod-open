// Tauri command surface for the companion lifecycle.
// Each command is a thin wrapper around `crate::companion`.

use tauri::{AppHandle, Runtime, State};

use crate::companion::{self, CompanionState, CompanionStatus};
use crate::settings::{self, SettingsState};

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[tauri::command(rename_all = "camelCase")]
pub async fn companion_start<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, CompanionState>,
    _settings_state: State<'_, SettingsState>,
) -> Result<CompanionStatus, String> {
    let settings = settings::load_settings(&app);
    let rcon_password = settings::get_rcon_password(&app);
    companion::spawn_companion(&state, &app, &settings, rcon_password)
        .await
        .map_err(err)?;
    Ok(companion::current_status(&state))
}

#[tauri::command(rename_all = "camelCase")]
pub async fn companion_stop(
    state: State<'_, CompanionState>,
) -> Result<CompanionStatus, String> {
    companion::stop_companion(&state).await.map_err(err)?;
    Ok(companion::current_status(&state))
}

#[tauri::command(rename_all = "camelCase")]
pub async fn companion_restart<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, CompanionState>,
    _settings_state: State<'_, SettingsState>,
) -> Result<CompanionStatus, String> {
    let settings = settings::load_settings(&app);
    let rcon_password = settings::get_rcon_password(&app);
    companion::restart_companion(&state, &app, &settings, rcon_password)
        .await
        .map_err(err)?;
    Ok(companion::current_status(&state))
}

#[tauri::command(rename_all = "camelCase")]
pub fn companion_get_status(
    state: State<'_, CompanionState>,
) -> CompanionStatus {
    companion::current_status(&state)
}