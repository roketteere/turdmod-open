// Elevated (dev-command) users — turdmod-managed mirror of SCUM.db `elevated_users`.
// SCUM reads `elevated_users` at BOOT only (confirmed via community guides + the WAL/SHM lock),
// so we keep the authoritative desired set here and flush it into the DB right before each SCUM
// cold start (engine::start_server PRE-START, DB free). add/remove are instant + restart-free;
// they take effect on the next restart — which the 6h scheduler rides, so no extra downtime.
// @dep: scumdb::{list_elevated, apply_elevated}; engine::start_server calls sync_to_db().
// @inv: never writes SCUM.db live (corruption risk) — only via sync_to_db while SCUM is stopped.

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};
use serde::{Deserialize, Serialize};

const CFG_PATH: &str = r"C:\TurdMOD\data\elevated.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ElevatedCfg {
    /// IDs we want present — re-asserted into the DB every boot (survives any SCUM clobber).
    #[serde(default)]
    pub add: Vec<String>,
    /// IDs we want absent — DELETEd every boot.
    #[serde(default)]
    pub remove: Vec<String>,
}

pub fn load() -> ElevatedCfg {
    std::fs::read_to_string(CFG_PATH).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save(cfg: &ElevatedCfg) {
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(j) = serde_json::to_string_pretty(cfg) {
        let tmp = format!("{}.tmp", CFG_PATH);
        if std::fs::write(&tmp, &j).is_ok() {
            let _ = std::fs::rename(&tmp, CFG_PATH);
        }
    }
}

pub fn valid_steam(s: &str) -> bool {
    s.len() == 17 && s.chars().all(|c| c.is_ascii_digit())
}

/// Stage an add: ensure `id` in `add`, drop it from `remove`.
pub fn add_user(id: &str) {
    let mut c = load();
    c.remove.retain(|x| x != id);
    if !c.add.iter().any(|x| x == id) {
        c.add.push(id.to_string());
    }
    save(&c);
}

/// Stage a removal: ensure `id` in `remove`, drop it from `add`.
pub fn remove_user(id: &str) {
    let mut c = load();
    c.add.retain(|x| x != id);
    if !c.remove.iter().any(|x| x == id) {
        c.remove.push(id.to_string());
    }
    save(&c);
}

/// Flush staged changes into SCUM.db. Call ONLY with SCUM stopped (engine::start_server PRE-START).
pub fn sync_to_db(scumdb_path: &str) {
    let c = load();
    if c.add.is_empty() && c.remove.is_empty() {
        return;
    }
    match crate::scumdb::apply_elevated(scumdb_path, &c.add, &c.remove) {
        Ok(n) => tracing::info!("[elevated] synced to SCUM.db ({} add / {} remove, {} rows changed)", c.add.len(), c.remove.len(), n),
        Err(e) => tracing::warn!("[elevated] sync failed: {}", e),
    }
}

async fn reply(msg: &str, player: &str) {
    pipe_rpc::call("sendChatLineToPlayer", Some(serde_json::json!({
        "message": msg, "playerName": player, "channel": "4"
    }))).await.ok();
}

/// In-game elevated-user management for admins without the TMM app.
/// `!elevate [steamID]` · `!unelevate [steamID]` · `!elevated` (no arg = self). Admin-gated.
pub struct Elevated;

#[async_trait::async_trait]
impl Mod for Elevated {
    fn name(&self) -> &'static str { "elevated_users" }
    fn commands(&self) -> &'static [&'static str] { &["!elevate", "!unelevate", "!elevated"] }

    async fn handle(&self, ev: &GameEvent, ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let cmd = text.split_whitespace().next().unwrap_or("").to_string();
        if !self.commands().contains(&cmd.as_str()) { return Outcome::Ignored; }

        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();

        // Dev powers — Admin/owner only.
        let tier = crate::permissions::player_tier(&*ctx.perms.read().await, &steam);
        if tier < crate::permissions::Tier::Admin {
            reply("[Elevated] Admin only.", &player).await;
            return Outcome::Handled;
        }

        let arg = text.split_whitespace().nth(1).unwrap_or("").trim().to_string();
        match cmd.as_str() {
            "!elevated" => {
                let effective = crate::scumdb::list_elevated(crate::scumdb::db_path(None)).unwrap_or_default();
                let cfg = load();
                let pending = cfg.add.iter().filter(|id| !effective.contains(id)).count();
                reply(&format!(
                    "[Elevated] active now: {} | queued for next restart: {}", effective.len(), pending
                ), &player).await;
            }
            "!elevate" => {
                let target = if arg.is_empty() { steam.clone() } else { arg.clone() };
                if !valid_steam(&target) {
                    reply("[Elevated] Usage: !elevate <17-digit SteamID>  (or !elevate alone for yourself).", &player).await;
                    return Outcome::Handled;
                }
                add_user(&target);
                reply(&format!("[Elevated] {} queued for dev access — applies on next restart (<=6h).", target), &player).await;
            }
            "!unelevate" => {
                let target = if arg.is_empty() { steam.clone() } else { arg.clone() };
                if !valid_steam(&target) {
                    reply("[Elevated] Usage: !unelevate <17-digit SteamID>.", &player).await;
                    return Outcome::Handled;
                }
                remove_user(&target);
                reply(&format!("[Elevated] {} queued for removal — applies on next restart.", target), &player).await;
            }
            _ => {}
        }
        Outcome::Handled
    }
}
