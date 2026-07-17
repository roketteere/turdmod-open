// Mod list - !mods shows all installed TurdMOD modules with creator credits.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const RATE_LIMIT: Duration = Duration::from_secs(5);

const MODS: &[(&str, &str)] = &[
    ("FriendlyPuppets", "ScummyMap"),
    ("Economy System", "ScummyMap"),
    ("Vehicle Registry", "ScummyMap"),
    ("Teleport Points", "ScummyMap"),
    ("God Mode / Hulk", "ScummyMap"),
    ("Companion Taming", "ScummyMap"),
    ("Leaderboard", "ScummyMap"),
    ("Duel System", "ScummyMap"),
    ("Clan System", "ScummyMap"),
    ("Territory Control", "ScummyMap"),
    ("Gambling Casino", "ScummyMap"),
    ("Lottery", "ScummyMap"),
    ("Voting System", "ScummyMap"),
    ("Kit Loadouts", "ScummyMap"),
    ("Warzone Events", "ScummyMap"),
    ("Boss Fights", "ScummyMap"),
    ("Horde Survival", "ScummyMap"),
    ("Racing", "ScummyMap"),
    ("Bounty System", "ScummyMap"),
    ("Bounty Hunter", "ScummyMap"),
    ("Reputation Karma", "ScummyMap"),
    ("Jail System", "ScummyMap"),
    ("Safe Zones", "ScummyMap"),
    ("Auction House", "ScummyMap"),
    ("Banking + Interest", "ScummyMap"),
    ("Trading Post", "ScummyMap"),
    ("Quest System", "ScummyMap"),
    ("Achievements", "ScummyMap"),
    ("Title System", "ScummyMap"),
    ("Scoreboard Stats", "ScummyMap"),
    ("Login Streak", "ScummyMap"),
    ("Spawn Protection", "ScummyMap"),
    ("Spawn Loadouts", "ScummyMap"),
    ("Vehicle Interactions", "ScummyMap"),
    ("Radio DJ", "ScummyMap"),
    ("Death Recap", "ScummyMap"),
    ("Weather Alerts", "ScummyMap"),
    ("Weather Cycle", "ScummyMap"),
    ("Supply Drops", "ScummyMap"),
    ("Airdrop System", "ScummyMap"),
    ("Event Scheduler", "ScummyMap"),
    ("Server Rules + MOTD", "ScummyMap"),
    ("Report System", "ScummyMap"),
    ("Chat Filter", "ScummyMap"),
    ("AFK Detection", "ScummyMap"),
    ("Admin Audit Log", "ScummyMap"),
    ("VAC Screening", "ScummyMap"),
    ("Raid Protection", "ScummyMap"),
    ("Base Protection", "ScummyMap"),
    ("Analytics Heatmap", "ScummyMap"),
    ("Map Tracker", "ScummyMap"),
    ("DIMs NPCs (Ziggy, Doc, Rust)", "ScummyMap"),
    ("Auto-Restart + Scheduler", "ScummyMap"),
    ("Welcome Messages", "ScummyMap"),
    ("Player Database", "ScummyMap"),
    ("Metabolism Control", "ScummyMap"),
    ("Announcements", "ScummyMap"),
];

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

pub struct ModList { rate: Mutex<HashMap<String, Instant>> }
impl ModList {
    pub fn new() -> Self { Self { rate: Mutex::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl Mod for ModList {
    fn name(&self) -> &'static str { "mod_list" }
    fn commands(&self) -> &'static [&'static str] { &["!mods"] }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        if text.to_lowercase() != "!mods" { return Outcome::Ignored; }
        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();

        let rate_key = if steam.is_empty() { player.clone() } else { steam.clone() };
        {
            let mut rate = self.rate.lock().await;
            let now = Instant::now();
            if let Some(prev) = rate.get(&rate_key) {
                if now.duration_since(*prev) < RATE_LIMIT { return Outcome::Ignored; }
            }
            rate.insert(rate_key.clone(), now);
        }

        reply(&format!("[ScummyMap] {} mods installed:", MODS.len()), &player).await;
        for chunk in MODS.chunks(5) {
            for (name, creator) in chunk {
                reply(&format!("{} | By {}", name, creator), &player).await;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        Outcome::Handled
    }

    fn timeout(&self) -> Duration { Duration::from_secs(30) } // sends ~57 lines in throttled chunks
}
