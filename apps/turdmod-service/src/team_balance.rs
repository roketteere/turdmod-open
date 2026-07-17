// Team balance - auto-balance teams for warzone/event participation.
// !team join <red/blue> / leave / list / Admin: balance / reset. Command-only (in-memory).

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const OWNER_STEAM_ID: &str = "YOUR_STEAM_ID_1";
const OWNER_NAME: &str = "YOUR_OWNER_NAME";
const RATE_LIMIT: Duration = Duration::from_secs(3);

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

#[derive(Default)]
struct TeamState {
    red: HashSet<String>,
    blue: HashSet<String>,
    names: HashMap<String, String>, // steam -> name
}

pub struct TeamBalance { state: Mutex<TeamState>, rate: Mutex<HashMap<String, Instant>> }
impl TeamBalance {
    pub fn new() -> Self { Self { state: Mutex::new(TeamState::default()), rate: Mutex::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl Mod for TeamBalance {
    fn name(&self) -> &'static str { "team_balance" }
    fn commands(&self) -> &'static [&'static str] { &["!team"] }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let parts: Vec<&str> = text.split_whitespace().collect();
        if parts.first().map(|s| s.to_lowercase()).unwrap_or_default() != "!team" { return Outcome::Ignored; }

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
        let mut msgs: Vec<(bool, String)> = Vec::new(); // (is_broadcast, msg); replies go to player
        {
            let mut st = self.state.lock().await;
            st.names.insert(steam.clone(), player.clone());
            match sub.as_str() {
                "join" => {
                    let team = parts.get(2).map(|s| s.to_lowercase()).unwrap_or_default();
                    st.red.remove(&steam);
                    st.blue.remove(&steam);
                    match team.as_str() {
                        "red" => { st.red.insert(steam.clone()); msgs.push((false, "[Team] Joined RED team!".to_string())); }
                        "blue" => { st.blue.insert(steam.clone()); msgs.push((false, "[Team] Joined BLUE team!".to_string())); }
                        _ => {
                            if st.red.len() <= st.blue.len() { st.red.insert(steam.clone()); msgs.push((false, "[Team] Auto-assigned to RED team!".to_string())); }
                            else { st.blue.insert(steam.clone()); msgs.push((false, "[Team] Auto-assigned to BLUE team!".to_string())); }
                        }
                    }
                }
                "leave" => {
                    st.red.remove(&steam);
                    st.blue.remove(&steam);
                    msgs.push((false, "[Team] Left your team.".to_string()));
                }
                "balance" | "shuffle" => {
                    if !is_owner(&steam, &player) { msgs.push((false, "[Team] Admin only.".to_string())); }
                    else {
                        let all: Vec<String> = st.red.iter().chain(st.blue.iter()).cloned().collect();
                        st.red.clear();
                        st.blue.clear();
                        for (i, s) in all.iter().enumerate() {
                            if i % 2 == 0 { st.red.insert(s.clone()); } else { st.blue.insert(s.clone()); }
                        }
                        msgs.push((true, format!("[Team] Teams balanced! Red: {} / Blue: {}", st.red.len(), st.blue.len())));
                    }
                }
                "list" | "teams" | "" => {
                    let red_names: Vec<&str> = st.red.iter().filter_map(|s| st.names.get(s).map(|n| n.as_str())).collect();
                    let blue_names: Vec<&str> = st.blue.iter().filter_map(|s| st.names.get(s).map(|n| n.as_str())).collect();
                    msgs.push((false, format!("[RED {}] {}", st.red.len(), if red_names.is_empty() { "empty".to_string() } else { red_names.join(", ") })));
                    msgs.push((false, format!("[BLUE {}] {}", st.blue.len(), if blue_names.is_empty() { "empty".to_string() } else { blue_names.join(", ") })));
                }
                "reset" => {
                    if !is_owner(&steam, &player) { msgs.push((false, "[Team] Admin only.".to_string())); }
                    else { st.red.clear(); st.blue.clear(); msgs.push((true, "[Team] All teams cleared.".to_string())); }
                }
                _ => msgs.push((false, "[Team] Commands: join [red/blue] / leave / list / Admin: balance / reset".to_string())),
            }
        }
        for (is_bc, m) in &msgs {
            if *is_bc { broadcast(m).await } else { reply(m, &player).await }
        }
        Outcome::Handled
    }
}
