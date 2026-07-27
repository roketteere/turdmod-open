// Safe zones - designated areas where PvP damage is disabled.
// Admin defines zones by center coords + radius.
// Monitors player positions (ctx.map, 5s tick) and applies god mode inside zones.

use std::collections::HashSet;
use std::time::{Duration, Instant};
use std::collections::HashMap;
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const STATE_PATH: &str = r"C:\TurdMOD\data\safe_zones.json";
const CHECK_INTERVAL: Duration = Duration::from_secs(5);
const RATE_LIMIT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SafeZone {
    name: String,
    x: f64,
    y: f64,
    z: f64,
    radius: f64,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
struct ZoneState {
    zones: Vec<SafeZone>,
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
        if std::fs::write(&tmp, &json).is_ok() {
            let _ = std::fs::rename(&tmp, STATE_PATH);
        }
    }
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

fn is_owner(steam: &str, player: &str) -> bool {
    crate::owner::is_owner(steam, player)
}

fn in_zone(px: f64, py: f64, zone: &SafeZone) -> bool {
    let dx = px - zone.x;
    let dy = py - zone.y;
    (dx * dx + dy * dy).sqrt() <= zone.radius
}

async fn set_god(player: &str, enable: bool) {
    let p1 = serde_json::json!({ "playerName": player, "enable": enable });
    let p2 = serde_json::json!({ "playerName": player, "enable": enable });
    pipe_rpc::call("setGodMode", Some(p1)).await.ok();
    pipe_rpc::call("setImmortal", Some(p2)).await.ok();
}

// Admins own their god flag via god_mode (!god) — the safe-zone sweep must NEVER
// touch their flags, or it would strip an intentional !god. Steam IDs confirmed
// by Joel (his own + Zilla).

pub struct SafeZones {
    state: Mutex<ZoneState>,
    rate: Mutex<HashMap<String, Instant>>,
    protected: Mutex<HashSet<String>>, // players currently god-moded by safe zone
    cleared: Mutex<HashSet<String>>,   // non-admins force-cleared once (stuck-flag guard)
}
impl SafeZones {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(load()),
            rate: Mutex::new(HashMap::new()),
            protected: Mutex::new(HashSet::new()),
            cleared: Mutex::new(HashSet::new()),
        }
    }
}

#[async_trait::async_trait]
impl Mod for SafeZones {
    fn name(&self) -> &'static str { "safe_zones" }
    fn commands(&self) -> &'static [&'static str] { &["!addzone", "!delzone", "!zones"] }
    fn interval(&self) -> Option<Duration> { Some(CHECK_INTERVAL) }

    // Apply/remove safe-zone god mode (the old select! interval branch).
    async fn tick(&self, ctx: &ModCtx) {
        let zones = {
            let s = self.state.lock().await;
            if s.zones.is_empty() { return; }
            s.zones.clone()
        };
        let snapshot = ctx.map.read().await.clone();
        let mut protected = self.protected.lock().await;
        let mut cleared = self.cleared.lock().await;
        for p in &snapshot.players {
            // Admins own their god via !god (god_mode) — never touch their flags here.
            if crate::owner::is_owner_steam(&p.steam_id) { continue; }
            let in_safe = zones.iter().any(|z| in_zone(p.x, p.y, z));
            if in_safe {
                cleared.remove(&p.name);
                if !protected.contains(&p.name) {
                    set_god(&p.name, true).await;
                    protected.insert(p.name.clone());
                    reply("[Safe Zone] You are in a safe zone. PvP disabled.", &p.name).await;
                }
            } else if protected.contains(&p.name) {
                set_god(&p.name, false).await;
                protected.remove(&p.name);
                cleared.insert(p.name.clone());
                reply("[Safe Zone] You left the safe zone. PvP enabled.", &p.name).await;
            } else if !cleared.contains(&p.name) {
                // @brk: stuck-flag guard. A non-admin out of every zone whom we have
                // NOT cleared (immortal stuck after a disconnect-in-zone, or a service
                // restart wiped `protected`) gets god forced OFF exactly once. This is
                // what fixes the "everyone stuck immortal in bunkers" bug, and clears
                // returning offline-stuck players on their next login.
                set_god(&p.name, false).await;
                cleared.insert(p.name.clone());
            }
        }
        // Disconnected players: best-effort clear (if still loaded) + drop tracking.
        let online: HashSet<String> = snapshot.players.iter().map(|p| p.name.clone()).collect();
        let stale: Vec<String> = protected.iter().filter(|n| !online.contains(*n)).cloned().collect();
        for name in stale {
            set_god(&name, false).await; // @brk: was missing — left disconnected players immortal
            protected.remove(&name);
        }
        cleared.retain(|n| online.contains(n)); // forget offline players → re-clear on their return
    }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        if !text.starts_with('!') { return Outcome::Ignored; }

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
        let cmd = parts[0].to_lowercase();

        let replies: Vec<String> = {
            let mut state = self.state.lock().await;
            match cmd.as_str() {
                "!addzone" => {
                    if !is_owner(&steam, &player) { vec!["[Zone] Admin only.".to_string()] }
                    else if parts.len() < 5 {
                        vec!["[Zone] Usage: !addzone <name> <x> <y> <radius>".to_string()]
                    } else {
                        let name = parts[1].to_string();
                        let coords: Option<(f64, f64, f64)> = (|| Some((parts[2].parse().ok()?, parts[3].parse().ok()?, 0.0)))();
                        let radius: f64 = parts.get(4).and_then(|s| s.parse().ok()).unwrap_or(5000.0);
                        match coords {
                            Some((x, y, z)) => {
                                state.zones.push(SafeZone { name: name.clone(), x, y, z, radius });
                                save(&state);
                                vec![format!("[Zone] '{}' created at ({}, {}) r={}", name, x, y, radius)]
                            }
                            None => vec!["[Zone] Invalid coordinates".to_string()],
                        }
                    }
                }
                "!delzone" => {
                    if !is_owner(&steam, &player) { vec!["[Zone] Admin only.".to_string()] }
                    else if parts.len() < 2 { vec!["[Zone] Usage: !delzone <name>".to_string()] }
                    else {
                        let name = parts[1].to_lowercase();
                        let before = state.zones.len();
                        state.zones.retain(|z| z.name.to_lowercase() != name);
                        if state.zones.len() < before {
                            save(&state);
                            vec![format!("[Zone] '{}' deleted", name)]
                        } else {
                            vec![format!("[Zone] '{}' not found", name)]
                        }
                    }
                }
                "!zones" => {
                    if state.zones.is_empty() {
                        vec!["[Zone] No safe zones defined.".to_string()]
                    } else {
                        state.zones.iter().map(|z| format!("[Zone] {} - ({}, {}) r={}", z.name, z.x, z.y, z.radius)).collect()
                    }
                }
                _ => return Outcome::Ignored,
            }
        };
        for r in replies { reply(&r, &player).await; }
        Outcome::Handled
    }
}
