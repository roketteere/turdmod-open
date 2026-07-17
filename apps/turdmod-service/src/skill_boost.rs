// Skill boost events - temporary XP multiplier for all players.
// !xpboost <multiplier> <minutes> (admin) / !xpstatus. Bonus fame on kill while active.
// Event-driven (needs kill); a 10s tick ends expired boosts.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const OWNER_STEAM_ID: &str = "YOUR_STEAM_ID_1";
const OWNER_NAME: &str = "YOUR_OWNER_NAME";
const BASE_KILL_XP: f64 = 50.0;
const RATE_LIMIT: Duration = Duration::from_secs(3);

fn is_owner(steam: &str, player: &str) -> bool {
    steam == OWNER_STEAM_ID || steam == "YOUR_STEAM_ID_2" || player == OWNER_NAME
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

async fn broadcast(msg: &str) {
    let params = serde_json::json!({ "text": msg });
    pipe_rpc::call("broadcastChat", Some(params)).await.ok();
}

struct BoostState { multiplier: f64, expires: Option<Instant> }

pub struct SkillBoost { state: Mutex<BoostState>, rate: Mutex<HashMap<String, Instant>> }
impl SkillBoost {
    pub fn new() -> Self { Self { state: Mutex::new(BoostState { multiplier: 1.0, expires: None }), rate: Mutex::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl Mod for SkillBoost {
    fn name(&self) -> &'static str { "skill_boost" }
    // event-driven (no commands()): needs `kill` events plus !xpboost/!xpstatus.
    fn interval(&self) -> Option<Duration> { Some(Duration::from_secs(10)) }

    async fn tick(&self, _ctx: &ModCtx) {
        let ended = {
            let mut st = self.state.lock().await;
            match st.expires {
                Some(exp) if Instant::now() >= exp => { st.multiplier = 1.0; st.expires = None; true }
                _ => false,
            }
        };
        if ended { broadcast("[XP] Bonus XP event has ENDED. Back to normal rates.").await; }
    }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        match ev.event.as_str() {
            "kill" => {
                let mult = { self.state.lock().await.multiplier };
                if mult <= 1.0 { return Outcome::Ignored; }
                let killer = ev.data.get("killer").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let killer_steam = ev.data.get("killerSteam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if killer_steam.is_empty() { return Outcome::Ignored; }
                let bonus = (BASE_KILL_XP * (mult - 1.0)) as i64;
                if bonus <= 0 { return Outcome::Ignored; }
                let params = serde_json::json!({ "playerName": killer, "points": bonus });
                pipe_rpc::call("setFamePoints", Some(params)).await.ok();
                reply(&format!("[XP] Bonus {}xp from {:.0}x event!", bonus, mult), &killer).await;
                Outcome::Handled
            }
            "chat" => {
                let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
                if !text.starts_with('!') { return Outcome::Ignored; }
                let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let parts: Vec<&str> = text.split_whitespace().collect();
                let cmd = parts.first().map(|s| s.to_lowercase()).unwrap_or_default();
                if !matches!(cmd.as_str(), "!xpboost" | "!xpstatus") { return Outcome::Ignored; }

                let rate_key = if steam.is_empty() { player.clone() } else { steam.clone() };
                {
                    let mut rate = self.rate.lock().await;
                    let now = Instant::now();
                    if let Some(prev) = rate.get(&rate_key) {
                        if now.duration_since(*prev) < RATE_LIMIT { return Outcome::Ignored; }
                    }
                    rate.insert(rate_key.clone(), now);
                }

                match cmd.as_str() {
                    "!xpboost" => {
                        if !is_owner(&steam, &player) { reply("[XP] Admin only.", &player).await; return Outcome::Handled; }
                        let mult: f64 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(2.0);
                        let mins: u64 = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(30);
                        let mult = mult.min(5.0).max(1.0);
                        {
                            let mut st = self.state.lock().await;
                            st.multiplier = mult;
                            st.expires = Some(Instant::now() + Duration::from_secs(mins * 60));
                        }
                        broadcast(&format!("[XP] {:.0}x XP EVENT for {} minutes! Kill for bonus fame!", mult, mins)).await;
                        Outcome::Handled
                    }
                    "!xpstatus" => {
                        let msg = {
                            let st = self.state.lock().await;
                            if st.multiplier > 1.0 {
                                let remaining = st.expires.map(|e| e.saturating_duration_since(Instant::now()).as_secs() / 60).unwrap_or(0);
                                format!("[XP] {:.0}x active - {}min remaining", st.multiplier, remaining)
                            } else {
                                "[XP] No boost active. Normal rates.".to_string()
                            }
                        };
                        reply(&msg, &player).await;
                        Outcome::Handled
                    }
                    _ => Outcome::Ignored,
                }
            }
            _ => Outcome::Ignored,
        }
    }
}
