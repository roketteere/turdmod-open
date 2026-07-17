// PvP zones — permanent RED no-rules areas. The opposite of safe_zones.
// Entering shows a red danger HUD/warning; kills INSIDE a zone pay bonus coins
// (a permanent warzone — incentivizes fighting over high-value POIs).
// Commands: !addpvp <name> <x> <y> <radius> (admin), !delpvp <name> (admin), !pvpzones.
// @dep: ctx.map snapshot (player positions), economy.json, events.rs (kill: killer/killerSteam)
// @note: event-driven (needs kill events) + 5s interval tick for the danger HUD.

use std::collections::HashSet;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const OWNER_STEAM_ID: &str = "YOUR_STEAM_ID_1";
const OWNER_NAME: &str = "YOUR_OWNER_NAME";
const STATE_PATH: &str = r"C:\TurdMOD\data\pvp_zones.json";
const CHECK_INTERVAL: Duration = Duration::from_secs(5);
const RATE_LIMIT: Duration = Duration::from_secs(3);
const KILL_BONUS: i64 = 40; // coins per kill inside a red zone

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PvpZone {
    name: String,
    x: f64,
    y: f64,
    radius: f64,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
struct ZoneState {
    zones: Vec<PvpZone>,
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

fn in_zone(px: f64, py: f64, z: &PvpZone) -> bool {
    let dx = px - z.x;
    let dy = py - z.y;
    (dx * dx + dy * dy).sqrt() <= z.radius
}
fn zone_at<'a>(px: f64, py: f64, zones: &'a [PvpZone]) -> Option<&'a PvpZone> {
    zones.iter().find(|z| in_zone(px, py, z))
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}
// Red danger line — channel "1" renders red in SCUM chat (admin/alert color).
async fn red_line(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "1" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}
async fn hud(text: &str, player: &str) {
    let params = serde_json::json!({ "text": text, "playerName": player });
    pipe_rpc::call("sendHudMessage", Some(params)).await.ok();
}
async fn broadcast(msg: &str) {
    let params = serde_json::json!({ "text": msg });
    pipe_rpc::call("broadcastChat", Some(params)).await.ok();
}
fn is_owner(steam: &str, player: &str) -> bool { steam == OWNER_STEAM_ID || steam == "YOUR_STEAM_ID_2" || player == OWNER_NAME }

fn credit(steam: &str, amount: i64) {
    let path = r"C:\TurdMOD\data\economy.json";
    let Ok(data) = std::fs::read_to_string(path) else { return };
    let Ok(mut state) = serde_json::from_str::<serde_json::Value>(&data) else { return };
    if let Some(bal) = state.get_mut("players")
        .and_then(|p| p.get_mut(steam))
        .and_then(|p| p.get_mut("balance"))
        .and_then(|b| b.as_i64())
    {
        state["players"][steam]["balance"] = serde_json::json!(bal + amount);
        if let Ok(json) = serde_json::to_string_pretty(&state) {
            let tmp = format!("{}.tmp", path);
            if std::fs::write(&tmp, &json).is_ok() { let _ = std::fs::rename(&tmp, path); }
        }
    }
}

pub struct PvpZones {
    state: Mutex<ZoneState>,
    rate: Mutex<HashMap<String, Instant>>,
    inside: Mutex<HashSet<String>>,
}
impl PvpZones {
    pub fn new() -> Self {
        Self { state: Mutex::new(load()), rate: Mutex::new(HashMap::new()), inside: Mutex::new(HashSet::new()) }
    }
}

#[async_trait::async_trait]
impl Mod for PvpZones {
    fn name(&self) -> &'static str { "pvp_zones" }
    // event-driven (no commands()): needs `kill` events for the zone bonus, plus chat commands.
    fn interval(&self) -> Option<Duration> { Some(CHECK_INTERVAL) }

    // Danger HUD on enter/leave (the old select! interval branch).
    async fn tick(&self, ctx: &ModCtx) {
        let zones = {
            let s = self.state.lock().await;
            if s.zones.is_empty() { return; }
            s.zones.clone()
        };
        let snapshot = ctx.map.read().await.clone();
        let mut inside = self.inside.lock().await;
        for p in &snapshot.players {
            if let Some(z) = zone_at(p.x, p.y, &zones) {
                if !inside.contains(&p.name) {
                    inside.insert(p.name.clone());
                    hud(&format!("!! {} -- RED ZONE -- NO RULES. Kill, raid, rob. Bonus coins per kill.", z.name), &p.name).await;
                    red_line(&format!("[RED ZONE] You entered {}. NO RULES here -- watch your back.", z.name), &p.name).await;
                }
            } else if inside.contains(&p.name) {
                inside.remove(&p.name);
                reply("[RED ZONE] You left the red zone. Back to civilization.", &p.name).await;
            }
        }
        let online: HashSet<String> = snapshot.players.iter().map(|p| p.name.clone()).collect();
        inside.retain(|n| online.contains(n));
    }

    async fn handle(&self, ev: &GameEvent, ctx: &ModCtx) -> Outcome {
        match ev.event.as_str() {
            "kill" => {
                let ks = ev.data.get("killerSteam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let kn = ev.data.get("killer").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if ks.is_empty() || kn.is_empty() { return Outcome::Ignored; }
                let zones = {
                    let s = self.state.lock().await;
                    if s.zones.is_empty() { return Outcome::Ignored; }
                    s.zones.clone()
                };
                let snapshot = ctx.map.read().await.clone();
                if let Some(kp) = snapshot.players.iter().find(|p| p.name == kn) {
                    if let Some(z) = zone_at(kp.x, kp.y, &zones) {
                        credit(&ks, KILL_BONUS);
                        broadcast(&format!("[RED ZONE] {} drew blood in {}! +{} coins.", kn, z.name, KILL_BONUS)).await;
                        return Outcome::Handled;
                    }
                }
                Outcome::Ignored
            }

            "chat" => {
                let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
                if !text.starts_with('!') { return Outcome::Ignored; }
                let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let cmd = text.split_whitespace().next().unwrap_or("").to_lowercase();
                if !matches!(cmd.as_str(), "!addpvp" | "!delpvp" | "!pvpzones") { return Outcome::Ignored; }

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
                match cmd.as_str() {
                    "!addpvp" => {
                        if !is_owner(&steam, &player) { reply("[RED ZONE] Admin only.", &player).await; return Outcome::Handled; }
                        if parts.len() < 5 { reply("[RED ZONE] Usage: !addpvp <name> <x> <y> <radius>", &player).await; return Outcome::Handled; }
                        let name = parts[1].to_string();
                        let x: Option<f64> = parts[2].parse().ok();
                        let y: Option<f64> = parts[3].parse().ok();
                        let radius: f64 = parts[4].parse().unwrap_or(10000.0);
                        let (Some(x), Some(y)) = (x, y) else { reply("[RED ZONE] Invalid coords.", &player).await; return Outcome::Handled; };
                        {
                            let mut state = self.state.lock().await;
                            state.zones.push(PvpZone { name: name.clone(), x, y, radius });
                            save(&state);
                        }
                        broadcast(&format!("[RED ZONE] A new RED ZONE '{}' is live -- no rules, bonus kills. Enter if you dare.", name)).await;
                        reply(&format!("[RED ZONE] '{}' created at ({}, {}) r={}", name, x, y, radius), &player).await;
                        Outcome::Handled
                    }
                    "!delpvp" => {
                        if !is_owner(&steam, &player) { reply("[RED ZONE] Admin only.", &player).await; return Outcome::Handled; }
                        if parts.len() < 2 { reply("[RED ZONE] Usage: !delpvp <name>", &player).await; return Outcome::Handled; }
                        let name = parts[1].to_lowercase();
                        let removed = {
                            let mut state = self.state.lock().await;
                            let before = state.zones.len();
                            state.zones.retain(|z| z.name.to_lowercase() != name);
                            let removed = state.zones.len() < before;
                            if removed { save(&state); }
                            removed
                        };
                        if removed { reply(&format!("[RED ZONE] '{}' removed.", name), &player).await; }
                        else { reply(&format!("[RED ZONE] '{}' not found.", name), &player).await; }
                        Outcome::Handled
                    }
                    "!pvpzones" => {
                        let zones = { self.state.lock().await.zones.clone() };
                        if zones.is_empty() {
                            reply("[RED ZONE] No red zones yet. Admin: !addpvp <name> <x> <y> <radius>", &player).await;
                        } else {
                            for z in &zones {
                                red_line(&format!("[RED ZONE] {} -- ({:.0}, {:.0}) r={:.0} -- NO RULES, +{} per kill", z.name, z.x, z.y, z.radius, KILL_BONUS), &player).await;
                            }
                        }
                        Outcome::Handled
                    }
                    _ => Outcome::Ignored,
                }
            }

            _ => Outcome::Ignored,
        }
    }
}
