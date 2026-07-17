// Map markers - shared pins between players/clans.
// !mark <name> - drop a marker at your position. !marks - list. !delmark <name> - remove.
// !sharemark <name> - toggle public. Command-only; reads ctx.map for the player's position.
// (commands()-based routing also fixes a latent bug where the old starts_with("!mark") gate
//  dead-ended !delmark/!sharemark.)

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const STATE_PATH: &str = r"C:\TurdMOD\data\markers.json";
const RATE_LIMIT: Duration = Duration::from_secs(3);
const MAX_MARKERS_PER_PLAYER: usize = 10;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Marker {
    name: String,
    owner_steam: String,
    owner_name: String,
    x: f64,
    y: f64,
    public: bool,
    clan: Option<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
struct MarkerState {
    markers: Vec<Marker>,
}

fn load() -> MarkerState {
    std::fs::read_to_string(STATE_PATH).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(state: &MarkerState) {
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let tmp = format!("{}.tmp", STATE_PATH);
        if std::fs::write(&tmp, &json).is_ok() { let _ = std::fs::rename(&tmp, STATE_PATH); }
    }
}

fn grid_ref(x: f64, y: f64) -> String {
    let col = ((x + 400000.0) / 100000.0) as u8;
    let row = ((y + 400000.0) / 100000.0) as u8;
    format!("{}{}", (b'A' + col.min(7)) as char, row.min(7) + 1)
}

fn get_player_clan(steam: &str) -> Option<String> {
    let data = std::fs::read_to_string(r"C:\TurdMOD\data\clans.json").ok()?;
    let state: serde_json::Value = serde_json::from_str(&data).ok()?;
    state.get("player_clan")?.get(steam)?.as_str().map(String::from)
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

pub struct MapMarkers { state: Mutex<MarkerState>, rate: Mutex<HashMap<String, Instant>> }
impl MapMarkers {
    pub fn new() -> Self { Self { state: Mutex::new(load()), rate: Mutex::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl Mod for MapMarkers {
    fn name(&self) -> &'static str { "map_markers" }
    fn commands(&self) -> &'static [&'static str] { &["!mark", "!marks", "!delmark", "!sharemark"] }

    async fn handle(&self, ev: &GameEvent, ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let parts: Vec<&str> = text.split_whitespace().collect();
        let cmd = parts.first().map(|s| s.to_lowercase()).unwrap_or_default();
        if !matches!(cmd.as_str(), "!mark" | "!marks" | "!delmark" | "!sharemark") { return Outcome::Ignored; }

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
            "!mark" => {
                if parts.len() < 2 { reply("[Marker] Usage: !mark <name>", &player).await; return Outcome::Handled; }
                let name = parts[1].to_string();
                let pos = { let snap = ctx.map.read().await; snap.players.iter().find(|p| p.name == player).map(|p| (p.x, p.y)) };
                let Some((px, py)) = pos else { reply("[Marker] Position unknown.", &player).await; return Outcome::Handled; };
                let clan = get_player_clan(&steam);
                let msg = {
                    let mut state = self.state.lock().await;
                    let own_count = state.markers.iter().filter(|m| m.owner_steam == steam).count();
                    if own_count >= MAX_MARKERS_PER_PLAYER {
                        format!("[Marker] Max {} markers. !delmark <name> to free one.", MAX_MARKERS_PER_PLAYER)
                    } else {
                        state.markers.push(Marker { name: name.clone(), owner_steam: steam.clone(), owner_name: player.clone(), x: px, y: py, public: false, clan });
                        save(&state);
                        format!("[Marker] '{}' set at grid {}", name, grid_ref(px, py))
                    }
                };
                reply(&msg, &player).await;
                Outcome::Handled
            }
            "!marks" => {
                let clan = get_player_clan(&steam);
                let lines: Vec<String> = {
                    let state = self.state.lock().await;
                    let visible: Vec<&Marker> = state.markers.iter().filter(|m| {
                        m.owner_steam == steam || m.public || (clan.is_some() && m.clan == clan)
                    }).collect();
                    if visible.is_empty() {
                        vec!["[Marker] No markers.".to_string()]
                    } else {
                        visible.iter().take(10).map(|m| {
                            let scope = if m.public { "public" } else if m.owner_steam == steam { "yours" } else { "clan" };
                            format!("[Marker] {} - {} ({})", m.name, grid_ref(m.x, m.y), scope)
                        }).collect()
                    }
                };
                for l in lines { reply(&l, &player).await; }
                Outcome::Handled
            }
            "!delmark" => {
                let name = parts.get(1).map(|s| s.to_lowercase()).unwrap_or_default();
                let removed = {
                    let mut state = self.state.lock().await;
                    let before = state.markers.len();
                    state.markers.retain(|m| !(m.name.to_lowercase() == name && m.owner_steam == steam));
                    let removed = state.markers.len() < before;
                    if removed { save(&state); }
                    removed
                };
                if removed { reply(&format!("[Marker] '{}' deleted.", name), &player).await; }
                else { reply("[Marker] Not found or not yours.", &player).await; }
                Outcome::Handled
            }
            "!sharemark" => {
                let name = parts.get(1).map(|s| s.to_lowercase()).unwrap_or_default();
                let msg = {
                    let mut state = self.state.lock().await;
                    if let Some(m) = state.markers.iter_mut().find(|m| m.name.to_lowercase() == name && m.owner_steam == steam) {
                        m.public = !m.public;
                        let status = if m.public { "PUBLIC" } else { "private" };
                        let s = format!("[Marker] '{}' is now {}", name, status);
                        save(&state);
                        s
                    } else {
                        "[Marker] Not found or not yours.".to_string()
                    }
                };
                reply(&msg, &player).await;
                Outcome::Handled
            }
            _ => Outcome::Ignored,
        }
    }
}
