// Airdrop events - admin triggers supply drops at random/specified locations.
// !airdrop - drop at random player, !airdrop <player> - drop near player.
// Announces location, spawns loot crates via placeItemInInventory.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const OWNER_STEAM_ID: &str = "YOUR_STEAM_ID_1";
const OWNER_NAME: &str = "YOUR_OWNER_NAME";
const RATE_LIMIT: Duration = Duration::from_secs(5);

fn is_owner(steam: &str, player: &str) -> bool {
    steam == OWNER_STEAM_ID || steam == "YOUR_STEAM_ID_2" || player == OWNER_NAME
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

async fn broadcast_msg(msg: &str) {
    crate::auto_announce::announce(msg).await; // server-wide event -> #Announce banner + chat
    let params = serde_json::json!({ "text": msg });
    pipe_rpc::call("broadcastChat", Some(params)).await.ok();
}

const AIRDROP_LOOT: &[&str] = &[
    "BP_Weapon_AKM_C",
    "BP_Item_Ammo_762x39_C",
    "BP_Item_Ammo_762x39_C",
    "BP_Item_Military_Backpack_C",
    "BP_Item_Bandage_Military_C",
    "BP_Item_Bandage_Military_C",
    "BP_Item_Antibiotics_C",
    "BP_Item_MRE_C",
    "BP_Item_MRE_C",
    "BP_Item_Water_Bottle_C",
];

async fn drop_loot(target: &str) -> u32 {
    let mut given = 0u32;
    for item in AIRDROP_LOOT {
        let params = serde_json::json!({ "playerName": target, "className": item });
        if pipe_rpc::call("placeItemInInventory", Some(params)).await.is_ok() {
            given += 1;
        }
    }
    given
}

pub struct Airdrop { rate: Mutex<HashMap<String, Instant>> }
impl Airdrop {
    pub fn new() -> Self { Self { rate: Mutex::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl Mod for Airdrop {
    fn name(&self) -> &'static str { "airdrop" }
    fn commands(&self) -> &'static [&'static str] { &["!airdrop", "!care"] }

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

        if !matches!(cmd.as_str(), "!airdrop" | "!care") { return Outcome::Ignored; }

        match cmd.as_str() {
            "!airdrop" => {
                if !is_owner(&steam, &player) { reply("[Airdrop] Admin only.", &player).await; return Outcome::Handled; }

                let target = if args.is_empty() {
                    // Pick random online player
                    match pipe_rpc::call("getOnlinePlayers", None).await {
                        Ok(resp) => {
                            let players = resp.get("players").and_then(|v| v.as_array());
                            match players {
                                Some(arr) if !arr.is_empty() => {
                                    let seed = std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default().as_nanos() as usize;
                                    arr[seed % arr.len()].get("name").and_then(|n| n.as_str())
                                        .unwrap_or(&player).to_string()
                                }
                                _ => player.clone(),
                            }
                        }
                        Err(_) => player.clone(),
                    }
                } else {
                    args
                };

                broadcast_msg(&format!("[AIRDROP] Supply drop incoming for {}! Gear up!", target)).await;

                let t = target.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    let items = drop_loot(&t).await;
                    broadcast_msg(&format!("[AIRDROP] {} received {} items!", t, items)).await;
                });
                Outcome::Handled
            }

            "!care" => {
                // Self-care package - cheaper than airdrop, available to everyone
                let items = ["BP_Item_Bandage_Improvised_C", "BP_Item_Water_Bottle_C", "BP_Item_Berries_C"];
                for item in &items {
                    let params = serde_json::json!({ "playerName": player, "className": item });
                    pipe_rpc::call("placeItemInInventory", Some(params)).await.ok();
                }
                reply("[Care] Emergency care package received.", &player).await;
                Outcome::Handled
            }

            _ => Outcome::Ignored,
        }
    }
}
