// Loot multiplier - admin adjusts item spawn probability via config writes.
// !loot <multiplier> / !loot status / !loot reset. Command-only.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const OWNER_STEAM_ID: &str = "YOUR_STEAM_ID_1";
const OWNER_NAME: &str = "YOUR_OWNER_NAME";
const RATE_LIMIT: Duration = Duration::from_secs(3);

fn is_owner(steam: &str, player: &str) -> bool {
    steam == OWNER_STEAM_ID || steam == "YOUR_STEAM_ID_2" || player == OWNER_NAME
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

async fn broadcast(msg: &str) {
    crate::auto_announce::announce(msg).await; // server-wide event -> #Announce banner + chat
    let params = serde_json::json!({ "text": msg });
    pipe_rpc::call("broadcastChat", Some(params)).await.ok();
}

pub struct LootMultiplier { mult: Mutex<f64>, rate: Mutex<HashMap<String, Instant>> }
impl LootMultiplier {
    pub fn new() -> Self { Self { mult: Mutex::new(1.0), rate: Mutex::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl Mod for LootMultiplier {
    fn name(&self) -> &'static str { "loot_multiplier" }
    fn commands(&self) -> &'static [&'static str] { &["!loot"] }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let parts: Vec<&str> = text.split_whitespace().collect();
        if parts.first().map(|s| s.to_lowercase()).unwrap_or_default() != "!loot" { return Outcome::Ignored; }

        let rate_key = if steam.is_empty() { player.clone() } else { steam.clone() };
        {
            let mut rate = self.rate.lock().await;
            let now = Instant::now();
            if let Some(prev) = rate.get(&rate_key) {
                if now.duration_since(*prev) < RATE_LIMIT { return Outcome::Ignored; }
            }
            rate.insert(rate_key.clone(), now);
        }

        let sub = parts.get(1).map(|s| s.to_lowercase()).unwrap_or_default();
        match sub.as_str() {
            "status" | "" => {
                let m = *self.mult.lock().await;
                reply(&format!("[Loot] Current multiplier: {:.1}x", m), &player).await;
                Outcome::Handled
            }
            "reset" => {
                if !is_owner(&steam, &player) { reply("[Loot] Admin only.", &player).await; return Outcome::Handled; }
                *self.mult.lock().await = 1.0;
                let params = serde_json::json!({ "section": "ServerSettings", "key": "ItemSpawnProbability", "value": "1.0" });
                pipe_rpc::call("writeConfig", Some(params)).await.ok();
                broadcast("[Loot] Spawn rate reset to 1x.").await;
                Outcome::Handled
            }
            _ => {
                if !is_owner(&steam, &player) { reply("[Loot] Admin only.", &player).await; return Outcome::Handled; }
                let mult: f64 = sub.parse().unwrap_or(0.0);
                if mult < 0.1 || mult > 10.0 {
                    reply("[Loot] Multiplier must be 0.1 - 10.0", &player).await;
                    return Outcome::Handled;
                }
                *self.mult.lock().await = mult;
                let params = serde_json::json!({ "section": "ServerSettings", "key": "ItemSpawnProbability", "value": format!("{:.1}", mult) });
                pipe_rpc::call("writeConfig", Some(params)).await.ok();
                broadcast(&format!("[Loot] Spawn rate set to {:.1}x!", mult)).await;
                Outcome::Handled
            }
        }
    }
}
