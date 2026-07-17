// Lottery - periodic draws with economy integration.
// !lottery buy (50 coins) / status. Draw every 30 min on the interval tick; winner takes the pot.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const TICKET_PRICE: i64 = 50;
const DRAW_INTERVAL: Duration = Duration::from_secs(1800); // 30 min
const TICK: Duration = Duration::from_secs(30);
const RATE_LIMIT: Duration = Duration::from_secs(3);
const ECON_PATH: &str = r"C:\TurdMOD\data\economy.json";

#[derive(Clone)]
struct Ticket {
    player: String,
    steam: String,
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

async fn broadcast(msg: &str) {
    let params = serde_json::json!({ "text": msg });
    pipe_rpc::call("broadcastChat", Some(params)).await.ok();
}

fn debit(steam: &str, amount: i64) -> Result<i64, &'static str> {
    let data = std::fs::read_to_string(ECON_PATH).map_err(|_| "no economy data")?;
    let mut state: serde_json::Value = serde_json::from_str(&data).map_err(|_| "bad json")?;
    let bal = state.get("players").and_then(|p| p.get(steam))
        .and_then(|p| p.get("balance")).and_then(|b| b.as_i64()).unwrap_or(0);
    if bal < amount { return Err("insufficient funds"); }
    state["players"][steam]["balance"] = serde_json::json!(bal - amount);
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(&state) {
        let tmp = format!("{}.tmp", ECON_PATH);
        if std::fs::write(&tmp, &json).is_ok() { let _ = std::fs::rename(&tmp, ECON_PATH); }
    }
    Ok(bal - amount)
}

fn credit(steam: &str, amount: i64) {
    let Ok(data) = std::fs::read_to_string(ECON_PATH) else { return };
    let Ok(mut state) = serde_json::from_str::<serde_json::Value>(&data) else { return };
    let bal = state.get("players").and_then(|p| p.get(steam))
        .and_then(|p| p.get("balance")).and_then(|b| b.as_i64()).unwrap_or(0);
    state["players"][steam]["balance"] = serde_json::json!(bal + amount);
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(&state) {
        let tmp = format!("{}.tmp", ECON_PATH);
        if std::fs::write(&tmp, &json).is_ok() { let _ = std::fs::rename(&tmp, ECON_PATH); }
    }
}

fn pick_winner(tickets: &[Ticket]) -> Option<&Ticket> {
    if tickets.is_empty() { return None; }
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_nanos() as usize;
    Some(&tickets[seed % tickets.len()])
}

struct LotteryState {
    tickets: Vec<Ticket>,
    last_draw: Instant,
    round: u32,
}

pub struct Lottery { state: Mutex<LotteryState>, rate: Mutex<HashMap<String, Instant>> }
impl Lottery {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(LotteryState { tickets: Vec::new(), last_draw: Instant::now(), round: 1 }),
            rate: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait::async_trait]
impl Mod for Lottery {
    fn name(&self) -> &'static str { "lottery" }
    fn commands(&self) -> &'static [&'static str] { &["!lottery"] }
    fn interval(&self) -> Option<Duration> { Some(TICK) }

    // Draw when the round timer elapses (the old top-of-loop draw check).
    async fn tick(&self, _ctx: &ModCtx) {
        let announce: Vec<String> = {
            let mut st = self.state.lock().await;
            let mut msgs = Vec::new();
            if st.last_draw.elapsed() > DRAW_INTERVAL {
                if !st.tickets.is_empty() {
                    let n = st.tickets.len();
                    let pot = n as i64 * TICKET_PRICE;
                    let winner_info = pick_winner(&st.tickets).map(|w| (w.steam.clone(), w.player.clone()));
                    if let Some((ws, wp)) = winner_info {
                        credit(&ws, pot);
                        msgs.push(format!("[Lottery] Round {} DRAW! {} wins {} coins! ({} tickets sold)", st.round, wp, pot, n));
                    }
                    st.tickets.clear();
                    st.round += 1;
                    st.last_draw = Instant::now();
                    let r = st.round;
                    msgs.push(format!("[Lottery] Round {} started! !lottery buy", r));
                } else {
                    st.last_draw = Instant::now();
                }
            }
            msgs
        };
        for m in &announce { broadcast(m).await; }
    }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let cmd = text.split_whitespace().next().unwrap_or("").to_lowercase();
        if cmd != "!lottery" { return Outcome::Ignored; }
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

        let args = match text.find(' ') { Some(i) => text[i + 1..].trim().to_lowercase(), None => String::new() };
        let msg = match args.as_str() {
            "buy" => {
                let mut st = self.state.lock().await;
                let already = st.tickets.iter().filter(|t| t.steam == steam).count();
                if already >= 5 {
                    "[Lottery] Max 5 tickets per round.".to_string()
                } else {
                    match debit(&steam, TICKET_PRICE) {
                        Ok(new_bal) => {
                            st.tickets.push(Ticket { player: player.clone(), steam: steam.clone() });
                            let remaining = DRAW_INTERVAL.checked_sub(st.last_draw.elapsed()).map(|d| d.as_secs() / 60).unwrap_or(0);
                            let pot = st.tickets.len() as i64 * TICKET_PRICE;
                            format!("[Lottery] Ticket bought! Pot: {} | Draw in {}min | Balance: {}", pot, remaining, new_bal)
                        }
                        Err(e) => format!("[Lottery] {}", e),
                    }
                }
            }
            "status" | "" => {
                let st = self.state.lock().await;
                let pot = st.tickets.len() as i64 * TICKET_PRICE;
                let remaining = DRAW_INTERVAL.checked_sub(st.last_draw.elapsed()).map(|d| d.as_secs() / 60).unwrap_or(0);
                let my_tickets = st.tickets.iter().filter(|t| t.steam == steam).count();
                format!("[Lottery] Round {} | Pot: {} | {} tickets | Draw in {}min | You: {} tickets", st.round, pot, st.tickets.len(), remaining, my_tickets)
            }
            _ => "[Lottery] Usage: !lottery buy | !lottery status".to_string(),
        };
        reply(&msg, &player).await;
        Outcome::Handled
    }
}
