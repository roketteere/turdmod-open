// In-game currency system - !balance !pay !daily !top !bounty
// State: C:\TurdMOD\data\economy.json (atomic write)

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const OWNER_STEAM_ID: &str = "YOUR_STEAM_ID_1";
const STATE_PATH: &str = r"C:\TurdMOD\data\economy.json";
const DAILY_COINS: i64 = 100;
const MIN_TRANSFER: i64 = 10;
const RATE_LIMIT: Duration = Duration::from_secs(3);

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
struct PlayerEcon {
    name: String,
    balance: i64,
    last_daily: Option<String>,
    total_earned: i64,
    total_spent: i64,
}

impl PlayerEcon {
    fn new(name: &str) -> Self {
        Self { name: name.into(), balance: 0, last_daily: None, total_earned: 0, total_spent: 0 }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
struct Bounty {
    target: String,
    amount: i64,
    placed_by: String,
    ts: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
struct EconState {
    players: HashMap<String, PlayerEcon>,
    bounties: Vec<Bounty>,
}

fn load() -> EconState {
    std::fs::read_to_string(STATE_PATH)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(state: &EconState) {
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let tmp = format!("{}.tmp", STATE_PATH);
        if std::fs::write(&tmp, &json).is_ok() {
            let _ = std::fs::rename(&tmp, STATE_PATH);
        }
    }
}

fn now_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{}", secs)
}

fn daily_eligible(last: &Option<String>) -> bool {
    let Some(last_s) = last else { return true };
    let Ok(last_secs) = last_s.parse::<u64>() else { return true };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    now - last_secs >= 86400
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

pub struct Economy {
    state: Mutex<EconState>,
    rate: Mutex<HashMap<String, Instant>>,
}

impl Economy {
    pub fn new() -> Self {
        Self { state: Mutex::new(load()), rate: Mutex::new(HashMap::new()) }
    }
}

#[async_trait::async_trait]
impl Mod for Economy {
    fn name(&self) -> &'static str { "economy" }
    fn commands(&self) -> &'static [&'static str] {
        &["!balance", "!daily", "!pay", "!top", "!bounty"]
    }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        if !text.starts_with('!') { return Outcome::Ignored; }

        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if player.is_empty() { return Outcome::Ignored; }

        let rate_key = if steam.is_empty() { player.clone() } else { steam.clone() };
        {
            let mut rate = self.rate.lock().await;
            let now = Instant::now();
            if let Some(prev) = rate.get(&rate_key) {
                if now.duration_since(*prev) < RATE_LIMIT { return Outcome::Ignored; }
            }
            rate.insert(rate_key.clone(), now);
        }

        let parts: Vec<&str> = text.split_whitespace().collect();
        let cmd = parts[0].to_lowercase();

        match cmd.as_str() {
            "!balance" => {
                let mut st = self.state.lock().await;
                let e = st.players.entry(steam.clone()).or_insert_with(|| PlayerEcon::new(&player));
                e.name = player.clone();
                let msg = format!("[Economy] Balance: {} coins", e.balance);
                drop(st);
                reply(&msg, &player).await;
                Outcome::Handled
            }

            "!daily" => {
                let msg = {
                    let mut st = self.state.lock().await;
                    let e = st.players.entry(steam.clone()).or_insert_with(|| PlayerEcon::new(&player));
                    e.name = player.clone();
                    if daily_eligible(&e.last_daily) {
                        e.balance += DAILY_COINS;
                        e.total_earned += DAILY_COINS;
                        e.last_daily = Some(now_iso());
                        let bal = e.balance;
                        save(&st);
                        format!("[Economy] Daily bonus: +{} coins. Balance: {}", DAILY_COINS, bal)
                    } else {
                        "[Economy] Daily already claimed. Try again later.".to_string()
                    }
                };
                reply(&msg, &player).await;
                Outcome::Handled
            }

            "!pay" => {
                if parts.len() < 3 {
                    reply("[Economy] Usage: !pay <player> <amount>", &player).await;
                    return Outcome::Handled;
                }
                let target_name = parts[1].to_string();
                let amount: i64 = match parts[2].parse() {
                    Ok(n) if n >= MIN_TRANSFER => n,
                    _ => {
                        reply(&format!("[Economy] Min transfer: {} coins", MIN_TRANSFER), &player).await;
                        return Outcome::Handled;
                    }
                };

                let (msg, recv_msg) = {
                    let mut st = self.state.lock().await;
                    let target_steam = st.players.iter()
                        .find(|(_, p)| p.name.eq_ignore_ascii_case(&target_name))
                        .map(|(sid, _)| sid.clone());

                    let Some(target_sid) = target_steam else {
                        drop(st);
                        reply(&format!("[Economy] Player '{}' not found", target_name), &player).await;
                        return Outcome::Handled;
                    };
                    if target_sid == steam {
                        drop(st);
                        reply("[Economy] Cannot pay yourself", &player).await;
                        return Outcome::Handled;
                    }

                    let sender_bal = {
                        let sender = st.players.entry(steam.clone()).or_insert_with(|| PlayerEcon::new(&player));
                        sender.balance
                    };
                    if sender_bal < amount {
                        drop(st);
                        reply(&format!("[Economy] Insufficient funds: {}", sender_bal), &player).await;
                        return Outcome::Handled;
                    }
                    {
                        let sender = st.players.get_mut(&steam).unwrap();
                        sender.balance -= amount;
                        sender.total_spent += amount;
                    }
                    let recv_name = {
                        let recv = st.players.entry(target_sid).or_insert_with(|| PlayerEcon::new(&target_name));
                        recv.balance += amount;
                        recv.total_earned += amount;
                        recv.name.clone()
                    };
                    save(&st);
                    (
                        format!("[Economy] Sent {} coins to {}", amount, recv_name),
                        (recv_name, format!("[Economy] Received {} coins from {}", amount, player)),
                    )
                };
                reply(&msg, &player).await;
                reply(&recv_msg.1, &recv_msg.0).await;
                Outcome::Handled
            }

            "!top" => {
                let msg = {
                    let st = self.state.lock().await;
                    let mut entries: Vec<(&str, i64)> = st.players.values()
                        .map(|p| (p.name.as_str(), p.balance)).collect();
                    entries.sort_by(|a, b| b.1.cmp(&a.1));
                    entries.truncate(5);
                    if entries.is_empty() {
                        "[Economy] No players yet".to_string()
                    } else {
                        let lines: Vec<String> = entries.iter().enumerate()
                            .map(|(i, (n, b))| format!("{}. {}: {}", i + 1, n, b)).collect();
                        format!("[Economy] Top 5: {}", lines.join(" | "))
                    }
                };
                reply(&msg, &player).await;
                Outcome::Handled
            }

            "!bounty" => {
                if parts.len() < 3 {
                    reply("[Economy] Usage: !bounty <player> <amount>", &player).await;
                    return Outcome::Handled;
                }
                let target = parts[1].to_string();
                let amount: i64 = match parts[2].parse() {
                    Ok(n) if n > 0 => n,
                    _ => {
                        reply("[Economy] Amount must be positive", &player).await;
                        return Outcome::Handled;
                    }
                };
                let msg = {
                    let mut st = self.state.lock().await;
                    let e = st.players.entry(steam.clone()).or_insert_with(|| PlayerEcon::new(&player));
                    if e.balance < amount {
                        let bal = e.balance;
                        drop(st);
                        reply(&format!("[Economy] Insufficient funds: {}", bal), &player).await;
                        return Outcome::Handled;
                    }
                    e.balance -= amount;
                    e.total_spent += amount;
                    st.bounties.push(Bounty {
                        target: target.clone(), amount, placed_by: steam.clone(), ts: now_iso(),
                    });
                    save(&st);
                    format!("[Economy] Bounty of {} coins placed on {}", amount, target)
                };
                reply(&msg, &player).await;
                Outcome::Handled
            }

            _ => Outcome::Ignored,
        }
    }
}
