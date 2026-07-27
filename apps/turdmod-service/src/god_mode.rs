// Persistent god mode + hulk mode - re-applies flags for toggled players.
// !god - toggle god+immortal (owner only)
// !hulk - toggle hulk jump mode (owner only) - every jump is a directional leap

use std::collections::HashSet;
use std::time::Duration;
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const REAPPLY_INTERVAL: Duration = Duration::from_secs(30);

fn is_owner(steam: &str, player: &str) -> bool {
    crate::owner::is_owner(steam, player)
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

async fn apply_god(player: &str) {
    for handler in &["setGodMode", "setImmortal"] {
        let params = serde_json::json!({ "playerName": player, "enable": true });
        pipe_rpc::call(handler, Some(params)).await.ok();
    }
}

async fn remove_god(player: &str) {
    for handler in &["setGodMode", "setImmortal"] {
        let params = serde_json::json!({ "playerName": player, "enable": false });
        pipe_rpc::call(handler, Some(params)).await.ok();
    }
}

async fn do_launch(player: &str) {
    let params = serde_json::json!({
        "playerName": player,
        "speed": 3000.0,
        "upward": 1500.0
    });
    pipe_rpc::call("launchPlayer", Some(params)).await.ok();
}

pub struct GodMode {
    godded: Mutex<HashSet<String>>,
    hulked: Mutex<HashSet<String>>,
}

impl GodMode {
    pub fn new() -> Self {
        Self {
            godded: Mutex::new(HashSet::new()),
            hulked: Mutex::new(HashSet::new()),
        }
    }
}

#[async_trait::async_trait]
impl Mod for GodMode {
    fn name(&self) -> &'static str { "god_mode" }
    fn commands(&self) -> &'static [&'static str] { &["!god", "!hulk", "!jump"] }
    fn interval(&self) -> Option<Duration> { Some(REAPPLY_INTERVAL) }

    // Reapply god mode flags periodically (the old spawned ticker).
    async fn tick(&self, _ctx: &ModCtx) {
        let names: Vec<String> = self.godded.lock().await.iter().cloned().collect();
        for name in &names {
            apply_god(name).await;
        }
    }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }

        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();

        match text.to_lowercase().as_str() {
            "!god" => {
                if !is_owner(&steam, &player) {
                    reply("[Server] Owner only.", &player).await;
                    return Outcome::Handled;
                }
                let already = self.godded.lock().await.contains(&player);
                if already {
                    self.godded.lock().await.remove(&player);
                    remove_god(&player).await;
                    reply("[God] God mode OFF", &player).await;
                } else {
                    self.godded.lock().await.insert(player.clone());
                    apply_god(&player).await;
                    reply("[God] God mode ON (persistent)", &player).await;
                }
                Outcome::Handled
            }

            "!hulk" => {
                if !is_owner(&steam, &player) {
                    reply("[Server] Owner only.", &player).await;
                    return Outcome::Handled;
                }
                let already = self.hulked.lock().await.contains(&player);
                if already {
                    self.hulked.lock().await.remove(&player);
                    reply("[Hulk] Hulk mode OFF", &player).await;
                } else {
                    self.hulked.lock().await.insert(player.clone());
                    // Enable god when hulk activates
                    self.godded.lock().await.insert(player.clone());
                    apply_god(&player).await;
                    do_launch(&player).await;
                    reply("[Hulk] HULK MODE ON - every !jump is a leap. Type !hulk to disable.", &player).await;
                }
                Outcome::Handled
            }

            "!jump" => {
                let is_hulked = self.hulked.lock().await.contains(&player);
                if is_hulked {
                    do_launch(&player).await;
                    Outcome::Handled
                } else {
                    Outcome::Ignored
                }
            }

            _ => Outcome::Ignored,
        }
    }
}
