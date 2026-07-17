// Banking - deposit/withdraw with interest. Separate from wallet balance.
// !deposit <amount> / !withdraw <amount> / !bank. Interest accrues 1%/hr on the interval tick.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const STATE_PATH: &str = r"C:\TurdMOD\data\banking.json";
const ECON_PATH: &str = r"C:\TurdMOD\data\economy.json";
const RATE_LIMIT: Duration = Duration::from_secs(3);
const INTEREST_INTERVAL: Duration = Duration::from_secs(3600); // 1 hour
const INTEREST_RATE: f64 = 0.01; // 1% per hour
const MAX_BANK_BALANCE: i64 = 100000;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
struct BankAccount {
    name: String,
    balance: i64,
    last_interest_ts: u64,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
struct BankState {
    accounts: HashMap<String, BankAccount>,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_secs()
}

fn load() -> BankState {
    std::fs::read_to_string(STATE_PATH).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(state: &BankState) {
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let tmp = format!("{}.tmp", STATE_PATH);
        if std::fs::write(&tmp, &json).is_ok() { let _ = std::fs::rename(&tmp, STATE_PATH); }
    }
}

fn wallet_balance(steam: &str) -> i64 {
    let Ok(data) = std::fs::read_to_string(ECON_PATH) else { return 0 };
    let Ok(state) = serde_json::from_str::<serde_json::Value>(&data) else { return 0 };
    state.get("players").and_then(|p| p.get(steam))
        .and_then(|p| p.get("balance")).and_then(|b| b.as_i64()).unwrap_or(0)
}

fn wallet_adjust(steam: &str, delta: i64) {
    let Ok(data) = std::fs::read_to_string(ECON_PATH) else { return };
    let Ok(mut state) = serde_json::from_str::<serde_json::Value>(&data) else { return };
    let bal = state.get("players").and_then(|p| p.get(steam))
        .and_then(|p| p.get("balance")).and_then(|b| b.as_i64()).unwrap_or(0);
    state["players"][steam]["balance"] = serde_json::json!(bal + delta);
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(&state) {
        let tmp = format!("{}.tmp", ECON_PATH);
        if std::fs::write(&tmp, &json).is_ok() { let _ = std::fs::rename(&tmp, ECON_PATH); }
    }
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

pub struct Banking { state: Mutex<BankState>, rate: Mutex<HashMap<String, Instant>> }
impl Banking {
    pub fn new() -> Self { Self { state: Mutex::new(load()), rate: Mutex::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl Mod for Banking {
    fn name(&self) -> &'static str { "banking" }
    fn commands(&self) -> &'static [&'static str] { &["!bank", "!deposit", "!withdraw"] }
    fn interval(&self) -> Option<Duration> { Some(INTEREST_INTERVAL) }

    // Accrue 1%/hr interest (the old select! interest branch).
    async fn tick(&self, _ctx: &ModCtx) {
        let now = now_secs();
        let mut st = self.state.lock().await;
        for (_, acc) in st.accounts.iter_mut() {
            if acc.balance > 0 {
                let interest = ((acc.balance as f64 * INTEREST_RATE) as i64).max(1);
                acc.balance = (acc.balance + interest).min(MAX_BANK_BALANCE);
                acc.last_interest_ts = now;
            }
        }
        save(&st);
    }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        if !text.starts_with('!') { return Outcome::Ignored; }
        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let (cmd, args) = match text.find(' ') {
            Some(i) => (text[..i].to_lowercase(), text[i + 1..].trim().to_string()),
            None => (text.to_lowercase(), String::new()),
        };
        if !matches!(cmd.as_str(), "!bank" | "!deposit" | "!withdraw") { return Outcome::Ignored; }

        let rate_key = if steam.is_empty() { player.clone() } else { steam.clone() };
        {
            let mut rate = self.rate.lock().await;
            let now = Instant::now();
            if let Some(prev) = rate.get(&rate_key) {
                if now.duration_since(*prev) < RATE_LIMIT { return Outcome::Ignored; }
            }
            rate.insert(rate_key.clone(), now);
        }

        match cmd.as_str() {
            "!bank" => {
                let bank_bal = { self.state.lock().await.accounts.get(&steam).map(|a| a.balance).unwrap_or(0) };
                let wallet = wallet_balance(&steam);
                reply(&format!("[Bank] Wallet: {} | Bank: {} | Interest: 1%/hr (max {})", wallet, bank_bal, MAX_BANK_BALANCE), &player).await;
                Outcome::Handled
            }
            "!deposit" => {
                let amount: i64 = args.parse().unwrap_or(0);
                if amount <= 0 { reply("[Bank] Usage: !deposit <amount>", &player).await; return Outcome::Handled; }
                let wallet = wallet_balance(&steam);
                if wallet < amount { reply(&format!("[Bank] Wallet only has {}", wallet), &player).await; return Outcome::Handled; }
                let msg = {
                    let mut st = self.state.lock().await;
                    let acc = st.accounts.entry(steam.clone()).or_insert_with(|| BankAccount { name: player.clone(), balance: 0, last_interest_ts: now_secs() });
                    if acc.balance + amount > MAX_BANK_BALANCE {
                        let room = MAX_BANK_BALANCE - acc.balance;
                        format!("[Bank] Would exceed max ({}). Can deposit {}", MAX_BANK_BALANCE, room)
                    } else {
                        acc.balance += amount;
                        let new_bal = acc.balance;
                        wallet_adjust(&steam, -amount);
                        save(&st);
                        format!("[Bank] Deposited {}. Bank: {} | Wallet: {}", amount, new_bal, wallet - amount)
                    }
                };
                reply(&msg, &player).await;
                Outcome::Handled
            }
            "!withdraw" => {
                let amount: i64 = args.parse().unwrap_or(0);
                if amount <= 0 { reply("[Bank] Usage: !withdraw <amount>", &player).await; return Outcome::Handled; }
                let msg = {
                    let mut st = self.state.lock().await;
                    let acc = st.accounts.entry(steam.clone()).or_insert_with(|| BankAccount { name: player.clone(), balance: 0, last_interest_ts: now_secs() });
                    if acc.balance < amount {
                        format!("[Bank] Bank only has {}", acc.balance)
                    } else {
                        acc.balance -= amount;
                        let new_bal = acc.balance;
                        wallet_adjust(&steam, amount);
                        save(&st);
                        format!("[Bank] Withdrew {}. Bank: {} | Wallet: {}", amount, new_bal, wallet_balance(&steam))
                    }
                };
                reply(&msg, &player).await;
                Outcome::Handled
            }
            _ => Outcome::Ignored,
        }
    }
}
