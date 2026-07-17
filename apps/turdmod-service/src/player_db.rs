// Player database - Steam ID -> identity, aliases, session history, playtime.
// State: C:\TurdMOD\data\players.json

use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const DB_PATH: &str = r"C:\TurdMOD\data\players.json";
const RATE_LIMIT: Duration = Duration::from_secs(3);
const ADMIN_IDS: &[&str] = &["YOUR_STEAM_ID_1"];

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PlayerRecord {
    names: Vec<String>,
    first_seen: u64,
    last_seen: u64,
    last_login: u64,
    sessions: u64,
    total_playtime_secs: u64,
    is_admin: bool,
    is_online: bool,
}

impl PlayerRecord {
    fn new(name: &str, now: u64) -> Self {
        Self { names: vec![name.into()], first_seen: now, last_seen: now, last_login: now, sessions: 0, total_playtime_secs: 0, is_admin: false, is_online: false }
    }
    fn add_name(&mut self, name: &str) {
        if !self.names.iter().any(|n| n == name) { self.names.push(name.into()); }
    }
    fn primary(&self) -> &str { self.names.first().map(|s| s.as_str()).unwrap_or("unknown") }
    fn playtime_fmt(&self) -> String { format!("{}h {}m", self.total_playtime_secs / 3600, (self.total_playtime_secs % 3600) / 60) }
}

fn now_secs() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() }

fn load() -> HashMap<String, PlayerRecord> {
    std::fs::read_to_string(DB_PATH).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save(db: &HashMap<String, PlayerRecord>) {
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(db) {
        let tmp = format!("{}.tmp", DB_PATH);
        if std::fs::write(&tmp, &json).is_ok() { let _ = std::fs::rename(&tmp, DB_PATH); }
    }
}

pub fn is_admin(steam_id: &str) -> bool { ADMIN_IDS.contains(&steam_id) }

#[derive(serde::Serialize)]
pub struct PlayerSummary {
    pub name: String,
    pub steam: String,
    pub online: bool,
    pub sessions: u64,
    pub playtime_secs: u64,
    pub last_seen: u64,
    pub is_admin: bool,
}

/// Roster for the Live Monitor - every known player + metrics. `online_steam` = currently-live
/// steamids from the bridge (authoritative over the stored is_online flag). Online first, then most
/// recently seen.
pub fn roster(online_steam: &std::collections::HashSet<String>) -> Vec<PlayerSummary> {
    let mut out: Vec<PlayerSummary> = load().into_iter().map(|(steam, r)| {
        let name = r.primary().to_string();
        let online = online_steam.contains(&steam) || r.is_online;
        PlayerSummary {
            name, steam, online,
            sessions: r.sessions,
            playtime_secs: r.total_playtime_secs,
            last_seen: r.last_seen,
            is_admin: r.is_admin,
        }
    }).collect();
    out.sort_by(|a, b| b.online.cmp(&a.online).then(b.last_seen.cmp(&a.last_seen)));
    out
}

pub fn resolve_steam_id(name: &str) -> Option<String> {
    let db = load();
    let needle = name.to_lowercase();
    db.into_iter().find_map(|(sid, rec)| {
        if rec.names.iter().any(|n| n.to_lowercase() == needle) { Some(sid) } else { None }
    })
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

pub struct PlayerDb {
    db: Mutex<HashMap<String, PlayerRecord>>,
    sessions: Mutex<HashMap<String, Instant>>,
    rate: Mutex<HashMap<String, Instant>>,
}

impl PlayerDb {
    pub fn new() -> Self {
        let mut db = load();
        // Clear stale online flags from prior crash
        for rec in db.values_mut() { rec.is_online = false; }
        save(&db);
        tracing::info!("player_db: {} records loaded", db.len());
        Self {
            db: Mutex::new(db),
            sessions: Mutex::new(HashMap::new()),
            rate: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait::async_trait]
impl Mod for PlayerDb {
    fn name(&self) -> &'static str { "player_db" }
    // event-driven: handles login/logout/chat
    fn commands(&self) -> &'static [&'static str] { &[] }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        match ev.event.as_str() {
            "login" => {
                let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if steam.is_empty() || player.is_empty() { return Outcome::Ignored; }

                let now = now_secs();
                let sess = {
                    let mut sessions = self.sessions.lock().await;
                    sessions.insert(steam.clone(), Instant::now());
                    let mut db = self.db.lock().await;
                    let rec = db.entry(steam.clone()).or_insert_with(|| PlayerRecord::new(&player, now));
                    rec.add_name(&player);
                    rec.last_login = now;
                    rec.last_seen = now;
                    rec.sessions += 1;
                    rec.is_online = true;
                    rec.is_admin = is_admin(&steam);
                    let s = rec.sessions;
                    save(&db);
                    s
                };
                tracing::info!("player_db: login {} ({}) session #{}", player, steam, sess);
                Outcome::Handled
            }

            "logout" => {
                let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();

                let now = now_secs();
                let elapsed = {
                    let mut sessions = self.sessions.lock().await;
                    sessions.remove(&steam).map(|s| s.elapsed().as_secs()).unwrap_or(0)
                };
                {
                    let mut db = self.db.lock().await;
                    let rec = db.entry(steam.clone()).or_insert_with(|| PlayerRecord::new(&player, now));
                    rec.add_name(&player);
                    rec.last_seen = now;
                    rec.is_online = false;
                    rec.total_playtime_secs += elapsed;
                    save(&db);
                }
                tracing::info!("player_db: logout {} ({})", player, steam);
                Outcome::Handled
            }

            "chat" => {
                let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
                if !text.starts_with('!') { return Outcome::Ignored; }

                let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if player.is_empty() { return Outcome::Ignored; }

                {
                    let mut rate = self.rate.lock().await;
                    let now = Instant::now();
                    if let Some(prev) = rate.get(&player) {
                        if now.duration_since(*prev) < RATE_LIMIT { return Outcome::Ignored; }
                    }
                    rate.insert(player.clone(), now);
                }

                let (cmd, args) = match text.find(' ') {
                    Some(i) => (text[..i].to_lowercase(), text[i + 1..].trim().to_string()),
                    None => (text.to_lowercase(), String::new()),
                };

                match cmd.as_str() {
                    "!profile" => {
                        let msg = {
                            let db = self.db.lock().await;
                            let key: String = if args.is_empty() {
                                steam.clone()
                            } else {
                                let needle = args.to_lowercase();
                                match db.iter().find(|(_, r)| r.names.iter().any(|n| n.to_lowercase() == needle)) {
                                    Some((k, _)) => k.clone(),
                                    None => {
                                        drop(db);
                                        reply(&format!("[Profile] '{}' not found", args), &player).await;
                                        return Outcome::Handled;
                                    }
                                }
                            };
                            if let Some(rec) = db.get(&key) {
                                let status = if rec.is_online { "ONLINE" } else { "offline" };
                                let aliases = if rec.names.len() > 1 { format!(" | aka: {}", rec.names[1..].join(", ")) } else { String::new() };
                                format!("[Profile] {} ({}) | Sessions: {} | Playtime: {}{}", rec.primary(), status, rec.sessions, rec.playtime_fmt(), aliases)
                            } else {
                                return Outcome::Ignored;
                            }
                        };
                        reply(&msg, &player).await;
                        Outcome::Handled
                    }

                    "!whois" => {
                        if args.is_empty() {
                            reply("[Whois] Usage: !whois <name>", &player).await;
                            return Outcome::Handled;
                        }
                        let msg = {
                            let db = self.db.lock().await;
                            let needle = args.to_lowercase();
                            let matches: Vec<String> = db.iter()
                                .filter(|(_, r)| r.names.iter().any(|n| n.to_lowercase().contains(&needle)))
                                .map(|(_, r)| {
                                    let status = if r.is_online { "ON" } else { "off" };
                                    format!("{} [{}]", r.primary(), status)
                                }).collect();
                            if matches.is_empty() {
                                format!("[Whois] No match for '{}'", args)
                            } else {
                                format!("[Whois] {}: {}", matches.len(), matches.join(", "))
                            }
                        };
                        reply(&msg, &player).await;
                        Outcome::Handled
                    }

                    "!online" => {
                        let msg = {
                            let db = self.db.lock().await;
                            let online: Vec<String> = db.values()
                                .filter(|r| r.is_online)
                                .map(|r| format!("{} ({})", r.primary(), r.playtime_fmt()))
                                .collect();
                            if online.is_empty() {
                                "[Online] Nobody online".to_string()
                            } else {
                                format!("[Online] {}: {}", online.len(), online.join(", "))
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
