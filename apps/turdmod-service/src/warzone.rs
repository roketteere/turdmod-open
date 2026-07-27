// Warzone - timed PvP events in designated areas.
// !warzone starts a 10-min high-loot PvP zone with kill tracking.
// Survivors get economy rewards. Announcements at start/end.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const WARZONE_DURATION: Duration = Duration::from_secs(600); // 10 min
const REWARD_PER_KILL: i64 = 50;
const SURVIVAL_BONUS: i64 = 100;
const RATE_LIMIT: Duration = Duration::from_secs(3);
// @inv: interval tick is 5s; expiry check happens every tick
const CHECK_INTERVAL: Duration = Duration::from_secs(5);

struct ActiveWarzone {
    name: String,
    x: f64,
    y: f64,
    radius: f64,
    started: Instant,
    kills: HashMap<String, u32>, // steam -> kill count
    participants: HashSet<String>,
    started_by: String,
}

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

fn credit(steam: &str, amount: i64) {
    let path = r"C:\TurdMOD\data\economy.json";
    let Ok(data) = std::fs::read_to_string(path) else { return };
    let Ok(mut state) = serde_json::from_str::<serde_json::Value>(&data) else { return };
    if let Some(bal) = state.get_mut("players")
        .and_then(|p| p.get_mut(steam))
        .and_then(|p| p.get_mut("balance"))
        .and_then(|b| b.as_i64()) {
        state["players"][steam]["balance"] = serde_json::json!(bal + amount);
        let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
        if let Ok(json) = serde_json::to_string_pretty(&state) {
            let tmp = format!("{}.tmp", path);
            if std::fs::write(&tmp, &json).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }
}

pub struct Warzone {
    active: Mutex<Option<ActiveWarzone>>,
    rate: Mutex<HashMap<String, Instant>>,
}

impl Warzone {
    pub fn new() -> Self {
        Self { active: Mutex::new(None), rate: Mutex::new(HashMap::new()) }
    }
}

#[async_trait::async_trait]
impl Mod for Warzone {
    fn name(&self) -> &'static str { "warzone" }
    // event-driven: handles kill events + chat commands; no commands() so it sees all events.
    fn commands(&self) -> &'static [&'static str] { &[] }
    fn interval(&self) -> Option<Duration> { Some(CHECK_INTERVAL) }

    // Dormant unless a warzone is running.
    async fn active(&self) -> bool { self.active.lock().await.is_some() }

    // Tick checks for warzone expiry (replaces the timeout(5s) poll in the original loop).
    async fn tick(&self, _ctx: &ModCtx) {
        let expired = {
            let active = self.active.lock().await;
            active.as_ref().map(|wz| wz.started.elapsed() > WARZONE_DURATION).unwrap_or(false)
        };
        if !expired { return; }

        // Pull data out and clear active under lock; drop lock before all the awaits.
        let (name, kills, participants) = {
            let mut active = self.active.lock().await;
            let Some(wz) = active.take() else { return; };
            (wz.name, wz.kills, wz.participants)
        };

        broadcast_msg(&format!("[Warzone] '{}' ENDED!", name)).await;

        let mut results: Vec<(String, u32)> = kills.iter().map(|(s, k)| (s.clone(), *k)).collect();
        results.sort_by(|a, b| b.1.cmp(&a.1));

        if let Some((top_steam, top_kills)) = results.first() {
            let bonus = SURVIVAL_BONUS + (REWARD_PER_KILL * *top_kills as i64);
            credit(top_steam, bonus);
            broadcast_msg(&format!("[Warzone] MVP: {} kills - +{} coins!", top_kills, bonus)).await;
        }

        for steam in &participants {
            if !kills.contains_key(steam) {
                credit(steam, SURVIVAL_BONUS / 2);
            }
        }
    }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        match ev.event.as_str() {
            "kill" => {
                let (killer_steam, killer, kill_count, reward) = {
                    let mut active = self.active.lock().await;
                    let Some(wz) = active.as_mut() else { return Outcome::Ignored; };
                    let killer_steam = ev.data.get("killerSteam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let killer = ev.data.get("killer").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    if killer_steam.is_empty() { return Outcome::Ignored; }
                    *wz.kills.entry(killer_steam.clone()).or_insert(0) += 1;
                    wz.participants.insert(killer_steam.clone());
                    let count = wz.kills[&killer_steam];
                    (killer_steam, killer, count, REWARD_PER_KILL)
                };
                credit(&killer_steam, reward);
                broadcast_msg(&format!("[Warzone] {} got a kill! ({} total) +{} coins", killer, kill_count, reward)).await;
                Outcome::Handled
            }

            "chat" => {
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

                match cmd.as_str() {
                    "!warzone" => {
                        if !is_owner(&steam, &player) { reply("[Warzone] Admin only.", &player).await; return Outcome::Handled; }
                        let already_active = self.active.lock().await.is_some();
                        if already_active { reply("[Warzone] Already active!", &player).await; return Outcome::Handled; }
                        let name = if args.is_empty() { "Thunderdome".into() } else { args };
                        let name_clone = name.clone();
                        {
                            let mut active = self.active.lock().await;
                            *active = Some(ActiveWarzone {
                                name,
                                x: 0.0, y: 0.0, radius: 10000.0,
                                started: Instant::now(),
                                kills: HashMap::new(),
                                participants: HashSet::new(),
                                started_by: player.clone(),
                            });
                        }
                        broadcast_msg(&format!("[Warzone] '{}' ACTIVATED! 10 minutes of chaos! Kill for coins!", name_clone)).await;
                        Outcome::Handled
                    }
                    "!endwarzone" => {
                        if !is_owner(&steam, &player) { reply("[Warzone] Admin only.", &player).await; return Outcome::Handled; }
                        let wz_name = {
                            let mut active = self.active.lock().await;
                            active.take().map(|wz| wz.name)
                        };
                        if let Some(name) = wz_name {
                            broadcast_msg(&format!("[Warzone] '{}' ended early by admin.", name)).await;
                        } else {
                            reply("[Warzone] No active warzone.", &player).await;
                        }
                        Outcome::Handled
                    }
                    "!wzstatus" => {
                        let msg = {
                            let active = self.active.lock().await;
                            if let Some(wz) = active.as_ref() {
                                let remaining = WARZONE_DURATION.checked_sub(wz.started.elapsed())
                                    .map(|d| d.as_secs() / 60).unwrap_or(0);
                                let total_kills: u32 = wz.kills.values().sum();
                                format!("[Warzone] '{}' - {}min left, {} kills", wz.name, remaining, total_kills)
                            } else {
                                "[Warzone] No active warzone.".to_string()
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
