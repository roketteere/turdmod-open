// Tauri command surface for the engine lifecycle (install + start + stop).
// Thin wrappers around `crate::engine`.

use std::path::PathBuf;

use tauri::{AppHandle, Runtime, State};

use crate::engine::{
    self, EnginePaths, EngineState, EngineStatus, InstallReport,
};
use crate::engine_rpc;

fn err<E: std::fmt::Display>(e: E) -> String { e.to_string() }

#[tauri::command(rename_all = "camelCase")]
pub fn engine_install(
    server_install: PathBuf,
    paths: Option<EnginePaths>,
) -> Result<InstallReport, String> {
    let paths = paths.unwrap_or(EnginePaths {
        ue4ss_dll: None,
        bridge_dll: None,
        loader_dll: None,
        launcher_exe: None,
    });
    engine::install_engine(&server_install, &paths).map_err(err)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn engine_start<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, EngineState>,
    server_install: PathBuf,
    paths: Option<EnginePaths>,
    skip_safety_check: Option<bool>,
) -> Result<EngineStatus, String> {
    let paths = paths.unwrap_or(EnginePaths {
        ue4ss_dll: None,
        bridge_dll: None,
        loader_dll: None,
        launcher_exe: None,
    });
    engine::spawn_engine(
        &state,
        &app,
        &server_install,
        &paths,
        skip_safety_check.unwrap_or(true),
    )
    .await
    .map_err(err)?;
    Ok(engine::current_status(&state))
}

#[tauri::command(rename_all = "camelCase")]
pub fn engine_stop(state: State<'_, EngineState>) -> Result<EngineStatus, String> {
    engine::stop_engine(&state).map_err(err)?;
    Ok(engine::current_status(&state))
}

#[tauri::command(rename_all = "camelCase")]
pub fn engine_get_status(state: State<'_, EngineState>) -> EngineStatus {
    engine::current_status(&state)
}

/// Generic JSON-RPC pass-through to the engine's named pipe. Used by the
/// Schema Browser and any future UI page that needs to call a bridge
/// handler (dumpWidgets, describeWidget, broadcastChat, ...).
///
/// Pipe path comes from %LOCALAPPDATA%\TurdMOD\engine\pipe.txt, written
/// by the bridge cppmod on init. If the engine isn't running, this
/// returns a useful error rather than silently hanging.
#[tauri::command(rename_all = "camelCase")]
pub async fn engine_rpc(
    method: String,
    params: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    engine_rpc::call(&method, params).await
}
