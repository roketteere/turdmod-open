// nightvote — `!skipnight` fires SCUM's NATIVE `Vote SetTimeOfDay` to skip night to morning, with
// Joel's "no two nights in a row" rule. The native vote (F2/F3, 60s, game-tallied) does the actual
// voting AND the time change — we never call setTimeOfDay, so ZERO crash risk.
//
// MIGRATED to the registry spine (2026-06-10): was a standalone chat loop; now a registry::Mod
// claiming "!skipnight" (rate-limit 15s = bus-level anti-spam; only the forced-night clock is local
// state). Runs identically in console + service — can't drift out of production.
//
// @dep bridge: runAdminCommand (native Vote is admin-initiated, hosted via an online admin).
// @brk no chat reader -> forced-night clock starts on vote-START, not on pass. @inv [[registry]].

use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const MORNING_HOUR: u32 = 7;
const FORCED_NIGHT_COOLDOWN: Duration = Duration::from_secs(3600); // ~one in-game day-night cycle
const OWNER_IDS: [&str; 2] = ["YOUR_STEAM_ID_1", "YOUR_STEAM_ID_2"];

async fn online_admin() -> Option<String> {
    let r = pipe_rpc::call("getOnlinePlayers", Some(serde_json::json!({}))).await.ok()?;
    for p in r.get("players")?.as_array()? {
        if OWNER_IDS.contains(&p.get("steamId").and_then(|v| v.as_str()).unwrap_or("")) {
            return p.get("name").and_then(|v| v.as_str()).map(String::from);
        }
    }
    None
}

async fn broadcast(msg: &str) {
    crate::auto_announce::announce(msg).await; // event -> #Announce banner + chat
    pipe_rpc::call("broadcastChat", Some(serde_json::json!({ "text": msg }))).await.ok();
}

/// `!skipnight` — fire the native Vote SetTimeOfDay, with the forced-night cooldown.
pub struct NightVote {
    last_started: Mutex<Option<Instant>>,
}

impl NightVote {
    pub fn new() -> Self { Self { last_started: Mutex::new(None) } }
}

#[async_trait::async_trait]
impl Mod for NightVote {
    fn name(&self) -> &'static str { "nightvote" }
    fn commands(&self) -> &'static [&'static str] { &["!skipnight"] }
    fn rate_limit(&self) -> Duration { Duration::from_secs(15) } // collapse simultaneous callers

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim();
        if !text.eq_ignore_ascii_case("!skipnight") { return Outcome::Ignored; }
        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();

        // forced-night: a skip vote already ran this cycle -> this night stays.
        if let Some(ls) = *self.last_started.lock().await {
            if ls.elapsed() < FORCED_NIGHT_COOLDOWN {
                broadcast("[ScummyMap] This is a forced night, last night was skipped!").await;
                return Outcome::Handled;
            }
        }

        let admin = match online_admin().await {
            Some(a) => a,
            None => {
                pipe_rpc::call("sendChatLineToPlayer", Some(serde_json::json!({
                    "message": "[ScummyMap] Can't start the night-skip vote — no admin online to host it.",
                    "playerName": player, "channel": "4"
                }))).await.ok();
                return Outcome::Handled;
            }
        };
        match pipe_rpc::call("runAdminCommand", Some(serde_json::json!({
            "command": format!("Vote SetTimeOfDay {}", MORNING_HOUR),
            "playerName": admin
        }))).await {
            Ok(_) => {
                *self.last_started.lock().await = Some(Instant::now());
                broadcast("[ScummyMap] Night-skip vote started! F2 = skip to morning, F3 = keep the night. (60s)").await;
                tracing::info!("[nightvote] {} started a night-skip vote (host {})", player, admin);
                Outcome::Handled
            }
            Err(e) => {
                tracing::warn!("[nightvote] runAdminCommand failed: {}", e);
                Outcome::Failed(format!("vote start: {}", e))
            }
        }
    }
}
