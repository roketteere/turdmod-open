// Anti-combat-log - punishes players who disconnect during combat.
// Event-driven: `kill` records last-damage time; `logout` within 30s of damage = kick.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const COMBAT_WINDOW: Duration = Duration::from_secs(30);

async fn broadcast(msg: &str) {
    let params = serde_json::json!({ "text": msg });
    pipe_rpc::call("broadcastChat", Some(params)).await.ok();
}

pub struct CombatLog { last_damage: Mutex<HashMap<String, Instant>> }
impl CombatLog {
    pub fn new() -> Self { Self { last_damage: Mutex::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl Mod for CombatLog {
    fn name(&self) -> &'static str { "combat_log" }
    // event-driven (no commands()): tracks `kill` damage + punishes `logout` mid-combat.

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        match ev.event.as_str() {
            "kill" => {
                let victim_steam = ev.data.get("victimSteam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let killer_steam = ev.data.get("killerSteam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let mut ld = self.last_damage.lock().await;
                if !victim_steam.is_empty() { ld.insert(victim_steam, Instant::now()); }
                if !killer_steam.is_empty() { ld.insert(killer_steam, Instant::now()); }
                Outcome::Ignored // silent tracking, no activity-feed noise
            }
            "logout" => {
                let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let punish = {
                    let mut ld = self.last_damage.lock().await;
                    let recent = ld.get(&steam).map(|t| t.elapsed() < COMBAT_WINDOW).unwrap_or(false);
                    ld.remove(&steam);
                    recent
                };
                if punish {
                    broadcast(&format!("[Combat Log] {} disconnected during combat! Coward penalty applied.", player)).await;
                    let params = serde_json::json!({ "playerName": player, "reason": "combat logging" });
                    pipe_rpc::call("kickPlayer", Some(params)).await.ok();
                    tracing::info!("combat_log: {} combat-logged", player);
                    Outcome::Handled
                } else {
                    Outcome::Ignored
                }
            }
            _ => Outcome::Ignored,
        }
    }
}
