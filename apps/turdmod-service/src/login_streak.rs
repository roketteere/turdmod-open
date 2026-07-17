// Daily login streak - escalating rewards for consecutive days.
// Day 1: 25c, Day 2: 50c, Day 3: 75c, ... Day 7: 250c (weekly bonus).
// Resets if you miss a day. !streak shows current progress.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const STATE_PATH: &str = r"C:\TurdMOD\data\login_streak.json";
const ECON_PATH: &str = r"C:\TurdMOD\data\economy.json";
const RATE_LIMIT: Duration = Duration::from_secs(3);
const BASE_REWARD: i64 = 25;
const WEEKLY_BONUS: i64 = 250;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
struct StreakData {
    name: String,
    current_streak: u32,
    last_claim_day: u64, // unix day number
    total_days: u32,
    longest_streak: u32,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
struct StreakState {
    players: HashMap<String, StreakData>,
}

fn today() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_secs() / 86400
}

fn load() -> StreakState {
    std::fs::read_to_string(STATE_PATH).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(state: &StreakState) {
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

pub struct LoginStreak { state: Mutex<StreakState>, rate: Mutex<HashMap<String, Instant>> }
impl LoginStreak {
    pub fn new() -> Self { Self { state: Mutex::new(load()), rate: Mutex::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl Mod for LoginStreak {
    fn name(&self) -> &'static str { "login_streak" }
    // event-driven (no commands()): needs `login` events plus the `!streak` chat command.

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        match ev.event.as_str() {
            "login" => {
                let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();

                let day = today();
                let result = {
                    let mut state = self.state.lock().await;
                    let sd = state.players.entry(steam.clone()).or_insert_with(|| StreakData {
                        name: player.clone(), ..Default::default()
                    });
                    sd.name = player.clone();
                    if sd.last_claim_day == day {
                        None // already claimed today
                    } else {
                        if sd.last_claim_day == day - 1 {
                            sd.current_streak += 1;
                        } else if sd.last_claim_day < day - 1 {
                            sd.current_streak = 1;
                        }
                        sd.last_claim_day = day;
                        sd.total_days += 1;
                        if sd.current_streak > sd.longest_streak { sd.longest_streak = sd.current_streak; }
                        let mut reward = BASE_REWARD * sd.current_streak as i64;
                        let is_weekly = sd.current_streak % 7 == 0;
                        if is_weekly { reward += WEEKLY_BONUS; }
                        reward = reward.min(500);
                        let r = (reward, sd.current_streak, sd.longest_streak, is_weekly);
                        save(&state);
                        Some(r)
                    }
                };

                let Some((reward, streak, longest, is_weekly)) = result else { return Outcome::Ignored; };
                credit(&steam, reward);
                tokio::time::sleep(Duration::from_secs(8)).await;
                let weekly_msg = if is_weekly { " WEEKLY BONUS!" } else { "" };
                reply(&format!("[Streak] Day {} - +{} coins!{} (best: {} days)", streak, reward, weekly_msg, longest), &player).await;
                Outcome::Handled
            }

            "chat" => {
                let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
                if text.to_lowercase() != "!streak" { return Outcome::Ignored; }

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

                let msg = {
                    let state = self.state.lock().await;
                    match state.players.get(&steam) {
                        Some(s) => format!("[Streak] Current: {} days | Best: {} | Total: {} days played", s.current_streak, s.longest_streak, s.total_days),
                        None => "[Streak] No login history yet.".to_string(),
                    }
                };
                reply(&msg, &player).await;
                Outcome::Handled
            }
            _ => Outcome::Ignored,
        }
    }
}
