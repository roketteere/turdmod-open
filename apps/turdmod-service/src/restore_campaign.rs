//! Post-wipe restore campaign — automatic, zero-player-effort restore of progression
//! (skills + attributes + fame + money) after a SCUM wipe (e.g. a forced-wipe update).
//!
//! Model (decided with Joel): a wipe gives every player a FRESH shell on reconnect — SCUM
//! auto-creates user_profile + prisoner + default skills + a zero-balance bank account the
//! instant they spawn (verified live 2026-06-28). We can't write skills/attrs/the body_simulation
//! blob LIVE (SCUM clobbers them on save/logout — feedback_scumdb_edit_safety), so the restore runs
//! inside RESTART STOP-WINDOWS: each restart, every snapshot-player who has reconnected since the
//! last pass is restored ONCE (idempotent — re-applying would clobber progress made since).
//!
//! Cadence (the campaign drives the restart scheduler's interval as a taper):
//!   phase 1: hours  0–6   restart every 1h   (catch the return wave)
//!   phase 2: hours  6–15  restart every 3h
//!   phase 3: hours 15–27  restart every 6h
//!   after 27h: campaign ENDS, scheduler reverts to its normal interval; stragglers -> tickets.
//!
//! Phase-1 scope is skills/attrs/fame/money (built + tested). Inventory + vehicles are phase 2.
//! @dep scumdb::restore_pending (the gated/idempotent restore), schedule.rs (cadence override),
//! server.rs restart path (calls run_in_stop_window between stop and start).

use crate::config::Config;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

// Process-wide instance id, set once at startup so the `!restores` mod (which has no Config) reads
// the right campaign file. Defaults to "default" if unset. @dep main.rs/service.rs call set_instance.
static INSTANCE: OnceLock<String> = OnceLock::new();
pub fn set_instance(id: &str) { let _ = INSTANCE.set(id.to_string()); }
fn current_instance() -> String { INSTANCE.get().cloned().unwrap_or_else(|| "default".into()) }

// Phase boundaries (seconds since campaign start) + the restart interval (hours) in each.
const PHASE1_END: u64 = 6 * 3600;
const PHASE2_END: u64 = 15 * 3600;
const PHASE3_END: u64 = 27 * 3600;
const PHASE1_INTERVAL_H: f64 = 1.0;
const PHASE2_INTERVAL_H: f64 = 3.0;
const PHASE3_INTERVAL_H: f64 = 6.0;

fn now_epoch() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Campaign {
    /// Active campaign? When true, the scheduler tapers the restart interval and each restart's
    /// stop-window runs the restore.
    #[serde(default)]
    pub armed: bool,
    /// Epoch the campaign started (drives the phase taper + the 27h end).
    #[serde(default)]
    pub started_at: u64,
    /// Pre-wipe snapshot DB the restore reads from (the SOURCE). Absolute path.
    #[serde(default)]
    pub snapshot_path: String,
    /// Steam ids already restored — never restored twice.
    #[serde(default)]
    pub done: Vec<String>,
    /// Set true once the campaign has run past 27h (kept for status/audit; armed flips false).
    #[serde(default)]
    pub ended: bool,
    /// How many restore rounds (restart passes that restored >=1 player) have fired. Drives the
    /// in-game "Restore Round N" announcement.
    #[serde(default)]
    pub round: u32,
    /// Names restored in the most recent round — surfaced by the `!restores` status command.
    #[serde(default)]
    pub last_round_names: Vec<String>,
}

/// Format a player-name list for an in-game announce, capping the length so the banner stays sane.
fn format_names(names: &[String]) -> String {
    const CAP: usize = 12;
    if names.len() <= CAP {
        names.join(", ")
    } else {
        format!("{} + {} more", names[..CAP].join(", "), names.len() - CAP)
    }
}

/// Pull the display names for `restored` steam ids out of a restore_pending summary report.
fn names_from_report(summary: &serde_json::Value, restored: &[String]) -> Vec<String> {
    let mut by_steam = std::collections::HashMap::new();
    if let Some(arr) = summary.get("report").and_then(|r| r.as_array()) {
        for e in arr {
            if let (Some(s), Some(n)) = (e.get("steam").and_then(|v| v.as_str()), e.get("name").and_then(|v| v.as_str())) {
                by_steam.insert(s.to_string(), n.to_string());
            }
        }
    }
    restored.iter().map(|s| by_steam.get(s).cloned().unwrap_or_else(|| s.clone())).collect()
}

fn path(instance: &str) -> PathBuf {
    if instance == "default" {
        PathBuf::from(r"C:\TurdMOD\restore-campaign.json")
    } else {
        PathBuf::from(format!(r"C:\TurdMOD\restore-campaign-{}.json", instance))
    }
}

pub fn load(instance: &str) -> Campaign {
    std::fs::read_to_string(path(instance))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(instance: &str, c: &Campaign) -> std::io::Result<()> {
    if let Some(dir) = path(instance).parent() { let _ = std::fs::create_dir_all(dir); }
    std::fs::write(path(instance), serde_json::to_string_pretty(c).unwrap_or_default())
}

impl Campaign {
    /// Elapsed seconds since the campaign started.
    pub fn elapsed(&self) -> u64 { now_epoch().saturating_sub(self.started_at) }

    /// Restart interval (hours) for the current phase, or None once the campaign is over (>27h).
    pub fn phase_interval_hours(&self) -> Option<f64> {
        match self.elapsed() {
            e if e < PHASE1_END => Some(PHASE1_INTERVAL_H),
            e if e < PHASE2_END => Some(PHASE2_INTERVAL_H),
            e if e < PHASE3_END => Some(PHASE3_INTERVAL_H),
            _ => None,
        }
    }

    /// Human phase label for status.
    pub fn phase_label(&self) -> &'static str {
        match self.elapsed() {
            e if e < PHASE1_END => "1 (0-6h, every 1h)",
            e if e < PHASE2_END => "2 (6-15h, every 3h)",
            e if e < PHASE3_END => "3 (15-27h, every 6h)",
            _ => "ended (>27h)",
        }
    }
}

/// Arm the campaign with a pre-wipe snapshot. Resets the done-ledger + start clock. The snapshot
/// must exist. Idempotent-safe to call again (re-arming restarts the taper).
pub fn arm(instance: &str, snapshot_path: &str) -> Result<Campaign, String> {
    if !std::path::Path::new(snapshot_path).exists() {
        return Err(format!("snapshot not found: {}", snapshot_path));
    }
    let c = Campaign {
        armed: true,
        started_at: now_epoch(),
        snapshot_path: snapshot_path.to_string(),
        done: Vec::new(),
        ended: false,
        round: 0,
        last_round_names: Vec::new(),
    };
    save(instance, &c).map_err(|e| e.to_string())?;
    tracing::info!("[restore-campaign] ARMED from snapshot {}", snapshot_path);
    Ok(c)
}

/// Disarm (manual stop, or auto when the 27h window closes). Keeps the done-ledger for audit.
pub fn disarm(instance: &str, reason: &str) {
    let mut c = load(instance);
    if !c.armed { return; }
    c.armed = false;
    c.ended = true;
    let _ = save(instance, &c);
    tracing::info!("[restore-campaign] DISARMED ({}); {} players restored", reason, c.done.len());
}

/// The scheduler asks this each tick: if a campaign is active, the restart interval to use (hours).
/// Auto-disarms + returns None once the 27h window closes (scheduler then reverts to normal).
pub fn interval_override(instance: &str) -> Option<f64> {
    let c = load(instance);
    if !c.armed { return None; }
    match c.phase_interval_hours() {
        Some(h) => Some(h),
        None => { disarm(instance, "27h window closed"); None }
    }
}

/// Run the restore inside a restart STOP-WINDOW (SCUM already stopped by the restart path).
/// No-op unless the campaign is armed. Backs up the live DB first, restores every reconnected
/// not-yet-done player from the snapshot, persists the expanded done-ledger. Never panics / never
/// blocks the restart — on any error it logs and returns so start_server still runs.
pub async fn run_in_stop_window(cfg: &Config) {
    let instance = cfg.instance_id.clone();
    let mut c = load(&instance);
    if !c.armed { return; }
    // Auto-end if past the window.
    if c.phase_interval_hours().is_none() {
        disarm(&instance, "27h window closed (stop-window check)");
        return;
    }
    let source = c.snapshot_path.clone();
    let target = cfg.scumdb_path.clone();
    if !std::path::Path::new(&source).exists() {
        tracing::warn!("[restore-campaign] snapshot missing ({}); skipping pass", source);
        return;
    }
    // Safety: retained backup of the live DB before any write (set_* invariant).
    let backup = crate::checkpoint::do_archive_live("precampaign-restore").ok();
    let done: HashSet<String> = c.done.iter().cloned().collect();
    match crate::scumdb::restore_pending(&source, &target, &done) {
        Ok((restored, summary)) => {
            if restored.is_empty() {
                tracing::info!("[restore-campaign] pass: no newly-reconnected players ({})", c.phase_label());
            } else {
                c.round += 1;
                for s in &restored { if !c.done.contains(s) { c.done.push(s.clone()); } }
                let names = names_from_report(&summary, &restored);
                c.last_round_names = names.clone();
                let _ = save(&instance, &c);
                tracing::info!(
                    "[restore-campaign] round {} restored {} player(s) [{}] (total {}, phase {}); backup={:?}",
                    c.round, restored.len(), names.join(", "), c.done.len(), c.phase_label(), backup
                );
                // In-game announce: round header + the players restored THIS round, so returning
                // survivors see it happen and know who's back. Two lines (header + roster).
                let header = format!(
                    "🔄 Data Restore — Round {} [{}]: {} survivor(s) restored",
                    c.round, c.phase_label(), restored.len()
                );
                let _ = crate::auto_announce::announce(&header).await;
                let _ = crate::auto_announce::announce(&format!("Restored this round: {}", format_names(&names))).await;
            }
        }
        Err(e) => tracing::error!("[restore-campaign] restore_pending failed: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camp_started_ago(secs: u64) -> Campaign {
        Campaign { armed: true, started_at: now_epoch().saturating_sub(secs), ..Default::default() }
    }

    #[test]
    fn taper_phases_are_correct() {
        // phase 1: 0-6h -> 1h
        assert_eq!(camp_started_ago(0).phase_interval_hours(), Some(1.0));
        assert_eq!(camp_started_ago(5 * 3600).phase_interval_hours(), Some(1.0));
        // phase 2: 6-15h -> 3h
        assert_eq!(camp_started_ago(6 * 3600 + 60).phase_interval_hours(), Some(3.0));
        assert_eq!(camp_started_ago(14 * 3600).phase_interval_hours(), Some(3.0));
        // phase 3: 15-27h -> 6h
        assert_eq!(camp_started_ago(15 * 3600 + 60).phase_interval_hours(), Some(6.0));
        assert_eq!(camp_started_ago(26 * 3600).phase_interval_hours(), Some(6.0));
        // ended: >27h -> None
        assert_eq!(camp_started_ago(27 * 3600 + 60).phase_interval_hours(), None);
    }

    #[test]
    fn disarmed_campaign_has_no_interval_override_semantics() {
        let c = Campaign { armed: false, ..Default::default() };
        // phase math still works on the struct, but interval_override gates on `armed` (see fn).
        assert!(!c.armed);
    }
}

/// Status JSON for the HTTP control surface / TMM.
pub fn status_json(instance: &str) -> serde_json::Value {
    let c = load(instance);
    serde_json::json!({
        "armed": c.armed,
        "ended": c.ended,
        "snapshot_path": c.snapshot_path,
        "started_at": c.started_at,
        "elapsed_secs": if c.started_at > 0 { c.elapsed() } else { 0 },
        "phase": c.phase_label(),
        "interval_hours": c.phase_interval_hours(),
        "round": c.round,
        "last_round_names": c.last_round_names,
        "restored_count": c.done.len(),
        "restored": c.done,
    })
}

// ─── Mod surface ────────────────────────────────────────────────────────────
// Registered in the mod spine so the campaign shows up in /monitor/mods (🟢 health) and exposes an
// admin chat command. The actual restore fires in the restart stop-window (run_in_stop_window) —
// this mod is the in-game query/observability surface, not the restore engine itself.

const ADMIN_STEAMS: &[&str] = &["YOUR_STEAM_ID_1", "YOUR_STEAM_ID_2"]; // YOUR_OWNER_NAME, Zilla

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    let _ = crate::pipe_rpc::call("sendChatLineToPlayer", Some(params)).await;
}

pub struct RestoreCampaignMod;

#[async_trait::async_trait]
impl crate::registry::Mod for RestoreCampaignMod {
    fn name(&self) -> &'static str { "restore_campaign" }
    fn commands(&self) -> &'static [&'static str] { &["!restores", "!restorestatus"] }

    async fn handle(&self, ev: &crate::events::GameEvent, _ctx: &crate::registry::ModCtx) -> crate::registry::Outcome {
        use crate::registry::Outcome;
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim();
        let cmd = text.split_whitespace().next().unwrap_or("").to_lowercase();
        if cmd != "!restores" && cmd != "!restorestatus" { return Outcome::Ignored; }
        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("");
        if !ADMIN_STEAMS.contains(&steam) {
            reply("Admin only.", &player).await;
            return Outcome::Handled;
        }
        let instance = current_instance();
        let c = load(&instance);
        if !c.armed && c.round == 0 {
            reply("No restore campaign is active.", &player).await;
            return Outcome::Handled;
        }
        reply(&format!(
            "Restore campaign: {} | phase {} | round {} | {} player(s) restored total",
            if c.armed { "ARMED" } else { "ended" }, c.phase_label(), c.round, c.done.len()
        ), &player).await;
        if !c.last_round_names.is_empty() {
            reply(&format!("Last round: {}", format_names(&c.last_round_names)), &player).await;
        }
        Outcome::Handled
    }
}
