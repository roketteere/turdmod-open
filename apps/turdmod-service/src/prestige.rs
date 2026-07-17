// Prestige / gun-game progression — kill-driven ranks with level-up broadcasts.
// Tracks lifetime PvP kills per player (from "kill" events) and awards ascending
// ranks at thresholds. On crossing a threshold the whole server hears about it.
// !prestige        — show your rank, kills, and kills-to-next.
// !prestige top     — top 5 by kills.
// @dep: events::GameEvent ("kill"/"chat"). State JSON at C:\TurdMOD\data\prestige.json.
// @inv: same module shape as combat_log/title_system — one event loop, atomic save,
//   rate-limited !cmd parsing. All writes go through pipe_rpc (serial bridge).

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const STATE_PATH: &str = r"C:\TurdMOD\data\prestige.json";
const RATE_LIMIT: Duration = Duration::from_secs(3);

// (min kills, rank name). Ordered ascending; current rank = highest reached.
const RANKS: &[(u32, &str)] = &[
    (0, "Fresh Spawn"),
    (10, "Scavenger"),
    (25, "Raider"),
    (50, "Veteran"),
    (100, "Elite"),
    (200, "Warlord"),
    (350, "Apex Predator"),
    (500, "Legend"),
];

#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
struct PrestigeState {
    kills: HashMap<String, u32>, // steam -> lifetime kills
    names: HashMap<String, String>, // steam -> last-seen display name
}

fn load() -> PrestigeState {
    std::fs::read_to_string(STATE_PATH)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(state: &PrestigeState) {
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let tmp = format!("{}.tmp", STATE_PATH);
        if std::fs::write(&tmp, &json).is_ok() {
            let _ = std::fs::rename(&tmp, STATE_PATH);
        }
    }
}

/// Highest rank whose threshold is <= kills. Returns (name, index).
fn rank_for(kills: u32) -> (&'static str, usize) {
    let mut idx = 0;
    for (i, (min, _)) in RANKS.iter().enumerate() {
        if kills >= *min {
            idx = i;
        }
    }
    (RANKS[idx].1, idx)
}

/// Kills remaining to the next rank, or None at max rank.
fn to_next(kills: u32) -> Option<(u32, &'static str)> {
    RANKS.iter().find(|(min, _)| *min > kills).map(|(min, name)| (min - kills, *name))
}

async fn broadcast(msg: &str) {
    let params = serde_json::json!({ "text": msg });
    pipe_rpc::call("broadcastChat", Some(params)).await.ok();
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

pub struct Prestige { state: Mutex<PrestigeState>, rate: Mutex<HashMap<String, Instant>> }
impl Prestige {
    pub fn new() -> Self { Self { state: Mutex::new(load()), rate: Mutex::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl Mod for Prestige {
    fn name(&self) -> &'static str { "prestige" }
    // event-driven (no commands()): needs `kill` events plus the `!prestige` chat command.

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        match ev.event.as_str() {
            "kill" => {
                let killer = ev.data.get("killer").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let killer_steam = ev.data.get("killerSteam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                // Only count PvP kills with a known killer (skip env/suicide).
                if killer_steam.is_empty() || killer.is_empty() { return Outcome::Ignored; }

                let announce = {
                    let mut state = self.state.lock().await;
                    let entry = state.kills.entry(killer_steam.clone()).or_insert(0);
                    let before = *entry;
                    *entry += 1;
                    let after = *entry;
                    state.names.insert(killer_steam.clone(), killer.clone());
                    let (_, before_idx) = rank_for(before);
                    let (after_name, after_idx) = rank_for(after);
                    save(&state);
                    if after_idx > before_idx { Some((after_name, after)) } else { None }
                };
                if let Some((after_name, after)) = announce {
                    broadcast(&format!("[Prestige] {} reached rank [{}] ({} kills)!", killer, after_name, after)).await;
                    tracing::info!("prestige: {} -> {} ({} kills)", killer, after_name, after);
                }
                Outcome::Handled
            }

            "chat" => {
                let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
                if !text.starts_with("!prestige") { return Outcome::Ignored; }

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

                let parts: Vec<&str> = text.split_whitespace().collect();
                let sub = parts.get(1).map(|s| s.to_lowercase()).unwrap_or_default();

                // Collect reply lines under a short lock, then send after releasing it.
                let lines: Vec<String> = {
                    let state = self.state.lock().await;
                    if sub == "top" {
                        let mut all: Vec<(&String, &u32)> = state.kills.iter().collect();
                        all.sort_by(|a, b| b.1.cmp(a.1));
                        if all.is_empty() {
                            vec!["[Prestige] No kills recorded yet.".to_string()]
                        } else {
                            all.iter().take(5).enumerate().map(|(i, (s, k))| {
                                let name = state.names.get(*s).cloned().unwrap_or_else(|| "?".into());
                                let (rank, _) = rank_for(**k);
                                format!("[Prestige] #{} {} — [{}] {} kills", i + 1, name, rank, k)
                            }).collect()
                        }
                    } else {
                        let kills = state.kills.get(&steam).copied().unwrap_or(0);
                        let (rank, _) = rank_for(kills);
                        let next = match to_next(kills) {
                            Some((need, name)) => format!(" — {} kills to [{}]", need, name),
                            None => " — MAX RANK".into(),
                        };
                        vec![format!("[Prestige] [{}] {} kills{}", rank, kills, next)]
                    }
                };
                for line in lines { reply(&line, &player).await; }
                Outcome::Handled
            }

            _ => Outcome::Ignored,
        }
    }
}
