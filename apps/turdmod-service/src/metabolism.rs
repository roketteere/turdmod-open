// Metabolism control - admin commands for player health/hunger/thirst.
// !heal !feed !cure - uses writePlayerProperty bridge handler.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const RATE_LIMIT: Duration = Duration::from_secs(3);

fn is_owner(steam: &str, player: &str) -> bool {
    crate::owner::is_owner(steam, player)
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

async fn write_prop(player: &str, component: &str, prop: &str, val: &str, kind: &str) -> bool {
    let params = serde_json::json!({
        "playerName": player,
        "component": component,
        "propertyName": prop,
        "value": val,
        "valueKind": kind
    });
    pipe_rpc::call("writePlayerProperty", Some(params)).await.is_ok()
}

pub struct Metabolism { rate: Mutex<HashMap<String, Instant>> }
impl Metabolism {
    pub fn new() -> Self { Self { rate: Mutex::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl Mod for Metabolism {
    fn name(&self) -> &'static str { "metabolism" }
    fn commands(&self) -> &'static [&'static str] { &["!heal", "!feed", "!cure", "!xp", "!money"] }

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

        if !matches!(cmd.as_str(), "!heal" | "!feed" | "!cure" | "!xp" | "!money") {
            return Outcome::Ignored;
        }

        let target = if args.is_empty() { player.clone() } else { args.clone() };

        match cmd.as_str() {
            "!heal" => {
                if !is_owner(&steam, &player) { reply("[Med] Admin only.", &player).await; return Outcome::Handled; }
                // Set health-related props - god mode is the reliable path
                let p1 = serde_json::json!({ "playerName": target, "enable": true });
                let p2 = serde_json::json!({ "playerName": target, "enable": true });
                pipe_rpc::call("setGodMode", Some(p1)).await.ok();
                pipe_rpc::call("setImmortal", Some(p2)).await.ok();
                // Brief invulnerability then remove (simulates "full heal")
                let t = target.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    let p1 = serde_json::json!({ "playerName": t, "enable": false });
                    let p2 = serde_json::json!({ "playerName": t, "enable": false });
                    pipe_rpc::call("setGodMode", Some(p1)).await.ok();
                    pipe_rpc::call("setImmortal", Some(p2)).await.ok();
                });
                reply(&format!("[Med] {} healed.", target), &player).await;
                Outcome::Handled
            }

            "!feed" => {
                if !is_owner(&steam, &player) { reply("[Med] Admin only.", &player).await; return Outcome::Handled; }
                // Give food items via placeItemInInventory
                let items = [
                    "BP_Item_MRE_C",
                    "BP_Item_Water_Bottle_C",
                    "BP_Item_EnergyDrink_C",
                ];
                for item in &items {
                    let params = serde_json::json!({ "playerName": target, "className": item });
                    pipe_rpc::call("placeItemInInventory", Some(params)).await.ok();
                }
                reply(&format!("[Med] {} fed (MRE + water + energy drink).", target), &player).await;
                Outcome::Handled
            }

            "!cure" => {
                if !is_owner(&steam, &player) { reply("[Med] Admin only.", &player).await; return Outcome::Handled; }
                // Give medical items
                let items = [
                    "BP_Item_Antibiotics_C",
                    "BP_Item_Painkillers_C",
                    "BP_Item_Bandage_Military_C",
                    "BP_Item_Vitamins_C",
                ];
                for item in &items {
                    let params = serde_json::json!({ "playerName": target, "className": item });
                    pipe_rpc::call("placeItemInInventory", Some(params)).await.ok();
                }
                reply(&format!("[Med] {} received medical kit.", target), &player).await;
                Outcome::Handled
            }

            "!xp" => {
                if !is_owner(&steam, &player) { reply("[XP] Admin only.", &player).await; return Outcome::Handled; }
                let parts: Vec<&str> = args.split_whitespace().collect();
                if parts.len() < 2 {
                    reply("[XP] Usage: !xp <player> <amount>", &player).await;
                    return Outcome::Handled;
                }
                let xp_target = parts[0].to_string();
                let amount = parts.get(1).and_then(|s| s.parse::<f64>().ok()).unwrap_or(100.0);
                let params = serde_json::json!({ "playerName": xp_target, "points": amount });
                match pipe_rpc::call("setFamePoints", Some(params)).await {
                    Ok(_) => reply(&format!("[XP] {} received {} fame points.", xp_target, amount), &player).await,
                    Err(e) => reply(&format!("[XP] Failed: {}", e), &player).await,
                }
                Outcome::Handled
            }

            "!money" => {
                if !is_owner(&steam, &player) { reply("[Eco] Admin only.", &player).await; return Outcome::Handled; }
                let parts: Vec<&str> = args.split_whitespace().collect();
                if parts.len() < 2 {
                    reply("[Eco] Usage: !money <player> <amount>", &player).await;
                    return Outcome::Handled;
                }
                let eco_target = parts[0].to_string();
                let amount = parts.get(1).and_then(|s| s.parse::<f64>().ok()).unwrap_or(1000.0);
                let params = serde_json::json!({ "playerName": eco_target, "balance": amount });
                match pipe_rpc::call("setCurrencyBalance", Some(params)).await {
                    Ok(_) => reply(&format!("[Eco] {} received {} coins (in-game).", eco_target, amount), &player).await,
                    Err(e) => reply(&format!("[Eco] Failed: {}", e), &player).await,
                }
                Outcome::Handled
            }

            _ => Outcome::Ignored,
        }
    }
}
