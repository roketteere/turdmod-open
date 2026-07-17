// Tauri commands for the SCUM-server connector. Each one is a thin
// wrapper that pulls credentials out of tauri-plugin-store, calls into
// `crate::server`, and translates anyhow::Error → String for the JS layer.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;

use crate::scum_paths;
use crate::server::{
    self, RemoteMod, RemotePath, ServerProbeResult, ServerProfile,
    SECRET_RCON_PREFIX, SECRET_SSH_PREFIX, STORE_PROFILES_KEY,
};

const STORE_FILE: &str = "manager.json";
const STORE_KEY_INSTALL: &str = "scum_install";

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn new_uuid() -> String {
    // Lightweight v4-style id. We avoid pulling in `uuid` just for this.
    use std::time::SystemTime;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let rand_part: u64 = (nanos as u64).wrapping_mul(6364136223846793005);
    format!("srv-{:016x}-{:08x}", nanos as u64, (rand_part >> 32) as u32)
}

fn load_profiles<R: Runtime>(app: &AppHandle<R>) -> Result<Vec<ServerProfile>, String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    match store.get(STORE_PROFILES_KEY) {
        Some(value) => serde_json::from_value::<Vec<ServerProfile>>(value)
            .map_err(|e| format!("decode profiles: {}", e)),
        None => Ok(Vec::new()),
    }
}

fn save_profiles<R: Runtime>(
    app: &AppHandle<R>,
    profiles: &[ServerProfile],
) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set(
        STORE_PROFILES_KEY,
        serde_json::to_value(profiles).map_err(|e| e.to_string())?,
    );
    store.save().map_err(|e| e.to_string())?;
    Ok(())
}

fn store_secret<R: Runtime>(
    app: &AppHandle<R>,
    key: &str,
    value: &str,
) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    store.set(key, Value::String(value.to_string()));
    store.save().map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn read_secret<R: Runtime>(app: &AppHandle<R>, key: &str) -> Option<String> {
    let store = app.store(STORE_FILE).ok()?;
    store
        .get(key)
        .and_then(|v| v.as_str().map(|s| s.to_string()))
}

fn drop_secret<R: Runtime>(app: &AppHandle<R>, key: &str) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    let _ = store.delete(key);
    store.save().map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn ssh_secret_key(id: &str) -> String {
    format!("{}{}", SECRET_SSH_PREFIX, id)
}
fn rcon_secret_key(id: &str) -> String {
    format!("{}{}", SECRET_RCON_PREFIX, id)
}

pub(crate) fn get_profile<R: Runtime>(
    app: &AppHandle<R>,
    id: &str,
) -> Result<ServerProfile, String> {
    load_profiles(app)?
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| format!("server profile {} not found", id))
}

fn manager_mods_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    let install_path = store
        .get(STORE_KEY_INSTALL)
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .ok_or_else(|| "SCUM install path is not configured".to_string())?;
    let install = PathBuf::from(install_path);
    Ok(scum_paths::detect_mods_dir(&install))
}

// Resolve the on-disk artifact for a slug. The install agent extracts
// each mod into <mods_dir>/<slug>/, so we have to re-zip its contents
// into a single .turdmod blob to push to the server. We try a cached
// .turdmod sibling first to avoid re-zipping every push.
fn read_local_mod_artifact(mods_dir: &PathBuf, slug: &str) -> Result<Vec<u8>, String> {
    let cached = mods_dir.join(format!("{}.turdmod", slug));
    if cached.exists() {
        return std::fs::read(&cached)
            .map_err(|e| format!("read {}: {}", cached.display(), e));
    }

    let dir = mods_dir.join(slug);
    if !dir.is_dir() {
        return Err(format!("local mod dir {} not found", dir.display()));
    }

    let mut buf: Vec<u8> = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut buf);
        let mut zip = zip::ZipWriter::new(cursor);
        let opts: zip::write::FileOptions<()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip_dir_into(&mut zip, &dir, &dir, opts).map_err(|e| format!("zip mod: {}", e))?;
        zip.finish().map_err(|e| format!("finalize zip: {}", e))?;
    }
    std::fs::write(&cached, &buf).ok();
    Ok(buf)
}

fn zip_dir_into<W: std::io::Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    root: &std::path::Path,
    cursor: &std::path::Path,
    opts: zip::write::FileOptions<()>,
) -> Result<(), Box<dyn std::error::Error>> {
    for entry in std::fs::read_dir(cursor)? {
        let entry = entry?;
        let path = entry.path();
        let rel = path.strip_prefix(root)?.to_string_lossy().replace('\\', "/");
        if rel == ".turdmod-installed.json" {
            continue;
        }
        if path.is_dir() {
            zip.add_directory(format!("{}/", rel), opts)?;
            zip_dir_into(zip, root, &path, opts)?;
        } else {
            zip.start_file(rel, opts)?;
            let bytes = std::fs::read(&path)?;
            std::io::Write::write_all(zip, &bytes)?;
        }
    }
    Ok(())
}

// Tauri commands ------------------------------------------------------------

#[tauri::command(rename_all = "camelCase")]
pub async fn manager_server_add<R: Runtime>(
    app: AppHandle<R>,
    profile: ServerProfile,
    secret_ssh: Option<String>,
    secret_rcon: Option<String>,
) -> Result<ServerProfile, String> {
    let mut profiles = load_profiles(&app)?;
    let mut p = profile;
    if p.id.trim().is_empty() {
        p.id = new_uuid();
    }
    if p.created_at == 0 {
        p.created_at = now_secs();
    }
    if let Some(secret) = secret_ssh.as_ref().filter(|s| !s.is_empty()) {
        store_secret(&app, &ssh_secret_key(&p.id), secret)?;
    }
    if let Some(secret) = secret_rcon.as_ref().filter(|s| !s.is_empty()) {
        let key = rcon_secret_key(&p.id);
        store_secret(&app, &key, secret)?;
        p.rcon_password_ref = Some(key);
    }
    if let Some(idx) = profiles.iter().position(|x| x.id == p.id) {
        profiles[idx] = p.clone();
    } else {
        profiles.push(p.clone());
    }
    save_profiles(&app, &profiles)?;
    Ok(p)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn manager_server_remove<R: Runtime>(
    app: AppHandle<R>,
    id: String,
) -> Result<(), String> {
    let mut profiles = load_profiles(&app)?;
    profiles.retain(|p| p.id != id);
    save_profiles(&app, &profiles)?;
    drop_secret(&app, &ssh_secret_key(&id)).ok();
    drop_secret(&app, &rcon_secret_key(&id)).ok();
    Ok(())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn manager_server_list<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Vec<ServerProfile>, String> {
    load_profiles(&app)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn manager_server_test<R: Runtime>(
    app: AppHandle<R>,
    id: String,
) -> Result<ServerProbeResult, String> {
    let profile = get_profile(&app, &id)?;
    let secret_ssh = read_secret(&app, &ssh_secret_key(&id));
    let secret_rcon = read_secret(&app, &rcon_secret_key(&id));
    Ok(server::probe(&profile, secret_ssh.as_deref(), secret_rcon.as_deref()).await)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn manager_server_upload_mod<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    slug: String,
) -> Result<RemotePath, String> {
    let profile = get_profile(&app, &id)?;
    let mods_dir = manager_mods_dir(&app)?;
    let bytes = read_local_mod_artifact(&mods_dir, &slug)?;
    let secret = read_secret(&app, &ssh_secret_key(&id));
    server::upload_mod(&profile, secret.as_deref(), &slug, &bytes)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn manager_server_remove_remote_mod<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    slug: String,
) -> Result<(), String> {
    let profile = get_profile(&app, &id)?;
    let secret = read_secret(&app, &ssh_secret_key(&id));
    server::delete_remote_mod(&profile, secret.as_deref(), &slug)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn manager_server_list_remote_mods<R: Runtime>(
    app: AppHandle<R>,
    id: String,
) -> Result<Vec<RemoteMod>, String> {
    let profile = get_profile(&app, &id)?;
    let secret = read_secret(&app, &ssh_secret_key(&id));
    server::list_remote_mods(&profile, secret.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn manager_server_tail_log<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    log: String,
    lines: usize,
) -> Result<Vec<String>, String> {
    let profile = get_profile(&app, &id)?;
    let secret = read_secret(&app, &ssh_secret_key(&id));
    server::tail_log(&profile, secret.as_deref(), &log, lines.max(1).min(5000))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn manager_server_rcon<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    command: String,
) -> Result<String, String> {
    let profile = get_profile(&app, &id)?;
    let password = read_secret(&app, &rcon_secret_key(&id))
        .ok_or_else(|| "RCON password not stored for this profile".to_string())?;
    server::rcon_send(&profile, &password, &command)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub fn manager_server_known_logs() -> Value {
    json!([
        "gameplay",
        "admin",
        "chat",
        "kill",
        "login",
        "violations",
        "vehicle_destruction",
        "network_objects",
        "raid_protection",
        "economy",
        "famepoints",
        "trade",
        "container_owner",
        "buried_chest",
        "item_lock",
        "quest",
        "mine",
        "event_kill",
        "bunker_lock"
    ])
}

