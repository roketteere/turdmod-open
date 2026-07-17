// Vehicle registration - !register !garage !spawn !park !unregister !insure
// One active vehicle per player. Max 3 registered.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const STATE_PATH: &str = r"C:\TurdMOD\data\vehicle_registry.json";
const MAX_VEHICLES: usize = 3;
const RATE_LIMIT: Duration = Duration::from_secs(3);

const VTYPES: &[(&str, &str)] = &[
    ("duster",     "BP_Kinglet_Duster_C"),
    ("suv",        "BP_Kinglet_SUV_C"),
    ("pickup",     "BP_Kinglet_Pickup_C"),
    ("tractor",    "BP_Tractor_C"),
    ("quad",       "BP_Quad_C"),
    ("bicycle",    "BP_Bicycle_C"),
    ("boat",       "BP_Boat_C"),
    ("gyrocopter", "BP_Gyrocopter_C"),
];

fn class_for(short: &str) -> Option<&'static str> {
    VTYPES.iter().find(|(k, _)| *k == short).map(|(_, v)| *v)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
struct VEntry { registered: bool, insured: bool }

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
struct PlayerRec {
    name: String,
    vehicles: HashMap<String, VEntry>,
    active_vehicle: Option<String>,
    active_spawn_id: Option<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
struct RegState { players: HashMap<String, PlayerRec> }

fn load() -> RegState {
    std::fs::read_to_string(STATE_PATH).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(state: &RegState) {
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

async fn do_spawn(class: &str, player: &str) -> Option<String> {
    let params = serde_json::json!({ "className": class, "playerName": player });
    pipe_rpc::call("spawnVehicle", Some(params)).await.ok()
        .and_then(|r| r.get("vehicleId").and_then(|v| v.as_str()).map(String::from))
}

async fn do_destroy(vid: &str) {
    let params = serde_json::json!({ "vehicleId": vid });
    pipe_rpc::call("destroyVehicle", Some(params)).await.ok();
}

pub struct VehicleRegistry {
    state: Mutex<RegState>,
    rate: Mutex<HashMap<String, Instant>>,
}

impl VehicleRegistry {
    pub fn new() -> Self {
        let state = load();
        tracing::info!("vehicle_registry: loaded {} players", state.players.len());
        Self { state: Mutex::new(state), rate: Mutex::new(HashMap::new()) }
    }
}

#[async_trait::async_trait]
impl Mod for VehicleRegistry {
    fn name(&self) -> &'static str { "vehicle_registry" }
    fn commands(&self) -> &'static [&'static str] {
        &["!register_legacy", "!garage", "!spawn", "!park", "!unregister", "!insure"]
    }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        if !text.starts_with('!') { return Outcome::Ignored; }

        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
        // steam may be empty - use player name as fallback
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
        let arg = parts.get(1).map(|s| s.to_lowercase()).unwrap_or_default();

        match cmd.as_str() {
            // DISABLED - replaced by vehicle_ownership module's mount-checked !register
            "!register_legacy" => {
                let msg = if arg.is_empty() {
                    "Usage: !register <vehicle>".into()
                } else if class_for(&arg).is_none() {
                    let names: Vec<&str> = VTYPES.iter().map(|(k, _)| *k).collect();
                    format!("Unknown. Types: {}", names.join(", "))
                } else {
                    let mut st = self.state.lock().await;
                    let rec = st.players.entry(steam.clone()).or_insert_with(|| PlayerRec { name: player.clone(), ..Default::default() });
                    rec.name = player.clone();
                    if rec.vehicles.get(&arg).map(|v| v.registered).unwrap_or(false) {
                        format!("You already own a {}", arg)
                    } else if rec.vehicles.values().filter(|v| v.registered).count() >= MAX_VEHICLES {
                        format!("Garage full (max {})", MAX_VEHICLES)
                    } else {
                        rec.vehicles.insert(arg.clone(), VEntry { registered: true, insured: false });
                        save(&st);
                        format!("[Garage] {} registered! Use !spawn {}", arg, arg)
                    }
                };
                reply(&msg, &player).await;
                Outcome::Handled
            }

            "!garage" => {
                let msg = {
                    let st = self.state.lock().await;
                    match st.players.get(&steam) {
                        None => "Garage empty. Use !register <vehicle>".into(),
                        Some(rec) => {
                            let owned: Vec<String> = rec.vehicles.iter()
                                .filter(|(_, v)| v.registered)
                                .map(|(k, v)| {
                                    let mut s = k.clone();
                                    if rec.active_vehicle.as_deref() == Some(k.as_str()) { s += " [OUT]"; }
                                    if v.insured { s += " [INS]"; }
                                    s
                                }).collect();
                            if owned.is_empty() { "Garage empty".into() }
                            else { format!("[Garage] {}/{}: {}", owned.len(), MAX_VEHICLES, owned.join(", ")) }
                        }
                    }
                };
                reply(&msg, &player).await;
                Outcome::Handled
            }

            "!spawn" => {
                if arg.is_empty() {
                    reply("Usage: !spawn <vehicle>", &player).await;
                    return Outcome::Handled;
                }
                let Some(class) = class_for(&arg) else {
                    reply(&format!("Unknown vehicle '{}'", arg), &player).await;
                    return Outcome::Handled;
                };
                // Check ownership + pull old spawn id under lock; drop lock before awaiting.
                let (owned, old_vid) = {
                    let st = self.state.lock().await;
                    let rec = st.players.get(&steam);
                    let owned = rec.map(|r| r.vehicles.get(&arg).map(|v| v.registered).unwrap_or(false)).unwrap_or(false);
                    let old_vid = rec.and_then(|r| r.active_spawn_id.clone());
                    (owned, old_vid)
                };
                if !owned {
                    reply(&format!("You don't own a {}. !register {} first", arg, arg), &player).await;
                    return Outcome::Handled;
                }
                // Destroy old vehicle before spawning new one.
                if let Some(vid) = old_vid {
                    do_destroy(&vid).await;
                }
                let spawn_id = do_spawn(class, &player).await;
                {
                    let mut st = self.state.lock().await;
                    let rec = st.players.entry(steam.clone()).or_insert_with(|| PlayerRec { name: player.clone(), ..Default::default() });
                    rec.active_vehicle = Some(arg.clone());
                    rec.active_spawn_id = spawn_id;
                    save(&st);
                }
                reply(&format!("[Garage] {} spawned!", arg), &player).await;
                Outcome::Handled
            }

            "!park" => {
                let old_vid = {
                    let mut st = self.state.lock().await;
                    let rec = st.players.entry(steam.clone()).or_insert_with(|| PlayerRec { name: player.clone(), ..Default::default() });
                    let vid = rec.active_spawn_id.take();
                    if vid.is_some() {
                        rec.active_vehicle = None;
                        save(&st);
                    }
                    vid
                };
                if let Some(vid) = old_vid {
                    do_destroy(&vid).await;
                    reply("[Garage] Vehicle parked", &player).await;
                } else {
                    reply("No vehicle out", &player).await;
                }
                Outcome::Handled
            }

            "!unregister" => {
                if arg.is_empty() {
                    reply("Usage: !unregister <vehicle>", &player).await;
                    return Outcome::Handled;
                }
                let old_vid = {
                    let mut st = self.state.lock().await;
                    let rec = st.players.entry(steam.clone()).or_insert_with(|| PlayerRec { name: player.clone(), ..Default::default() });
                    if !rec.vehicles.get(&arg).map(|v| v.registered).unwrap_or(false) {
                        drop(st);
                        reply(&format!("You don't own a {}", arg), &player).await;
                        return Outcome::Handled;
                    }
                    let vid = if rec.active_vehicle.as_deref() == Some(arg.as_str()) {
                        let v = rec.active_spawn_id.take();
                        rec.active_vehicle = None;
                        v
                    } else { None };
                    rec.vehicles.remove(&arg);
                    save(&st);
                    vid
                };
                if let Some(vid) = old_vid {
                    do_destroy(&vid).await;
                }
                reply(&format!("[Garage] {} removed", arg), &player).await;
                Outcome::Handled
            }

            "!insure" => {
                if arg.is_empty() {
                    reply("Usage: !insure <vehicle>", &player).await;
                    return Outcome::Handled;
                }
                let msg = {
                    let mut st = self.state.lock().await;
                    let rec = st.players.entry(steam.clone()).or_insert_with(|| PlayerRec { name: player.clone(), ..Default::default() });
                    if let Some(entry) = rec.vehicles.get_mut(&arg) {
                        if !entry.registered { format!("You don't own a {}", arg) }
                        else if entry.insured { format!("{} already insured", arg) }
                        else { entry.insured = true; save(&st); format!("[Garage] {} insured!", arg) }
                    } else {
                        format!("You don't own a {}", arg)
                    }
                };
                reply(&msg, &player).await;
                Outcome::Handled
            }

            _ => Outcome::Ignored,
        }
    }
}
