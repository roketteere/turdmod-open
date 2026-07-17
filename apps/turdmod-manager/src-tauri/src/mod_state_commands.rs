// Tauri command surface for mod freeze state and per-mod config.

use tauri::{AppHandle, Runtime};

use crate::commands::resolve_active_install;
use crate::mod_state::{self, ModState};
use crate::scum_paths;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn resolve_mods_dir<R: Runtime>(app: &AppHandle<R>) -> Option<std::path::PathBuf> {
    let install = resolve_active_install(app)?;
    Some(scum_paths::detect_mods_dir(&install))
}

#[tauri::command(rename_all = "camelCase")]
pub fn manager_get_mod_state<R: Runtime>(app: AppHandle<R>) -> Result<ModState, String> {
    let mods_dir = match resolve_mods_dir(&app) {
        Some(d) => d,
        None => return Ok(ModState::default()),
    };
    mod_state::read_mod_state(&mods_dir).map_err(err)
}

#[tauri::command(rename_all = "camelCase")]
pub fn manager_set_mod_frozen<R: Runtime>(
    app: AppHandle<R>,
    slug: String,
    frozen: bool,
) -> Result<ModState, String> {
    let mods_dir = resolve_mods_dir(&app)
        .ok_or_else(|| "scum_install_not_found".to_string())?;
    mod_state::set_frozen(&mods_dir, &slug, frozen).map_err(err)
}

#[tauri::command(rename_all = "camelCase")]
pub fn manager_read_mod_config<R: Runtime>(
    app: AppHandle<R>,
    slug: String,
) -> Result<serde_json::Value, String> {
    let mods_dir = resolve_mods_dir(&app)
        .ok_or_else(|| "scum_install_not_found".to_string())?;
    mod_state::read_mod_config(&mods_dir, &slug).map_err(err)
}

#[tauri::command(rename_all = "camelCase")]
pub fn manager_write_mod_config<R: Runtime>(
    app: AppHandle<R>,
    slug: String,
    config: serde_json::Value,
) -> Result<(), String> {
    let mods_dir = resolve_mods_dir(&app)
        .ok_or_else(|| "scum_install_not_found".to_string())?;
    mod_state::write_mod_config(&mods_dir, &slug, &config).map_err(err)
}

#[tauri::command(rename_all = "camelCase")]
pub fn manager_get_mod_manifest<R: Runtime>(
    app: AppHandle<R>,
    slug: String,
) -> Result<serde_json::Value, String> {
    let mods_dir = resolve_mods_dir(&app)
        .ok_or_else(|| "scum_install_not_found".to_string())?;
    mod_state::read_mod_manifest(&mods_dir, &slug).map_err(err)
}
