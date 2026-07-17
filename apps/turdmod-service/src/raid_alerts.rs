// Raid alerts - notify base owner when their structures take damage (with grid + attacker).
// Event-driven (baseDamage/structureDamage); 2-min per-owner cooldown.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const ALERT_COOLDOWN: Duration = Duration::from_secs(120);

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

pub struct RaidAlerts { last_alert: Mutex<HashMap<String, Instant>> }
impl RaidAlerts {
    pub fn new() -> Self { Self { last_alert: Mutex::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl Mod for RaidAlerts {
    fn name(&self) -> &'static str { "raid_alerts" }
    // event-driven (no commands()): reacts to baseDamage/structureDamage.

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "baseDamage" && ev.event != "structureDamage" { return Outcome::Ignored; }
        let owner = ev.data.get("owner").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let attacker = ev.data.get("attacker").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
        let structure = ev.data.get("structure").and_then(|v| v.as_str()).unwrap_or("base").to_string();
        let x = ev.data.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let y = ev.data.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);
        if owner.is_empty() { return Outcome::Ignored; }

        let alert = {
            let mut la = self.last_alert.lock().await;
            let now = Instant::now();
            if la.get(&owner).map(|t| t.elapsed() < ALERT_COOLDOWN).unwrap_or(false) {
                false
            } else {
                la.insert(owner.clone(), now);
                true
            }
        };
        if !alert { return Outcome::Ignored; }

        let col = ((x + 400000.0) / 100000.0) as u8;
        let row = ((y + 400000.0) / 100000.0) as u8;
        let grid = format!("{}{}", (b'A' + col.min(7)) as char, row.min(7) + 1);
        reply(&format!("[RAID ALERT] Your {} at grid {} is under attack by {}!", structure, grid, attacker), &owner).await;
        Outcome::Handled
    }
}
