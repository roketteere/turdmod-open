//! Banner helpers. Banners fire via SCUM's Notifications.json — the NotificationsManager re-reads it
//! live and fires within ~30s, COLORED, with NO admin needed (proven 2026-06-10). The resting state
//! is a blanked entry on a 20h cadence so nothing auto-fires; fire()/scheduled banners temporarily
//! write a real banner, then restore the silent resting state so it shows once.
//!
//! @ctx the instant bridge path (fireBanner/captureNotification capture-replay) is DEAD: the captured
//!   notification's message is a nested FText at a non-obvious offset (fireBanner reports cap:0,
//!   msgSet:false), so it could only re-fire the captured text, never custom text. Notifications.json
//!   is the reliable colored zero-admin channel. @dep [[reference_colored_banners]].
//! @inv NOTIF_PATH is set ONCE at boot from cfg.scum_server_exe (see main.rs).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use serde_json::json;

use crate::pipe_rpc;

static NOTIF_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Resting Notifications.json: a blank message on a 20h cadence -> effectively silent, so a banner
/// only shows when WE write one (scheduled or ad-hoc fire()). Joel's "blank them out / every 20h".
const QUIET: &str = "{\n  \"Notifications\": [\n    { \"day\": \"Everyday\", \"duration\": \"1\", \"color\": \"0-0-0\", \"wait\": \"72000\", \"message\": \" \" }\n  ]\n}";

pub fn set_notif_path(p: PathBuf) { let _ = NOTIF_PATH.set(p); }

fn write_quiet(path: &Path) { let _ = std::fs::write(path, QUIET); }

/// Set the resting (silent) Notifications.json. Call once at boot.
pub fn write_quiet_default() { if let Some(p) = NOTIF_PATH.get() { write_quiet(p); } }

fn write_banner_file(path: &Path, message: &str, color: &str, duration: u32) {
    // sanitize: keep the JSON valid + single-line
    let safe: String = message.chars()
        .map(|c| match c { '"' => '\'', '\\' => '/', '\r' | '\n' => ' ', _ => c })
        .collect();
    let json = format!(
        "{{\n  \"Notifications\": [\n    {{ \"day\": \"Everyday\", \"duration\": \"{}\", \"color\": \"{}\", \"wait\": \"0\", \"message\": \"{}\" }}\n  ]\n}}",
        duration, color, safe);
    let _ = std::fs::write(path, json);
}

/// Fire a COLORED center banner with custom text, NO admin needed (~30s — the manager's re-read
/// cycle). Writes the banner to Notifications.json, then restores the silent resting state after it
/// has had time to fire + display, so it shows once and doesn't loop.
pub async fn fire(text: &str, r: u8, g: u8, b: u8, duration: u32) {
    let Some(path) = NOTIF_PATH.get() else {
        // path not resolved (shouldn't happen post-boot) — degrade to a chat line.
        pipe_rpc::call("broadcastChat", Some(json!({ "text": format!("\u{1F4E2} {}", text), "channel": "6" }))).await.ok();
        return;
    };
    let color = format!("{}-{}-{}", r, g, b);
    write_banner_file(path, text, &color, duration);
    let path = path.clone();
    let hold = 45 + duration as u64; // ~30s detection + the display duration + buffer
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(hold)).await;
        write_quiet(&path);
    });
}

/// Reliable scheduled banner via Notifications.json (kept for restart_banner callers — restart
/// countdown, MOTD). Returns prior file content for restore. @dep restart_banner::write_banner.
pub fn write_scheduled(cfg: &crate::config::Config, message: &str, color: &str, duration_s: u32) -> Option<String> {
    crate::restart_banner::write_banner(cfg, message, color, duration_s)
}

/// No-op: banners now use Notifications.json directly (see fire). Kept so existing callers compile.
pub fn spawn_keeper() {}
