// Scoreboard - all-time persistent stats beyond leaderboard.
// !mystats - full player stat card. !topplayed - most playtime. !toptraveled - most distance.
// Tracks playtime + distance traveled from ctx.map (60s tick) + sessions from login events.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const STATE_PATH: &str = r"C:\TurdMOD\data\scoreboard.json";
const RATE_LIMIT: Duration = Duration::from_secs(3);
const TRACK_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
struct PlayerScore {
    name: String,
    playtime_mins: u64,
    distance_traveled: f64,
    sessions: u32,
    first_seen_ts: u64,
    last_seen_ts: u64,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
struct ScoreState {
    players: HashMap<String, PlayerScore>,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_secs()
}

fn load() -> ScoreState {
    std::fs::read_to_string(STATE_PATH).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(state: &ScoreState) {
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

pub struct Scoreboard {
    state: Mutex<ScoreState>,
    rate: Mutex<HashMap<String, Instant>>,
    last_positions: Mutex<HashMap<String, (f64, f64)>>,
}
impl Scoreboard {
    pub fn new() -> Self {
        Self { state: Mutex::new(load()), rate: Mutex::new(HashMap::new()), last_positions: Mutex::new(HashMap::new()) }
    }
}

#[async_trait::async_trait]
impl Mod for Scoreboard {
    fn name(&self) -> &'static str { "scoreboard" }
    // event-driven (no commands()): needs `login` events plus !mystats/!topplayed/!toptraveled.
    fn interval(&self) -> Option<Duration> { Some(TRACK_INTERVAL) }

    // Accrue playtime + distance every 60s (the old select! interval branch).
    async fn tick(&self, ctx: &ModCtx) {
        let snapshot = ctx.map.read().await.clone();
        let now = now_secs();
        let mut state = self.state.lock().await;
        let mut last = self.last_positions.lock().await;
        for p in &snapshot.players {
            let entry = state.players.entry(p.steam_id.clone())
                .or_insert_with(|| PlayerScore { name: p.name.clone(), first_seen_ts: now, ..Default::default() });
            entry.name = p.name.clone();
            entry.playtime_mins += 1;
            entry.last_seen_ts = now;
            if let Some((lx, ly)) = last.get(&p.steam_id) {
                let dx = p.x - lx;
                let dy = p.y - ly;
                let dist = (dx * dx + dy * dy).sqrt();
                if dist > 50.0 { entry.distance_traveled += dist; }
            }
            last.insert(p.steam_id.clone(), (p.x, p.y));
        }
        save(&state);
    }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        match ev.event.as_str() {
            "login" => {
                let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if steam.is_empty() { return Outcome::Ignored; }
                let now = now_secs();
                let mut state = self.state.lock().await;
                let entry = state.players.entry(steam)
                    .or_insert_with(|| PlayerScore { name: player.clone(), first_seen_ts: now, ..Default::default() });
                entry.sessions += 1;
                entry.last_seen_ts = now;
                save(&state);
                Outcome::Handled
            }
            "chat" => {
                let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
                if !text.starts_with('!') { return Outcome::Ignored; }
                let player_name = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let cmd = text.split_whitespace().next().unwrap_or("").to_lowercase();
                if !matches!(cmd.as_str(), "!mystats" | "!topplayed" | "!toptraveled") { return Outcome::Ignored; }

                let rate_key = if steam.is_empty() { player_name.clone() } else { steam.clone() };
                {
                    let mut rate = self.rate.lock().await;
                    let now_i = Instant::now();
                    if let Some(prev) = rate.get(&rate_key) {
                        if now_i.duration_since(*prev) < RATE_LIMIT { return Outcome::Ignored; }
                    }
                    rate.insert(rate_key.clone(), now_i);
                }

                let lines: Vec<String> = {
                    let state = self.state.lock().await;
                    match cmd.as_str() {
                        "!mystats" => match state.players.get(&steam) {
                            Some(s) => {
                                let hours = s.playtime_mins / 60;
                                let km = s.distance_traveled / 100000.0;
                                vec![format!("[Stats] {} - {}h playtime | {:.1}km traveled | {} sessions", s.name, hours, km, s.sessions)]
                            }
                            None => vec!["[Stats] No data yet.".to_string()],
                        },
                        "!topplayed" => {
                            let mut entries: Vec<(&str, u64)> = state.players.values().map(|p| (p.name.as_str(), p.playtime_mins)).collect();
                            entries.sort_by(|a, b| b.1.cmp(&a.1));
                            entries.truncate(5);
                            if entries.is_empty() { vec!["[Stats] No data.".to_string()] }
                            else {
                                let l: Vec<String> = entries.iter().enumerate().map(|(i, (n, m))| format!("{}. {} ({}h)", i + 1, n, m / 60)).collect();
                                vec![format!("[Top Playtime] {}", l.join(" | "))]
                            }
                        }
                        _ => { // !toptraveled
                            let mut entries: Vec<(&str, f64)> = state.players.values().map(|p| (p.name.as_str(), p.distance_traveled)).collect();
                            entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                            entries.truncate(5);
                            if entries.is_empty() { vec!["[Stats] No data.".to_string()] }
                            else {
                                let l: Vec<String> = entries.iter().enumerate().map(|(i, (n, d))| format!("{}. {} ({:.1}km)", i + 1, n, d / 100000.0)).collect();
                                vec![format!("[Top Travelers] {}", l.join(" | "))]
                            }
                        }
                    }
                };
                for l in lines { reply(&l, &player_name).await; }
                Outcome::Handled
            }
            _ => Outcome::Ignored,
        }
    }
}
