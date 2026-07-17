// Quest system - daily/weekly missions for economy rewards.
// !quest - see active quest. !quests - list available. !claim - claim reward.
// Quests track kills, distance traveled, items crafted, time survived.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const RATE_LIMIT: Duration = Duration::from_secs(3);
const ECON_PATH: &str = r"C:\TurdMOD\data\economy.json";
const STATE_PATH: &str = r"C:\TurdMOD\data\quests.json";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct QuestDef {
    id: String,
    name: String,
    desc: String,
    goal: QuestGoal,
    reward: i64,
    daily: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
enum QuestGoal {
    Kills(u32),
    Survive(u64),     // seconds online
    Travel(f64),      // UU distance
    Login,            // just log in
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
struct PlayerQuest {
    quest_id: String,
    progress: f64,
    started_ts: u64,
    completed: bool,
    claimed: bool,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
struct QuestState {
    players: HashMap<String, Vec<PlayerQuest>>,
    last_daily_reset: u64,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_secs()
}

fn daily_quests() -> Vec<QuestDef> {
    vec![
        QuestDef { id: "daily_login".into(), name: "Check In".into(),
            desc: "Log in today".into(), goal: QuestGoal::Login, reward: 25, daily: true },
        QuestDef { id: "daily_kills3".into(), name: "Hunter".into(),
            desc: "Get 3 player kills".into(), goal: QuestGoal::Kills(3), reward: 75, daily: true },
        QuestDef { id: "daily_kills10".into(), name: "Rampage".into(),
            desc: "Get 10 player kills".into(), goal: QuestGoal::Kills(10), reward: 200, daily: true },
        QuestDef { id: "daily_survive".into(), name: "Survivor".into(),
            desc: "Stay alive for 30 minutes".into(), goal: QuestGoal::Survive(1800), reward: 50, daily: true },
    ]
}

fn load() -> QuestState {
    std::fs::read_to_string(STATE_PATH).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(state: &QuestState) {
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

fn ensure_dailies(state: &mut QuestState, steam: &str) {
    let now = now_secs();
    let day = now / 86400;
    let last_day = state.last_daily_reset / 86400;
    if day != last_day {
        state.players.clear();
        state.last_daily_reset = now;
    }
    if !state.players.contains_key(steam) {
        let quests = daily_quests().iter().map(|q| PlayerQuest {
            quest_id: q.id.clone(), progress: 0.0, started_ts: now,
            completed: false, claimed: false,
        }).collect();
        state.players.insert(steam.into(), quests);
    }
}

pub struct Quests {
    state: Mutex<QuestState>,
    rate: Mutex<HashMap<String, Instant>>,
}
impl Quests {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(load()),
            rate: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait::async_trait]
impl Mod for Quests {
    fn name(&self) -> &'static str { "quests" }
    // event-driven: needs kill + login events as well as !quest/!claim chat commands

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        let defs = daily_quests();
        match ev.event.as_str() {
            "kill" => {
                let killer_steam = ev.data.get("killerSteam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if killer_steam.is_empty() { return Outcome::Ignored; }
                let mut state = self.state.lock().await;
                ensure_dailies(&mut state, &killer_steam);
                if let Some(quests) = state.players.get_mut(&killer_steam) {
                    for pq in quests.iter_mut() {
                        if pq.completed || pq.claimed { continue; }
                        let def = defs.iter().find(|d| d.id == pq.quest_id);
                        if let Some(QuestDef { goal: QuestGoal::Kills(n), .. }) = def {
                            pq.progress += 1.0;
                            if pq.progress >= *n as f64 { pq.completed = true; }
                        }
                    }
                    save(&state);
                }
                Outcome::Handled
            }

            "login" => {
                let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                // steam may be empty - use player name as fallback
                let mut state = self.state.lock().await;
                ensure_dailies(&mut state, &steam);
                if let Some(quests) = state.players.get_mut(&steam) {
                    for pq in quests.iter_mut() {
                        if pq.quest_id == "daily_login" && !pq.completed {
                            pq.progress = 1.0;
                            pq.completed = true;
                        }
                    }
                    save(&state);
                }
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
                    let now_i = Instant::now();
                    if let Some(prev) = rate.get(&rate_key) {
                        if now_i.duration_since(*prev) < RATE_LIMIT { return Outcome::Ignored; }
                    }
                    rate.insert(rate_key.clone(), now_i);
                }

                let cmd = text.split_whitespace().next().unwrap_or("").to_lowercase();
                if !matches!(cmd.as_str(), "!quest" | "!quests" | "!claim") { return Outcome::Ignored; }

                let mut out: Vec<(String, String)> = Vec::new(); // (recipient, msg)
                {
                    let mut state = self.state.lock().await;
                    ensure_dailies(&mut state, &steam);

                    match cmd.as_str() {
                        "!quest" | "!quests" => {
                            let quests = state.players.get(&steam).cloned();
                            match quests {
                                Some(qs) => {
                                    for pq in &qs {
                                        let def = defs.iter().find(|d| d.id == pq.quest_id);
                                        if let Some(d) = def {
                                            let status = if pq.claimed { "CLAIMED" }
                                                else if pq.completed { "COMPLETE" }
                                                else { "active" };
                                            let goal_str = match &d.goal {
                                                QuestGoal::Kills(n) => format!("{}/{}", pq.progress as u32, n),
                                                QuestGoal::Survive(s) => format!("{}/{}min", 0, s / 60),
                                                QuestGoal::Travel(d) => format!("0/{:.0}m", d / 100.0),
                                                QuestGoal::Login => if pq.completed { "done".into() } else { "pending".into() },
                                            };
                                            out.push((player.clone(), format!("[Quest] {} - {} [{}] ({}c)", d.name, goal_str, status, d.reward)));
                                        }
                                    }
                                    if out.is_empty() {
                                        out.push((player.clone(), "[Quest] No quests available.".to_string()));
                                    }
                                }
                                None => out.push((player.clone(), "[Quest] No quests available.".to_string())),
                            }
                        }

                        "!claim" => {
                            let mut claimed_total = 0i64;
                            if let Some(quests) = state.players.get_mut(&steam) {
                                for pq in quests.iter_mut() {
                                    if pq.completed && !pq.claimed {
                                        if let Some(d) = defs.iter().find(|d| d.id == pq.quest_id) {
                                            pq.claimed = true;
                                            claimed_total += d.reward;
                                        }
                                    }
                                }
                            }
                            if claimed_total > 0 {
                                credit(&steam, claimed_total);
                                save(&state);
                                out.push((player.clone(), format!("[Quest] Claimed {} coins!", claimed_total)));
                            } else {
                                out.push((player.clone(), "[Quest] No completed quests to claim.".to_string()));
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
