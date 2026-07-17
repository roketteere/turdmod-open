// Tauri command handlers — thin wrappers around the engine modules.
//
// All public commands rename to camelCase so the JS layer can call e.g.
// invoke('managerListMods', ...). Errors get string-ified at this
// boundary because Tauri's IPC bridge serializes errors as strings on
// the JS side anyway.

use std::path::PathBuf;

use serde::Serialize;
use tauri::{Emitter, Manager, Runtime, Window};
use tauri_plugin_store::StoreExt;

use crate::mod_index::{self, ModDetail, ModSummary};
use crate::mod_install::{self, InstallProgress, InstalledMod};
use crate::scum_paths;
use crate::settings::{self, InstallTarget};

pub const INSTALL_PROGRESS_EVENT: &str = "mod-install-progress";
const STORE_FILE: &str = "manager.json";
const STORE_KEY_INSTALL: &str = "scum_install";

// Enriched detect result — both detected paths, the resolved active path,
// and the current target setting. Replaces the old single-install shape.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DetectResult {
    pub active: Option<String>,
    pub active_target: InstallTarget,
    pub game: Option<String>,
    pub server: Option<String>,
    pub mods_dir: Option<String>,
    pub battleye_disabled: Option<bool>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DetectedInstallsPayload {
    pub game: Option<String>,
    pub server: Option<String>,
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

pub fn stored_install_path<R: Runtime>(app: &tauri::AppHandle<R>) -> Option<PathBuf> {
    let store = app.store(STORE_FILE).ok()?;
    let v = store.get(STORE_KEY_INSTALL)?;
    match v {
        serde_json::Value::String(s) => Some(PathBuf::from(s)),
        _ => None,
    }
}

fn save_install_path<R: Runtime>(app: &tauri::AppHandle<R>, path: &PathBuf) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(err)?;
    store.set(
        STORE_KEY_INSTALL,
        serde_json::Value::String(path.to_string_lossy().into_owned()),
    );
    store.save().map_err(err)?;
    Ok(())
}

/// Resolve the install path for all operations:
/// 1. Manual override via `manager_set_install_path`, if the path still exists.
/// 2. Detect both → return whichever matches `settings.active_target`.
/// 3. Graceful fallback to the other target if the preferred one isn't installed.
pub fn resolve_active_install<R: Runtime>(app: &tauri::AppHandle<R>) -> Option<PathBuf> {
    if let Some(p) = stored_install_path(app) {
        if p.exists() {
            return Some(p);
        }
    }

    let settings = settings::load_settings(app);
    let installs = scum_paths::detect_all_installs();

    let preferred = match settings.active_target {
        InstallTarget::Game => installs.game.clone(),
        InstallTarget::Server => installs.server.clone(),
    };
    if let Some(p) = preferred {
        return Some(p);
    }

    let fallback = match settings.active_target {
        InstallTarget::Game => installs.server,
        InstallTarget::Server => installs.game,
    };
    if let Some(ref p) = fallback {
        tracing::warn!(
            target = ?settings.active_target,
            fallback = %p.display(),
            "resolve_active_install: preferred target not found, using fallback"
        );
    }
    fallback
}

#[tauri::command(rename_all = "camelCase")]
pub fn manager_smoke_test() -> serde_json::Value {
    serde_json::json!({ "ok": true, "version": env!("CARGO_PKG_VERSION") })
}

#[tauri::command(rename_all = "camelCase")]
pub fn manager_detect_scum<R: Runtime>(app: tauri::AppHandle<R>) -> DetectResult {
    let settings = settings::load_settings(&app);
    let installs = scum_paths::detect_all_installs();

    let active = resolve_active_install(&app);
    let mods_dir = active.as_ref().map(|p| scum_paths::detect_mods_dir(p));
    let battleye_disabled = active.as_ref().and_then(|p| scum_paths::is_battleye_disabled(p));

    DetectResult {
        active: active.map(|p| p.to_string_lossy().into_owned()),
        active_target: settings.active_target,
        game: installs.game.map(|p| p.to_string_lossy().into_owned()),
        server: installs.server.map(|p| p.to_string_lossy().into_owned()),
        mods_dir: mods_dir.map(|p| p.to_string_lossy().into_owned()),
        battleye_disabled,
    }
}

#[tauri::command(rename_all = "camelCase")]
pub fn manager_list_detected_installs() -> DetectedInstallsPayload {
    let installs = scum_paths::detect_all_installs();
    DetectedInstallsPayload {
        game: installs.game.map(|p| p.to_string_lossy().into_owned()),
        server: installs.server.map(|p| p.to_string_lossy().into_owned()),
    }
}

#[tauri::command(rename_all = "camelCase")]
pub fn manager_get_active_target<R: Runtime>(app: tauri::AppHandle<R>) -> InstallTarget {
    settings::load_settings(&app).active_target
}

#[tauri::command(rename_all = "camelCase")]
pub fn manager_set_active_target<R: Runtime>(
    app: tauri::AppHandle<R>,
    target: InstallTarget,
) -> Result<(), String> {
    let mut s = settings::load_settings(&app);
    s.active_target = target;
    settings::save_settings(&app, &s)
}

#[tauri::command(rename_all = "camelCase")]
pub fn manager_set_install_path<R: Runtime>(
    app: tauri::AppHandle<R>,
    path: PathBuf,
) -> Result<(), String> {
    save_install_path(&app, &path)
}

#[tauri::command(rename_all = "camelCase")]
pub fn manager_get_install_path<R: Runtime>(app: tauri::AppHandle<R>) -> Option<PathBuf> {
    stored_install_path(&app)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn manager_list_mods(
    category: Option<String>,
    search: Option<String>,
) -> Result<Vec<ModSummary>, String> {
    mod_index::list_mods(category.as_deref(), search.as_deref())
        .await
        .map_err(err)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn manager_get_mod(slug: String) -> Result<ModDetail, String> {
    mod_index::get_mod(&slug).await.map_err(err)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn manager_install_mod<R: Runtime>(
    window: Window<R>,
    slug: String,
    auth_token: Option<String>,
) -> Result<InstalledMod, String> {
    let app = window.app_handle().clone();
    let install = resolve_active_install(&app)
        .ok_or_else(|| "scum_install_not_found".to_string())?;
    let mods_dir = scum_paths::detect_mods_dir(&install);

    let win = window.clone();
    let progress = move |p: InstallProgress| {
        if let Err(err) = win.emit(INSTALL_PROGRESS_EVENT, &p) {
            tracing::warn!(err = %err, "emit progress failed");
        }
    };

    mod_install::install_mod(&slug, &mods_dir, auth_token.as_deref(), progress)
        .await
        .map_err(err)
}

#[tauri::command(rename_all = "camelCase")]
pub fn manager_uninstall_mod<R: Runtime>(
    app: tauri::AppHandle<R>,
    slug: String,
) -> Result<(), String> {
    let install = resolve_active_install(&app)
        .ok_or_else(|| "scum_install_not_found".to_string())?;
    let mods_dir = scum_paths::detect_mods_dir(&install);
    mod_install::uninstall_mod(&slug, &mods_dir).map_err(err)
}

#[tauri::command(rename_all = "camelCase")]
pub fn manager_list_installed<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Vec<InstalledMod>, String> {
    let mut all = Vec::new();

    if let Some(install) = resolve_active_install(&app) {
        let mods_dir = scum_paths::detect_mods_dir(&install);
        if let Ok(mods) = mod_install::list_installed(&mods_dir) {
            all.extend(mods);
        }
    }

    // Also scan companion mods dir (server-side mods from TURDMOD_MODS_DIR)
    if let Ok(companion_dir) = std::env::var("TURDMOD_MODS_DIR") {
        let p = std::path::PathBuf::from(&companion_dir);
        if let Ok(mods) = mod_install::list_installed(&p) {
            let existing: std::collections::HashSet<String> =
                all.iter().map(|m| m.slug.clone()).collect();
            for m in mods {
                if !existing.contains(&m.slug) {
                    all.push(m);
                }
            }
        }
    }

    Ok(all)
}

#[tauri::command(rename_all = "camelCase")]
pub fn manager_smoke_test_full<R: Runtime>(app: tauri::AppHandle<R>) -> serde_json::Value {
    let install = resolve_active_install(&app);
    let mods_dir = install.as_ref().map(|p| scum_paths::detect_mods_dir(p));
    let installed_count = mods_dir
        .as_ref()
        .and_then(|d| mod_install::list_installed(d).ok())
        .map(|v| v.len())
        .unwrap_or(0);

    serde_json::json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
        "scumDetected": install.is_some(),
        "modsDir": mods_dir.map(|p| p.to_string_lossy().into_owned()),
        "installedCount": installed_count,
    })
}

// Writes arbitrary text to a user-chosen path. Exists because the
// frontend tauri-plugin-fs writeTextFile rejects paths outside its
// configured scope — but for the Schema/Functions export flow the user
// picks the path via the native save dialog, so a Rust-side write
// bypasses that scope check cleanly.
#[tauri::command(rename_all = "camelCase")]
pub async fn manager_write_text_file(path: String, content: String) -> Result<(), String> {
    std::fs::write(&path, content).map_err(|e| format!("write {}: {}", path, e))
}

// Appends text to a file (creating it + parent dir if missing). Used by
// the Console page's filter-export toggle so the user can grep with
// find-in-text tools and Claude can poll for live updates. Distinct from
// manager_write_text_file (which overwrites) — appending preserves
// history across boots and across filter-toggle cycles.
#[tauri::command(rename_all = "camelCase")]
pub async fn manager_append_text_file(path: String, content: String) -> Result<(), String> {
    use std::fs::OpenOptions;
    use std::io::Write;
    if let Some(parent) = std::path::Path::new(&path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create dir {}: {}", parent.display(), e))?;
        }
    }
    let mut f = OpenOptions::new()
        .append(true)
        .create(true)
        .open(&path)
        .map_err(|e| format!("open {}: {}", path, e))?;
    f.write_all(content.as_bytes())
        .map_err(|e| format!("write {}: {}", path, e))?;
    Ok(())
}

// Reads a UTF-8 text file from disk. Pair to manager_write_text_file —
// frontend tauri-plugin-fs's readTextFile would reject paths outside its
// configured scope. We need to read SCUM's Notifications.json wherever the
// user's install lives, so the Rust-side bypass is the right primitive.
// Returns ("", false) when the file doesn't exist so callers can render an
// empty editor without surfacing the missing-file as an error.
#[tauri::command(rename_all = "camelCase")]
pub async fn manager_read_text_file(path: String) -> Result<ReadTextResult, String> {
    match std::fs::read_to_string(&path) {
        Ok(content) => Ok(ReadTextResult { content, existed: true }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Ok(ReadTextResult { content: String::new(), existed: false })
        }
        Err(e) => Err(format!("read {}: {}", path, e)),
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ReadTextResult {
    pub content: String,
    pub existed: bool,
}

// Result of `manager_find_scumdump_database`. Returned to the Reflection
// Database page so it can present meta info and lazily fetch the big
// classes/enums/structs JSON via `manager_read_text_file`.
// Returns the absolute path to the engine-rpc log file (one JSON entry
// per request/response). Used by the Bridge Smoke page so the user can
// "Open log" in Notepad — the result panes in the WebView can't always
// be highlighted/copied.
#[tauri::command(rename_all = "camelCase")]
pub async fn manager_engine_rpc_log_path() -> Result<String, String> {
    crate::engine_rpc::rpc_log_path()
        .map(|p| p.to_string_lossy().into_owned())
        .ok_or_else(|| "LOCALAPPDATA/APPDATA not set".to_string())
}

// Returns file size + last-modified ms. Pair to manager_read_text_file —
// used by ServerFilesPage to render a one-line status per config file
// before the user clicks Edit.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FileMeta {
    pub existed: bool,
    pub bytes: u64,
    pub modified_ms: u64,
}

#[tauri::command(rename_all = "camelCase")]
pub async fn manager_file_meta(path: String) -> Result<FileMeta, String> {
    match std::fs::metadata(&path) {
        Ok(m) => {
            let modified_ms = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            Ok(FileMeta {
                existed: true,
                bytes: m.len(),
                modified_ms,
            })
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(FileMeta {
            existed: false,
            bytes: 0,
            modified_ms: 0,
        }),
        Err(e) => Err(format!("metadata {}: {}", path, e)),
    }
}

// Reads the last N lines of the engine-rpc log. Pair to the in-app log
// viewer on the Bridge Smoke page — avoids the shell-scope problems with
// trying to `open` arbitrary absolute paths in Tauri 2.
#[tauri::command(rename_all = "camelCase")]
pub async fn manager_engine_rpc_log_tail(lines: usize) -> Result<String, String> {
    let Some(path) = crate::engine_rpc::rpc_log_path() else {
        return Err("LOCALAPPDATA/APPDATA not set".to_string());
    };
    if !path.exists() {
        return Ok(String::new());
    }
    let content =
        std::fs::read_to_string(&path).map_err(|e| format!("read log: {}", e))?;
    let max = lines.max(1);
    let all: Vec<&str> = content.lines().collect();
    let start = all.len().saturating_sub(max);
    Ok(all[start..].join("\n"))
}

// Opens an arbitrary file in the OS default app via std::process::Command.
// Bypasses tauri-plugin-shell's path-scope check which rejects
// LOCALAPPDATA paths by default. Used for the RPC log "Open in Notepad"
// fallback.
#[tauri::command(rename_all = "camelCase")]
pub async fn manager_open_in_default_app(path: String) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::process::Command;
        // `cmd /c start "" "<path>"` reliably opens a file in the OS
        // default app on Windows. The empty `""` is the window title that
        // `start` requires when the path is quoted.
        Command::new("cmd")
            .args(["/c", "start", "", &path])
            .spawn()
            .map_err(|e| format!("spawn start: {}", e))?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err(format!("manager_open_in_default_app only supports Windows ({})", path))
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ScumdumpDatabase {
    pub root: String,
    pub build: String,
    pub classes_path: String,
    pub enums_path: String,
    pub structs_path: String,
    pub widgets_dir: String,
    pub datatables_dir: String,
    pub strings_dir: String,
    pub sdk_dir: String,
    pub meta_json: String,
}

fn newest_build_dir(extracted_root: &std::path::Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(extracted_root).ok()?;
    let mut best: Option<(String, PathBuf)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with('v') {
            continue;
        }
        match &best {
            None => best = Some((name, path)),
            Some((b, _)) if name.as_str() > b.as_str() => best = Some((name, path)),
            _ => {}
        }
    }
    best.map(|(_, p)| p)
}

// Locates the scumdump extraction output produced by the sibling
// `scumdump` repo. Search order:
//   1. `$SCUMDUMP_DATA_DIR` env var (an `extracted/` parent)
//   2. `C:/Development/Claude/scumdump/data/extracted/` (Joel's canonical path)
// Picks the lexicographically newest `v*` build subdir under it.
#[tauri::command(rename_all = "camelCase")]
pub async fn manager_find_scumdump_database() -> Result<ScumdumpDatabase, String> {
    let candidates: Vec<PathBuf> = std::env::var("SCUMDUMP_DATA_DIR")
        .ok()
        .map(PathBuf::from)
        .into_iter()
        .chain(std::iter::once(PathBuf::from(
            "C:/Development/Claude/scumdump/data/extracted",
        )))
        .collect();

    let build_dir = candidates
        .iter()
        .find(|p| p.exists())
        .and_then(|p| newest_build_dir(p))
        .ok_or_else(|| "no scumdump build directory found; run scumdump phase-a first".to_string())?;

    let build = build_dir
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    let meta_path = build_dir.join("_meta.json");
    let meta_json = std::fs::read_to_string(&meta_path)
        .map_err(|e| format!("read {}: {}", meta_path.display(), e))?;

    Ok(ScumdumpDatabase {
        root: build_dir.to_string_lossy().into_owned(),
        build,
        classes_path: build_dir.join("classes.json").to_string_lossy().into_owned(),
        enums_path: build_dir.join("enums.json").to_string_lossy().into_owned(),
        structs_path: build_dir.join("structs.json").to_string_lossy().into_owned(),
        widgets_dir: build_dir.join("widgets").to_string_lossy().into_owned(),
        datatables_dir: build_dir.join("datatables").to_string_lossy().into_owned(),
        strings_dir: build_dir.join("strings").to_string_lossy().into_owned(),
        sdk_dir: build_dir.join("sdk").to_string_lossy().into_owned(),
        meta_json,
    })
}
