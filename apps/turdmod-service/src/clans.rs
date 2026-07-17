// Clan system - persistent factions with territory claims.
// !clan create <name>, !clan invite <player>, !clan leave, !clan info
// !clan territory - claim zone around current position
// Clan wars: !clan war <clan> - declare war for PvP bonuses

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const STATE_PATH: &str = r"C:\TurdMOD\data\clans.json";
const RATE_LIMIT: Duration = Duration::from_secs(3);
const MAX_CLAN_SIZE: usize = 10;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Clan {
    name: String,
    tag: String,
    leader_steam: String,
    leader_name: String,
    members: Vec<ClanMember>,
    created_ts: u64,
    territories: Vec<Territory>,
    wars: Vec<String>, // clan names at war with
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ClanMember {
    steam: String,
    name: String,
    rank: String,
    joined_ts: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Territory {
    name: String,
    x: f64,
    y: f64,
    radius: f64,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
struct ClanState {
    clans: HashMap<String, Clan>,        // clan_name_lc -> clan
    player_clan: HashMap<String, String>, // steam -> clan_name_lc
    invites: HashMap<String, String>,    // target_steam -> clan_name_lc
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_secs()
}

fn load() -> ClanState {
    std::fs::read_to_string(STATE_PATH).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(state: &ClanState) {
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let tmp = format!("{}.tmp", STATE_PATH);
        if std::fs::write(&tmp, &json).is_ok() { let _ = std::fs::rename(&tmp, STATE_PATH); }
    }
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

async fn broadcast_msg(msg: &str) {
    let params = serde_json::json!({ "text": msg });
    pipe_rpc::call("broadcastChat", Some(params)).await.ok();
}

pub struct Clans {
    state: Mutex<ClanState>,
    rate: Mutex<HashMap<String, Instant>>,
}
impl Clans {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(load()),
            rate: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait::async_trait]
impl Mod for Clans {
    fn name(&self) -> &'static str { "clans" }
    fn commands(&self) -> &'static [&'static str] { &["!clan"] }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        if !text.starts_with("!clan") { return Outcome::Ignored; }

        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let rate_key = if steam.is_empty() { player.clone() } else { steam.clone() };
        // steam may be empty - use player name as fallback

        {
            let mut rate = self.rate.lock().await;
            let now = Instant::now();
            if let Some(prev) = rate.get(&rate_key) {
                if now.duration_since(*prev) < RATE_LIMIT { return Outcome::Ignored; }
            }
            rate.insert(rate_key.clone(), now);
        }

        let parts: Vec<&str> = text.split_whitespace().collect();
        let sub = parts.get(1).map(|s| s.to_lowercase()).unwrap_or_default();
        let arg = parts.get(2).map(|s| s.to_string()).unwrap_or_default();

        // Build reply strings under lock; send after dropping lock.
        // @inv: never hold state lock across pipe_rpc::call await
        let mut out: Vec<(String, String)> = Vec::new();    // (recipient, msg)
        let mut broadcasts: Vec<String> = Vec::new();

        {
            let mut state = self.state.lock().await;

            match sub.as_str() {
                "create" => {
                    if arg.is_empty() { out.push((player.clone(), "[Clan] Usage: !clan create <name>".into())); }
                    else if state.player_clan.contains_key(&steam) {
                        out.push((player.clone(), "[Clan] Leave your current clan first.".into()));
                    } else {
                        let key = arg.to_lowercase();
                        if state.clans.contains_key(&key) {
                            out.push((player.clone(), "[Clan] Name taken.".into()));
                        } else {
                            let tag = if arg.len() >= 3 { arg[..3].to_uppercase() } else { arg.to_uppercase() };
                            let clan = Clan {
                                name: arg.clone(), tag,
                                leader_steam: steam.clone(), leader_name: player.clone(),
                                members: vec![ClanMember { steam: steam.clone(), name: player.clone(),
                                    rank: "Leader".into(), joined_ts: now_secs() }],
                                created_ts: now_secs(), territories: vec![], wars: vec![],
                            };
                            state.clans.insert(key.clone(), clan);
                            state.player_clan.insert(steam.clone(), key);
                            save(&state);
                            broadcasts.push(format!("[Clan] {} founded clan '{}'!", player, arg));
                        }
                    }
                }

                "invite" => {
                    if arg.is_empty() { out.push((player.clone(), "[Clan] Usage: !clan invite <player>".into())); }
                    else {
                        let clan_key = state.player_clan.get(&steam).cloned();
                        match clan_key {
                            None => out.push((player.clone(), "[Clan] You're not in a clan.".into())),
                            Some(clan_key) => {
                                let ok = if let Some(c) = state.clans.get(&clan_key) {
                                    if c.leader_steam != steam {
                                        out.push((player.clone(), "[Clan] Only the leader can invite.".into()));
                                        false
                                    } else if c.members.len() >= MAX_CLAN_SIZE {
                                        out.push((player.clone(), "[Clan] Clan is full.".into()));
                                        false
                                    } else { true }
                                } else { true };
                                if ok {
                                    // Store invite (resolve steam later on accept)
                                    state.invites.insert(format!("name:{}", arg.to_lowercase()), clan_key);
                                    save(&state);
                                    out.push((player.clone(), format!("[Clan] Invite sent to {}", arg)));
                                    out.push((arg.clone(), format!("[Clan] {} invites you to their clan! !clan accept", player)));
                                }
                            }
                        }
                    }
                }

                "accept" => {
                    let invite_key = format!("name:{}", player.to_lowercase());
                    let clan_key = state.invites.remove(&invite_key);
                    match clan_key {
                        None => out.push((player.clone(), "[Clan] No pending invite.".into())),
                        Some(clan_key) => {
                            if state.player_clan.contains_key(&steam) {
                                out.push((player.clone(), "[Clan] Leave your current clan first.".into()));
                            } else {
                                let clan_name = if let Some(clan) = state.clans.get_mut(&clan_key) {
                                    clan.members.push(ClanMember { steam: steam.clone(), name: player.clone(),
                                        rank: "Member".into(), joined_ts: now_secs() });
                                    Some(clan.name.clone())
                                } else { None };
                                if let Some(cn) = clan_name {
                                    state.player_clan.insert(steam.clone(), clan_key);
                                    save(&state);
                                    out.push((player.clone(), format!("[Clan] Joined {}!", cn)));
                                    broadcasts.push(format!("[Clan] {} joined {}!", player, cn));
                                }
                            }
                        }
                    }
                }

                "leave" => {
                    let clan_key = state.player_clan.remove(&steam);
                    match clan_key {
                        None => out.push((player.clone(), "[Clan] Not in a clan.".into())),
                        Some(clan_key) => {
                            let mut disbanded = false;
                            if let Some(clan) = state.clans.get_mut(&clan_key) {
                                clan.members.retain(|m| m.steam != steam);
                                if clan.members.is_empty() {
                                    disbanded = true;
                                } else if clan.leader_steam == steam {
                                    clan.leader_steam = clan.members[0].steam.clone();
                                    clan.leader_name = clan.members[0].name.clone();
                                }
                            }
                            if disbanded {
                                state.clans.remove(&clan_key);
                                broadcasts.push(format!("[Clan] {} disbanded (last member left).", clan_key));
                            }
                            save(&state);
                            out.push((player.clone(), "[Clan] You left your clan.".into()));
                        }
                    }
                }

                "info" => {
                    let key = if arg.is_empty() {
                        state.player_clan.get(&steam).cloned()
                    } else {
                        Some(arg.to_lowercase())
                    };
                    match key {
                        None => out.push((player.clone(), "[Clan] Not in a clan. Specify: !clan info <name>".into())),
                        Some(clan_key) => {
                            match state.clans.get(&clan_key) {
                                None => out.push((player.clone(), "[Clan] Not found.".into())),
                                Some(clan) => {
                                    let members: Vec<&str> = clan.members.iter().map(|m| m.name.as_str()).collect();
                                    out.push((player.clone(), format!("[{}] {} - {} members: {}",
                                        clan.tag, clan.name, clan.members.len(), members.join(", "))));
                                    if !clan.wars.is_empty() {
                                        out.push((player.clone(), format!("[{}] At war with: {}", clan.tag, clan.wars.join(", "))));
                                    }
                                }
                            }
                        }
                    }
                }

                "war" => {
                    if arg.is_empty() { out.push((player.clone(), "[Clan] Usage: !clan war <clanname>".into())); }
                    else {
                        let my_clan_key = state.player_clan.get(&steam).cloned();
                        match my_clan_key {
                            None => out.push((player.clone(), "[Clan] Not in a clan.".into())),
                            Some(my_clan_key) => {
                                let target_key = arg.to_lowercase();
                                if !state.clans.contains_key(&target_key) {
                                    out.push((player.clone(), "[Clan] Target clan not found.".into()));
                                } else {
                                    let allowed = if let Some(clan) = state.clans.get(&my_clan_key) {
                                        if clan.leader_steam != steam {
                                            out.push((player.clone(), "[Clan] Only the leader can declare war.".into()));
                                            false
                                        } else { true }
                                    } else { false };
                                    if allowed {
                                        if let Some(clan) = state.clans.get_mut(&my_clan_key) {
                                            if !clan.wars.contains(&target_key) { clan.wars.push(target_key.clone()); }
                                        }
                                        if let Some(target) = state.clans.get_mut(&target_key) {
                                            if !target.wars.contains(&my_clan_key) { target.wars.push(my_clan_key.clone()); }
                                        }
                                        save(&state);
                                        let my_name = state.clans.get(&my_clan_key).map(|c| c.name.clone()).unwrap_or_else(|| "?".into());
                                        broadcasts.push(format!("[WAR] {} declares war on {}!", my_name, arg));
                                    }
                                }
                            }
                        }
                    }
                }

                "peace" => {
                    if arg.is_empty() { out.push((player.clone(), "[Clan] Usage: !clan peace <clanname>".into())); }
                    else {
                        let my_clan_key = state.player_clan.get(&steam).cloned();
                        match my_clan_key {
                            None => out.push((player.clone(), "[Clan] Not in a clan.".into())),
                            Some(my_clan_key) => {
                                let target_key = arg.to_lowercase();
                                if let Some(clan) = state.clans.get_mut(&my_clan_key) {
                                    clan.wars.retain(|w| *w != target_key);
                                }
                                if let Some(target) = state.clans.get_mut(&target_key) {
                                    target.wars.retain(|w| *w != my_clan_key);
                                }
                                save(&state);
                                broadcasts.push(format!("[PEACE] War between {} and {} is over!", my_clan_key, arg));
                            }
                        }
                    }
                }

                "list" => {
                    if state.clans.is_empty() {
                        out.push((player.clone(), "[Clan] No clans exist yet.".into()));
                    } else {
                        for clan in state.clans.values() {
                            out.push((player.clone(), format!("[{}] {} - {} members (leader: {})",
                                clan.tag, clan.name, clan.members.len(), clan.leader_name)));
                        }
                    }
                }

                _ => {
                    out.push((player.clone(), "[Clan] Commands: create/invite/accept/leave/info/war/peace/list".into()));
                }
            }
        } // lock dropped here

        for (rcpt, msg) in &out { reply(msg, rcpt).await; }
        for msg in &broadcasts { broadcast_msg(msg).await; }
        Outcome::Handled
    }
}
