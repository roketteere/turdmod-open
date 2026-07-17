// Tauri command surface for install snapshots.

use crate::scum_paths;
use crate::snapshot;
use crate::dump;
use std::path::PathBuf;

#[tauri::command(rename_all = "camelCase")]
pub async fn snapshot_install(target: String) -> Result<snapshot::SnapshotResult, String> {
    // Resolve source install dir per target.
    let installs = scum_paths::detect_all_installs();
    let source: Option<PathBuf> = match target.as_str() {
        "server" => installs.server,
        "client" => installs.game,
        other => return Err(format!("unknown target '{}' (expected server|client)", other)),
    };
    let source = source.ok_or_else(|| {
        format!("no {} install detected — set the path in Manager settings", target)
    })?;

    // Build ID per target's Steam app manifest. Falls back to "unknown"
    // if Steam isn't readable (snapshot still happens; we just use the
    // string "unknown" as the version tag and warn the operator).
    let scum_build = match target.as_str() {
        "server" => dump::read_steam_buildid()
            .unwrap_or_else(|| "unknown".to_string()),
        "client" => dump::read_steam_client_buildid()
            .unwrap_or_else(|| "unknown".to_string()),
        _ => unreachable!(),
    };

    // Long-running synchronous robocopy → offload to a blocking task.
    let source_clone = source.clone();
    let target_clone = target.clone();
    let build_clone = scum_build.clone();
    let result = tokio::task::spawn_blocking(move || {
        snapshot::snapshot_install(&target_clone, &source_clone, &build_clone)
    })
    .await
    .map_err(|e| format!("blocking task: {}", e))??;

    Ok(result)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn snapshot_list(target: Option<String>) -> Result<Vec<snapshot::SnapshotEntry>, String> {
    let t = target.as_deref();
    Ok(snapshot::list_snapshots(t))
}
