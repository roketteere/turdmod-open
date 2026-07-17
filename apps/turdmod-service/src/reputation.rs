// Reputation system - player karma based on behavior.
// Killing = rep down. !thank = rep up. !rep / !rep <player> / !toprep.
// Rep tiers: Outlaw / Shady / Neutral / Trusted / Hero / Legend. Event-driven (needs kill).

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const STATE_PATH: &str = r"C:\TurdMOD\data\reputation.json";
const RATE_LIMIT: Duration = Duration::from_secs(3);
const KILL_PENALTY: i32 = -10;
const HEAL_BONUS: i32 = 3;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PlayerRep {
    name: String,
    score: i32,
    kills_tracked: u32,
    trades_tracked: u32,
}

impl PlayerRep {
    fn tier(&self) -> &'static str {
        match self.score {
            i32::MIN..=-50 => "Outlaw",
            -49..=-1 => "Shady",
            0..=24 => "Neutral",
            25..=74 => "Trusted",
            75..=149 => "Hero",
            _ => "Legend",
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
struct RepState {
    players: HashMap<String, PlayerRep>,
}

fn load() -> RepState {
    std::fs::read_to_string(STATE_PATH).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(state: &RepState) {
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

pub struct Reputation { state: Mutex<RepState>, rate: Mutex<HashMap<String, Instant>> }
impl Reputation {
    pub fn new() -> Self { Self { state: Mutex::new(load()), rate: Mutex::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl Mod for Reputation {
    fn name(&self) -> &'static str { "reputation" }
    // event-driven (no commands()): needs `kill` events plus !rep/!toprep/!thank.

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        match ev.event.as_str() {
            "kill" => {
                let killer_steam = ev.data.get("killerSteam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let killer_name = ev.data.get("killer").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if killer_steam.is_empty() { return Outcome::Ignored; }
                let mut state = self.state.lock().await;
                let entry = state.players.entry(killer_steam).or_insert_with(|| PlayerRep {
                    name: killer_name.clone(), score: 0, kills_tracked: 0, trades_tracked: 0,
                });
                entry.name = killer_name;
                entry.score += KILL_PENALTY;
                entry.kills_tracked += 1;
                save(&state);
                Outcome::Handled
            }
            "chat" => {
                let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
                if !text.starts_with('!') { return Outcome::Ignored; }
                let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let (cmd, args) = match text.find(' ') {
                    Some(i) => (text[..i].to_lowercase(), text[i + 1..].trim().to_string()),
                    None => (text.to_lowercase(), String::new()),
                };
                if !matches!(cmd.as_str(), "!rep" | "!toprep" | "!thank") { return Outcome::Ignored; }

                let rate_key = if steam.is_empty() { player.clone() } else { steam.clone() };
                {
                    let mut rate = self.rate.lock().await;
                    let now = Instant::now();
                    if let Some(prev) = rate.get(&rate_key) {
                        if now.duration_since(*prev) < RATE_LIMIT { return Outcome::Ignored; }
                    }
                    rate.insert(rate_key.clone(), now);
                }

                let mut out: Vec<(String, String)> = Vec::new(); // (recipient, msg)
                {
                    let mut state = self.state.lock().await;
                    match cmd.as_str() {
                        "!rep" => {
                            if args.is_empty() {
                                match state.players.get(&steam) {
                                    Some(r) => out.push((player.clone(), format!("[Rep] {} - Score: {} ({})", r.name, r.score, r.tier()))),
                                    None => out.push((player.clone(), "[Rep] Neutral (no history)".to_string())),
                                }
                            } else {
                                match state.players.values().find(|r| r.name.eq_ignore_ascii_case(&args)) {
                                    Some(r) => out.push((player.clone(), format!("[Rep] {} - Score: {} ({})", r.name, r.score, r.tier()))),
                                    None => out.push((player.clone(), format!("[Rep] {} has no history", args))),
                                }
                            }
                        }
                        "!toprep" => {
                            let mut entries: Vec<(&str, i32, &str)> = state.players.values().map(|r| (r.name.as_str(), r.score, r.tier())).collect();
                            entries.sort_by(|a, b| b.1.cmp(&a.1));
                            entries.truncate(5);
                            if entries.is_empty() {
                                out.push((player.clone(), "[Rep] No reputation data yet.".to_string()));
                            } else {
                                let lines: Vec<String> = entries.iter().enumerate().map(|(i, (n, s, t))| format!("{}. {} ({}:{})", i + 1, n, t, s)).collect();
                                out.push((player.clone(), format!("[Rep] Top: {}", lines.join(" | "))));
                            }
                        }
                        "!thank" => {
                            if args.is_empty() {
                                out.push((player.clone(), "[Rep] Usage: !thank <player>".to_string()));
                            } else if args.eq_ignore_ascii_case(&player) {
                                out.push((player.clone(), "[Rep] Can't thank yourself.".to_string()));
                            } else {
                                let found = if let Some((_, r)) = state.players.iter_mut().find(|(_, r)| r.name.eq_ignore_ascii_case(&args)) {
                                    r.score += HEAL_BONUS;
                                    true
                                } else { false };
                                if found {
                                    save(&state);
                                    out.push((player.clone(), format!("[Rep] {} thanked - +{} rep!", args, HEAL_BONUS)));
                                    out.push((args.clone(), format!("[Rep] {} thanked you! +{} rep", player, HEAL_BONUS)));
                                } else {
                                    out.push((player.clone(), format!("[Rep] {} not found.", args)));
                                }
                            }
                        }
                        _ => return Outcome::Ignored,
                    }
                }
                for (rcpt, msg) in &out { reply(msg, rcpt).await; }
                Outcome::Handled
            }
            _ => Outcome::Ignored,
        }
    }
}
