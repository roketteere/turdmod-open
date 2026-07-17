// Convoy escort - cargo transport missions between two points.
// !convoy start/status/cancel/routes. Drive from A to B within 15min; reward on arrival.
// 5s interval tick checks completion/timeout via ctx.map.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const ECON_PATH: &str = r"C:\TurdMOD\data\economy.json";
const RATE_LIMIT: Duration = Duration::from_secs(5);
const CONVOY_TIMEOUT: Duration = Duration::from_secs(900); // 15 min
const DESTINATION_RADIUS: f64 = 5000.0; // 50m
const CHECK_INTERVAL: Duration = Duration::from_secs(5);

const ROUTES: &[(&str, f64, f64, f64, f64, i64)] = &[
    ("Airfield Run",      -100000.0, -100000.0,  200000.0,  200000.0, 300),
    ("Coastal Express",   -300000.0,  100000.0,  300000.0,  100000.0, 400),
    ("Mountain Pass",      50000.0,  -200000.0,   50000.0,  300000.0, 350),
    ("Cross-Island",      -250000.0, -250000.0,  250000.0,  250000.0, 500),
];

#[derive(Clone)]
struct ActiveConvoy {
    player: String,
    steam: String,
    route_name: String,
    dest_x: f64,
    dest_y: f64,
    reward: i64,
    started: Instant,
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

fn grid_ref(x: f64, y: f64) -> String {
    let col = ((x + 400000.0) / 100000.0) as u8;
    let row = ((y + 400000.0) / 100000.0) as u8;
    format!("{}{}", (b'A' + col.min(7)) as char, row.min(7) + 1)
}

fn pick_route() -> &'static (&'static str, f64, f64, f64, f64, i64) {
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_nanos() as usize;
    &ROUTES[seed % ROUTES.len()]
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

async fn broadcast(msg: &str) {
    crate::auto_announce::announce(msg).await; // server-wide event -> #Announce banner + chat
    let params = serde_json::json!({ "text": msg });
    pipe_rpc::call("broadcastChat", Some(params)).await.ok();
}

pub struct Convoy {
    active: Mutex<Vec<ActiveConvoy>>,
    rate: Mutex<HashMap<String, Instant>>,
}
impl Convoy {
    pub fn new() -> Self { Self { active: Mutex::new(Vec::new()), rate: Mutex::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl Mod for Convoy {
    fn name(&self) -> &'static str { "convoy" }
    fn commands(&self) -> &'static [&'static str] { &["!convoy"] }
    fn interval(&self) -> Option<Duration> { Some(CHECK_INTERVAL) }

    // Dormant unless a convoy is active.
    async fn active(&self) -> bool { !self.active.lock().await.is_empty() }

    // Completion / timeout sweep (the old select! interval branch).
    async fn tick(&self, ctx: &ModCtx) {
        if self.active.lock().await.is_empty() { return; }
        let snapshot = ctx.map.read().await.clone();
        let positions: HashMap<String, (f64, f64)> = snapshot.players.iter().map(|p| (p.name.clone(), (p.x, p.y))).collect();

        let mut completed_msgs: Vec<String> = Vec::new();
        let mut expired_msgs: Vec<(String, String)> = Vec::new(); // (player, route)
        {
            let mut active = self.active.lock().await;
            let mut completed = Vec::new();
            let mut expired = Vec::new();
            for (i, convoy) in active.iter().enumerate() {
                if convoy.started.elapsed() > CONVOY_TIMEOUT { expired.push(i); continue; }
                if let Some(&(px, py)) = positions.get(&convoy.player) {
                    let dx = px - convoy.dest_x;
                    let dy = py - convoy.dest_y;
                    if (dx * dx + dy * dy).sqrt() < DESTINATION_RADIUS { completed.push(i); }
                }
            }
            for &i in completed.iter().rev() {
                let c = active.remove(i);
                let time = c.started.elapsed().as_secs();
                credit(&c.steam, c.reward);
                completed_msgs.push(format!("[Convoy] {} completed '{}' in {}s! +{} coins!", c.player, c.route_name, time, c.reward));
            }
            for &i in expired.iter().rev() {
                if i < active.len() {
                    let c = active.remove(i);
                    expired_msgs.push((c.player.clone(), c.route_name.clone()));
                }
            }
        }
        for m in &completed_msgs { broadcast(m).await; }
        for (player, route) in &expired_msgs { reply(&format!("[Convoy] '{}' timed out!", route), player).await; }
    }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        if !text.starts_with("!convoy") { return Outcome::Ignored; }
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

        let sub = text.split_whitespace().nth(1).map(|s| s.to_lowercase()).unwrap_or_default();
        match sub.as_str() {
            "start" | "go" => {
                // Ok(broadcast) on start, Err(reply) if already active.
                let result: Result<String, String> = {
                    let mut active = self.active.lock().await;
                    if active.iter().any(|c| c.steam == steam) {
                        Err("[Convoy] You already have an active convoy!".to_string())
                    } else {
                        let &(name, _sx, _sy, ex, ey, reward) = pick_route();
                        active.push(ActiveConvoy {
                            player: player.clone(), steam: steam.clone(), route_name: name.into(),
                            dest_x: ex, dest_y: ey, reward, started: Instant::now(),
                        });
                        Ok(format!("[Convoy] {} started '{}' - deliver to grid {} within 15min for {}c!", player, name, grid_ref(ex, ey), reward))
                    }
                };
                match result { Ok(bc) => broadcast(&bc).await, Err(r) => reply(&r, &player).await }
                Outcome::Handled
            }
            "status" => {
                let msg = {
                    let active = self.active.lock().await;
                    match active.iter().find(|c| c.steam == steam) {
                        Some(c) => {
                            let remaining = CONVOY_TIMEOUT.checked_sub(c.started.elapsed()).map(|d| d.as_secs() / 60).unwrap_or(0);
                            format!("[Convoy] '{}' - {}min left, destination: {}", c.route_name, remaining, grid_ref(c.dest_x, c.dest_y))
                        }
                        None => "[Convoy] No active convoy. !convoy start".to_string(),
                    }
                };
                reply(&msg, &player).await;
                Outcome::Handled
            }
            "cancel" => {
                let cancelled = {
                    let mut active = self.active.lock().await;
                    let before = active.len();
                    active.retain(|c| c.steam != steam);
                    active.len() < before
                };
                if cancelled { reply("[Convoy] Mission cancelled.", &player).await; }
                else { reply("[Convoy] No active convoy.", &player).await; }
                Outcome::Handled
            }
            "routes" => {
                for &(name, _, _, ex, ey, reward) in ROUTES {
                    reply(&format!("[Convoy] {} - {} to {} ({}c)", name, "random start", grid_ref(ex, ey), reward), &player).await;
                }
                Outcome::Handled
            }
            _ => {
                reply("[Convoy] Commands: start / status / cancel / routes", &player).await;
                Outcome::Handled
            }
        }
    }
}
