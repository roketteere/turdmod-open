// Title system - earned titles displayed in chat via prefix.
// Titles from achievements, reputation, leaderboard rank, clan position.
// !title list - see available titles. !title set <title> - pick active title.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const STATE_PATH: &str = r"C:\TurdMOD\data\titles.json";
const RATE_LIMIT: Duration = Duration::from_secs(3);

const EARNED_TITLES: &[(&str, &str, &str)] = &[
    ("newcomer", "Newcomer", "Login for the first time"),
    ("hunter", "Hunter", "Get 10 kills"),
    ("legend", "Legend", "Get 100 kills"),
    ("survivor", "Survivor", "Play 10+ hours"),
    ("wealthy", "Wealthy", "Accumulate 1000+ coins"),
    ("hero", "Hero", "Reach Hero reputation"),
    ("outlaw", "Outlaw", "Reach Outlaw reputation"),
    ("champion", "Champion", "Win 10 duels"),
    ("gambler", "High Roller", "Win 500+ from gambling"),
    ("explorer", "Wanderer", "Travel 100km"),
    ("leader", "Leader", "Create a clan"),
    ("tamer", "Beast Master", "Tame an animal"),
];

#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
struct TitleState {
    active: HashMap<String, String>, // steam -> active title id
    earned: HashMap<String, Vec<String>>, // steam -> earned title ids
}

fn load() -> TitleState {
    std::fs::read_to_string(STATE_PATH).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(state: &TitleState) {
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let tmp = format!("{}.tmp", STATE_PATH);
        if std::fs::write(&tmp, &json).is_ok() { let _ = std::fs::rename(&tmp, STATE_PATH); }
    }
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

/// Public — other modules prepend a player's active title in chat. Reads from disk (save() keeps
/// it current), so it stays correct regardless of the mod's in-memory state.
pub fn get_title_prefix(steam: &str) -> Option<String> {
    let state = load();
    let title_id = state.active.get(steam)?;
    EARNED_TITLES.iter().find(|(id, _, _)| *id == title_id.as_str())
        .map(|(_, name, _)| format!("[{}]", name))
}

pub struct TitleSystem { state: Mutex<TitleState>, rate: Mutex<HashMap<String, Instant>> }
impl TitleSystem {
    pub fn new() -> Self { Self { state: Mutex::new(load()), rate: Mutex::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl Mod for TitleSystem {
    fn name(&self) -> &'static str { "title_system" }
    // event-driven (no commands()): grants "newcomer" on `login` + serves `!title` chat.

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        match ev.event.as_str() {
            "login" => {
                let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if steam.is_empty() { return Outcome::Ignored; }
                let mut state = self.state.lock().await;
                let earned = state.earned.entry(steam).or_default();
                if !earned.contains(&"newcomer".to_string()) {
                    earned.push("newcomer".into());
                    save(&state);
                    Outcome::Handled
                } else {
                    Outcome::Ignored
                }
            }

            "chat" => {
                let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
                if !text.starts_with("!title") { return Outcome::Ignored; }

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

                let replies: Vec<String> = {
                    let mut state = self.state.lock().await;
                    match sub.as_str() {
                        "list" => {
                            let earned = state.earned.get(&steam).cloned().unwrap_or_default();
                            let active = state.active.get(&steam).cloned().unwrap_or_default();
                            if earned.is_empty() {
                                vec!["[Title] No titles earned yet.".to_string()]
                            } else {
                                EARNED_TITLES.iter()
                                    .filter(|(id, _, _)| earned.contains(&id.to_string()))
                                    .map(|(id, name, desc)| {
                                        let marker = if active == *id { " (active)" } else { "" };
                                        format!("[Title] {} - {}{}", name, desc, marker)
                                    }).collect()
                            }
                        }
                        "set" => {
                            let title_id = parts.get(2).map(|s| s.to_lowercase()).unwrap_or_default();
                            if title_id.is_empty() {
                                vec!["[Title] Usage: !title set <title_id>".to_string()]
                            } else {
                                let earned = state.earned.get(&steam).cloned().unwrap_or_default();
                                if !earned.contains(&title_id) {
                                    vec!["[Title] You haven't earned that title.".to_string()]
                                } else {
                                    let name = EARNED_TITLES.iter().find(|(id, _, _)| *id == title_id.as_str())
                                        .map(|(_, n, _)| *n).unwrap_or("?");
                                    state.active.insert(steam.clone(), title_id);
                                    save(&state);
                                    vec![format!("[Title] Active title set to [{}]", name)]
                                }
                            }
                        }
                        "clear" => {
                            state.active.remove(&steam);
                            save(&state);
                            vec!["[Title] Title cleared.".to_string()]
                        }
                        _ => vec!["[Title] Commands: list/set/clear".to_string()],
                    }
                };
                for r in replies { reply(&r, &player).await; }
                Outcome::Handled
            }

            _ => Outcome::Ignored,
        }
    }
}
