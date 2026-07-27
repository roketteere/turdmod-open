// hitmebaby — clothes-repair pass. Restores full durability to equipped clothes via the
// repairAllClothes bridge handler (Item::SetHealth(GetMaxHealth) on every equipped ClothesItem
// — same proven pattern as cleanAllClothes). Confirmed in-game 2026-06-09 (repaired 7/7).
//
// MIGRATED to the registry spine (2026-06-10): was a standalone chat-subscriber loop; now a
// `registry::Mod` claiming "!hitmebaby". First-wave proof of the spine — both console + service
// drive it through the one registered() list, so it can't drift out of production again.
//
// @ctx Joel's model: "!hitmebaby" = the player command (premium/VIP/admin), "!hitmebaby all" =
//   admin server-wide + the big banner. v1: owner-gated, server-wide for BOTH.
// @dep bridge: repairAllClothes; [[spa::fire_spa_banner]]; [[registry]].

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};


fn is_owner(steam: &str, player: &str) -> bool {
    crate::owner::is_owner_steam(steam) || player == crate::owner::name()
}

async fn do_repair() -> i64 {
    pipe_rpc::call("repairAllClothes", Some(serde_json::json!({}))).await
        .ok().and_then(|r| r.get("repaired").and_then(|v| v.as_i64())).unwrap_or(0)
}

/// `!hitmebaby` [`all`] — owner-gated clothes repair + gold banner. Registered mod.
pub struct HitMeBaby;

#[async_trait::async_trait]
impl Mod for HitMeBaby {
    fn name(&self) -> &'static str { "hitmebaby" }
    fn commands(&self) -> &'static [&'static str] { &["!hitmebaby"] }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim();
        let lc = text.to_ascii_lowercase();
        if lc != "!hitmebaby" && lc != "!hitmebaby all" { return Outcome::Ignored; }

        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam  = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if !is_owner(&steam, &player) {
            pipe_rpc::call("sendChatLineToPlayer", Some(serde_json::json!({
                "message": "[Repair] Admin only.", "playerName": player, "channel": "4"
            }))).await.ok();
            return Outcome::Handled;
        }

        let repaired = do_repair().await;
        tracing::info!("[hitmebaby] {} repaired {} clothes (all={})", player, repaired, lc.ends_with("all"));
        // gold banner — fires for both !hitmebaby and !hitmebaby all
        crate::spa::fire_spa_banner("Fresh Threads - Fully Repaired!", 255, 215, 0, 10).await;
        Outcome::Handled
    }
}
