// Vehicle management - !vspawn !vlist !vbring !vdestroy !vehicles

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const RATE_LIMIT: Duration = Duration::from_secs(3);

const VEHICLES: &[(&str, &str)] = &[
    ("duster",     "BP_Kinglet_Duster_C"),
    ("suv",        "BP_Kinglet_SUV_C"),
    ("pickup",     "BP_Kinglet_Pickup_C"),
    ("tractor",    "BP_Tractor_C"),
    ("quad",       "BP_Quad_C"),
    ("bicycle",    "BP_Bicycle_C"),
    ("boat",       "BP_Boat_C"),
    ("gyrocopter", "BP_Gyrocopter_C"),
];

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

pub struct Vehicles {
    rate: Mutex<HashMap<String, Instant>>,
}

impl Vehicles {
    pub fn new() -> Self { Self { rate: Mutex::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl Mod for Vehicles {
    fn name(&self) -> &'static str { "vehicles" }
    fn commands(&self) -> &'static [&'static str] {
        &["!vehicles", "!vspawn", "!vlist", "!vbring", "!vdestroy"]
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

        let (cmd, args) = match text.find(' ') {
            Some(i) => (text[..i].to_lowercase(), text[i + 1..].trim().to_string()),
            None => (text.to_lowercase(), String::new()),
        };

        let is_owner = crate::owner::is_owner_steam(&steam);

        match cmd.as_str() {
            "!vehicles" => {
                let names: Vec<&str> = VEHICLES.iter().map(|(k, _)| *k).collect();
                reply(&format!("[Vehicles] Available: {}", names.join(", ")), &player).await;
                Outcome::Handled
            }

            "!vspawn" => {
                if !is_owner { reply("[Vehicles] Owner only.", &player).await; return Outcome::Handled; }
                if args.is_empty() { reply("[Vehicles] Usage: !vspawn <type>", &player).await; return Outcome::Handled; }
                let lc = args.to_lowercase();
                let Some((_, class)) = VEHICLES.iter().find(|(k, _)| *k == lc.as_str()) else {
                    reply("[Vehicles] Unknown type. Use !vehicles", &player).await;
                    return Outcome::Handled;
                };
                let class = *class;
                let params = serde_json::json!({ "className": class, "playerName": player });
                match pipe_rpc::call("spawnVehicle", Some(params)).await {
                    Ok(_) => reply(&format!("[Vehicles] Spawning {} near you", args), &player).await,
                    Err(e) => { tracing::warn!("vehicles: spawn failed: {}", e); reply("[Vehicles] Spawn failed", &player).await; }
                }
                Outcome::Handled
            }

            "!vlist" => {
                if !is_owner { reply("[Vehicles] Owner only.", &player).await; return Outcome::Handled; }
                match pipe_rpc::call("listSpawnedVehicles", None).await {
                    Ok(resp) => reply(&format!("[Vehicles] {}", resp), &player).await,
                    Err(_) => reply("[Vehicles] List failed", &player).await,
                }
                Outcome::Handled
            }

            "!vbring" => {
                if !is_owner { reply("[Vehicles] Owner only.", &player).await; return Outcome::Handled; }
                if args.is_empty() { reply("[Vehicles] Usage: !vbring <id>", &player).await; return Outcome::Handled; }
                let params = serde_json::json!({ "vehicleId": args, "playerName": player });
                match pipe_rpc::call("bringVehicleToPlayer", Some(params)).await {
                    Ok(_) => reply(&format!("[Vehicles] Vehicle {} coming to you", args), &player).await,
                    Err(_) => reply("[Vehicles] Bring failed", &player).await,
                }
                Outcome::Handled
            }

            "!vdestroy" => {
                if !is_owner { reply("[Vehicles] Owner only.", &player).await; return Outcome::Handled; }
                if args.is_empty() { reply("[Vehicles] Usage: !vdestroy <id>", &player).await; return Outcome::Handled; }
                let params = serde_json::json!({ "vehicleId": args });
                match pipe_rpc::call("destroyVehicle", Some(params)).await {
                    Ok(_) => reply(&format!("[Vehicles] Vehicle {} destroyed", args), &player).await,
                    Err(_) => reply("[Vehicles] Destroy failed", &player).await,
                }
                Outcome::Handled
            }

            _ => Outcome::Ignored,
        }
    }
}
