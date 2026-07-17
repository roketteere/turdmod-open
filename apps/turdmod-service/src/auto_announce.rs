// auto_announce - server-driven #Announce banners on a 30-min rotation, using whichever
// admin is online as executor (no human types). announce() is public for other modules.

use std::time::Duration;
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const OWNER_IDS: [&str; 2] = ["YOUR_STEAM_ID_1", "YOUR_STEAM_ID_2"];
const INTERVAL: Duration = Duration::from_secs(30 * 60);

const MESSAGES: &[&str] = &[
    "Welcome to www.ScummyMap.com! Type !help in chat for all commands.",
    "RED zones are full-loot PVP - check !pvpzones before you roam.",
    "Live map + your stats at www.ScummyMap.com. Join our Discord!",
    "Lock a vehicle then !register to claim it. !garage manages your rides.",
    "Skills gain 4x here - grind smart. Traders: ask an admin for !tp.",
    "Support ScummyMap + grab perks: !donate. Keeps the server alive!",
];

/// Pick an executor channel for the zero-admin Announce: prefer an owner
/// (stable), else ANY online player. The bypass patches the CanExecute gate
/// so the chosen player needs no admin rights — they're just the dispatch
/// channel. Returns None only when the server is empty.
async fn online_player() -> Option<String> {
    let r = pipe_rpc::call("getOnlinePlayers", Some(serde_json::json!({}))).await.ok()?;
    let players = r.get("players")?.as_array()?;
    for p in players {
        if OWNER_IDS.contains(&p.get("steamId").and_then(|v| v.as_str()).unwrap_or("")) {
            if let Some(n) = p.get("name").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                return Some(n.to_string());
            }
        }
    }
    players
        .iter()
        .find_map(|p| p.get("name").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(|s| s.to_string()))
}

/// Run ANY admin command with the zero-admin bypass — the shared adminless
/// command primitive. Uses an online player as the executor channel + the
/// CanExecute gate patch, so it runs with no admin account.
/// @inv gameThread/bypass MUST be string "1" (the bridge's extract_json_str
/// ignores JSON numbers). Needs >=1 player online (the executor channel).
/// Used by announce(), vehicle destroy (DestroyVehicle <id>), and future
/// taxi/insurance/shop flows that need to fire admin verbs server-driven.
pub async fn run_admin_bypass(command: &str) -> bool {
    let Some(exec) = online_player().await else { return false };
    pipe_rpc::call(
        "runAdminCommand",
        Some(serde_json::json!({
            "command": command,
            "playerName": exec,
            "gameThread": "1",
            "bypass": "1",
        })),
    )
    .await
    .map(|r| r.get("error").is_none())
    .unwrap_or(false)
}

/// Fire the prominent #Announce banner with ZERO admins online.
pub async fn announce(text: &str) -> bool {
    run_admin_bypass(&format!("Announce {}", text)).await
}

pub struct AutoAnnounce { idx: Mutex<usize> }
impl AutoAnnounce { pub fn new() -> Self { Self { idx: Mutex::new(0) } } }

#[async_trait::async_trait]
impl Mod for AutoAnnounce {
    fn name(&self) -> &'static str { "auto_announce" }
    // event-driven: ONE rotating branded tip to each player on LOGIN (personal). NO periodic
    // broadcast (Joel: don't flood everyone). announce() stays pub for important live broadcasts.

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "login" { return Outcome::Ignored; }
        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if player.is_empty() { return Outcome::Ignored; }
        let msg = { let mut idx = self.idx.lock().await; let m = MESSAGES[*idx % MESSAGES.len()]; *idx += 1; m };
        pipe_rpc::call("sendChatLineToPlayer", Some(serde_json::json!({ "message": format!("[ScummyMap] {}", msg), "playerName": player, "channel": "6" }))).await.ok();
        Outcome::Handled
    }
}
