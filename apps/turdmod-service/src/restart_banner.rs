// restart_banner — escalating COLORED center-screen banners before a restart, written
// to SCUM's Notifications.json. Proven (2026-06-09): the NotificationsManager RE-READS
// Notifications.json on each fire (no boot caching), so writing the file changes the
// live banner with NO restart — fires on the manager's cycle (~30-60s granularity).
// @inv path derived from cfg.scum_server_exe:
//   ...\SCUM\Binaries\Win64\SCUMServer.exe -> ...\SCUM\Saved\Config\WindowsServer\Notifications.json
// The second-level countdown (30/15/10/5s) stays as bottom-left chat in server.rs.
// @dep [[reference_colored_banners]]. Best-effort: all fs errors swallowed (a failed
// banner write must never block/abort the actual restart).

use std::path::PathBuf;
use crate::config::Config;

pub fn notif_path(cfg: &Config) -> Option<PathBuf> {
    let exe = PathBuf::from(&cfg.scum_server_exe);
    let scum = exe.parent()?  // Win64
        .parent()?            // Binaries
        .parent()?;           // SCUM
    Some(scum.join("Saved").join("Config").join("WindowsServer").join("Notifications.json"))
}

/// Overwrite Notifications.json with a single colored banner. Fires on the manager's
/// next cycle (re-read live). Returns the PRIOR file content (so the first call's return
/// is the real rotation, for restore). `color` is "R-G-B" (0-255). Best-effort.
pub fn write_banner(cfg: &Config, message: &str, color: &str, duration_s: u32) -> Option<String> {
    let path = notif_path(cfg)?;
    let prev = std::fs::read_to_string(&path).ok();
    // sanitize: keep JSON valid + single-line
    let safe: String = message.chars()
        .map(|c| match c { '"' => '\'', '\\' => '/', '\r' | '\n' => ' ', _ => c })
        .collect();
    let json = format!(
        "{{\n  \"Notifications\": [\n    {{ \"day\": \"Everyday\", \"duration\": \"{}\", \"color\": \"{}\", \"wait\": \"0\", \"message\": \"{}\" }}\n  ]\n}}",
        duration_s, color, safe
    );
    if std::fs::write(&path, json).is_err() { return prev; }
    prev
}

/// Restore the saved Notifications.json content (the real rotation) so the post-restart
/// boot loads the normal banners, not a stale countdown entry.
pub fn restore(cfg: &Config, content: &str) {
    if let Some(path) = notif_path(cfg) { let _ = std::fs::write(path, content); }
}
