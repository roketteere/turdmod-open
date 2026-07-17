// Racing - vehicle races between checkpoints.
// !race create <name> / checkpoint / finish / start <name> / list. Admin creates; anyone races.
// 3s interval tick advances active racers via ctx.map positions.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const OWNER_STEAM_ID: &str = "YOUR_STEAM_ID_1";
const OWNER_NAME: &str = "YOUR_OWNER_NAME";
const STATE_PATH: &str = r"C:\TurdMOD\data\races.json";
const RATE_LIMIT: Duration = Duration::from_secs(3);
const CHECKPOINT_RADIUS: f64 = 3000.0; // 30m
const CHECK_INTERVAL: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Checkpoint { x: f64, y: f64 }

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct RaceDef {
    name: String,
    checkpoints: Vec<Checkpoint>,
    created_by: String,
    best_time_secs: Option<f64>,
    best_time_player: Option<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
struct RaceState {
    races: HashMap<String, RaceDef>,
}

#[derive(Clone)]
struct ActiveRacer {
    player: String,
    #[allow(dead_code)]
    steam: String,
    race_name: String,
    started: Instant,
    next_checkpoint: usize,
}

fn load() -> RaceState {
    std::fs::read_to_string(STATE_PATH).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(state: &RaceState) {
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let tmp = format!("{}.tmp", STATE_PATH);
        if std::fs::write(&tmp, &json).is_ok() { let _ = std::fs::rename(&tmp, STATE_PATH); }
    }
}

fn is_owner(steam: &str, player: &str) -> bool {
    steam == OWNER_STEAM_ID || steam == "YOUR_STEAM_ID_2" || player == OWNER_NAME
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

async fn broadcast(msg: &str) {
    let params = serde_json::json!({ "text": msg });
    pipe_rpc::call("broadcastChat", Some(params)).await.ok();
}

pub struct Racing {
    state: Mutex<RaceState>,
    rate: Mutex<HashMap<String, Instant>>,
    building: Mutex<HashMap<String, (String, Vec<Checkpoint>)>>, // steam -> (name, checkpoints)
    racers: Mutex<Vec<ActiveRacer>>,
}
impl Racing {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(load()),
            rate: Mutex::new(HashMap::new()),
            building: Mutex::new(HashMap::new()),
            racers: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait::async_trait]
impl Mod for Racing {
    fn name(&self) -> &'static str { "racing" }
    fn commands(&self) -> &'static [&'static str] { &["!race"] }
    fn interval(&self) -> Option<Duration> { Some(CHECK_INTERVAL) }

    // Dormant unless a race is in progress.
    async fn active(&self) -> bool { !self.racers.lock().await.is_empty() }

    // Advance active racers through checkpoints (the old select! interval branch).
    async fn tick(&self, ctx: &ModCtx) {
        if self.racers.lock().await.is_empty() { return; }
        let snapshot = ctx.map.read().await.clone();
        let positions: HashMap<String, (f64, f64)> = snapshot.players.iter().map(|p| (p.name.clone(), (p.x, p.y))).collect();

        let mut checkpoint_msgs: Vec<(String, usize, usize)> = Vec::new(); // (player, cp, total)
        let mut finished: Vec<(String, String, f64)> = Vec::new();          // (player, race, time)
        {
            // Lock order: state then racers (handle uses the same order).
            let state = self.state.lock().await;
            let mut racers = self.racers.lock().await;
            let mut to_remove = Vec::new();
            for (i, racer) in racers.iter_mut().enumerate() {
                let Some(&(px, py)) = positions.get(&racer.player) else { continue };
                let Some(race) = state.races.get(&racer.race_name) else { continue };
                if racer.next_checkpoint >= race.checkpoints.len() { continue; }
                let cp = &race.checkpoints[racer.next_checkpoint];
                let dx = px - cp.x;
                let dy = py - cp.y;
                if (dx * dx + dy * dy).sqrt() < CHECKPOINT_RADIUS {
                    racer.next_checkpoint += 1;
                    if racer.next_checkpoint >= race.checkpoints.len() {
                        finished.push((racer.player.clone(), racer.race_name.clone(), racer.started.elapsed().as_secs_f64()));
                        to_remove.push(i);
                    } else {
                        checkpoint_msgs.push((racer.player.clone(), racer.next_checkpoint, race.checkpoints.len()));
                    }
                }
            }
            for i in to_remove.iter().rev() { racers.remove(*i); }
        }

        // Record best times (re-lock state only).
        let mut records: Vec<(String, String, f64)> = Vec::new();
        if !finished.is_empty() {
            let mut state = self.state.lock().await;
            for (player, race_name, time) in &finished {
                if let Some(race) = state.races.get_mut(race_name) {
                    let is_best = race.best_time_secs.map(|b| *time < b).unwrap_or(true);
                    if is_best {
                        race.best_time_secs = Some(*time);
                        race.best_time_player = Some(player.clone());
                        records.push((player.clone(), race_name.clone(), *time));
                    }
                }
            }
            if !records.is_empty() { save(&state); }
        }

        for (player, cp, total) in &checkpoint_msgs { reply(&format!("[Race] Checkpoint {}/{}!", cp, total), player).await; }
        for (player, race_name, time) in &finished { broadcast(&format!("[Race] {} finished '{}' in {:.1}s!", player, race_name, time)).await; }
        for (player, race_name, time) in &records { broadcast(&format!("[Race] NEW RECORD on '{}'! {:.1}s by {}", race_name, time, player)).await; }
    }

    async fn handle(&self, ev: &GameEvent, ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let parts: Vec<&str> = text.split_whitespace().collect();
        let cmd = parts.first().map(|s| s.to_lowercase()).unwrap_or_default();
        if cmd != "!race" { return Outcome::Ignored; }

        let rate_key = if steam.is_empty() { player.clone() } else { steam.clone() };
        {
            let mut rate = self.rate.lock().await;
            let now = Instant::now();
            if let Some(prev) = rate.get(&rate_key) {
                if now.duration_since(*prev) < RATE_LIMIT { return Outcome::Ignored; }
            }
            rate.insert(rate_key.clone(), now);
        }

        let sub = parts.get(1).map(|s| s.to_lowercase()).unwrap_or_default();
        let arg = parts.get(2).map(|s| s.to_string()).unwrap_or_default();

        match sub.as_str() {
            "create" => {
                if !is_owner(&steam, &player) { reply("[Race] Admin only to create.", &player).await; return Outcome::Handled; }
                if arg.is_empty() { reply("[Race] Usage: !race create <name>", &player).await; return Outcome::Handled; }
                self.building.lock().await.insert(steam.clone(), (arg.clone(), Vec::new()));
                reply(&format!("[Race] Building '{}'. Go to each checkpoint and type !race checkpoint. Then !race finish.", arg), &player).await;
                Outcome::Handled
            }
            "checkpoint" | "cp" => {
                let pos = { let snap = ctx.map.read().await; snap.players.iter().find(|p| p.name == player).map(|p| (p.x, p.y)) };
                let msg: Option<String> = {
                    let mut b = self.building.lock().await;
                    match b.get_mut(&steam) {
                        None => Some("[Race] Not building a race. !race create <name> first.".to_string()),
                        Some((_, cps)) => match pos {
                            Some((px, py)) => { cps.push(Checkpoint { x: px, y: py }); Some(format!("[Race] Checkpoint {} added.", cps.len())) }
                            None => None,
                        },
                    }
                };
                if let Some(m) = msg { reply(&m, &player).await; }
                Outcome::Handled
            }
            "finish" => {
                let built = self.building.lock().await.remove(&steam);
                let Some((name, cps)) = built else { reply("[Race] Not building a race.", &player).await; return Outcome::Handled; };
                if cps.len() < 2 { reply("[Race] Need at least 2 checkpoints.", &player).await; return Outcome::Handled; }
                let n = cps.len();
                {
                    let mut state = self.state.lock().await;
                    state.races.insert(name.to_lowercase(), RaceDef {
                        name: name.clone(), checkpoints: cps, created_by: player.clone(),
                        best_time_secs: None, best_time_player: None,
                    });
                    save(&state);
                }
                broadcast(&format!("[Race] '{}' created with {} checkpoints! Type !race start {}", name, n, name)).await;
                Outcome::Handled
            }
            "start" => {
                if arg.is_empty() { reply("[Race] Usage: !race start <name>", &player).await; return Outcome::Handled; }
                let key = arg.to_lowercase();
                let n = {
                    let state = self.state.lock().await;
                    state.races.get(&key).map(|r| r.checkpoints.len())
                };
                let Some(n) = n else { reply(&format!("[Race] '{}' not found.", arg), &player).await; return Outcome::Handled; };
                self.racers.lock().await.push(ActiveRacer {
                    player: player.clone(), steam: steam.clone(), race_name: key,
                    started: Instant::now(), next_checkpoint: 0,
                });
                broadcast(&format!("[Race] {} started '{}' - {} checkpoints! GO!", player, arg, n)).await;
                Outcome::Handled
            }
            "list" => {
                let lines: Vec<String> = {
                    let state = self.state.lock().await;
                    if state.races.is_empty() {
                        vec!["[Race] No races defined.".to_string()]
                    } else {
                        state.races.values().map(|race| {
                            let best = race.best_time_secs.map(|t| format!(" (record: {:.1}s by {})", t, race.best_time_player.as_deref().unwrap_or("?"))).unwrap_or_default();
                            format!("[Race] {} - {} checkpoints{}", race.name, race.checkpoints.len(), best)
                        }).collect()
                    }
                };
                for l in lines { reply(&l, &player).await; }
                Outcome::Handled
            }
            _ => {
                reply("[Race] Commands: create/checkpoint/finish/start/list", &player).await;
                Outcome::Handled
            }
        }
    }
}
