// Building zones - restrict building in admin-defined areas.
// !nobuild add/remove (admin) / list, !nobuildzones (list alias). Command-only.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const OWNER_STEAM_ID: &str = "YOUR_STEAM_ID_1";
const OWNER_NAME: &str = "YOUR_OWNER_NAME";
const STATE_PATH: &str = r"C:\TurdMOD\data\building_zones.json";
const RATE_LIMIT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct NoBuildZone {
    name: String,
    x: f64,
    y: f64,
    radius: f64,
    reason: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
struct ZoneState {
    zones: Vec<NoBuildZone>,
}

fn load() -> ZoneState {
    std::fs::read_to_string(STATE_PATH).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(state: &ZoneState) {
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let tmp = format!("{}.tmp", STATE_PATH);
        if std::fs::write(&tmp, &json).is_ok() { let _ = std::fs::rename(&tmp, STATE_PATH); }
    }
}

fn is_owner(steam: &str, player: &str) -> bool {
    steam == OWNER_STEAM_ID || steam == "YOUR_STEAM_ID_2" || player == OWNER_NAME
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

pub struct BuildingZones { state: Mutex<ZoneState>, rate: Mutex<HashMap<String, Instant>> }
impl BuildingZones {
    pub fn new() -> Self { Self { state: Mutex::new(load()), rate: Mutex::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl Mod for BuildingZones {
    fn name(&self) -> &'static str { "building_zones" }
    fn commands(&self) -> &'static [&'static str] { &["!nobuild", "!nobuildzones"] }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let parts: Vec<&str> = text.split_whitespace().collect();
        let cmd = parts.first().map(|s| s.to_lowercase()).unwrap_or_default();
        if !matches!(cmd.as_str(), "!nobuild" | "!nobuildzones") { return Outcome::Ignored; }

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
            "add" => {
                if !is_owner(&steam, &player) { reply("[Build] Admin only.", &player).await; return Outcome::Handled; }
                if parts.len() < 5 { reply("[Build] Usage: !nobuild add <name> <x> <y> [radius] [reason]", &player).await; return Outcome::Handled; }
                let name = parts[2].to_string();
                let x: f64 = parts[3].parse().unwrap_or(0.0);
                let y: f64 = parts[4].parse().unwrap_or(0.0);
                let radius: f64 = parts.get(5).and_then(|s| s.parse().ok()).unwrap_or(10000.0);
                let reason = if parts.len() > 6 { parts[6..].join(" ") } else { "restricted area".to_string() };
                {
                    let mut st = self.state.lock().await;
                    st.zones.push(NoBuildZone { name: name.clone(), x, y, radius, reason });
                    save(&st);
                }
                reply(&format!("[Build] No-build zone '{}' at {} (r={})", name, grid_ref(x, y), radius), &player).await;
                Outcome::Handled
            }
            "remove" | "del" => {
                if !is_owner(&steam, &player) { reply("[Build] Admin only.", &player).await; return Outcome::Handled; }
                let name = parts.get(2).map(|s| s.to_lowercase()).unwrap_or_default();
                let removed = {
                    let mut st = self.state.lock().await;
                    let before = st.zones.len();
                    st.zones.retain(|z| z.name.to_lowercase() != name);
                    let removed = st.zones.len() < before;
                    if removed { save(&st); }
                    removed
                };
                if removed { reply(&format!("[Build] '{}' removed.", name), &player).await; }
                else { reply("[Build] Not found.", &player).await; }
                Outcome::Handled
            }
            "list" | "" => {
                let lines: Vec<String> = {
                    let st = self.state.lock().await;
                    if st.zones.is_empty() {
                        vec!["[Build] No restricted zones.".to_string()]
                    } else {
                        st.zones.iter().map(|z| format!("[Build] {} - {} r={} ({})", z.name, grid_ref(z.x, z.y), z.radius, z.reason)).collect()
                    }
                };
                for l in lines { reply(&l, &player).await; }
                Outcome::Handled
            }
            _ => {
                reply("[Build] Commands: list / Admin: add / remove", &player).await;
                Outcome::Handled
            }
        }
    }
}
