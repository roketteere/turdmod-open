// Spawn loadout - give items to players when they log in. Newbie vs veteran tier by login count.
// Event-driven (login only). Waits 10s for the player to load, hence a longer timeout().

use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const STATE_PATH: &str = r"C:\TurdMOD\data\spawn_loadouts.json";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PlayerLevel {
    logins: u32,
    last_login_ts: u64,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
struct LoadoutState {
    players: HashMap<String, PlayerLevel>,
}

const NEWBIE_ITEMS: &[&str] = &[
    "BP_Item_Improvised_Stone_Knife_C",
    "BP_Item_Bandage_Improvised_C",
    "BP_Item_Torch_C",
];

const VETERAN_ITEMS: &[&str] = &[
    "BP_Item_Improvised_Stone_Knife_C",
    "BP_Item_Bandage_Improvised_C",
    "BP_Item_Bandage_Improvised_C",
    "BP_Item_Water_Bottle_C",
    "BP_Item_Compass_C",
];

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_secs()
}

fn load() -> LoadoutState {
    std::fs::read_to_string(STATE_PATH).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(state: &LoadoutState) {
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let tmp = format!("{}.tmp", STATE_PATH);
        if std::fs::write(&tmp, &json).is_ok() { let _ = std::fs::rename(&tmp, STATE_PATH); }
    }
}

async fn give_items(player: &str, items: &[&str]) {
    for item in items {
        let params = serde_json::json!({ "playerName": player, "className": item });
        pipe_rpc::call("placeItemInInventory", Some(params)).await.ok();
    }
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

pub struct SpawnLoadout { state: Mutex<LoadoutState> }
impl SpawnLoadout {
    pub fn new() -> Self { Self { state: Mutex::new(load()) } }
}

#[async_trait::async_trait]
impl Mod for SpawnLoadout {
    fn name(&self) -> &'static str { "spawn_loadout" }
    fn timeout(&self) -> Duration { Duration::from_secs(30) } // 10s load wait + item grants

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "login" { return Outcome::Ignored; }
        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if player.is_empty() { return Outcome::Ignored; }

        let logins = {
            let mut st = self.state.lock().await;
            let entry = st.players.entry(steam).or_insert(PlayerLevel { logins: 0, last_login_ts: 0 });
            entry.logins += 1;
            entry.last_login_ts = now_secs();
            let l = entry.logins;
            save(&st);
            l
        };

        tokio::time::sleep(Duration::from_secs(10)).await; // wait for player to fully load
        if logins <= 3 {
            give_items(&player, NEWBIE_ITEMS).await;
            reply("[Welcome] Starter kit given! You're new here - good luck.", &player).await;
        } else {
            give_items(&player, VETERAN_ITEMS).await;
            reply(&format!("[Welcome] Veteran kit (login #{}).", logins), &player).await;
        }
        Outcome::Handled
    }
}
