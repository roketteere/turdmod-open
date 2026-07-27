// AFK detection - warns then kicks players idle too long. Reads ctx.map on a 60s tick.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const AFK_WARN_SECS: u64 = 900;
const AFK_KICK_SECS: u64 = 1200;
const CHECK_INTERVAL: Duration = Duration::from_secs(60);
const MIN_MOVE_DISTANCE: f64 = 100.0;

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

struct PlayerTrack { x: f64, y: f64, last_move: Instant, warned: bool }

pub struct Afk { tracked: Mutex<HashMap<String, PlayerTrack>> }
impl Afk { pub fn new() -> Self { Self { tracked: Mutex::new(HashMap::new()) } } }

#[async_trait::async_trait]
impl Mod for Afk {
    fn name(&self) -> &'static str { "afk" }
    fn interval(&self) -> Option<Duration> { Some(CHECK_INTERVAL) }

    async fn tick(&self, ctx: &ModCtx) {
        let snapshot = ctx.map.read().await.clone();
        let online: std::collections::HashSet<String> = snapshot.players.iter().map(|p| p.name.clone()).collect();
        let mut tracked = self.tracked.lock().await;
        tracked.retain(|name, _| online.contains(name));
        let mut to_kick: Vec<String> = Vec::new();
        let mut to_warn: Vec<(String, u64, u64)> = Vec::new();
        for p in &snapshot.players {
            if crate::owner::is_owner_steam(&p.steam_id) { continue; }
            let entry = tracked.entry(p.name.clone()).or_insert_with(|| PlayerTrack { x: p.x, y: p.y, last_move: Instant::now(), warned: false });
            let dx = p.x - entry.x; let dy = p.y - entry.y;
            if (dx * dx + dy * dy).sqrt() > MIN_MOVE_DISTANCE {
                entry.x = p.x; entry.y = p.y; entry.last_move = Instant::now(); entry.warned = false;
            }
            let idle_secs = entry.last_move.elapsed().as_secs();
            if idle_secs >= AFK_KICK_SECS { to_kick.push(p.name.clone()); }
            else if idle_secs >= AFK_WARN_SECS && !entry.warned { to_warn.push((p.name.clone(), idle_secs, AFK_KICK_SECS - idle_secs)); entry.warned = true; }
        }
        for name in &to_kick { tracked.remove(name); }
        drop(tracked);
        for name in &to_kick {
            tracing::info!("afk: kicking {} (idle)", name);
            pipe_rpc::call("kickPlayer", Some(serde_json::json!({ "playerName": name, "reason": "AFK too long" }))).await.ok();
        }
        for (name, idle_secs, remaining) in &to_warn {
            reply(&format!("[AFK] You've been idle for {}min. Move or be kicked in {}min.", idle_secs / 60, remaining / 60), name).await;
        }
    }

    async fn handle(&self, _ev: &GameEvent, _ctx: &ModCtx) -> Outcome { Outcome::Ignored }
}
