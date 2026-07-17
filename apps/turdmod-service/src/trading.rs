// Trading post - player-to-player trades with economy confirmation.
// !trade <player> <amount> - offer coins for trade
// !accept - accept pending trade offer
// Also tracks trade history for reputation bonuses.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const RATE_LIMIT: Duration = Duration::from_secs(3);
const TRADE_TIMEOUT: Duration = Duration::from_secs(30);
const ECON_PATH: &str = r"C:\TurdMOD\data\economy.json";

#[derive(Clone)]
struct TradeOffer {
    from_name: String,
    from_steam: String,
    amount: i64,
    created: Instant,
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

fn transfer(from_steam: &str, to_steam: &str, amount: i64) -> Result<(i64, i64), &'static str> {
    let data = std::fs::read_to_string(ECON_PATH).map_err(|_| "no economy data")?;
    let mut state: serde_json::Value = serde_json::from_str(&data).map_err(|_| "bad json")?;

    let from_bal = state.get("players").and_then(|p| p.get(from_steam))
        .and_then(|p| p.get("balance")).and_then(|b| b.as_i64()).unwrap_or(0);
    if from_bal < amount { return Err("insufficient funds"); }

    let to_bal = state.get("players").and_then(|p| p.get(to_steam))
        .and_then(|p| p.get("balance")).and_then(|b| b.as_i64()).unwrap_or(0);

    state["players"][from_steam]["balance"] = serde_json::json!(from_bal - amount);
    state["players"][to_steam]["balance"] = serde_json::json!(to_bal + amount);

    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(&state) {
        let tmp = format!("{}.tmp", ECON_PATH);
        if std::fs::write(&tmp, &json).is_ok() { let _ = std::fs::rename(&tmp, ECON_PATH); }
    }
    Ok((from_bal - amount, to_bal + amount))
}

pub struct Trading {
    rate: Mutex<HashMap<String, Instant>>,
    offers: Mutex<HashMap<String, TradeOffer>>, // target_name_lc -> offer
}
impl Trading {
    pub fn new() -> Self {
        Self {
            rate: Mutex::new(HashMap::new()),
            offers: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait::async_trait]
impl Mod for Trading {
    fn name(&self) -> &'static str { "trading" }
    fn commands(&self) -> &'static [&'static str] { &["!trade", "!accept", "!reject"] }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        if !text.starts_with('!') { return Outcome::Ignored; }

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

        // Expire old offers under lock before acting
        {
            let mut offers = self.offers.lock().await;
            offers.retain(|_, o| o.created.elapsed() < TRADE_TIMEOUT);
        }

        let parts: Vec<&str> = text.split_whitespace().collect();
        let cmd = parts[0].to_lowercase();

        if !matches!(cmd.as_str(), "!trade" | "!accept" | "!reject") { return Outcome::Ignored; }

        match cmd.as_str() {
            "!trade" => {
                if parts.len() < 3 {
                    reply("[Trade] Usage: !trade <player> <amount>", &player).await;
                    return Outcome::Handled;
                }
                let target = parts[1].to_string();
                let amount: i64 = match parts[2].parse() {
                    Ok(n) if n > 0 => n,
                    _ => { reply("[Trade] Amount must be positive", &player).await; return Outcome::Handled; }
                };
                if target.eq_ignore_ascii_case(&player) {
                    reply("[Trade] Can't trade with yourself.", &player).await;
                    return Outcome::Handled;
                }
                let offer = TradeOffer {
                    from_name: player.clone(),
                    from_steam: steam.clone(),
                    amount,
                    created: Instant::now(),
                };
                let msg_from = format!("[Trade] Offer sent to {}: {} coins (30s to accept)", target, amount);
                let msg_to = format!("[Trade] {} offers you {} coins. !accept to take it", player, amount);
                let target_key = target.to_lowercase();
                {
                    let mut offers = self.offers.lock().await;
                    offers.insert(target_key, offer);
                }
                reply(&msg_from, &player).await;
                reply(&msg_to, &target).await;
                Outcome::Handled
            }

            "!accept" => {
                let key = player.to_lowercase();
                let offer = {
                    let mut offers = self.offers.lock().await;
                    offers.remove(&key)
                };
                let Some(offer) = offer else {
                    reply("[Trade] No pending offers.", &player).await;
                    return Outcome::Handled;
                };
                match transfer(&offer.from_steam, &steam, offer.amount) {
                    Ok((from_bal, to_bal)) => {
                        reply(&format!("[Trade] Received {} coins from {}! Balance: {}", offer.amount, offer.from_name, to_bal), &player).await;
                        reply(&format!("[Trade] {} accepted! -{} coins. Balance: {}", player, offer.amount, from_bal), &offer.from_name).await;
                    }
                    Err(e) => {
                        reply(&format!("[Trade] Failed: {}", e), &player).await;
                        reply(&format!("[Trade] Trade to {} failed: {}", player, e), &offer.from_name).await;
                    }
                }
                Outcome::Handled
            }

            "!reject" => {
                let key = player.to_lowercase();
                let offer = {
                    let mut offers = self.offers.lock().await;
                    offers.remove(&key)
                };
                if let Some(offer) = offer {
                    reply("[Trade] Offer rejected.", &player).await;
                    reply(&format!("[Trade] {} rejected your offer.", player), &offer.from_name).await;
                }
                Outcome::Handled
            }

            _ => Outcome::Ignored,
        }
    }
}
