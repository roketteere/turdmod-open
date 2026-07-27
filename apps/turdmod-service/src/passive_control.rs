// Passive control - admin toggles zombie/animal hostility LIVE (no restart, no crash).
// !zombies passive on/off, !animals passive on/off (admin). "on" = passive/friendly, ENFORCED
// every 30s (catches newly-spawned). Big colored center banner fires on each toggle.
// Replaces the old always-on friendly_passive block. @dep setZombiePassive/setAnimalPassive (live-safe).

use std::time::Duration;
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const ENFORCE_INTERVAL: Duration = Duration::from_secs(30);

fn is_owner(steam: &str, player: &str) -> bool {
    crate::owner::is_owner(steam, player)
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

// Default: zombies HOSTILE (off), animals passive. Zombie-passive is a special-occasion
// toggle (`!zombies passive on`), not the normal state — so it must default OFF.
struct PassiveState { zombies: bool, animals: bool }

pub struct PassiveControl { state: Mutex<PassiveState> }
impl PassiveControl {
    pub fn new() -> Self { Self { state: Mutex::new(PassiveState { zombies: false, animals: true }) } }
}

#[async_trait::async_trait]
impl Mod for PassiveControl {
    fn name(&self) -> &'static str { "passive_control" }
    fn commands(&self) -> &'static [&'static str] { &["!zombies", "!animals"] }
    fn interval(&self) -> Option<Duration> { Some(ENFORCE_INTERVAL) }

    // Re-assert passivity every 30s for whichever types are ON (newly-spawned puppets spawn hostile).
    async fn tick(&self, _ctx: &ModCtx) {
        let (z, a) = { let s = self.state.lock().await; (s.zombies, s.animals) };
        if z {
            if let Ok(v) = pipe_rpc::call("setZombiePassive", Some(serde_json::json!({ "passive": true }))).await {
                let n = v.get("modified").and_then(|x| x.as_u64()).unwrap_or(0);
                if n > 0 { tracing::info!("passive_control: {} puppets kept passive", n); }
            }
        }
        if a {
            if let Ok(v) = pipe_rpc::call("setAnimalPassive", Some(serde_json::json!({ "passive": true }))).await {
                let n = v.get("modified").and_then(|x| x.as_u64()).unwrap_or(0);
                if n > 0 { tracing::info!("passive_control: {} animals kept passive", n); }
            }
        }
    }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let parts: Vec<&str> = text.split_whitespace().collect();
        let cmd = parts.first().map(|s| s.to_lowercase()).unwrap_or_default();
        if !matches!(cmd.as_str(), "!zombies" | "!animals") { return Outcome::Ignored; }

        let on = parts.iter().any(|p| p.eq_ignore_ascii_case("on"));
        let off = parts.iter().any(|p| p.eq_ignore_ascii_case("off"));
        let kind = if cmd == "!zombies" { "Zombies" } else { "Animals" };

        // Status query (no on/off) — anyone can check.
        if on == off {
            let cur = { let s = self.state.lock().await; if cmd == "!zombies" { s.zombies } else { s.animals } };
            reply(&format!("[Passive] {} are currently {}. Admin: {} passive on/off", kind,
                if cur { "PASSIVE (friendly)" } else { "HOSTILE" }, cmd), &player).await;
            return Outcome::Handled;
        }

        if !is_owner(&steam, &player) { reply("[Passive] Admin only.", &player).await; return Outcome::Handled; }
        let enable = on; // "on" = passive/friendly

        // Update the enforced flag + apply immediately via the bridge (live-safe).
        {
            let mut s = self.state.lock().await;
            if cmd == "!zombies" { s.zombies = enable; } else { s.animals = enable; }
        }
        let rpc = if cmd == "!zombies" { "setZombiePassive" } else { "setAnimalPassive" };
        pipe_rpc::call(rpc, Some(serde_json::json!({ "passive": enable }))).await.ok();

        // Big colored center banner.
        if enable {
            crate::banner::fire(&format!("{} are now your friends!", kind), 80, 200, 80, 8).await;
        } else {
            crate::banner::fire(&format!("{} are HOSTILE again!", kind), 200, 40, 40, 8).await;
        }
        reply(&format!("[Passive] {} now {}.", kind, if enable { "PASSIVE" } else { "HOSTILE" }), &player).await;
        Outcome::Handled
    }
}
