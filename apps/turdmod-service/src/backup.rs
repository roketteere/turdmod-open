// Auto-backup - copies all TurdMOD data files to a timestamped folder every 30 min. !backup forces one.

use std::time::Duration;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const DATA_DIR: &str = r"C:\TurdMOD\data";
const BACKUP_DIR: &str = r"C:\TurdMOD\backups";
const BACKUP_INTERVAL: Duration = Duration::from_secs(1800);
const MAX_BACKUPS: usize = 24;

fn now_tag() -> String {
    format!("{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs())
}

fn do_backup() -> Result<String, String> {
    let tag = now_tag();
    let dest = std::path::PathBuf::from(BACKUP_DIR).join(&tag);
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    let src = std::path::PathBuf::from(DATA_DIR);
    if !src.exists() { return Err("data dir missing".into()); }
    let mut count = 0u32;
    if let Ok(entries) = std::fs::read_dir(&src) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let name = path.file_name().unwrap_or_default();
                if std::fs::copy(&path, dest.join(name)).is_ok() { count += 1; }
            }
        }
    }
    if let Ok(dirs) = std::fs::read_dir(BACKUP_DIR) {
        let mut backup_dirs: Vec<std::path::PathBuf> = dirs.flatten().filter(|e| e.path().is_dir()).map(|e| e.path()).collect();
        backup_dirs.sort();
        while backup_dirs.len() > MAX_BACKUPS {
            if !backup_dirs.is_empty() { let old = backup_dirs.remove(0); let _ = std::fs::remove_dir_all(old); }
        }
    }
    Ok(format!("{} files backed up to {}", count, tag))
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

pub struct Backup;
impl Backup { pub fn new() -> Self { Self } }

#[async_trait::async_trait]
impl Mod for Backup {
    fn name(&self) -> &'static str { "backup" }
    fn commands(&self) -> &'static [&'static str] { &["!backup"] }
    fn interval(&self) -> Option<Duration> { Some(BACKUP_INTERVAL) }

    async fn tick(&self, _ctx: &ModCtx) {
        match do_backup() {
            Ok(msg) => tracing::info!("backup: {}", msg),
            Err(e) => tracing::warn!("backup: failed: {}", e),
        }
    }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        if text.to_lowercase() != "!backup" { return Outcome::Ignored; }
        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if !crate::owner::is_owner_steam(&steam) && player != crate::owner::name() {
            reply("[Backup] Admin only.", &player).await;
            return Outcome::Handled;
        }
        match do_backup() {
            Ok(msg) => reply(&format!("[Backup] {}", msg), &player).await,
            Err(e) => reply(&format!("[Backup] Failed: {}", e), &player).await,
        }
        Outcome::Handled
    }
}
