// Horde mode - admin spawns waves of zombies/animals around a player.
// !horde <waves> - spawn increasing zombie waves for PvE fun.
// Uses provokeZombies to make them attack, setZombiePassive to reset after.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const RATE_LIMIT: Duration = Duration::from_secs(5);
const WAVE_INTERVAL: Duration = Duration::from_secs(45);

fn is_owner(steam: &str, player: &str) -> bool {
    crate::owner::is_owner(steam, player)
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

async fn broadcast_msg(msg: &str) {
    crate::auto_announce::announce(msg).await; // server-wide event -> #Announce banner + chat
    let params = serde_json::json!({ "text": msg });
    pipe_rpc::call("broadcastChat", Some(params)).await.ok();
}

pub struct Horde { rate: Mutex<HashMap<String, Instant>> }
impl Horde {
    pub fn new() -> Self { Self { rate: Mutex::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl Mod for Horde {
    fn name(&self) -> &'static str { "horde" }
    fn commands(&self) -> &'static [&'static str] { &["!horde", "!purge"] }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        if !text.starts_with('!') { return Outcome::Ignored; }

        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let rate_key = if steam.is_empty() { player.clone() } else { steam.clone() };

        {
            let mut rate = self.rate.lock().await;
            let now = Instant::now();
            if let Some(prev) = rate.get(&rate_key) {
                if now.duration_since(*prev) < RATE_LIMIT { return Outcome::Ignored; }
            }
            rate.insert(rate_key.clone(), now);
        }

        let (cmd, args) = match text.find(' ') {
            Some(i) => (text[..i].to_lowercase(), text[i + 1..].trim().to_string()),
            None => (text.to_lowercase(), String::new()),
        };

        if !matches!(cmd.as_str(), "!horde" | "!purge") { return Outcome::Ignored; }

        match cmd.as_str() {
            "!horde" => {
                if !is_owner(&steam, &player) { reply("[Horde] Admin only.", &player).await; return Outcome::Handled; }
                let waves: u32 = args.parse().unwrap_or(3);
                let waves = waves.min(10); // cap at 10
                let p = player.clone();
                tokio::spawn(async move {
                    broadcast_msg(&format!("[Horde] {} waves incoming for {}! Survive!", waves, p)).await;
                    for wave in 1..=waves {
                        let provoke_count = (wave * 3).min(15);
                        tokio::time::sleep(Duration::from_secs(3)).await;
                        broadcast_msg(&format!("[Horde] Wave {}/{} - {} zombies attacking!", wave, waves, provoke_count)).await;

                        // Provoke nearby zombies
                        let mode = if provoke_count > 5 { "ranged" } else { "melee" };
                        let params = serde_json::json!({
                            "playerName": p,
                            "mode": mode
                        });
                        pipe_rpc::call("provokeZombies", Some(params)).await.ok();

                        if wave < waves {
                            tokio::time::sleep(WAVE_INTERVAL).await;
                        }
                    }
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    broadcast_msg(&format!("[Horde] All waves complete! {} survived!", p)).await;
                    // Re-pacify all zombies
                    pipe_rpc::call("setZombiePassive", Some(serde_json::json!({ "passive": true }))).await.ok();
                });
                Outcome::Handled
            }

            "!purge" => {
                if !is_owner(&steam, &player) { reply("[Horde] Admin only.", &player).await; return Outcome::Handled; }
                // Re-pacify everything
                pipe_rpc::call("setZombiePassive", Some(serde_json::json!({ "passive": true }))).await.ok();
                pipe_rpc::call("setAnimalPassive", Some(serde_json::json!({ "passive": true }))).await.ok();
                broadcast_msg("[Horde] All creatures pacified.").await;
                Outcome::Handled
            }

            _ => Outcome::Ignored,
        }
    }
}
