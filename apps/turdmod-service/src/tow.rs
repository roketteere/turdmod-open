// Vehicle TOWING - !tow !untow. Heading-aware follow-tow: while active, the towed
// vehicle is locked FOLLOW_DIST behind the tower vehicle along its heading every
// tick(), facing the same yaw - it tracks like a trailer.
//
// @ctx: bridge `towStep {towerPtr,towedPtr,dist,offsetZ}` reads the tower's location
//   + forward vector and K2_TeleportTo's the towed vehicle to the point behind it.
//   tower_ptr + towed_ptr are cached in TowState (getNearbyActors hex ptrs) so tick()
//   makes exactly ONE bridge call per active tow.
// @brk: reposition_towed() is the ONLY mover. @dep: bridge handle_tow_step.
// @dep: bridge handle_get_nearby_actors (classFilter substring, returns ptr+x/y/z).

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const RATE_LIMIT: Duration = Duration::from_secs(3);
const TICK_MS: u64 = 200;
const FOLLOW_DIST: f32 = 600.0;     // uu (cm) the towed vehicle trails the driver
const FOLLOW_OFFSET_Z: f32 = 50.0;  // keep it off the ground when re-dropped
const MAX_TOW_RANGE: f32 = 1500.0;  // attach scan radius around the tower

// Motorcycles: towable by ANY vehicle.
const MOTORCYCLES: &[&str] = &[
    "BPC_SidecarBike_C", "BPC_Dirtbike_C", "BPC_MountainBike_C", "BPC_CityBike_C",
];
// Cars: towable ONLY by a Cruiser or Rager.
const CARS: &[&str] = &[
    "BPC_Rager_C", "BPC_WolfsWagen_C", "BPC_Cruiser_C", "BPC_Laika_C", "BPC_RIS_C",
    "BPC_Kinglet_Duster_C", "BPC_Kinglet_SUV_C", "BPC_Kinglet_Pickup_C",
    "BPC_Tractor_C", "BPC_Quad_C",
];
// Boats/planes: NOT towable in v1.
const NONTOWABLE: &[&str] = &[
    "BPC_Dinghy_C", "BPC_SUP_C", "BPC_Barba_C", "BPC_Kinglet_Mariner_C",
];
// Only a Cruiser/Rager may tow a CAR.
const CAR_CAPABLE_TOWERS: &[&str] = &["BPC_Cruiser_C", "BPC_Rager_C"];

// Every class we scan for when looking for vehicles around the tower. @inv: union of all above.
const ALL_VEHICLE_CLASSES: &[&str] = &[
    "BPC_Rager_C", "BPC_WolfsWagen_C", "BPC_Cruiser_C", "BPC_Laika_C", "BPC_RIS_C",
    "BPC_Kinglet_Duster_C", "BPC_Kinglet_SUV_C", "BPC_Kinglet_Pickup_C",
    "BPC_Tractor_C", "BPC_Quad_C",
    "BPC_SidecarBike_C", "BPC_Dirtbike_C", "BPC_MountainBike_C", "BPC_CityBike_C",
    "BPC_Dinghy_C", "BPC_SUP_C", "BPC_Barba_C", "BPC_Kinglet_Mariner_C",
];

#[derive(Clone)]
struct Vehicle {
    class: String,
    ptr: String,
    dist: f32, // distance from the tower-player
}

#[derive(Clone)]
struct TowState {
    tower_ptr: String,
    towed_ptr: String,
    follow_dist: f32,
}

fn short(class: &str) -> String { class.replace("BPC_", "").replace("_C", "") }
fn is_motorcycle(c: &str) -> bool { MOTORCYCLES.contains(&c) }
fn is_car(c: &str) -> bool { CARS.contains(&c) }
fn is_nontowable(c: &str) -> bool { NONTOWABLE.contains(&c) }
fn can_tow_cars(c: &str) -> bool { CAR_CAPABLE_TOWERS.contains(&c) }

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

// Scan all vehicle classes near a player; return each match with class+ptr+dist.
// One getNearbyActors call per class (bridge filters by substring, so we strip _C).
async fn scan_vehicles(player: &str, radius: f32) -> Vec<Vehicle> {
    let mut out: Vec<Vehicle> = Vec::new();
    for class in ALL_VEHICLE_CLASSES {
        let filter = class.replace("_C", "");
        let params = serde_json::json!({
            "playerName": player, "classFilter": filter, "radius": radius,
        });
        let Ok(resp) = pipe_rpc::call("getNearbyActors", Some(params)).await else { continue; };
        let Some(actors) = resp.get("actors").and_then(|a| a.as_array()) else { continue; };
        for a in actors {
            // The substring filter can over-match (e.g. "Kinglet"); pin to the exact class.
            let cls = a.get("class").and_then(|c| c.as_str()).unwrap_or("");
            if cls != *class { continue; }
            let ptr = a.get("ptr").and_then(|p| p.as_str()).unwrap_or("").to_string();
            if ptr.is_empty() { continue; }
            out.push(Vehicle {
                class: cls.to_string(),
                ptr,
                dist: a.get("distance").and_then(|v| v.as_f64()).unwrap_or(f64::MAX) as f32,
            });
        }
    }
    out
}

// Lock the towed vehicle FOLLOW_DIST behind the tower along its heading (towStep
// reads the tower's loc+forward and teleports the towed vehicle there, same yaw).
async fn reposition_towed(st: &TowState) -> bool {
    let params = serde_json::json!({
        "towerPtr": st.tower_ptr,
        "towedPtr": st.towed_ptr,
        "dist": st.follow_dist,
        "offsetZ": FOLLOW_OFFSET_Z,
    });
    match pipe_rpc::call("towStep", Some(params)).await {
        Ok(v) => v.get("ok").and_then(|o| o.as_bool()).unwrap_or(false),
        Err(_) => false,
    }
}

pub struct Tow {
    active: Mutex<HashMap<String, TowState>>, // key = towing player's name
    rate: Mutex<HashMap<String, Instant>>,
}

impl Tow {
    pub fn new() -> Self {
        Self { active: Mutex::new(HashMap::new()), rate: Mutex::new(HashMap::new()) }
    }
}

#[async_trait::async_trait]
impl Mod for Tow {
    fn name(&self) -> &'static str { "tow" }
    fn commands(&self) -> &'static [&'static str] { &["!tow", "!untow"] }
    fn interval(&self) -> Option<Duration> { Some(Duration::from_millis(TICK_MS)) }

    // @inv: never hold `active` across an await. Snapshot under lock, drop, then move.
    async fn tick(&self, _ctx: &ModCtx) {
        let snapshot: Vec<(String, TowState)> = {
            let active = self.active.lock().await;
            active.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
        };
        for (_driver, st) in snapshot {
            let _ = reposition_towed(&st).await;
        }
    }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        if !text.starts_with('!') { return Outcome::Ignored; }

        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if player.is_empty() { return Outcome::Ignored; }
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
            "!untow" => {
                let had = { self.active.lock().await.remove(&player).is_some() };
                if had {
                    reply("[Tow] Released. The towed vehicle is no longer following you.", &player).await;
                } else {
                    reply("[Tow] You aren't towing anything.", &player).await;
                }
                Outcome::Handled
            }

            "!tow" => {
                let already = { self.active.lock().await.contains_key(&player) };
                if already {
                    reply("[Tow] You're already towing. Use !untow first.", &player).await;
                    return Outcome::Handled;
                }

                // Scan nearby vehicles (bridge await - no lock held).
                let mut vehicles = scan_vehicles(&player, MAX_TOW_RANGE).await;
                if vehicles.is_empty() {
                    reply("[Tow] No vehicles nearby. Sit in the vehicle you want to tow WITH.", &player).await;
                    return Outcome::Handled;
                }
                vehicles.sort_by(|a, b| a.dist.partial_cmp(&b.dist).unwrap_or(std::cmp::Ordering::Equal));

                // Tower = nearest vehicle to the player (the one they're driving).
                let tower = vehicles[0].clone();
                // Towed candidate = the next nearest vehicle.
                let Some(towed) = vehicles.get(1).cloned() else {
                    reply("[Tow] No second vehicle found to tow. Park one near you.", &player).await;
                    return Outcome::Handled;
                };

                if is_nontowable(&towed.class) {
                    reply(&format!("[Tow] A {} (boat/plane) can't be towed in v1.", short(&towed.class)), &player).await;
                    return Outcome::Handled;
                }
                let ok = if is_motorcycle(&towed.class) {
                    true // any tower may tow a motorcycle
                } else if is_car(&towed.class) {
                    can_tow_cars(&tower.class) // only Cruiser/Rager may tow a car
                } else {
                    false
                };
                if !ok {
                    if is_car(&towed.class) {
                        reply(&format!(
                            "[Tow] A {} can only be towed by a Cruiser or Rager (you're in a {}).",
                            short(&towed.class), short(&tower.class)), &player).await;
                    } else {
                        reply(&format!("[Tow] A {} can't be towed by a {}.",
                            short(&towed.class), short(&tower.class)), &player).await;
                    }
                    return Outcome::Handled;
                }

                let st = TowState {
                    tower_ptr: tower.ptr.clone(),
                    towed_ptr: towed.ptr.clone(),
                    follow_dist: FOLLOW_DIST,
                };
                let attached = reposition_towed(&st).await;
                { self.active.lock().await.insert(player.clone(), st); }

                if attached {
                    reply(&format!("[Tow] Now towing a {} behind your {}. Drive on - !untow to release.",
                        short(&towed.class), short(&tower.class)), &player).await;
                } else {
                    reply(&format!("[Tow] Attached a {} (reposition pending). !untow to release.",
                        short(&towed.class)), &player).await;
                }
                Outcome::Handled
            }

            _ => Outcome::Ignored,
        }
    }
}
