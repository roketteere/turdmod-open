// Vehicle ownership - register, transfer, track vehicles by sitting in them.
// !register - claim vehicle you're sitting in (mount check required)
// !myride - show your registered vehicle info
// !transfer <player> - initiate ownership transfer (requires confirmation)
// !yes / !no - confirm or deny pending transfer
// !unregister - release ownership

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const STATE_PATH: &str = r"C:\TurdMOD\data\vehicle_ownership.json";
const RATE_LIMIT: Duration = Duration::from_secs(3);
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct OwnedVehicle {
    vehicle_entity_id: u64,
    vehicle_class: String,
    mount_slot_ptr: String,
    owner_steam: String,
    owner_name: String,
    registered_at: u64,
}

#[derive(Clone)]
struct PendingTransfer {
    vehicle_idx: usize,
    from_steam: String,
    from_name: String,
    to_name: String,
    created: Instant,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
struct OwnershipState {
    vehicles: Vec<OwnedVehicle>,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_secs()
}

fn load() -> OwnershipState {
    std::fs::read_to_string(STATE_PATH).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(state: &OwnershipState) {
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let tmp = format!("{}.tmp", STATE_PATH);
        if std::fs::write(&tmp, &json).is_ok() {
            let _ = std::fs::rename(&tmp, STATE_PATH);
        }
    }
}

async fn say(player: &str, lines: &[&str]) {
    for line in lines {
        let params = serde_json::json!({ "message": *line, "playerName": player, "channel": "4" });
        pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn check_mounted(player: &str) -> Option<String> {
    let params = serde_json::json!({
        "playerName": player,
        "classFilter": "Prisoner_C",
        "radius": 100
    });
    let resp = pipe_rpc::call("getNearbyActors", Some(params)).await.ok()?;
    let actors = resp.get("actors")?.as_array()?;
    let prisoner = actors.first()?;
    let ptr_str = prisoner.get("ptr")?.as_str()?;
    let ptr_val = u64::from_str_radix(ptr_str.trim_start_matches("0x"), 16).ok()?;
    let addr = format!("0x{:X}", ptr_val + 7464);
    let params = serde_json::json!({ "addr": addr, "size": "8" });
    let resp = pipe_rpc::call("readMemory", Some(params)).await.ok()?;
    let hex = resp.get("bytesHex")?.as_str()?;
    if hex.len() < 16 { return None; }
    let mut bytes = [0u8; 8];
    for i in 0..8 {
        bytes[i] = u8::from_str_radix(&hex[i*2..i*2+2], 16).ok()?;
    }
    let slot_ptr = u64::from_le_bytes(bytes);
    if slot_ptr == 0 { None } else { Some(format!("0x{:X}", slot_ptr)) }
}

async fn find_vehicle_at_player(player: &str) -> Option<(String, String)> {
    for class in &["BPC_Tractor_C", "BPC_WolfsWagen_C", "BPC_Laika_C",
                    "BPC_Rager_C", "BPC_Cruiser_C", "BPC_RIS_C",
                    "BPC_SidecarBike_C", "BPC_Dirtbike_C", "BPC_MountainBike_C",
                    "BPC_CityBike_C", "BPC_Kinglet_Duster_C", "BPC_Kinglet_Mariner_C",
                    "BPC_Dinghy_C", "BPC_SUP_C", "BPC_Barba_C"] {
        let params = serde_json::json!({
            "playerName": player,
            "classFilter": class.replace("_C", ""),
            "radius": 1000
        });
        if let Ok(resp) = pipe_rpc::call("getNearbyActors", Some(params)).await {
            if let Some(count) = resp.get("count").and_then(|c| c.as_u64()) {
                if count > 0 {
                    if let Some(actors) = resp.get("actors").and_then(|a| a.as_array()) {
                        if let Some(actor) = actors.first() {
                            let cls = actor.get("class").and_then(|c| c.as_str()).unwrap_or("").to_string();
                            let ptr = actor.get("ptr").and_then(|p| p.as_str()).unwrap_or("").to_string();
                            return Some((cls, ptr));
                        }
                    }
                }
            }
        }
    }
    None
}

pub struct VehicleOwnership {
    state: Mutex<OwnershipState>,
    rate: Mutex<HashMap<String, Instant>>,
    pending_transfers: Mutex<HashMap<String, PendingTransfer>>, // target_name_lc -> transfer
}

impl VehicleOwnership {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(load()),
            rate: Mutex::new(HashMap::new()),
            pending_transfers: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait::async_trait]
impl Mod for VehicleOwnership {
    fn name(&self) -> &'static str { "vehicle_ownership" }
    fn commands(&self) -> &'static [&'static str] {
        &["!register", "!myride", "!transfer", "!yes", "!no", "!unregister"]
    }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        if !text.starts_with('!') { return Outcome::Ignored; }

        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if player.is_empty() { return Outcome::Ignored; }
        let owner_key = if steam.is_empty() { player.clone() } else { steam.clone() };

        {
            let mut rate = self.rate.lock().await;
            let now = Instant::now();
            if let Some(prev) = rate.get(&owner_key) {
                if now.duration_since(*prev) < RATE_LIMIT { return Outcome::Ignored; }
            }
            rate.insert(owner_key.clone(), now);
        }

        // Expire stale transfers outside the per-command lock.
        {
            let mut pt = self.pending_transfers.lock().await;
            pt.retain(|_, t| t.created.elapsed() < TRANSFER_TIMEOUT);
        }

        let (cmd, args) = match text.find(' ') {
            Some(i) => (text[..i].to_lowercase(), text[i + 1..].trim().to_string()),
            None => (text.to_lowercase(), String::new()),
        };

        match cmd.as_str() {
            "!register" => {
                // check_mounted + find_vehicle_at_player are bridge awaits — do them before locking state.
                let slot = check_mounted(&player).await;
                if slot.is_none() {
                    say(&player, &["[Ownership] You must be seated in a vehicle to register it."]).await;
                    return Outcome::Handled;
                }
                let slot_ptr = slot.unwrap();
                let vehicle = find_vehicle_at_player(&player).await;
                let Some((cls, ptr)) = vehicle else {
                    say(&player, &["[Ownership] No vehicle detected nearby."]).await;
                    return Outcome::Handled;
                };
                let short = cls.replace("BPC_", "").replace("_C", "");
                let msg = {
                    let mut state = self.state.lock().await;
                    if state.vehicles.iter().any(|v| v.owner_steam == owner_key) {
                        "You already have a registered vehicle. !unregister first.".to_string()
                    } else {
                        state.vehicles.push(OwnedVehicle {
                            vehicle_entity_id: 0,
                            vehicle_class: cls,
                            mount_slot_ptr: slot_ptr,
                            owner_steam: owner_key.clone(),
                            owner_name: player.clone(),
                            registered_at: now_secs(),
                        });
                        save(&state);
                        format!("{} registered to you!", short)
                    }
                };
                say(&player, &[&format!("[Ownership] {}", msg)]).await;
                Outcome::Handled
            }

            "!myride" => {
                let msg: Vec<String> = {
                    let state = self.state.lock().await;
                    if let Some(v) = state.vehicles.iter().find(|v| v.owner_steam == owner_key) {
                        let short = v.vehicle_class.replace("BPC_", "").replace("_C", "");
                        vec![
                            format!("[Ownership] Your ride: {}", short),
                            format!("Registered: {} secs ago", now_secs().saturating_sub(v.registered_at)),
                        ]
                    } else {
                        vec!["[Ownership] No registered vehicle.".to_string()]
                    }
                };
                let refs: Vec<&str> = msg.iter().map(|s| s.as_str()).collect();
                say(&player, &refs).await;
                Outcome::Handled
            }

            "!transfer" => {
                if args.is_empty() {
                    say(&player, &["[Ownership] Usage: !transfer <player>"]).await;
                    return Outcome::Handled;
                }
                let to_name = args.clone();
                let vehicle_idx = {
                    let state = self.state.lock().await;
                    state.vehicles.iter().position(|v| v.owner_steam == owner_key)
                };
                let Some(idx) = vehicle_idx else {
                    say(&player, &["[Ownership] You don't own a vehicle."]).await;
                    return Outcome::Handled;
                };
                let short = {
                    let state = self.state.lock().await;
                    state.vehicles[idx].vehicle_class.replace("BPC_", "").replace("_C", "")
                };
                {
                    let mut pt = self.pending_transfers.lock().await;
                    pt.insert(to_name.to_lowercase(), PendingTransfer {
                        vehicle_idx: idx,
                        from_steam: owner_key.clone(),
                        from_name: player.clone(),
                        to_name: to_name.clone(),
                        created: Instant::now(),
                    });
                }
                say(&player, &[&format!("[Ownership] Transfer offer sent to {}. They must !yes within 30s.", to_name)]).await;
                say(&to_name, &[
                    &format!("[Ownership] {} wants to transfer their {} to you.", player, short),
                    "Type !yes to accept or !no to decline.",
                ]).await;
                Outcome::Handled
            }

            "!yes" => {
                let key = player.to_lowercase();
                let transfer = { self.pending_transfers.lock().await.remove(&key) };
                let Some(transfer) = transfer else { return Outcome::Ignored; };

                let result: Result<String, &'static str> = {
                    let mut state = self.state.lock().await;
                    if transfer.vehicle_idx >= state.vehicles.len() {
                        Err("Transfer expired.")
                    } else {
                        let short = state.vehicles[transfer.vehicle_idx].vehicle_class.replace("BPC_", "").replace("_C", "");
                        state.vehicles[transfer.vehicle_idx].owner_steam = owner_key.clone();
                        state.vehicles[transfer.vehicle_idx].owner_name = player.clone();
                        save(&state);
                        Ok(short)
                    }
                };

                match result {
                    Err(msg) => say(&player, &[&format!("[Ownership] {}", msg)]).await,
                    Ok(short) => {
                        say(&player, &[
                            "[Ownership] Transfer ACCEPTED!",
                            &format!("You now own the {}.", short),
                        ]).await;
                        say(&transfer.from_name, &[
                            "[Ownership] Transfer complete.",
                            &format!("{} now owns your {}.", player, short),
                        ]).await;
                    }
                }
                Outcome::Handled
            }

            "!no" => {
                let key = player.to_lowercase();
                let transfer = { self.pending_transfers.lock().await.remove(&key) };
                if let Some(transfer) = transfer {
                    say(&player, &["[Ownership] Transfer declined."]).await;
                    say(&transfer.from_name, &[
                        &format!("[Ownership] {} declined the transfer.", player),
                    ]).await;
                }
                Outcome::Handled
            }

            "!unregister" => {
                let removed = {
                    let mut state = self.state.lock().await;
                    let before = state.vehicles.len();
                    state.vehicles.retain(|v| v.owner_steam != owner_key);
                    let removed = state.vehicles.len() < before;
                    if removed { save(&state); }
                    removed
                };
                if removed {
                    say(&player, &[
                        "[Ownership] Vehicle unregistered.",
                        "It is no longer claimed.",
                    ]).await;
                } else {
                    say(&player, &["[Ownership] No vehicle to unregister."]).await;
                }
                Outcome::Handled
            }

            _ => Outcome::Ignored,
        }
    }
}
