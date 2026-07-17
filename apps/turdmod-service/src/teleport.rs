// Teleport points - !setpoint !tp !points !delpoint (admin-set coords, anyone can use)

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const OWNER_STEAM_ID: &str = "YOUR_STEAM_ID_1";
const STATE_PATH: &str = r"C:\TurdMOD\data\teleports.json";
const RATE_LIMIT: Duration = Duration::from_secs(3);

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
struct TpPoint { x: f64, y: f64, z: f64, set_by: String }

#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
struct TpState { public_points: HashMap<String, TpPoint> }

fn load() -> TpState {
    std::fs::read_to_string(STATE_PATH).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(state: &TpState) {
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

pub struct Teleport {
    state: Mutex<TpState>,
    rate: Mutex<HashMap<String, Instant>>,
}

impl Teleport {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(load()),
            rate: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait::async_trait]
impl Mod for Teleport {
    fn name(&self) -> &'static str { "teleport" }
    // event-driven: no commands() - sees ALL chat and self-filters

    async fn handle(&self, ev: &GameEvent, ctx: &ModCtx) -> Outcome {
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

        // Gate the teleport commands to Admin tier or higher (owner always passes).
        // Teleport is its own event subscriber, so it enforces this itself rather
        // than relying on the chat_cmds permission gate. Non-teleport commands fall
        // through untouched (the match below ignores them).
        if matches!(cmd.as_str(), "!tp" | "!teleport" | "!setpoint" | "!delpoint" | "!points") {
            let tier = crate::permissions::player_tier(&*ctx.perms.read().await, &steam);
            if tier < crate::permissions::Tier::Admin {
                reply("[TP] Admin only.", &player).await;
                return Outcome::Handled;
            }
        }

        match cmd.as_str() {
            "!setpoint" => {
                if steam != OWNER_STEAM_ID && steam != "YOUR_STEAM_ID_2" {
                    reply("[TP] Owner only.", &player).await;
                    return Outcome::Handled;
                }
                if parts.len() < 5 {
                    reply("[TP] Usage: !setpoint <name> <x> <y> <z>", &player).await;
                    return Outcome::Handled;
                }
                let name = parts[1].to_lowercase();
                let coords: Option<(f64, f64, f64)> = (|| {
                    Some((parts[2].parse().ok()?, parts[3].parse().ok()?, parts[4].parse().ok()?))
                })();
                let Some((x, y, z)) = coords else {
                    reply("[TP] Invalid coordinates", &player).await;
                    return Outcome::Handled;
                };
                let msg = {
                    let mut state = self.state.lock().await;
                    state.public_points.insert(name.clone(), TpPoint { x, y, z, set_by: steam.clone() });
                    save(&state);
                    format!("[TP] Point '{}' saved at ({}, {}, {})", name, x, y, z)
                };
                reply(&msg, &player).await;
                Outcome::Handled
            }

            "!tp" => {
                if parts.len() < 2 {
                    reply("[TP] Usage: !tp <a0|b4|c2|z3 | saved point>", &player).await;
                    return Outcome::Handled;
                }
                let name = parts[1].to_lowercase();
                // Built-in trader outposts - always available, no !setpoint needed.
                // Coords are the trader-building centroids from the catalog extraction.
                // @inv: bridge teleportPlayer wants "name" (NOT "playerName") + x/y/z, and
                // K2_TeleportTo returns teleported:false on collision - so retry from higher
                // to drop into clear air. (The old "playerName" param silently no-op'd.)
                // @inv: bridge mis-parses large BARE numbers (x:566442.0 -> 0); send coords
                // as STRINGS, which parse correctly (confirmed live 2026-06-08).
                let trader: Option<(&str, f64, f64, f64, f64)> = match name.as_str() {
                    "a0" | "a1" => Some(("A0 trader (NW)", -621236.562, -556020.812, 2555.764, 157.755325)),
                    "b4"        => Some(("B4 trader (E)", 571289.188, -227421.203, 479.246, 33.527874)),
                    "c2"        => Some(("C2 trader (central)", -152076.781, 288267.750, 69805.820, 356.845673)),
                    "z3"        => Some(("Z3 trader (SE)", 23734.988, -675404.375, 458.242, 173.982361)),
                    _ => None,
                };
                let (label, x, y, gz, yaw) = if let Some(t) = trader {
                    (t.0.to_string(), t.1, t.2, t.3, t.4)
                } else {
                    let state = self.state.lock().await;
                    if let Some(pt) = state.public_points.get(&name) {
                        (format!("'{}'", name), pt.x, pt.y, pt.z, 0.0)
                    } else {
                        reply(&format!("[TP] Unknown '{}'. Traders: a0 b4 c2 z3 | saved: !points", name), &player).await;
                        return Outcome::Handled;
                    }
                };
                let mut landed = false;
                for zoff in [0.0_f64, 800.0, 2000.0] {
                    if let Ok(r) = pipe_rpc::call("teleportPlayer", Some(serde_json::json!({
                        "name": player, "x": x.to_string(), "y": y.to_string(), "z": (gz + zoff).to_string(),
                        "yaw": yaw.to_string()
                    }))).await {
                        if r["teleported"].as_bool().unwrap_or(false) { landed = true; break; }
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                }
                let msg = if landed { format!("[TP] Teleported to {}.", label) }
                          else { format!("[TP] {} blocked by collision - try again.", label) };
                reply(&msg, &player).await;
                Outcome::Handled
            }

            "!points" => {
                let msg = {
                    let state = self.state.lock().await;
                    if state.public_points.is_empty() {
                        "[TP] No points set yet.".to_string()
                    } else {
                        let mut names: Vec<&str> = state.public_points.keys().map(|s| s.as_str()).collect();
                        names.sort();
                        format!("[TP] Points: {}", names.join(", "))
                    }
                };
                reply(&msg, &player).await;
                Outcome::Handled
            }

            "!delpoint" => {
                if steam != OWNER_STEAM_ID && steam != "YOUR_STEAM_ID_2" {
                    reply("[TP] Owner only.", &player).await;
                    return Outcome::Handled;
                }
                if parts.len() < 2 {
                    reply("[TP] Usage: !delpoint <name>", &player).await;
                    return Outcome::Handled;
                }
                let name = parts[1].to_lowercase();
                let msg = {
                    let mut state = self.state.lock().await;
                    if state.public_points.remove(&name).is_some() {
                        save(&state);
                        format!("[TP] Point '{}' deleted", name)
                    } else {
                        format!("[TP] No point named '{}'", name)
                    }
                };
                reply(&msg, &player).await;
                Outcome::Handled
            }

            _ => Outcome::Ignored,
        }
    }
}
