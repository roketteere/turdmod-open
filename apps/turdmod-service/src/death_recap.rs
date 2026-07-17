// Death recap - when a player dies, sends them details about the kill (distance, weapon, killer stats).
// Event-driven (kill); stateless (reads leaderboard.json on demand).

use std::time::Duration;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const LB_PATH: &str = r"C:\TurdMOD\data\leaderboard.json";

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

fn get_killer_stats(steam: &str) -> Option<(u64, u64, f64)> {
    let data = std::fs::read_to_string(LB_PATH).ok()?;
    let state: serde_json::Value = serde_json::from_str(&data).ok()?;
    let p = state.get("players")?.get(steam)?;
    let kills = p.get("kills")?.as_u64()?;
    let deaths = p.get("deaths")?.as_u64()?;
    let kd = if deaths == 0 { kills as f64 } else { kills as f64 / deaths as f64 };
    Some((kills, deaths, kd))
}

pub struct DeathRecap;
impl DeathRecap {
    pub fn new() -> Self { Self }
}

#[async_trait::async_trait]
impl Mod for DeathRecap {
    fn name(&self) -> &'static str { "death_recap" }
    fn timeout(&self) -> Duration { Duration::from_secs(10) } // 3s post-respawn delay

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "kill" { return Outcome::Ignored; }
        let killer = ev.data.get("killer").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let killer_steam = ev.data.get("killerSteam").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let victim = ev.data.get("victim").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let weapon = ev.data.get("weapon").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
        let distance = ev.data.get("distance").and_then(|v| v.as_f64()).unwrap_or(0.0);
        if victim.is_empty() || killer.is_empty() { return Outcome::Ignored; }

        let mut recap = format!("[Death Recap] Killed by {}", killer);
        if distance > 0.0 { recap += &format!(" ({:.0}m)", distance / 100.0); }
        if weapon != "unknown" { recap += &format!(" with {}", weapon); }
        if let Some((k, d, kd)) = get_killer_stats(&killer_steam) {
            recap += &format!(" - K:{} D:{} KD:{:.1}", k, d, kd);
        }

        // Short delay so the victim sees it after the respawn screen.
        tokio::time::sleep(Duration::from_secs(3)).await;
        reply(&recap, &victim).await;
        Outcome::Handled
    }
}
