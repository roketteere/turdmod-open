// Bounty hunter - active bounty tracking with proximity alerts.
// When a player with a bounty comes near a bounty hunter, alert them.
// !hunt - toggle bounty hunter mode. !targets - see active bounties.
// 15s interval tick reads ctx.map for proximity; commands() = !hunt/!targets.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const RATE_LIMIT: Duration = Duration::from_secs(3);
const PROXIMITY_ALERT_RADIUS: f64 = 5000.0; // 50m in UU
const ALERT_COOLDOWN: Duration = Duration::from_secs(60);
const ECON_PATH: &str = r"C:\TurdMOD\data\economy.json";

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

fn get_bounties() -> Vec<(String, i64)> {
    let Ok(data) = std::fs::read_to_string(ECON_PATH) else { return vec![] };
    let Ok(state) = serde_json::from_str::<serde_json::Value>(&data) else { return vec![] };
    state.get("bounties").and_then(|b| b.as_array()).map(|arr| {
        arr.iter().filter_map(|b| {
            let target = b.get("target")?.as_str()?.to_string();
            let amount = b.get("amount")?.as_i64()?;
            Some((target, amount))
        }).collect()
    }).unwrap_or_default()
}

#[derive(Default)]
struct Hunters {
    set: HashSet<String>,            // steam IDs of active hunters
    names: HashMap<String, String>,  // steam -> name
}

pub struct BountyHunter {
    hunters: Mutex<Hunters>,
    rate: Mutex<HashMap<String, Instant>>,
    last_alert: Mutex<HashMap<(String, String), Instant>>, // (hunter, target) -> last alert
}
impl BountyHunter {
    pub fn new() -> Self {
        Self { hunters: Mutex::new(Hunters::default()), rate: Mutex::new(HashMap::new()), last_alert: Mutex::new(HashMap::new()) }
    }
}

#[async_trait::async_trait]
impl Mod for BountyHunter {
    fn name(&self) -> &'static str { "bounty_hunter" }
    fn commands(&self) -> &'static [&'static str] { &["!hunt", "!targets"] }
    fn interval(&self) -> Option<Duration> { Some(Duration::from_secs(15)) }

    // Proximity alerts (the old select! interval branch).
    async fn tick(&self, ctx: &ModCtx) {
        let hunters_snap: Vec<(String, String)> = {
            let h = self.hunters.lock().await;
            if h.set.is_empty() { return; }
            h.set.iter().filter_map(|s| h.names.get(s).map(|n| (s.clone(), n.clone()))).collect()
        };
        let bounties = get_bounties();
        if bounties.is_empty() { return; }
        let snapshot = ctx.map.read().await.clone();
        let positions: HashMap<String, (f64, f64)> = snapshot.players.iter()
            .map(|p| (p.name.to_lowercase(), (p.x, p.y))).collect();

        let mut last_alert = self.last_alert.lock().await;
        for (hunter_steam, hunter_name) in &hunters_snap {
            let Some(&(hx, hy)) = positions.get(&hunter_name.to_lowercase()) else { continue };
            for (target_name, amount) in &bounties {
                let Some(&(tgx, tgy)) = positions.get(&target_name.to_lowercase()) else { continue };
                let dx = hx - tgx;
                let dy = hy - tgy;
                let dist = (dx * dx + dy * dy).sqrt();
                if dist < PROXIMITY_ALERT_RADIUS {
                    let key = (hunter_steam.clone(), target_name.clone());
                    let should_alert = last_alert.get(&key).map(|t| t.elapsed() > ALERT_COOLDOWN).unwrap_or(true);
                    if should_alert {
                        let dir = if tgx > hx { "east" } else { "west" };
                        let dir2 = if tgy > hy { "north" } else { "south" };
                        reply(&format!("[Bounty] TARGET NEARBY! {} ({} coins) - ~{:.0}m {}-{}", target_name, amount, dist / 100.0, dir2, dir), hunter_name).await;
                        last_alert.insert(key, Instant::now());
                    }
                }
            }
        }
    }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        if !text.starts_with('!') { return Outcome::Ignored; }
        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let cmd = text.split_whitespace().next().unwrap_or("").to_lowercase();
        if !matches!(cmd.as_str(), "!hunt" | "!targets") { return Outcome::Ignored; }

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
            "!hunt" => {
                let msg = {
                    let mut h = self.hunters.lock().await;
                    if h.set.contains(&steam) {
                        h.set.remove(&steam);
                        h.names.remove(&steam);
                        "[Bounty] Bounty hunter mode OFF.".to_string()
                    } else {
                        h.set.insert(steam.clone());
                        h.names.insert(steam.clone(), player.clone());
                        format!("[Bounty] Bounty hunter mode ON! {} active targets.", get_bounties().len())
                    }
                };
                reply(&msg, &player).await;
                Outcome::Handled
            }
            "!targets" => {
                let bounties = get_bounties();
                if bounties.is_empty() {
                    reply("[Bounty] No active bounties.", &player).await;
                } else {
                    for (i, (target, amount)) in bounties.iter().enumerate().take(10) {
                        reply(&format!("[Bounty] {}. {} - {} coins", i + 1, target, amount), &player).await;
                    }
                }
                Outcome::Handled
            }
            _ => Outcome::Ignored,
        }
    }
}
