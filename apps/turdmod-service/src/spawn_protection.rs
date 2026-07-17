// Spawn protection - 30s god mode on login. Prevents spawn camping.

use std::time::Duration;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const PROTECTION_SECS: u64 = 30;

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

async fn set_god(player: &str, on: bool) {
    pipe_rpc::call("setGodMode", Some(serde_json::json!({ "playerName": player, "enable": on }))).await.ok();
    pipe_rpc::call("setImmortal", Some(serde_json::json!({ "playerName": player, "enable": on }))).await.ok();
}

pub struct SpawnProtection;
impl SpawnProtection { pub fn new() -> Self { Self } }

#[async_trait::async_trait]
impl Mod for SpawnProtection {
    fn name(&self) -> &'static str { "spawn_protection" }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "login" { return Outcome::Ignored; }
        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if player.is_empty() { return Outcome::Ignored; }
        let p = player.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(5)).await;
            set_god(&p, true).await;
            reply(&format!("[Protection] {}s spawn protection active.", PROTECTION_SECS), &p).await;
            tokio::time::sleep(Duration::from_secs(PROTECTION_SECS)).await;
            set_god(&p, false).await;
            reply("[Protection] Spawn protection expired. Good luck!", &p).await;
        });
        Outcome::Handled
    }
}
