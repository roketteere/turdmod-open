// Leaderboard â€” tracks kills, deaths, K/D ratio from game events.
// !leaderboard !kd !topkills !topdeaths
// State: C:\TurdMOD\data\leaderboard.json

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const STATE_PATH: &str = r"C:\TurdMOD\data\leaderboard.json";
const RATE_LIMIT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
struct PlayerStats {
    name: String,
    kills: u64,
    deaths: u64,
    kill_streak: u64,
    best_streak: u64,
    headshots: u64,
    distance_kills: u64,
}

impl PlayerStats {
    fn kd(&self) -> f64 {
        if self.deaths == 0 { self.kills as f64 }
        else { self.kills as f64 / self.deaths as f64 }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
struct LbState {
    players: HashMap<String, PlayerStats>,
}

fn load() -> LbState {
    std::fs::read_to_string(STATE_PATH).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(state: &LbState) {
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let tmp = format!("{}.tmp", STATE_PATH);
        if std::fs::write(&tmp, &json).is_ok() {
            let _ = std::fs::rename(&tmp, STATE_PATH);
        }
    }
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

pub struct Leaderboard { state: Mutex<LbState>, rate: Mutex<HashMap<String, Instant>> }
impl Leaderboard {
    pub fn new() -> Self {
        let s = load();
        tracing::info!("leaderboard: {} players tracked", s.players.len());
        Self { state: Mutex::new(s), rate: Mutex::new(HashMap::new()) }
    }
}

#[async_trait::async_trait]
impl Mod for Leaderboard {
    fn name(&self) -> &'static str { "leaderboard" }
    // event-driven (no commands()): needs `kill` events too, so it sees everything + self-filters.

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        match ev.event.as_str() {
            "kill" => {
                let killer = ev.data.get("killer").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let killer_steam = ev.data.get("killerSteam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let victim = ev.data.get("victim").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let victim_steam = ev.data.get("victimSteam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if killer_steam.is_empty() && victim_steam.is_empty() { return Outcome::Ignored; }

                let mut state = self.state.lock().await;
                if !killer_steam.is_empty() {
                    let k = state.players.entry(killer_steam).or_insert_with(|| PlayerStats { name: killer.clone(), ..Default::default() });
                    k.name = killer;
                    k.kills += 1;
                    k.kill_streak += 1;
                    if k.kill_streak > k.best_streak { k.best_streak = k.kill_streak; }
                }
                if !victim_steam.is_empty() {
                    let v = state.players.entry(victim_steam).or_insert_with(|| PlayerStats { name: victim.clone(), ..Default::default() });
                    v.name = victim;
                    v.deaths += 1;
                    v.kill_streak = 0;
                }
                save(&state);
                Outcome::Handled
            }
            "chat" => {
                let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
                if !text.starts_with('!') { return Outcome::Ignored; }

                let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let rate_key = if steam.is_empty() { player.clone() } else { steam.clone() };

                let cmd = text.split_whitespace().next().unwrap_or("").to_lowercase();
                if !matches!(cmd.as_str(), "!kd" | "!leaderboard" | "!topkills" | "!topdeaths") {
                    return Outcome::Ignored;
                }

                {
                    let mut rate = self.rate.lock().await;
                    let now = Instant::now();
                    if let Some(prev) = rate.get(&rate_key) {
                        if now.duration_since(*prev) < RATE_LIMIT { return Outcome::Ignored; }
                    }
                    rate.insert(rate_key.clone(), now);
                }

                // Build the reply under a short lock, then release before the bridge round-trip.
                let msg = {
                    let state = self.state.lock().await;
                    match cmd.as_str() {
                        "!kd" => match state.players.get(&rate_key) {
                            Some(s) => format!("[Stats] K:{} D:{} K/D:{:.2} Streak:{} Best:{}", s.kills, s.deaths, s.kd(), s.kill_streak, s.best_streak),
                            None => "[Stats] No kills/deaths recorded yet.".to_string(),
                        },
                        "!leaderboard" | "!topkills" => {
                            let mut entries: Vec<(&str, u64, f64)> = state.players.values().map(|p| (p.name.as_str(), p.kills, p.kd())).collect();
                            entries.sort_by(|a, b| b.1.cmp(&a.1));
                            entries.truncate(5);
                            if entries.is_empty() { "[Leaderboard] No data yet.".to_string() }
                            else {
                                let lines: Vec<String> = entries.iter().enumerate().map(|(i, (n, k, kd))| format!("{}. {} ({}K {:.1}KD)", i + 1, n, k, kd)).collect();
                                format!("[Top Kills] {}", lines.join(" | "))
                            }
                        }
                        _ => { // !topdeaths
                            let mut entries: Vec<(&str, u64)> = state.players.values().map(|p| (p.name.as_str(), p.deaths)).collect();
                            entries.sort_by(|a, b| b.1.cmp(&a.1));
                            entries.truncate(5);
                            if entries.is_empty() { "[Leaderboard] No data yet.".to_string() }
                            else {
                                let lines: Vec<String> = entries.iter().enumerate().map(|(i, (n, d))| format!("{}. {} ({}D)", i + 1, n, d)).collect();
                                format!("[Most Deaths] {}", lines.join(" | "))
                            }
                        }
                    }
                };
                reply(&msg, &player).await;
                Outcome::Handled
            }
            _ => Outcome::Ignored,
        }
    }
}
