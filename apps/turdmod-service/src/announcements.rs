// Join/leave/kill announcements — broadcasts server events to in-game chat.
// Join/leave: orange (channel 6 = ServerMessage)
// Kill feed: blue (channel 2 = Global)

use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

async fn bcast(msg: &str, channel: u8) {
    let params = serde_json::json!({ "text": msg, "channel": channel });
    if let Err(e) = pipe_rpc::call("broadcastChat", Some(params)).await {
        tracing::warn!("announcements: send failed: {}", e);
    }
}

pub struct Announcements;

#[async_trait::async_trait]
impl Mod for Announcements {
    fn name(&self) -> &'static str { "announcements" }
    // event-driven: login/logout/kill

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        match ev.event.as_str() {
            "login" => {
                let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("Unknown");
                let msg = format!("[Server] {} joined the server.", player);
                tracing::info!("announcements: {}", msg);
                bcast(&msg, 6).await;
                Outcome::Handled
            }
            "logout" => {
                let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("Unknown");
                let msg = format!("[Server] {} left the server.", player);
                tracing::info!("announcements: {}", msg);
                bcast(&msg, 6).await;
                Outcome::Handled
            }
            "kill" => {
                let killer = ev.data.get("killer").and_then(|v| v.as_str());
                let victim = ev.data.get("victim").and_then(|v| v.as_str());
                if let (Some(k), Some(v)) = (killer, victim) {
                    let msg = format!("[Kill] {} killed {}", k, v);
                    tracing::info!("announcements: {}", msg);
                    bcast(&msg, 2).await;
                    Outcome::Handled
                } else {
                    Outcome::Ignored
                }
            }
            _ => Outcome::Ignored,
        }
    }
}
