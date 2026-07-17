// Player shops - set up a shop at your base, others browse and buy.
// !shop create <name> - create a shop at your position.
// !shop add <item> <price> - add item to your shop.
// !shop list - browse all player shops. !shop buy <shop> <item>.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const STATE_PATH: &str = r"C:\TurdMOD\data\player_shops.json";
const ECON_PATH: &str = r"C:\TurdMOD\data\economy.json";
const RATE_LIMIT: Duration = Duration::from_secs(3);
const MAX_ITEMS_PER_SHOP: usize = 10;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ShopItem {
    class_name: String,
    display_name: String,
    price: i64,
    stock: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PlayerShop {
    name: String,
    owner_steam: String,
    owner_name: String,
    x: f64,
    y: f64,
    items: Vec<ShopItem>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
struct ShopState {
    shops: HashMap<String, PlayerShop>,
}

fn load() -> ShopState {
    std::fs::read_to_string(STATE_PATH).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(state: &ShopState) {
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let tmp = format!("{}.tmp", STATE_PATH);
        if std::fs::write(&tmp, &json).is_ok() { let _ = std::fs::rename(&tmp, STATE_PATH); }
    }
}

fn transfer(from: &str, to: &str, amount: i64) -> bool {
    let Ok(data) = std::fs::read_to_string(ECON_PATH) else { return false };
    let Ok(mut state) = serde_json::from_str::<serde_json::Value>(&data) else { return false };
    let from_bal = state.get("players").and_then(|p| p.get(from))
        .and_then(|p| p.get("balance")).and_then(|b| b.as_i64()).unwrap_or(0);
    if from_bal < amount { return false; }
    let to_bal = state.get("players").and_then(|p| p.get(to))
        .and_then(|p| p.get("balance")).and_then(|b| b.as_i64()).unwrap_or(0);
    state["players"][from]["balance"] = serde_json::json!(from_bal - amount);
    state["players"][to]["balance"] = serde_json::json!(to_bal + amount);
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(&state) {
        let tmp = format!("{}.tmp", ECON_PATH);
        if std::fs::write(&tmp, &json).is_ok() { let _ = std::fs::rename(&tmp, ECON_PATH); }
    }
    true
}

fn grid_ref(x: f64, y: f64) -> String {
    let col = ((x + 400000.0) / 100000.0) as u8;
    let row = ((y + 400000.0) / 100000.0) as u8;
    format!("{}{}", (b'A' + col.min(7)) as char, row.min(7) + 1)
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

pub struct PlayerShops {
    state: Mutex<ShopState>,
    rate: Mutex<HashMap<String, Instant>>,
}

impl PlayerShops {
    pub fn new() -> Self {
        Self { state: Mutex::new(load()), rate: Mutex::new(HashMap::new()) }
    }
}

#[async_trait::async_trait]
impl Mod for PlayerShops {
    fn name(&self) -> &'static str { "player_shops" }
    fn commands(&self) -> &'static [&'static str] { &["!shop"] }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        if !text.starts_with("!shop") { return Outcome::Ignored; }

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
        let sub = parts.get(1).map(|s| s.to_lowercase()).unwrap_or_default();

        match sub.as_str() {
            "create" => {
                if parts.len() < 3 {
                    reply("[Shop] Usage: !shop create <name>", &player).await;
                    return Outcome::Handled;
                }
                let name = parts[2].to_string();
                let key = name.to_lowercase();
                let msg = {
                    let mut state = self.state.lock().await;
                    if state.shops.values().any(|s| s.owner_steam == steam) {
                        "[Shop] You already have a shop. !shop close first.".to_string()
                    } else if state.shops.contains_key(&key) {
                        "[Shop] Name taken.".to_string()
                    } else {
                        state.shops.insert(key, PlayerShop {
                            name: name.clone(), owner_steam: steam.clone(), owner_name: player.clone(),
                            x: 0.0, y: 0.0, items: Vec::new(),
                        });
                        save(&state);
                        format!("[Shop] '{}' created! Use !shop add <item> <price> to stock.", name)
                    }
                };
                reply(&msg, &player).await;
                Outcome::Handled
            }

            "add" => {
                if parts.len() < 4 {
                    reply("[Shop] Usage: !shop add <item_class> <price>", &player).await;
                    return Outcome::Handled;
                }
                let item_class = parts[2].to_string();
                let price: i64 = parts[3].parse().unwrap_or(0);
                if price <= 0 {
                    reply("[Shop] Price must be positive.", &player).await;
                    return Outcome::Handled;
                }
                let msg = {
                    let mut state = self.state.lock().await;
                    let shop = state.shops.values_mut().find(|s| s.owner_steam == steam);
                    if shop.is_none() {
                        "[Shop] Create a shop first: !shop create <name>".to_string()
                    } else {
                        let shop = shop.unwrap();
                        if shop.items.len() >= MAX_ITEMS_PER_SHOP {
                            format!("[Shop] Max {} items per shop.", MAX_ITEMS_PER_SHOP)
                        } else {
                            let display = item_class.replace("BP_Item_", "").replace("BP_Weapon_", "").replace("_C", "");
                            shop.items.push(ShopItem { class_name: item_class, display_name: display.clone(), price, stock: 99 });
                            save(&state);
                            format!("[Shop] Added {} for {}c.", display, price)
                        }
                    }
                };
                reply(&msg, &player).await;
                Outcome::Handled
            }

            "list" | "browse" => {
                let shop_lines: Vec<String> = {
                    let state = self.state.lock().await;
                    if state.shops.is_empty() {
                        vec!["[Shop] No player shops open.".to_string()]
                    } else {
                        state.shops.values().map(|shop| {
                            let items: Vec<String> = shop.items.iter()
                                .map(|i| format!("{} ({}c)", i.display_name, i.price)).collect();
                            let item_str = if items.is_empty() { "empty".into() } else { items.join(", ") };
                            format!("[Shop] {} by {} - {}", shop.name, shop.owner_name, item_str)
                        }).collect()
                    }
                };
                for line in &shop_lines { reply(line, &player).await; }
                Outcome::Handled
            }

            "buy" => {
                if parts.len() < 4 {
                    reply("[Shop] Usage: !shop buy <shop_name> <item>", &player).await;
                    return Outcome::Handled;
                }
                let shop_key = parts[2].to_lowercase();
                let item_query = parts[3].to_lowercase();

                // Extract needed data under lock, then do bridge calls after.
                let buy_info: Option<Result<(String, String, i64, String, String), String>> = {
                    let state = self.state.lock().await;
                    if let Some(shop) = state.shops.get(&shop_key) {
                        if shop.owner_steam == steam {
                            Some(Err("[Shop] Can't buy from your own shop.".to_string()))
                        } else if let Some(item) = shop.items.iter().find(|i| i.display_name.to_lowercase().contains(&item_query)) {
                            Some(Ok((item.class_name.clone(), item.display_name.clone(), item.price, shop.owner_steam.clone(), shop.owner_name.clone())))
                        } else {
                            Some(Err("[Shop] Item not found in that shop.".to_string()))
                        }
                    } else {
                        None
                    }
                };

                match buy_info {
                    None => { reply("[Shop] Shop not found.", &player).await; }
                    Some(Err(msg)) => { reply(&msg, &player).await; }
                    Some(Ok((class, display, price, seller_steam, seller_name))) => {
                        if !transfer(&steam, &seller_steam, price) {
                            reply("[Shop] Not enough coins.", &player).await;
                        } else {
                            let params = serde_json::json!({ "playerName": player, "className": class });
                            pipe_rpc::call("placeItemInInventory", Some(params)).await.ok();
                            reply(&format!("[Shop] Bought {} from {} for {}c!", display, seller_name, price), &player).await;
                            reply(&format!("[Shop] {} bought your {} for {}c!", player, display, price), &seller_name).await;
                        }
                    }
                }
                Outcome::Handled
            }

            "close" => {
                let msg = {
                    let mut state = self.state.lock().await;
                    let key_to_remove: Option<String> = state.shops.iter()
                        .find(|(_, s)| s.owner_steam == steam)
                        .map(|(k, _)| k.clone());
                    if let Some(key) = key_to_remove {
                        state.shops.remove(&key);
                        save(&state);
                        "[Shop] Shop closed.".to_string()
                    } else {
                        "[Shop] You don't have a shop.".to_string()
                    }
                };
                reply(&msg, &player).await;
                Outcome::Handled
            }

            _ => {
                reply("[Shop] Commands: create/add/list/buy/close", &player).await;
                Outcome::Handled
            }
        }
    }
}
