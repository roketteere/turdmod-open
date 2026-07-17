// Gambling - !coinflip/!flip, !dice/!roll, !slots. Uses economy.json balances. House edge on slots.
// Command-only; balances live on disk so the only in-memory state is the per-player rate limit.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const STATE_PATH: &str = r"C:\TurdMOD\data\economy.json";
const RATE_LIMIT: Duration = Duration::from_secs(5);

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

fn load_balance(steam: &str) -> i64 {
    let Ok(data) = std::fs::read_to_string(STATE_PATH) else { return 0 };
    let Ok(state) = serde_json::from_str::<serde_json::Value>(&data) else { return 0 };
    state.get("players").and_then(|p| p.get(steam))
        .and_then(|p| p.get("balance")).and_then(|b| b.as_i64()).unwrap_or(0)
}

fn update_balance(steam: &str, delta: i64) -> i64 {
    let Ok(data) = std::fs::read_to_string(STATE_PATH) else { return 0 };
    let Ok(mut state) = serde_json::from_str::<serde_json::Value>(&data) else { return 0 };
    let bal = state.get_mut("players")
        .and_then(|p| p.get_mut(steam))
        .and_then(|p| p.get_mut("balance"))
        .and_then(|b| b.as_i64());
    let Some(old) = bal else { return 0 };
    let new_bal = old + delta;
    state["players"][steam]["balance"] = serde_json::json!(new_bal);
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(&state) {
        let tmp = format!("{}.tmp", STATE_PATH);
        if std::fs::write(&tmp, &json).is_ok() {
            let _ = std::fs::rename(&tmp, STATE_PATH);
        }
    }
    new_bal
}

fn rand_u32() -> u32 {
    use std::time::SystemTime;
    let seed = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default().as_nanos() as u32;
    seed.wrapping_mul(1103515245).wrapping_add(12345) >> 16
}

pub struct Gambling { rate: Mutex<HashMap<String, Instant>> }
impl Gambling {
    pub fn new() -> Self { Self { rate: Mutex::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl Mod for Gambling {
    fn name(&self) -> &'static str { "gambling" }
    fn commands(&self) -> &'static [&'static str] { &["!coinflip", "!flip", "!dice", "!roll", "!slots"] }

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
        if !matches!(cmd.as_str(), "!coinflip" | "!flip" | "!dice" | "!roll" | "!slots") { return Outcome::Ignored; }

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
            "!coinflip" | "!flip" => {
                let bet: i64 = args.parse().unwrap_or(10);
                if bet < 1 { reply("[Casino] Min bet: 1", &player).await; return Outcome::Handled; }
                let bal = load_balance(&steam);
                if bal < bet { reply(&format!("[Casino] Not enough coins ({})", bal), &player).await; return Outcome::Handled; }
                if rand_u32() % 2 == 0 {
                    let new_bal = update_balance(&steam, bet);
                    reply(&format!("[Casino] HEADS! +{} coins (bal: {})", bet, new_bal), &player).await;
                } else {
                    let new_bal = update_balance(&steam, -bet);
                    reply(&format!("[Casino] TAILS! -{} coins (bal: {})", bet, new_bal), &player).await;
                }
                Outcome::Handled
            }
            "!dice" | "!roll" => {
                let bet: i64 = args.parse().unwrap_or(10);
                if bet < 1 { reply("[Casino] Min bet: 1", &player).await; return Outcome::Handled; }
                let bal = load_balance(&steam);
                if bal < bet { reply(&format!("[Casino] Not enough coins ({})", bal), &player).await; return Outcome::Handled; }
                let player_roll = (rand_u32() % 6) + 1;
                let house_roll = (rand_u32() % 6) + 1;
                if player_roll > house_roll {
                    let new_bal = update_balance(&steam, bet);
                    reply(&format!("[Casino] You:{} House:{} - WIN +{} (bal: {})", player_roll, house_roll, bet, new_bal), &player).await;
                } else if player_roll < house_roll {
                    let new_bal = update_balance(&steam, -bet);
                    reply(&format!("[Casino] You:{} House:{} - LOSE -{} (bal: {})", player_roll, house_roll, bet, new_bal), &player).await;
                } else {
                    reply(&format!("[Casino] You:{} House:{} - TIE! No change", player_roll, house_roll), &player).await;
                }
                Outcome::Handled
            }
            "!slots" => {
                let bet: i64 = args.parse().unwrap_or(10);
                if bet < 5 { reply("[Casino] Slots min bet: 5", &player).await; return Outcome::Handled; }
                let bal = load_balance(&steam);
                if bal < bet { reply(&format!("[Casino] Not enough coins ({})", bal), &player).await; return Outcome::Handled; }
                let symbols = ["7", "BAR", "CHR", "LMN", "BEL", "GRP"];
                let r1 = symbols[(rand_u32() as usize) % symbols.len()];
                let r2 = symbols[(rand_u32().wrapping_add(7) as usize) % symbols.len()];
                let r3 = symbols[(rand_u32().wrapping_add(13) as usize) % symbols.len()];
                let payout = if r1 == r2 && r2 == r3 {
                    if r1 == "7" { bet * 10 } else { bet * 5 }
                } else if r1 == r2 || r2 == r3 { bet } else { -bet };
                let new_bal = update_balance(&steam, payout);
                let result = if payout > 0 { format!("WIN +{}", payout) } else { format!("LOSE {}", payout) };
                reply(&format!("[Slots] [{}|{}|{}] {} (bal: {})", r1, r2, r3, result, new_bal), &player).await;
                Outcome::Handled
            }
            _ => Outcome::Ignored,
        }
    }
}
