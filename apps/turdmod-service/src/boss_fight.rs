// Boss fights - admin-triggered zombie boss encounters with rewards.
// !boss - spawn a massive horde with escalating waves + final boss reward.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const OWNER_STEAM_ID: &str = "YOUR_STEAM_ID_1";
const OWNER_NAME: &str = "YOUR_OWNER_NAME";
const RATE_LIMIT: Duration = Duration::from_secs(5);
const BOSS_REWARD: i64 = 500;
const ECON_PATH: &str = r"C:\TurdMOD\data\economy.json";

fn is_owner(steam: &str, player: &str) -> bool {
    steam == OWNER_STEAM_ID || steam == "YOUR_STEAM_ID_2" || player == OWNER_NAME
}

async fn broadcast_msg(msg: &str) {
    crate::auto_announce::announce(msg).await; // server-wide event -> #Announce banner + chat
    let params = serde_json::json!({ "text": msg });
    pipe_rpc::call("broadcastChat", Some(params)).await.ok();
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

fn credit_all_online(amount: i64) {
    let Ok(resp_str) = std::fs::read_to_string(r"C:\TurdMOD\data\economy.json") else { return };
    let Ok(mut econ) = serde_json::from_str::<serde_json::Value>(&resp_str) else { return };
    if let Some(players) = econ.get_mut("players").and_then(|p| p.as_object_mut()) {
        for (_, player) in players.iter_mut() {
            if let Some(bal) = player.get("balance").and_then(|b| b.as_i64()) {
                player["balance"] = serde_json::json!(bal + amount);
            }
        }
    }
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(&econ) {
        let tmp = format!("{}.tmp", ECON_PATH);
        if std::fs::write(&tmp, &json).is_ok() { let _ = std::fs::rename(&tmp, ECON_PATH); }
    }
}

pub struct BossFight { rate: Mutex<HashMap<String, Instant>> }
impl BossFight {
    pub fn new() -> Self { Self { rate: Mutex::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl Mod for BossFight {
    fn name(&self) -> &'static str { "boss_fight" }
    fn commands(&self) -> &'static [&'static str] { &["!boss"] }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if text.to_lowercase() != "!boss" { return Outcome::Ignored; }

        let rate_key = if steam.is_empty() { player.clone() } else { steam.clone() };
        {
            let mut rate = self.rate.lock().await;
            let now = Instant::now();
            if let Some(prev) = rate.get(&rate_key) {
                if now.duration_since(*prev) < RATE_LIMIT { return Outcome::Ignored; }
            }
            rate.insert(rate_key.clone(), now);
        }

        if !is_owner(&steam, &player) {
            reply("[Boss] Admin only.", &player).await;
            return Outcome::Handled;
        }

        let p = player.clone();
        tokio::spawn(async move {
            broadcast_msg("[BOSS FIGHT] WARNING: Massive threat incoming!").await;
            tokio::time::sleep(Duration::from_secs(5)).await;
            broadcast_msg("[BOSS] Wave 1/5 - Scouts approaching!").await;
            pipe_rpc::call("provokeZombies", Some(serde_json::json!({ "playerName": p, "mode": "melee" }))).await.ok();
            tokio::time::sleep(Duration::from_secs(30)).await;
            broadcast_msg("[BOSS] Wave 2/5 - They're getting angry!").await;
            pipe_rpc::call("provokeZombies", Some(serde_json::json!({ "playerName": p, "mode": "ranged" }))).await.ok();
            tokio::time::sleep(Duration::from_secs(30)).await;
            broadcast_msg("[BOSS] Wave 3/5 - The horde awakens!").await;
            pipe_rpc::call("provokeZombies", Some(serde_json::json!({ "playerName": p, "mode": "ranged" }))).await.ok();
            tokio::time::sleep(Duration::from_secs(30)).await;
            broadcast_msg("[BOSS] Wave 4/5 - MAXIMUM AGGRESSION!").await;
            pipe_rpc::call("provokeZombies", Some(serde_json::json!({ "playerName": p, "mode": "ranged" }))).await.ok();
            tokio::time::sleep(Duration::from_secs(30)).await;
            broadcast_msg("[BOSS] FINAL WAVE - SURVIVE THIS!").await;
            pipe_rpc::call("provokeZombies", Some(serde_json::json!({ "playerName": p, "mode": "ranged" }))).await.ok();
            tokio::time::sleep(Duration::from_secs(45)).await;
            pipe_rpc::call("setZombiePassive", Some(serde_json::json!({ "passive": true }))).await.ok();
            broadcast_msg(&format!("[BOSS] DEFEATED! All survivors receive {} coins!", BOSS_REWARD)).await;
            credit_all_online(BOSS_REWARD);
        });
        Outcome::Handled
    }
}
