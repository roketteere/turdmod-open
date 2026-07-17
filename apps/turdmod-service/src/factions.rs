// Faction overview - read-only unified view across the clans + territory modules.
// clans.json holds membership/wars/embedded zones; territories.json holds claimed
// POIs. They were never joined (clan.territories vs territory.clan are separate
// data). This module glues them into a single !faction summary. READ-ONLY: it
// never writes either file, so it cannot break clan/territory behavior.
//
// !faction          - your clan: tag, leader, members, wars, claimed POIs.
// !faction <name>    - public overview of a named clan.
// @dep: clans.rs (clans.json) + territory.rs (territories.json) schemas. Mirrors
//   only the fields displayed, all #[serde(default)] so schema drift degrades to
//   blanks rather than failing to load.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const CLANS_PATH: &str = r"C:\TurdMOD\data\clans.json";
const TERR_PATH: &str = r"C:\TurdMOD\data\territories.json";
const RATE_LIMIT: Duration = Duration::from_secs(3);

#[derive(serde::Deserialize, Default)]
struct ClanMemberRo {
    #[serde(default)]
    name: String,
}
#[derive(serde::Deserialize, Default)]
struct ClanZoneRo {
    #[serde(default)]
    name: String,
}
#[derive(serde::Deserialize, Default)]
struct ClanRo {
    #[serde(default)]
    name: String,
    #[serde(default)]
    tag: String,
    #[serde(default)]
    leader_name: String,
    #[serde(default)]
    members: Vec<ClanMemberRo>,
    #[serde(default)]
    territories: Vec<ClanZoneRo>,
    #[serde(default)]
    wars: Vec<String>,
}
#[derive(serde::Deserialize, Default)]
struct ClanStateRo {
    #[serde(default)]
    clans: HashMap<String, ClanRo>,
    #[serde(default)]
    player_clan: HashMap<String, String>, // steam -> clan_name_lc
}
#[derive(serde::Deserialize, Default)]
struct TerrRo {
    #[serde(default)]
    name: String,
    #[serde(default)]
    clan: String,
}
#[derive(serde::Deserialize, Default)]
struct TerrStateRo {
    #[serde(default)]
    territories: Vec<TerrRo>,
}

fn load_clans() -> ClanStateRo {
    std::fs::read_to_string(CLANS_PATH).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}
fn load_terr() -> TerrStateRo {
    std::fs::read_to_string(TERR_PATH).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

/// All claimed POIs (from territories.json) belonging to a clan, matched by name
/// case-insensitively against the clan's display name.
fn claimed_pois(terr: &TerrStateRo, clan_name: &str) -> Vec<String> {
    let lc = clan_name.to_lowercase();
    terr.territories
        .iter()
        .filter(|t| t.clan.to_lowercase() == lc)
        .map(|t| t.name.clone())
        .collect()
}

async fn show_clan(clan: &ClanRo, terr: &TerrStateRo, player: &str) {
    let tag = if clan.tag.is_empty() { String::new() } else { format!("[{}] ", clan.tag) };
    reply(&format!("[Faction] {}{} - led by {}", tag, clan.name, if clan.leader_name.is_empty() { "?" } else { &clan.leader_name }), player).await;
    reply(&format!("[Faction] Members: {}", clan.members.len().max(1)), player).await;
    // Unified POIs: clan-embedded zones + territory-module claims, deduped.
    let mut pois: Vec<String> = clan.territories.iter().map(|z| z.name.clone()).collect();
    for p in claimed_pois(terr, &clan.name) {
        if !pois.iter().any(|x| x.eq_ignore_ascii_case(&p)) {
            pois.push(p);
        }
    }
    if pois.is_empty() {
        reply("[Faction] Territories: none claimed", player).await;
    } else {
        reply(&format!("[Faction] Territories: {}", pois.join(", ")), player).await;
    }
    if !clan.wars.is_empty() {
        reply(&format!("[Faction] At war with: {}", clan.wars.join(", ")), player).await;
    }
}

pub struct Factions {
    rate: Mutex<HashMap<String, Instant>>,
}

impl Factions {
    pub fn new() -> Self { Self { rate: Mutex::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl Mod for Factions {
    fn name(&self) -> &'static str { "factions" }
    fn commands(&self) -> &'static [&'static str] { &["!faction"] }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        if !text.starts_with("!faction") { return Outcome::Ignored; }

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

        let clans = load_clans();
        let terr = load_terr();
        let parts: Vec<&str> = text.split_whitespace().collect();

        if parts.len() >= 2 {
            // !faction <name>
            let query = parts[1..].join(" ").to_lowercase();
            match clans.clans.get(&query).or_else(|| clans.clans.values().find(|c| c.name.to_lowercase() == query || c.tag.to_lowercase() == query)) {
                Some(c) => show_clan(c, &terr, &player).await,
                None => reply(&format!("[Faction] No clan matching '{}'", parts[1..].join(" ")), &player).await,
            }
        } else {
            // !faction - your own
            match clans.player_clan.get(&steam).and_then(|lc| clans.clans.get(lc)) {
                Some(c) => show_clan(c, &terr, &player).await,
                None => reply("[Faction] You're not in a clan. Use clan commands to join or create one.", &player).await,
            }
        }

        Outcome::Handled
    }
}
