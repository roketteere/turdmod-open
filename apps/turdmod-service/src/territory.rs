// Territory control - clans capture and hold zones for passive income.
// !territory claim [name] / list / unclaim <name> (!terr alias). Income paid every 10 min.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const STATE_PATH: &str = r"C:\TurdMOD\data\territories.json";
const ECON_PATH: &str = r"C:\TurdMOD\data\economy.json";
const RATE_LIMIT: Duration = Duration::from_secs(3);
const INCOME_INTERVAL: Duration = Duration::from_secs(600); // 10 min
const INCOME_PER_TERRITORY: i64 = 25;
const MIN_DISTANCE: f64 = 30000.0; // territories must be 300m apart

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Territory {
    name: String,
    clan: String,
    claimed_by: String,
    x: f64,
    y: f64,
    claimed_ts: u64,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
struct TerritoryState {
    territories: Vec<Territory>,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_secs()
}

fn load() -> TerritoryState {
    std::fs::read_to_string(STATE_PATH).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(state: &TerritoryState) {
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let tmp = format!("{}.tmp", STATE_PATH);
        if std::fs::write(&tmp, &json).is_ok() { let _ = std::fs::rename(&tmp, STATE_PATH); }
    }
}

fn credit_clan_members(clan_name: &str, amount: i64) {
    let clan_path = r"C:\TurdMOD\data\clans.json";
    let Ok(clan_data) = std::fs::read_to_string(clan_path) else { return };
    let Ok(clans) = serde_json::from_str::<serde_json::Value>(&clan_data) else { return };
    let key = clan_name.to_lowercase();
    let Some(clan) = clans.get("clans").and_then(|c| c.get(&key)) else { return };
    let Some(members) = clan.get("members").and_then(|m| m.as_array()) else { return };

    let Ok(econ_data) = std::fs::read_to_string(ECON_PATH) else { return };
    let Ok(mut econ) = serde_json::from_str::<serde_json::Value>(&econ_data) else { return };
    for member in members {
        let Some(steam) = member.get("steam").and_then(|s| s.as_str()) else { continue };
        let bal = econ.get("players").and_then(|p| p.get(steam))
            .and_then(|p| p.get("balance")).and_then(|b| b.as_i64()).unwrap_or(0);
        econ["players"][steam]["balance"] = serde_json::json!(bal + amount);
    }
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(&econ) {
        let tmp = format!("{}.tmp", ECON_PATH);
        if std::fs::write(&tmp, &json).is_ok() { let _ = std::fs::rename(&tmp, ECON_PATH); }
    }
}

fn grid_ref(x: f64, y: f64) -> String {
    let col = ((x + 400000.0) / 100000.0) as u8;
    let row = ((y + 400000.0) / 100000.0) as u8;
    format!("{}{}", (b'A' + col.min(7)) as char, row.min(7) + 1)
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

async fn broadcast(msg: &str) {
    let params = serde_json::json!({ "text": msg });
    pipe_rpc::call("broadcastChat", Some(params)).await.ok();
}

fn get_player_clan(steam: &str) -> Option<String> {
    let path = r"C:\TurdMOD\data\clans.json";
    let data = std::fs::read_to_string(path).ok()?;
    let state: serde_json::Value = serde_json::from_str(&data).ok()?;
    state.get("player_clan")?.get(steam)?.as_str().map(String::from)
}

pub struct Territory_ { state: Mutex<TerritoryState>, rate: Mutex<HashMap<String, Instant>> }
impl Territory_ {
    pub fn new() -> Self { Self { state: Mutex::new(load()), rate: Mutex::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl Mod for Territory_ {
    fn name(&self) -> &'static str { "territory" }
    fn commands(&self) -> &'static [&'static str] { &["!territory", "!terr"] }
    fn interval(&self) -> Option<Duration> { Some(INCOME_INTERVAL) }

    // Passive clan income every 10 min (the old select! interval branch).
    async fn tick(&self, _ctx: &ModCtx) {
        let clans_paid: HashMap<String, u32> = {
            let st = self.state.lock().await;
            let mut m: HashMap<String, u32> = HashMap::new();
            for t in &st.territories { *m.entry(t.clan.clone()).or_insert(0) += 1; }
            m
        };
        for (clan, count) in &clans_paid {
            credit_clan_members(clan, INCOME_PER_TERRITORY * *count as i64);
        }
    }

    async fn handle(&self, ev: &GameEvent, ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let parts: Vec<&str> = text.split_whitespace().collect();
        let cmd = parts.first().map(|s| s.to_lowercase()).unwrap_or_default();
        if !matches!(cmd.as_str(), "!territory" | "!terr") { return Outcome::Ignored; }

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
        match sub.as_str() {
            "claim" => {
                let Some(clan) = get_player_clan(&steam) else {
                    reply("[Territory] Join a clan first (!clan create/accept).", &player).await;
                    return Outcome::Handled;
                };
                let pos = { let snap = ctx.map.read().await; snap.players.iter().find(|p| p.name == player).map(|p| (p.x, p.y)) };
                let Some((px, py)) = pos else { reply("[Territory] Position unknown.", &player).await; return Outcome::Handled; };

                // Ok(broadcast) on success, Err(reply) otherwise.
                let result: Result<String, String> = {
                    let mut st = self.state.lock().await;
                    let too_close = st.territories.iter().any(|t| {
                        let dx = t.x - px; let dy = t.y - py;
                        (dx * dx + dy * dy).sqrt() < MIN_DISTANCE
                    });
                    if too_close {
                        Err("[Territory] Too close to another territory (min 300m).".to_string())
                    } else {
                        let name = parts.get(2).map(|s| s.to_string()).unwrap_or_else(|| format!("{}-{}", clan, grid_ref(px, py)));
                        st.territories.push(Territory { name: name.clone(), clan: clan.clone(), claimed_by: player.clone(), x: px, y: py, claimed_ts: now_secs() });
                        save(&st);
                        Ok(format!("[Territory] {} claimed '{}' at {} for clan {}!", player, name, grid_ref(px, py), clan))
                    }
                };
                match result {
                    Ok(bc) => broadcast(&bc).await,
                    Err(r) => reply(&r, &player).await,
                }
                Outcome::Handled
            }
            "list" => {
                let lines: Vec<String> = {
                    let st = self.state.lock().await;
                    if st.territories.is_empty() {
                        vec!["[Territory] No territories claimed.".to_string()]
                    } else {
                        st.territories.iter().map(|t| format!("[Territory] {} - {} (clan: {}, grid: {})", t.name, t.claimed_by, t.clan, grid_ref(t.x, t.y))).collect()
                    }
                };
                for l in lines { reply(&l, &player).await; }
                Outcome::Handled
            }
            "unclaim" => {
                let name = parts.get(2).map(|s| s.to_lowercase()).unwrap_or_default();
                let removed = {
                    let mut st = self.state.lock().await;
                    let before = st.territories.len();
                    st.territories.retain(|t| t.name.to_lowercase() != name || t.claimed_by != player);
                    let removed = st.territories.len() < before;
                    if removed { save(&st); }
                    removed
                };
                if removed { reply(&format!("[Territory] '{}' released.", name), &player).await; }
                else { reply("[Territory] Not found or not yours.", &player).await; }
                Outcome::Handled
            }
            _ => {
                reply("[Territory] Commands: claim [name] / list / unclaim <name>", &player).await;
                Outcome::Handled
            }
        }
    }
}
