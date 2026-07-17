// Kit system â€” !kit <name> gives predefined item loadouts via bridge.
// Admin can define kits; players use them with cooldowns.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const OWNER_STEAM_ID: &str = "YOUR_STEAM_ID_1";
const OWNER_NAME: &str = "YOUR_OWNER_NAME";
const STATE_PATH: &str = r"C:\TurdMOD\data\kits.json";
const KIT_COOLDOWN: Duration = Duration::from_secs(300); // 5 min per kit

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Kit {
    name: String,
    items: Vec<KitItem>,
    admin_only: bool,
    cooldown_secs: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct KitItem {
    class_name: String,
    count: u32,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
struct KitState {
    kits: HashMap<String, Kit>,
}

fn default_kits() -> KitState {
    let mut kits = HashMap::new();
    kits.insert("starter".into(), Kit {
        name: "Starter".into(),
        items: vec![
            KitItem { class_name: "BP_Item_Improvised_Stone_Knife_C".into(), count: 1 },
            KitItem { class_name: "BP_Item_Bandage_Improvised_C".into(), count: 3 },
            KitItem { class_name: "BP_Item_Water_Bottle_C".into(), count: 1 },
        ],
        admin_only: false,
        cooldown_secs: 600,
    });
    kits.insert("medic".into(), Kit {
        name: "Medic".into(),
        items: vec![
            KitItem { class_name: "BP_Item_Bandage_Military_C".into(), count: 5 },
            KitItem { class_name: "BP_Item_Antibiotics_C".into(), count: 2 },
            KitItem { class_name: "BP_Item_Painkillers_C".into(), count: 2 },
        ],
        admin_only: false,
        cooldown_secs: 900,
    });
    kits.insert("builder".into(), Kit {
        name: "Builder".into(),
        items: vec![
            KitItem { class_name: "BP_Item_Toolbox_C".into(), count: 1 },
            KitItem { class_name: "BP_Item_Nails_C".into(), count: 5 },
            KitItem { class_name: "BP_Item_Plank_C".into(), count: 10 },
        ],
        admin_only: false,
        cooldown_secs: 1200,
    });
    kits.insert("admin".into(), Kit {
        name: "Admin".into(),
        items: vec![
            KitItem { class_name: "BP_Weapon_AKM_C".into(), count: 1 },
            KitItem { class_name: "BP_Item_Ammo_762x39_C".into(), count: 3 },
            KitItem { class_name: "BP_Item_Military_Backpack_C".into(), count: 1 },
        ],
        admin_only: true,
        cooldown_secs: 0,
    });
    KitState { kits }
}

fn load() -> KitState {
    std::fs::read_to_string(STATE_PATH).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| {
            let state = default_kits();
            save(&state);
            state
        })
}

fn save(state: &KitState) {
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
    steam == OWNER_STEAM_ID || steam == "YOUR_STEAM_ID_2" || player == OWNER_NAME
}

pub struct Kits {
    state: KitState, // kit definitions are read-only at runtime
    cooldowns: Mutex<HashMap<(String, String), Instant>>, // (steam, kit_name) -> last_used
    rate: Mutex<HashMap<String, Instant>>,
}
impl Kits {
    pub fn new() -> Self {
        Self { state: load(), cooldowns: Mutex::new(HashMap::new()), rate: Mutex::new(HashMap::new()) }
    }
}

#[async_trait::async_trait]
impl Mod for Kits {
    fn name(&self) -> &'static str { "kits" }
    fn commands(&self) -> &'static [&'static str] { &["!kit", "!kits"] }

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
                if now.duration_since(*prev) < Duration::from_secs(3) { return Outcome::Ignored; }
            }
            rate.insert(rate_key.clone(), now);
        }

        let (cmd, args) = match text.find(' ') {
            Some(i) => (text[..i].to_lowercase(), text[i + 1..].trim().to_lowercase()),
            None => (text.to_lowercase(), String::new()),
        };

        match cmd.as_str() {
            "!kit" => {
                if args.is_empty() {
                    let available: Vec<&str> = self.state.kits.iter()
                        .filter(|(_, k)| !k.admin_only || is_owner(&steam, &player))
                        .map(|(name, _)| name.as_str())
                        .collect();
                    reply(&format!("[Kit] Available: {}", available.join(", ")), &player).await;
                    return Outcome::Handled;
                }

                let Some(kit) = self.state.kits.get(&args) else {
                    reply(&format!("[Kit] Unknown kit '{}'. Use !kit for list", args), &player).await;
                    return Outcome::Handled;
                };

                if kit.admin_only && !is_owner(&steam, &player) {
                    reply("[Kit] Admin only.", &player).await;
                    return Outcome::Handled;
                }

                let cd_key = (steam.clone(), args.clone());
                if kit.cooldown_secs > 0 {
                    let cds = self.cooldowns.lock().await;
                    if let Some(last) = cds.get(&cd_key) {
                        let cd = Duration::from_secs(kit.cooldown_secs);
                        if last.elapsed() < cd {
                            let remaining = (cd - last.elapsed()).as_secs();
                            drop(cds);
                            reply(&format!("[Kit] {} on cooldown: {}s", kit.name, remaining), &player).await;
                            return Outcome::Handled;
                        }
                    }
                }

                // Give items via placeItemInInventory (lock not held during the bridge round-trips)
                let mut given = 0u32;
                for item in &kit.items {
                    for _ in 0..item.count {
                        let params = serde_json::json!({ "playerName": player, "className": item.class_name });
                        if pipe_rpc::call("placeItemInInventory", Some(params)).await.is_ok() {
                            given += 1;
                        }
                    }
                }

                self.cooldowns.lock().await.insert(cd_key, Instant::now());
                reply(&format!("[Kit] {} received! ({} items)", kit.name, given), &player).await;
                Outcome::Handled
            }

            "!kits" => {
                let available: Vec<String> = self.state.kits.iter()
                    .filter(|(_, k)| !k.admin_only || is_owner(&steam, &player))
                    .map(|(name, k)| {
                        let items: Vec<String> = k.items.iter()
                            .map(|i| format!("{}x{}", i.count, i.class_name.trim_start_matches("BP_Item_").trim_end_matches("_C")))
                            .collect();
                        format!("{}: {}", name, items.join("+"))
                    })
                    .collect();
                for line in available {
                    reply(&format!("[Kit] {}", line), &player).await;
                }
                Outcome::Handled
            }

            _ => Outcome::Ignored,
        }
    }
}
