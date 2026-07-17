// "MOD By Zilla" - Offline base protection
// State: C:\TurdMOD\data\zilla_protection.json

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const OWNER_STEAM_ID: &str = "YOUR_STEAM_ID_1";
const OWNER_NAME: &str = "YOUR_OWNER_NAME";
const STATE_PATH: &str = r"C:\TurdMOD\data\zilla_protection.json";
const RATE_LIMIT: Duration = Duration::from_secs(3);
const CHECK_INTERVAL: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PlayerRec { name: String, last_seen: u64, is_protected: bool, sessions: u64 }

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct ZillaState {
    protection_hours: u64,
    players: HashMap<String, PlayerRec>,
}

impl Default for ZillaState {
    fn default() -> Self { Self { protection_hours: 48, players: HashMap::new() } }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs()
}
fn hours_since(ts: u64) -> f64 { now_secs().saturating_sub(ts) as f64 / 3600.0 }

fn load() -> ZillaState {
    std::fs::read_to_string(STATE_PATH).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}
fn save(st: &ZillaState) {
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(st) {
        let tmp = format!("{}.tmp", STATE_PATH);
        if std::fs::write(&tmp, &json).is_ok() { let _ = std::fs::rename(&tmp, STATE_PATH); }
    }
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

async fn broadcast_orange(msg: &str) {
    let params = serde_json::json!({ "text": msg, "channel": "6" });
    pipe_rpc::call("broadcastChat", Some(params)).await.ok();
}

fn is_owner(steam: &str, player: &str) -> bool {
    steam == OWNER_STEAM_ID || steam == "YOUR_STEAM_ID_2" || player == OWNER_NAME
}

pub struct ZillaProtection {
    state: Mutex<ZillaState>,
    rate: Mutex<HashMap<String, Instant>>,
}

impl ZillaProtection {
    pub fn new() -> Self {
        Self { state: Mutex::new(load()), rate: Mutex::new(HashMap::new()) }
    }
}

#[async_trait::async_trait]
impl Mod for ZillaProtection {
    fn name(&self) -> &'static str { "zilla_protection" }
    // event-driven: needs login + logout events plus !protect/!protectinfo/!protectwindow chat
    fn interval(&self) -> Option<Duration> { Some(CHECK_INTERVAL) }

    // Expire protection for long-offline players (the old select! sweep branch).
    async fn tick(&self, _ctx: &ModCtx) {
        let (expired_names, threshold) = {
            let mut st = self.state.lock().await;
            let threshold = st.protection_hours;
            let mut expired = Vec::new();
            for rec in st.players.values_mut() {
                if rec.is_protected && hours_since(rec.last_seen) > threshold as f64 {
                    rec.is_protected = false;
                    expired.push(rec.name.clone());
                }
            }
            if !expired.is_empty() { save(&st); }
            (expired, threshold)
        };
        for name in &expired_names {
            tracing::info!("zilla: protection expired for {}", name);
            broadcast_orange(&format!("[ZBase] {}'s base protection has EXPIRED (offline >{}h)", name, threshold)).await;
        }
    }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        match ev.event.as_str() {
            "login" => {
                let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if player.is_empty() { return Outcome::Ignored; }
                let key = if steam.is_empty() { player.clone() } else { steam };

                let (was_expired, reply_msg, bcast_msg) = {
                    let mut st = self.state.lock().await;
                    let rec = st.players.entry(key).or_insert_with(|| PlayerRec {
                        name: player.clone(), last_seen: now_secs(), is_protected: true, sessions: 0
                    });
                    rec.name = player.clone();
                    rec.last_seen = now_secs();
                    rec.sessions += 1;
                    let was_expired = !rec.is_protected;
                    rec.is_protected = true;
                    save(&st);
                    let bcast = if was_expired {
                        Some(format!("[ZBase] {}'s base protection RESTORED.", player))
                    } else {
                        None
                    };
                    (was_expired, "[ZBase] Your base is protected.".to_string(), bcast)
                };
                reply(&reply_msg, &player).await;
                if let Some(msg) = bcast_msg { broadcast_orange(&msg).await; }
                Outcome::Handled
            }

            "logout" => {
                let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if player.is_empty() { return Outcome::Ignored; }
                let key = if steam.is_empty() { player.clone() } else { steam };
                {
                    let mut st = self.state.lock().await;
                    let rec = st.players.entry(key).or_insert_with(|| PlayerRec {
                        name: player.clone(), last_seen: now_secs(), is_protected: true, sessions: 0
                    });
                    rec.name = player.clone();
                    rec.last_seen = now_secs();
                    save(&st);
                }
                Outcome::Handled
            }

            "chat" => {
                let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
                if !text.starts_with('!') { return Outcome::Ignored; }
                let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if player.is_empty() { return Outcome::Ignored; }

                let rk = if steam.is_empty() { player.clone() } else { steam.clone() };
                {
                    let mut rate = self.rate.lock().await;
                    let now = Instant::now();
                    if let Some(prev) = rate.get(&rk) {
                        if now.duration_since(*prev) < RATE_LIMIT { return Outcome::Ignored; }
                    }
                    rate.insert(rk.clone(), now);
                }

                let parts: Vec<&str> = text.splitn(3, ' ').collect();
                let cmd = parts[0].to_lowercase();

                match cmd.as_str() {
                    "!protect" => {
                        let msg = {
                            let st = self.state.lock().await;
                            match st.players.get(&rk) {
                                Some(rec) => {
                                    let h = hours_since(rec.last_seen);
                                    let status = if rec.is_protected { "PROTECTED" } else { "NOT PROTECTED" };
                                    format!("[ZBase] Your base: {} | Last seen: {:.1}h ago | Window: {}h", status, h, st.protection_hours)
                                }
                                None => "[ZBase] No record yet - protection starts on logout.".to_string(),
                            }
                        };
                        reply(&msg, &player).await;
                        Outcome::Handled
                    }
                    "!protectinfo" => {
                        if !is_owner(&steam, &player) {
                            reply("[ZBase] Owner only.", &player).await;
                            return Outcome::Handled;
                        }
                        let target = parts.get(1).copied().unwrap_or("");
                        if target.is_empty() {
                            reply("[ZBase] Usage: !protectinfo <player>", &player).await;
                            return Outcome::Handled;
                        }
                        let msg = {
                            let st = self.state.lock().await;
                            let found = st.players.values().find(|r| r.name.eq_ignore_ascii_case(target));
                            match found {
                                Some(rec) => {
                                    let status = if rec.is_protected { "PROTECTED" } else { "EXPIRED" };
                                    format!("[ZBase] {} - {} | {:.1}h ago | {} sessions", rec.name, status, hours_since(rec.last_seen), rec.sessions)
                                }
                                None => format!("[ZBase] '{}' not found", target),
                            }
                        };
                        reply(&msg, &player).await;
                        Outcome::Handled
                    }
                    "!protectwindow" => {
                        if !is_owner(&steam, &player) {
                            reply("[ZBase] Owner only.", &player).await;
                            return Outcome::Handled;
                        }
                        let h_str = parts.get(1).copied().unwrap_or("");
                        let msg = match h_str.parse::<u64>() {
                            Ok(h) if h >= 1 && h <= 720 => {
                                let mut st = self.state.lock().await;
                                st.protection_hours = h;
                                save(&st);
                                format!("[ZBase] Protection window set to {}h", h)
                            }
                            _ => {
                                let st = self.state.lock().await;
                                format!("[ZBase] Current: {}h. Usage: !protectwindow <1-720>", st.protection_hours)
                            }
                        };
                        reply(&msg, &player).await;
                        Outcome::Handled
                    }
                    _ => Outcome::Ignored,
                }
            }

            _ => Outcome::Ignored,
        }
    }
}
