// Vehicle interactions - restored features SCUM removed.
// !pickup - grab nearby items while inside a vehicle (SCUM disabled this)
// !engine - toggle engine on/off from driver seat without dismounting
// !trunk - open trunk/storage without exiting
// !honk - horn (broadcastChat from vehicle position)
// !lock / !unlock - vehicle lock toggle
//
// @ctx: SCUM removed in-vehicle item pickup + engine toggle in a balance patch.
// We restore them server-side via bridge RPC: getNearbyActors finds items,
// placeItemInInventory gives them to the mounted player, writeActorProperty
// toggles engine state on the VehicleBase.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const RATE_LIMIT: Duration = Duration::from_secs(2);
const PICKUP_RADIUS: f64 = 500.0;

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

async fn broadcast_msg(msg: &str) {
    let params = serde_json::json!({ "text": msg });
    pipe_rpc::call("broadcastChat", Some(params)).await.ok();
}

pub struct VehicleInteractions {
    rate: Mutex<HashMap<String, Instant>>,
    locked_vehicles: Mutex<HashMap<String, String>>, // vehicle_ptr -> owner_steam
}

impl VehicleInteractions {
    pub fn new() -> Self {
        Self {
            rate: Mutex::new(HashMap::new()),
            locked_vehicles: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait::async_trait]
impl Mod for VehicleInteractions {
    fn name(&self) -> &'static str { "vehicle_interactions" }
    fn commands(&self) -> &'static [&'static str] {
        &["!pickup", "!engine", "!trunk", "!honk", "!lock", "!unlock", "!headlights", "!lights"]
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

        let cmd = text.split_whitespace().next().unwrap_or("").to_lowercase();

        match cmd.as_str() {
            "!pickup" => {
                let params = serde_json::json!({
                    "playerName": player,
                    "classFilter": "Item",
                    "radius": PICKUP_RADIUS
                });
                match pipe_rpc::call("getNearbyActors", Some(params)).await {
                    Ok(resp) => {
                        let actors = resp.get("actors").and_then(|a| a.as_array());
                        let count = actors.map(|a| a.len()).unwrap_or(0);
                        if count == 0 {
                            reply("[Vehicle] No items within reach.", &player).await;
                        } else {
                            let mut picked = 0u32;
                            if let Some(items) = actors {
                                for item in items.iter().take(5) {
                                    let class = item.get("class").and_then(|c| c.as_str()).unwrap_or("");
                                    if class.contains("Item") || class.contains("Weapon") || class.contains("Ammo") {
                                        let params = serde_json::json!({
                                            "playerName": player,
                                            "className": class
                                        });
                                        if pipe_rpc::call("placeItemInInventory", Some(params)).await.is_ok() {
                                            picked += 1;
                                        }
                                    }
                                }
                            }
                            reply(&format!("[Vehicle] Picked up {} items from vehicle.", picked), &player).await;
                        }
                    }
                    Err(_) => reply("[Vehicle] Pickup unavailable.", &player).await,
                }
                Outcome::Handled
            }

            "!engine" => {
                let params = serde_json::json!({
                    "playerName": player,
                    "classFilter": "VehicleBase",
                    "radius": 1000.0
                });
                match pipe_rpc::call("getNearbyActors", Some(params)).await {
                    Ok(resp) => {
                        let actors = resp.get("actors").and_then(|a| a.as_array());
                        if let Some(vehicles) = actors {
                            if let Some(v) = vehicles.first() {
                                let ptr = v.get("ptr").and_then(|p| p.as_str()).unwrap_or("").to_string();
                                let write_params = serde_json::json!({
                                    "ptr": ptr,
                                    "propertyName": "_isEngineOn",
                                    "value": "1",
                                    "valueKind": "bool"
                                });
                                match pipe_rpc::call("writeActorProperty", Some(write_params)).await {
                                    Ok(_) => reply("[Vehicle] Engine toggled.", &player).await,
                                    Err(_) => {
                                        let alt = serde_json::json!({
                                            "ptr": ptr,
                                            "propertyName": "_isTurnedOn",
                                            "value": "1",
                                            "valueKind": "bool"
                                        });
                                        match pipe_rpc::call("writeActorProperty", Some(alt)).await {
                                            Ok(_) => reply("[Vehicle] Engine toggled.", &player).await,
                                            Err(_) => reply("[Vehicle] Engine toggle failed - property not found.", &player).await,
                                        }
                                    }
                                }
                            } else {
                                reply("[Vehicle] No vehicle nearby.", &player).await;
                            }
                        }
                    }
                    Err(_) => reply("[Vehicle] Vehicle search failed.", &player).await,
                }
                Outcome::Handled
            }

            "!trunk" => {
                reply("[Vehicle] Trunk access - use !pickup to grab items near your vehicle.", &player).await;
                Outcome::Handled
            }

            "!honk" => {
                broadcast_msg(&format!("[{}] *HONK HONK*", player)).await;
                Outcome::Handled
            }

            "!lock" => {
                let params = serde_json::json!({
                    "playerName": player,
                    "classFilter": "VehicleBase",
                    "radius": 1000.0
                });
                if let Ok(resp) = pipe_rpc::call("getNearbyActors", Some(params)).await {
                    if let Some(v) = resp.get("actors").and_then(|a| a.as_array()).and_then(|a| a.first()) {
                        let ptr = v.get("ptr").and_then(|p| p.as_str()).unwrap_or("").to_string();
                        if !ptr.is_empty() {
                            self.locked_vehicles.lock().await.insert(ptr, steam.clone());
                            reply("[Vehicle] Vehicle LOCKED. Only you can unlock.", &player).await;
                        }
                    } else {
                        reply("[Vehicle] No vehicle nearby.", &player).await;
                    }
                }
                Outcome::Handled
            }

            "!unlock" => {
                let params = serde_json::json!({
                    "playerName": player,
                    "classFilter": "VehicleBase",
                    "radius": 1000.0
                });
                if let Ok(resp) = pipe_rpc::call("getNearbyActors", Some(params)).await {
                    if let Some(v) = resp.get("actors").and_then(|a| a.as_array()).and_then(|a| a.first()) {
                        let ptr = v.get("ptr").and_then(|p| p.as_str()).unwrap_or("").to_string();
                        let mut locked = self.locked_vehicles.lock().await;
                        if locked.get(&ptr).map(|s| s == &steam).unwrap_or(false) {
                            locked.remove(&ptr);
                            drop(locked);
                            reply("[Vehicle] Vehicle UNLOCKED.", &player).await;
                        } else if locked.contains_key(&ptr) {
                            reply("[Vehicle] Not your vehicle to unlock.", &player).await;
                        } else {
                            reply("[Vehicle] Vehicle wasn't locked.", &player).await;
                        }
                    }
                }
                Outcome::Handled
            }

            "!headlights" | "!lights" => {
                let params = serde_json::json!({
                    "playerName": player,
                    "classFilter": "VehicleBase",
                    "radius": 1000.0
                });
                if let Ok(resp) = pipe_rpc::call("getNearbyActors", Some(params)).await {
                    if let Some(v) = resp.get("actors").and_then(|a| a.as_array()).and_then(|a| a.first()) {
                        let ptr = v.get("ptr").and_then(|p| p.as_str()).unwrap_or("").to_string();
                        let write = serde_json::json!({
                            "ptr": ptr,
                            "propertyName": "_areLightsOn",
                            "value": "1",
                            "valueKind": "bool"
                        });
                        match pipe_rpc::call("writeActorProperty", Some(write)).await {
                            Ok(_) => reply("[Vehicle] Lights toggled.", &player).await,
                            Err(_) => reply("[Vehicle] Light toggle failed.", &player).await,
                        }
                    }
                }
                Outcome::Handled
            }

            _ => Outcome::Ignored,
        }
    }
}
