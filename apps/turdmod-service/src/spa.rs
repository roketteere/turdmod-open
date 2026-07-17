// spa — the "spa day" wellness pass. Every 2h (and on the manual `!spa` / `!sfsc`
// admin triggers, which are aliases) every online player's clothes are CLEANED +
// EXHAUSTION reset + clothes REPAIRED to full durability, then the "So Fresh, So
// Clean!" banner fires. Mirrors a feature Whalley runs. @ctx Joel: auto every 2h;
// !spa and !sfsc are the manual triggers (same effect).
//
// @dep bridge handlers: cleanAllClothes, cleanAllExhaustion, repairAllClothes (all
//   server-wide, no gate). [[reference_spa_and_developer_gate]].
// @inv spa_loop is a SEPARATE chat subscriber, so it self-gates !spa/!sfsc (the
//   chat_cmds permission gate doesn't cover this loop). Owner-only, like metabolism.rs.

use std::time::Duration;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const SPA_INTERVAL: Duration = Duration::from_secs(2 * 60 * 60); // every 2h
const OWNER_IDS: [&str; 2]   = ["YOUR_STEAM_ID_1", "YOUR_STEAM_ID_2"];
const BANNER: &str           = "So Fresh, So Clean!";

fn is_owner(steam: &str, player: &str) -> bool {
    OWNER_IDS.contains(&steam) || player == "YOUR_OWNER_NAME"
}

/// Clean clothes + restore stamina for every online player, then fire the banner.
/// Skips the banner if nobody's online. Bridge calls are best-effort (never panic).
async fn do_spa() {
    let players = match pipe_rpc::call("getOnlinePlayers", Some(serde_json::json!({}))).await {
        Ok(r) => r.get("players").and_then(|v| v.as_array()).cloned().unwrap_or_default(),
        Err(_) => return,
    };
    if players.is_empty() { return; }

    // 1) Clean every online player's equipped clothes — ONE server-wide bridge call
    //    (cleanAllClothes scans all equipped ClothesItem instances + SetDirtiness(0)).
    let cleaned = pipe_rpc::call("cleanAllClothes", Some(serde_json::json!({}))).await
        .ok().and_then(|r| r.get("cleaned").and_then(|v| v.as_i64())).unwrap_or(0);

    // 2) Reset every online player's exhaustion — server-wide bridge call.
    //    cleanAllExhaustion scans all active Exhaustion_C body-effects + zeroes
    //    _exhaustionAmount @200. Confirmed in-game 2026-06-09 (6.08 -> 0, no gate).
    //    Stamina BAR intentionally untouched — exhaustion is the target (Joel).
    let exh = pipe_rpc::call("cleanAllExhaustion", Some(serde_json::json!({}))).await
        .ok().and_then(|r| r.get("cleared").and_then(|v| v.as_i64())).unwrap_or(0);

    // 3) Repair every equipped clothes item to full durability — server-wide bridge call
    //    (repairAllClothes scans equipped ClothesItem + Item::SetHealth(GetMaxHealth())).
    let repaired = pipe_rpc::call("repairAllClothes", Some(serde_json::json!({}))).await
        .ok().and_then(|r| r.get("repaired").and_then(|v| v.as_i64())).unwrap_or(0);

    // 4) "So Fresh, So Clean!" — colored center banner via the bridge.
    fire_spa_banner(BANNER, 0, 230, 255, 10).await;
    tracing::info!("[spa] ran for {} player(s): cleaned {}, exhaustion {}, repaired {}",
        players.len(), cleaned, exh, repaired);
}

/// Fire the "So Fresh, So Clean!" banner. Now uses the instant zero-admin
/// #Announce banner ([[reference_engine_rpc_cli]] proved the bypass) instead
/// of the ~30s Notifications.json path. Color params are kept for callers but
/// ignored for now — #Announce renders white; the colored-instant path is the
/// next RE target. @dep crate::auto_announce::announce.
pub(crate) async fn fire_spa_banner(text: &str, _r: u8, _g: u8, _b: u8, _duration: u32) {
    crate::auto_announce::announce(text).await;
}

/// MIGRATED to the registry spine (2026-06-10): the 2h auto pass is now `tick` (the registry's
/// scheduled-mod support), and `!spa`/`!sfsc` are command claims. Owner-gated like before.
pub struct Spa;

#[async_trait::async_trait]
impl Mod for Spa {
    fn name(&self) -> &'static str { "spa" }
    fn commands(&self) -> &'static [&'static str] { &["!spa", "!sfsc"] }
    fn interval(&self) -> Option<Duration> { Some(SPA_INTERVAL) }
    fn timeout(&self) -> Duration { Duration::from_secs(60) } // do_spa makes several bridge calls

    async fn tick(&self, _ctx: &ModCtx) {
        tracing::info!("[spa] 2h auto spa firing");
        do_spa().await;
    }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let cmd = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_ascii_lowercase();
        if cmd != "!spa" && cmd != "!sfsc" { return Outcome::Ignored; }
        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam  = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if !is_owner(&steam, &player) {
            pipe_rpc::call("sendChatLineToPlayer", Some(serde_json::json!({
                "message": "[Spa] Admin only.", "playerName": player, "channel": "4"
            }))).await.ok();
            return Outcome::Handled;
        }
        tracing::info!("[spa] manual {} by {}", cmd, player);
        do_spa().await;
        Outcome::Handled
    }
}
