// Bounty board - automated bounty announcements + claim resolution.
// Integrates with economy module for payouts.
// Monitors kill events to auto-claim bounties.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const STATE_PATH: &str = r"C:\TurdMOD\data\economy.json";
const RATE_LIMIT: Duration = Duration::from_secs(3);
const BOUNTY_ANNOUNCE_INTERVAL: Duration = Duration::from_secs(300); // 5 min

#[derive(Clone)]
struct ActiveBounty {
    target_name: String,
    amount: i64,
    placed_by_name: String,
    placed_by_steam: String,
}

struct BountyState {
    bounties: Vec<ActiveBounty>,
    rate: HashMap<String, Instant>,
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

async fn bcast(msg: &str) {
    let params = serde_json::json!({ "text": msg });
    pipe_rpc::call("broadcastChat", Some(params)).await.ok();
}

fn credit_player(steam: &str, amount: i64) {
    let Ok(data) = std::fs::read_to_string(STATE_PATH) else { return };
    let Ok(mut state) = serde_json::from_str::<serde_json::Value>(&data) else { return };
    if let Some(bal) = state.get_mut("players")
        .and_then(|p| p.get_mut(steam))
        .and_then(|p| p.get_mut("balance"))
        .and_then(|b| b.as_i64()) {
        state["players"][steam]["balance"] = serde_json::json!(bal + amount);
        let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
        if let Ok(json) = serde_json::to_string_pretty(&state) {
            let tmp = format!("{}.tmp", STATE_PATH);
            if std::fs::write(&tmp, &json).is_ok() {
                let _ = std::fs::rename(&tmp, STATE_PATH);
            }
        }
    }
}

pub struct BountyBoard {
    state: Mutex<BountyState>,
    last_announce: Mutex<Instant>,
}

impl BountyBoard {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(BountyState { bounties: Vec::new(), rate: HashMap::new() }),
            last_announce: Mutex::new(Instant::now()),
        }
    }
}

#[async_trait::async_trait]
impl Mod for BountyBoard {
    fn name(&self) -> &'static str { "bounty_board" }
    // event-driven: kill + all chat (needs !bounties/!wanted from any player)
    fn interval(&self) -> Option<Duration> { Some(BOUNTY_ANNOUNCE_INTERVAL) }

    // Periodic bounty board announcement (the old timeout branch).
    async fn tick(&self, _ctx: &ModCtx) {
        // @inv: collect msgs under lock, send after dropping lock
        let msgs: Vec<String> = {
            let st = self.state.lock().await;
            if st.bounties.is_empty() { return; }
            let mut msg = "[Bounty Board] Active bounties:".to_string();
            for (i, b) in st.bounties.iter().enumerate().take(5) {
                msg += &format!(" {}. {} ({}c)", i + 1, b.target_name, b.amount);
            }
            vec![msg]
        };
        for msg in &msgs { bcast(msg).await; }
    }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        match ev.event.as_str() {
            "kill" => {
                let killer = ev.data.get("killer").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let killer_steam = ev.data.get("killerSteam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let victim = ev.data.get("victim").and_then(|v| v.as_str()).unwrap_or("").to_string();

                // @inv: collect payout + msgs under lock, send/credit after dropping lock
                let (total_payout, bcast_msg) = {
                    let mut st = self.state.lock().await;
                    let claimed: Vec<usize> = st.bounties.iter().enumerate()
                        .filter(|(_, b)| b.target_name.eq_ignore_ascii_case(&victim))
                        .map(|(i, _)| i)
                        .collect();

                    let mut total_payout = 0i64;
                    for &idx in claimed.iter().rev() {
                        total_payout += st.bounties[idx].amount;
                        st.bounties.remove(idx);
                    }
                    let msg = if total_payout > 0 && !killer_steam.is_empty() {
                        Some(format!("[Bounty] {} claimed the bounty on {}! +{} coins!", killer, victim, total_payout))
                    } else { None };
                    (total_payout, msg)
                };

                if let Some(msg) = bcast_msg {
                    // killer_steam captured before lock release above
                    let killer_steam2 = ev.data.get("killerSteam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    credit_player(&killer_steam2, total_payout);
                    bcast(&msg).await;
                    Outcome::Handled
                } else {
                    Outcome::Ignored
                }
            }

            "chat" => {
                let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
                if !text.starts_with('!') { return Outcome::Ignored; }

                let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let rate_key = if steam.is_empty() { player.clone() } else { steam.clone() };

                {
                    let mut st = self.state.lock().await;
                    let now = Instant::now();
                    if let Some(prev) = st.rate.get(&rate_key) {
                        if now.duration_since(*prev) < RATE_LIMIT { return Outcome::Ignored; }
                    }
                    st.rate.insert(rate_key.clone(), now);
                }

                let parts: Vec<&str> = text.split_whitespace().collect();
                let cmd = parts[0].to_lowercase();

                match cmd.as_str() {
                    "!bounties" | "!wanted" => {
                        // @inv: collect msgs under lock, send after dropping
                        let msgs: Vec<String> = {
                            let st = self.state.lock().await;
                            if st.bounties.is_empty() {
                                vec!["[Bounty] No active bounties.".to_string()]
                            } else {
                                st.bounties.iter().enumerate().take(10).map(|(i, b)| {
                                    format!("[Bounty] {}. {} - {} coins (by {})",
                                        i + 1, b.target_name, b.amount, b.placed_by_name)
                                }).collect()
                            }
                        };
                        for msg in &msgs { reply(msg, &player).await; }
                        Outcome::Handled
                    }
                    "!bounty" => {
                        // Placing is handled in economy.rs - this is read-only view
                        Outcome::Ignored
                    }
                    _ => Outcome::Ignored,
                }
            }

            _ => Outcome::Ignored,
        }
    }
}
