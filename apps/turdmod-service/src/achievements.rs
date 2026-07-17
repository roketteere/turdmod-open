// Achievements - milestone rewards for player accomplishments.
// Tracks kills, playtime, trades, quests, etc. Awards economy bonuses.
// !achievements / !trophy - view your progress. Event-driven (needs kill events).

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const STATE_PATH: &str = r"C:\TurdMOD\data\achievements.json";
const RATE_LIMIT: Duration = Duration::from_secs(3);
const ECON_PATH: &str = r"C:\TurdMOD\data\economy.json";

#[derive(Debug, Clone)]
struct AchievementDef {
    id: &'static str,
    name: &'static str,
    desc: &'static str,
    reward: i64,
}

const ACHIEVEMENTS: &[AchievementDef] = &[
    AchievementDef { id: "first_blood", name: "First Blood", desc: "Get your first kill", reward: 50 },
    AchievementDef { id: "serial_killer", name: "Serial Killer", desc: "Get 10 kills", reward: 200 },
    AchievementDef { id: "massacre", name: "Massacre", desc: "Get 50 kills", reward: 500 },
    AchievementDef { id: "legend", name: "Legend", desc: "Get 100 kills", reward: 1000 },
    AchievementDef { id: "survivor_1h", name: "Survivor", desc: "Play for 1 hour", reward: 50 },
    AchievementDef { id: "veteran_10h", name: "Veteran", desc: "Play for 10 hours", reward: 200 },
    AchievementDef { id: "addict_100h", name: "Addict", desc: "Play for 100 hours", reward: 1000 },
    AchievementDef { id: "first_trade", name: "Merchant", desc: "Complete your first trade", reward: 25 },
    AchievementDef { id: "gambler", name: "High Roller", desc: "Win 500+ coins gambling", reward: 100 },
    AchievementDef { id: "social", name: "Social Butterfly", desc: "Join a clan", reward: 50 },
    AchievementDef { id: "quest_master", name: "Quest Master", desc: "Complete 10 quests", reward: 300 },
    AchievementDef { id: "explorer", name: "Explorer", desc: "Travel 100km", reward: 200 },
    AchievementDef { id: "duel_champ", name: "Duel Champion", desc: "Win 5 duels", reward: 250 },
];

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
struct PlayerAchievements {
    unlocked: Vec<String>,
    kills: u64,
    playtime_mins: u64,
    trades: u32,
    quests_done: u32,
    duels_won: u32,
    distance_km: f64,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
struct AchState {
    players: HashMap<String, PlayerAchievements>,
}

fn load() -> AchState {
    std::fs::read_to_string(STATE_PATH).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(state: &AchState) {
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let tmp = format!("{}.tmp", STATE_PATH);
        if std::fs::write(&tmp, &json).is_ok() { let _ = std::fs::rename(&tmp, STATE_PATH); }
    }
}

fn credit(steam: &str, amount: i64) {
    let Ok(data) = std::fs::read_to_string(ECON_PATH) else { return };
    let Ok(mut state) = serde_json::from_str::<serde_json::Value>(&data) else { return };
    let bal = state.get("players").and_then(|p| p.get(steam))
        .and_then(|p| p.get("balance")).and_then(|b| b.as_i64()).unwrap_or(0);
    state["players"][steam]["balance"] = serde_json::json!(bal + amount);
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(&state) {
        let tmp = format!("{}.tmp", ECON_PATH);
        if std::fs::write(&tmp, &json).is_ok() { let _ = std::fs::rename(&tmp, ECON_PATH); }
    }
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

async fn broadcast(msg: &str) {
    let params = serde_json::json!({ "text": msg });
    pipe_rpc::call("broadcastChat", Some(params)).await.ok();
}

fn check_unlocks(pa: &mut PlayerAchievements, steam: &str, _player: &str) -> Vec<&'static AchievementDef> {
    let mut newly_unlocked = Vec::new();
    for ach in ACHIEVEMENTS {
        if pa.unlocked.contains(&ach.id.to_string()) { continue; }
        let earned = match ach.id {
            "first_blood" => pa.kills >= 1,
            "serial_killer" => pa.kills >= 10,
            "massacre" => pa.kills >= 50,
            "legend" => pa.kills >= 100,
            "survivor_1h" => pa.playtime_mins >= 60,
            "veteran_10h" => pa.playtime_mins >= 600,
            "addict_100h" => pa.playtime_mins >= 6000,
            "first_trade" => pa.trades >= 1,
            "quest_master" => pa.quests_done >= 10,
            "duel_champ" => pa.duels_won >= 5,
            "explorer" => pa.distance_km >= 100.0,
            _ => false,
        };
        if earned {
            pa.unlocked.push(ach.id.to_string());
            credit(steam, ach.reward);
            newly_unlocked.push(ach);
        }
    }
    newly_unlocked
}

pub struct Achievements { state: Mutex<AchState>, rate: Mutex<HashMap<String, Instant>> }
impl Achievements {
    pub fn new() -> Self { Self { state: Mutex::new(load()), rate: Mutex::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl Mod for Achievements {
    fn name(&self) -> &'static str { "achievements" }
    // event-driven (no commands()): needs `kill` events plus !achievements/!trophy.

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        match ev.event.as_str() {
            "kill" => {
                let killer_steam = ev.data.get("killerSteam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let killer_name = ev.data.get("killer").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if killer_steam.is_empty() { return Outcome::Ignored; }
                let unlocked: Vec<(String, i64)> = {
                    let mut state = self.state.lock().await;
                    let pa = state.players.entry(killer_steam.clone()).or_default();
                    pa.kills += 1;
                    let u = check_unlocks(pa, &killer_steam, &killer_name);
                    let result: Vec<(String, i64)> = u.iter().map(|a| (a.name.to_string(), a.reward)).collect();
                    if !result.is_empty() { save(&state); }
                    result
                };
                for (name, reward) in &unlocked {
                    broadcast(&format!("[Achievement] {} unlocked '{}' - +{} coins!", killer_name, name, reward)).await;
                }
                if unlocked.is_empty() { Outcome::Ignored } else { Outcome::Handled }
            }
            "chat" => {
                let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
                let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let cmd = text.split_whitespace().next().unwrap_or("").to_lowercase();
                if !matches!(cmd.as_str(), "!achievements" | "!trophy") { return Outcome::Ignored; }

                let rate_key = if steam.is_empty() { player.clone() } else { steam.clone() };
                {
                    let mut rate = self.rate.lock().await;
                    let now = Instant::now();
                    if let Some(prev) = rate.get(&rate_key) {
                        if now.duration_since(*prev) < RATE_LIMIT { return Outcome::Ignored; }
                    }
                    rate.insert(rate_key.clone(), now);
                }

                let lines: Vec<String> = {
                    let state = self.state.lock().await;
                    match state.players.get(&steam) {
                        Some(pa) => {
                            let mut v = vec![format!("[Achievements] {}/{} unlocked", pa.unlocked.len(), ACHIEVEMENTS.len())];
                            for ach_id in &pa.unlocked {
                                if let Some(def) = ACHIEVEMENTS.iter().find(|a| a.id == ach_id.as_str()) {
                                    v.push(format!("  [x] {} - {}", def.name, def.desc));
                                }
                            }
                            let remaining: Vec<&AchievementDef> = ACHIEVEMENTS.iter().filter(|a| !pa.unlocked.contains(&a.id.to_string())).collect();
                            for def in remaining.iter().take(3) {
                                v.push(format!("  [ ] {} - {} ({}c)", def.name, def.desc, def.reward));
                            }
                            v
                        }
                        None => vec!["[Achievements] No progress yet. Start playing!".to_string()],
                    }
                };
                for l in lines { reply(&l, &player).await; }
                Outcome::Handled
            }
            _ => Outcome::Ignored,
        }
    }
}
