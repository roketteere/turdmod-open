// Fast travel / taxi - pay coins to teleport between named locations.
// !taxi <destination> / !destinations / !addtaxi (admin) / !deltaxi (admin). Command-only.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const OWNER_STEAM_ID: &str = "YOUR_STEAM_ID_1";
const OWNER_NAME: &str = "YOUR_OWNER_NAME";
const STATE_PATH: &str = r"C:\TurdMOD\data\fast_travel.json";
const ECON_PATH: &str = r"C:\TurdMOD\data\economy.json";
const RATE_LIMIT: Duration = Duration::from_secs(5);
const BASE_COST: i64 = 25;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Destination {
    name: String,
    x: f64,
    y: f64,
    z: f64,
    cost: i64,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
struct TaxiState {
    destinations: Vec<Destination>,
}

fn load() -> TaxiState {
    std::fs::read_to_string(STATE_PATH).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(state: &TaxiState) {
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let tmp = format!("{}.tmp", STATE_PATH);
        if std::fs::write(&tmp, &json).is_ok() { let _ = std::fs::rename(&tmp, STATE_PATH); }
    }
}

fn debit(steam: &str, amount: i64) -> bool {
    let Ok(data) = std::fs::read_to_string(ECON_PATH) else { return false };
    let Ok(mut state) = serde_json::from_str::<serde_json::Value>(&data) else { return false };
    let bal = state.get("players").and_then(|p| p.get(steam))
        .and_then(|p| p.get("balance")).and_then(|b| b.as_i64()).unwrap_or(0);
    if bal < amount { return false; }
    state["players"][steam]["balance"] = serde_json::json!(bal - amount);
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(&state) {
        let tmp = format!("{}.tmp", ECON_PATH);
        if std::fs::write(&tmp, &json).is_ok() { let _ = std::fs::rename(&tmp, ECON_PATH); }
    }
    true
}

fn is_owner(steam: &str, player: &str) -> bool {
    steam == OWNER_STEAM_ID || steam == "YOUR_STEAM_ID_2" || player == OWNER_NAME
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

pub struct FastTravel { state: Mutex<TaxiState>, rate: Mutex<HashMap<String, Instant>> }
impl FastTravel {
    pub fn new() -> Self { Self { state: Mutex::new(load()), rate: Mutex::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl Mod for FastTravel {
    fn name(&self) -> &'static str { "fast_travel" }
    fn commands(&self) -> &'static [&'static str] { &["!taxi", "!travel", "!destinations", "!stops", "!addtaxi", "!deltaxi"] }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        if !text.starts_with('!') { return Outcome::Ignored; }
        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let parts: Vec<&str> = text.split_whitespace().collect();
        let cmd = parts.first().map(|s| s.to_lowercase()).unwrap_or_default();
        if !matches!(cmd.as_str(), "!taxi" | "!travel" | "!destinations" | "!stops" | "!addtaxi" | "!deltaxi") { return Outcome::Ignored; }

        let rate_key = if steam.is_empty() { player.clone() } else { steam.clone() };
        {
            let mut rate = self.rate.lock().await;
            let now = Instant::now();
            if let Some(prev) = rate.get(&rate_key) {
                if now.duration_since(*prev) < RATE_LIMIT { return Outcome::Ignored; }
            }
            rate.insert(rate_key.clone(), now);
        }

        match cmd.as_str() {
            "!taxi" | "!travel" => {
                if parts.len() < 2 { reply("[Taxi] Usage: !taxi <destination>. !destinations for list.", &player).await; return Outcome::Handled; }
                let dest_name = parts[1].to_lowercase();
                let dest = { self.state.lock().await.destinations.iter().find(|d| d.name.to_lowercase() == dest_name).cloned() };
                let Some(dest) = dest else { reply(&format!("[Taxi] '{}' not found. !destinations for list.", dest_name), &player).await; return Outcome::Handled; };
                if !debit(&steam, dest.cost) { reply(&format!("[Taxi] Need {} coins (insufficient funds).", dest.cost), &player).await; return Outcome::Handled; }
                // @inv: bridge wants "name"; coords as strings (bridge mis-parses large bare numbers).
                let tp = serde_json::json!({ "name": player, "x": dest.x.to_string(), "y": dest.y.to_string(), "z": dest.z.to_string() });
                match pipe_rpc::call("teleportPlayer", Some(tp)).await {
                    Ok(r) if r["teleported"].as_bool().unwrap_or(false) => reply(&format!("[Taxi] Arrived at {}! (-{} coins)", dest.name, dest.cost), &player).await,
                    _ => reply("[Taxi] Teleport failed (try again).", &player).await,
                }
                Outcome::Handled
            }
            "!destinations" | "!stops" => {
                let lines: Vec<String> = {
                    let st = self.state.lock().await;
                    if st.destinations.is_empty() {
                        vec!["[Taxi] No destinations set. Admin: !addtaxi <name> <x> <y> <z> [cost]".to_string()]
                    } else {
                        st.destinations.iter().map(|d| format!("[Taxi] {} - {} coins", d.name, d.cost)).collect()
                    }
                };
                for l in lines { reply(&l, &player).await; }
                Outcome::Handled
            }
            "!addtaxi" => {
                if !is_owner(&steam, &player) { reply("[Taxi] Admin only.", &player).await; return Outcome::Handled; }
                if parts.len() < 5 { reply("[Taxi] Usage: !addtaxi <name> <x> <y> <z> [cost]", &player).await; return Outcome::Handled; }
                let name = parts[1].to_string();
                let x: f64 = parts[2].parse().unwrap_or(0.0);
                let y: f64 = parts[3].parse().unwrap_or(0.0);
                let z: f64 = parts[4].parse().unwrap_or(0.0);
                let cost: i64 = parts.get(5).and_then(|s| s.parse().ok()).unwrap_or(BASE_COST);
                {
                    let mut st = self.state.lock().await;
                    st.destinations.push(Destination { name: name.clone(), x, y, z, cost });
                    save(&st);
                }
                reply(&format!("[Taxi] '{}' added ({} coins).", name, cost), &player).await;
                Outcome::Handled
            }
            "!deltaxi" => {
                if !is_owner(&steam, &player) { reply("[Taxi] Admin only.", &player).await; return Outcome::Handled; }
                let name = parts.get(1).map(|s| s.to_lowercase()).unwrap_or_default();
                let removed = {
                    let mut st = self.state.lock().await;
                    let before = st.destinations.len();
                    st.destinations.retain(|d| d.name.to_lowercase() != name);
                    let removed = st.destinations.len() < before;
                    if removed { save(&st); }
                    removed
                };
                if removed { reply(&format!("[Taxi] '{}' removed.", name), &player).await; }
                else { reply("[Taxi] Not found.", &player).await; }
                Outcome::Handled
            }
            _ => Outcome::Ignored,
        }
    }
}
