// MechMan - player possesses a Sentry2 mech suit. !mech/!transform/!fire/!eject/!detransform/!mechstatus.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const OWNER_STEAM_ID: &str = "YOUR_STEAM_ID_1";
const OWNER_NAME: &str = "YOUR_OWNER_NAME";
const RATE_LIMIT: Duration = Duration::from_secs(2);

fn is_owner(steam: &str, player: &str) -> bool {
    steam == OWNER_STEAM_ID || steam == "YOUR_STEAM_ID_2" || player == OWNER_NAME
}

#[derive(Clone)]
struct MechState {
    mech_ptr: String,
    original_vehicle_class: Option<String>,
    activated: Instant,
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

async fn broadcast_msg(msg: &str) {
    let params = serde_json::json!({ "text": msg });
    pipe_rpc::call("broadcastChat", Some(params)).await.ok();
}

pub struct MechMan { rate: Mutex<HashMap<String, Instant>>, pilots: Mutex<HashMap<String, MechState>> }
impl MechMan {
    pub fn new() -> Self { Self { rate: Mutex::new(HashMap::new()), pilots: Mutex::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl Mod for MechMan {
    fn name(&self) -> &'static str { "mechman" }
    fn commands(&self) -> &'static [&'static str] { &["!mech", "!transform", "!fire", "!eject", "!detransform", "!mechstatus"] }

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
            if let Some(prev) = rate.get(&rate_key) { if now.duration_since(*prev) < RATE_LIMIT { return Outcome::Ignored; } }
            rate.insert(rate_key.clone(), now);
        }
        let (cmd, args) = match text.find(' ') {
            Some(i) => (text[..i].to_lowercase(), text[i + 1..].trim().to_string()),
            None => (text.to_lowercase(), String::new()),
        };

        match cmd.as_str() {
            "!mech" => {
                if !is_owner(&steam, &player) { reply("[MechMan] Admin only during testing.", &player).await; return Outcome::Handled; }
                if self.pilots.lock().await.contains_key(&steam) { reply("[MechMan] Already in a mech! !eject first.", &player).await; return Outcome::Handled; }
                match pipe_rpc::call("spawnMech", Some(serde_json::json!({ "playerName": player }))).await {
                    Ok(resp) => {
                        let mech_ptr = resp.get("mechPtr").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        if mech_ptr.is_empty() { reply("[MechMan] No sentry available in the world.", &player).await; return Outcome::Handled; }
                        match pipe_rpc::call("possessActor", Some(serde_json::json!({ "playerName": player, "targetClass": "Sentry2" }))).await {
                            Ok(_) => {
                                pipe_rpc::call("mechFireWeapon", Some(serde_json::json!({ "mechPtr": mech_ptr, "weapon": "activate" }))).await.ok();
                                self.pilots.lock().await.insert(steam.clone(), MechState { mech_ptr: mech_ptr.clone(), original_vehicle_class: None, activated: Instant::now() });
                                broadcast_msg(&format!("[MechMan] {} entered a MECH SUIT!", player)).await;
                                reply("[MechMan] Controls: !fire <weapon> | !eject to exit", &player).await;
                            }
                            Err(e) => reply(&format!("[MechMan] Possess failed: {}", e), &player).await,
                        }
                    }
                    Err(e) => reply(&format!("[MechMan] Spawn failed: {}", e), &player).await,
                }
                Outcome::Handled
            }
            "!transform" => {
                if !is_owner(&steam, &player) { reply("[MechMan] Admin only.", &player).await; return Outcome::Handled; }
                if self.pilots.lock().await.contains_key(&steam) { reply("[MechMan] Already in a mech!", &player).await; return Outcome::Handled; }
                reply("[MechMan] TRANSFORMING! Vehicle deconstructing...", &player).await;
                let vehicle_class = match pipe_rpc::call("getNearbyActors", Some(serde_json::json!({ "playerName": player, "classFilter": "VehicleBase", "radius": 3000 }))).await {
                    Ok(resp) => resp.get("actors").and_then(|a| a.as_array()).and_then(|arr| arr.first()).and_then(|v| v.get("class")).and_then(|c| c.as_str()).map(String::from),
                    Err(_) => None,
                };
                if vehicle_class.is_some() { broadcast_msg(&format!("[MechMan] {} vehicle DECONSTRUCTING... parts reassembling!", player)).await; }
                tokio::time::sleep(Duration::from_secs(2)).await;
                match pipe_rpc::call("spawnMech", Some(serde_json::json!({ "playerName": player }))).await {
                    Ok(resp) => {
                        let mech_ptr = resp.get("mechPtr").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        if mech_ptr.is_empty() { reply("[MechMan] No sentry available for transformation.", &player).await; return Outcome::Handled; }
                        if pipe_rpc::call("possessActor", Some(serde_json::json!({ "playerName": player, "targetClass": "Sentry2" }))).await.is_ok() {
                            pipe_rpc::call("mechFireWeapon", Some(serde_json::json!({ "mechPtr": mech_ptr, "weapon": "activate" }))).await.ok();
                            self.pilots.lock().await.insert(steam.clone(), MechState { mech_ptr, original_vehicle_class: vehicle_class, activated: Instant::now() });
                            broadcast_msg(&format!("[MechMan] TRANSFORMATION COMPLETE! {} is now a MECH!", player)).await;
                            reply("[MechMan] Weapons hot! !fire <weapon> to attack. !detransform to revert.", &player).await;
                        }
                    }
                    Err(e) => reply(&format!("[MechMan] Transform failed: {}", e), &player).await,
                }
                Outcome::Handled
            }
            "!fire" => {
                let mech_ptr = { self.pilots.lock().await.get(&steam).map(|m| m.mech_ptr.clone()) };
                let Some(mech_ptr) = mech_ptr else { reply("[MechMan] Not in a mech. !mech to enter one.", &player).await; return Outcome::Handled; };
                let weapon = if args.is_empty() { "longrange".to_string() } else { args.to_lowercase() };
                match pipe_rpc::call("mechFireWeapon", Some(serde_json::json!({ "mechPtr": mech_ptr, "weapon": weapon }))).await {
                    Ok(_) => {
                        let sound = match weapon.as_str() {
                            "longrange" => "BRRRRT!", "medium" => "RATATATAT!", "grenade" => "THUNK... BOOM!",
                            "melee" => "SMASH!", "highprecision" => "CRACK!", "teargas" => "PSSSHHH!",
                            "stun" => "FLASH!", "highspread" => "DAKKA DAKKA DAKKA!", _ => "FIRE!",
                        };
                        broadcast_msg(&format!("[MechMan] {} fires {} - {}", player, weapon, sound)).await;
                    }
                    Err(e) => reply(&format!("[MechMan] Fire failed: {}", e), &player).await,
                }
                Outcome::Handled
            }
            "!eject" | "!detransform" => {
                let mech = self.pilots.lock().await.remove(&steam);
                let Some(mech) = mech else { reply("[MechMan] Not in a mech.", &player).await; return Outcome::Handled; };
                let mech_ptr = mech.mech_ptr.clone();
                let uptime = mech.activated.elapsed().as_secs();
                let original_vehicle_class = mech.original_vehicle_class.clone();
                let is_detransform = cmd == "!detransform";
                pipe_rpc::call("mechFireWeapon", Some(serde_json::json!({ "mechPtr": mech_ptr, "weapon": "deactivate" }))).await.ok();
                pipe_rpc::call("unpossessActor", Some(serde_json::json!({ "playerName": player }))).await.ok();
                if is_detransform {
                    if let Some(class) = original_vehicle_class {
                        pipe_rpc::call("spawnVehicle", Some(serde_json::json!({ "className": class, "playerName": player }))).await.ok();
                        broadcast_msg(&format!("[MechMan] {} detransformed! Vehicle reassembled. ({}s in mech)", player, uptime)).await;
                    } else {
                        broadcast_msg(&format!("[MechMan] {} ejected from mech! ({}s piloted)", player, uptime)).await;
                    }
                } else {
                    broadcast_msg(&format!("[MechMan] {} ejected from mech! ({}s piloted)", player, uptime)).await;
                }
                Outcome::Handled
            }
            "!mechstatus" => {
                let status_line = {
                    let pilots = self.pilots.lock().await;
                    pilots.get(&steam).map(|mech| format!("[MechMan] Mech active for {}s | Vehicle source: {} | Ptr: {}", mech.activated.elapsed().as_secs(), mech.original_vehicle_class.as_deref().unwrap_or("none"), mech.mech_ptr))
                };
                match status_line { Some(line) => reply(&line, &player).await, None => reply("[MechMan] Not in a mech.", &player).await }
                Outcome::Handled
            }
            _ => Outcome::Ignored,
        }
    }
}
