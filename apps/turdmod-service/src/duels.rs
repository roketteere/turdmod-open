// Duel system - !duel <player> for 1v1 matches. Winner determined by kill event.
// Event-driven (needs kill); a 5s tick expires stale pendings/duels.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const RATE_LIMIT: Duration = Duration::from_secs(5);
const DUEL_TIMEOUT: Duration = Duration::from_secs(300); // 5 min max
const PENDING_TTL: Duration = Duration::from_secs(30);

#[derive(Clone)]
struct PendingDuel {
    challenger: String,
    challenger_steam: String,
    #[allow(dead_code)]
    target: String,
    created: Instant,
}

#[derive(Clone)]
struct ActiveDuel {
    #[allow(dead_code)]
    player_a: String,
    player_a_steam: String,
    #[allow(dead_code)]
    player_b: String,
    player_b_steam: String,
    started: Instant,
}

#[derive(Default)]
struct DuelState {
    pending: HashMap<String, PendingDuel>, // target_name(lowercase) -> pending
    active: Vec<ActiveDuel>,
    stats: HashMap<String, (u32, u32)>, // steam -> (wins, losses); in-memory only
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

async fn broadcast(msg: &str) {
    let params = serde_json::json!({ "text": msg });
    pipe_rpc::call("broadcastChat", Some(params)).await.ok();
}

pub struct Duels { state: Mutex<DuelState>, rate: Mutex<HashMap<String, Instant>> }
impl Duels {
    pub fn new() -> Self { Self { state: Mutex::new(DuelState::default()), rate: Mutex::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl Mod for Duels {
    fn name(&self) -> &'static str { "duels" }
    // event-driven (no commands()): needs `kill` events plus the chat commands.
    fn interval(&self) -> Option<Duration> { Some(Duration::from_secs(5)) }

    // Dormant unless a duel is pending or in progress — no tick when nothing's running.
    async fn active(&self) -> bool {
        let st = self.state.lock().await;
        !st.pending.is_empty() || !st.active.is_empty()
    }

    // Expire stale pendings + duels (the old top-of-loop cleanup).
    async fn tick(&self, _ctx: &ModCtx) {
        let mut st = self.state.lock().await;
        st.pending.retain(|_, p| p.created.elapsed() < PENDING_TTL);
        st.active.retain(|d| d.started.elapsed() < DUEL_TIMEOUT);
    }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        match ev.event.as_str() {
            "kill" => {
                let killer = ev.data.get("killer").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let killer_steam = ev.data.get("killerSteam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let victim = ev.data.get("victim").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let victim_steam = ev.data.get("victimSteam").and_then(|v| v.as_str()).unwrap_or("").to_string();

                let duration = {
                    let mut st = self.state.lock().await;
                    let idx = st.active.iter().position(|d| {
                        (d.player_a_steam == killer_steam && d.player_b_steam == victim_steam) ||
                        (d.player_b_steam == killer_steam && d.player_a_steam == victim_steam)
                    });
                    if let Some(idx) = idx {
                        let duel = st.active.remove(idx);
                        let dur = duel.started.elapsed().as_secs();
                        st.stats.entry(killer_steam.clone()).or_insert((0, 0)).0 += 1;
                        st.stats.entry(victim_steam.clone()).or_insert((0, 0)).1 += 1;
                        Some(dur)
                    } else { None }
                };
                if let Some(dur) = duration {
                    broadcast(&format!("[Duel] {} defeats {} in {}s!", killer, victim, dur)).await;
                    Outcome::Handled
                } else { Outcome::Ignored }
            }
            "chat" => {
                let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
                if !text.starts_with('!') { return Outcome::Ignored; }
                let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let (cmd, args) = match text.find(' ') {
                    Some(i) => (text[..i].to_lowercase(), text[i + 1..].trim().to_string()),
                    None => (text.to_lowercase(), String::new()),
                };
                if !matches!(cmd.as_str(), "!duel" | "!accept" | "!duelstats" | "!decline") { return Outcome::Ignored; }

                let rate_key = if steam.is_empty() { player.clone() } else { steam.clone() };
                {
                    let mut rate = self.rate.lock().await;
                    let now = Instant::now();
                    if let Some(prev) = rate.get(&rate_key) {
                        if now.duration_since(*prev) < RATE_LIMIT { return Outcome::Ignored; }
                    }
                    rate.insert(rate_key.clone(), now);
                }

                let mut out: Vec<(Option<String>, String)> = Vec::new(); // Some(name)=reply, None=broadcast
                {
                    let mut st = self.state.lock().await;
                    match cmd.as_str() {
                        "!duel" => {
                            if args.is_empty() {
                                out.push((Some(player.clone()), "[Duel] Usage: !duel <playerName>".to_string()));
                            } else if args.eq_ignore_ascii_case(&player) {
                                out.push((Some(player.clone()), "[Duel] Can't duel yourself.".to_string()));
                            } else if st.active.iter().any(|d| d.player_a_steam == steam || d.player_b_steam == steam) {
                                out.push((Some(player.clone()), "[Duel] Already in a duel!".to_string()));
                            } else {
                                st.pending.insert(args.to_lowercase(), PendingDuel {
                                    challenger: player.clone(), challenger_steam: steam.clone(), target: args.clone(), created: Instant::now(),
                                });
                                out.push((Some(player.clone()), format!("[Duel] Challenge sent to {}! They have 30s to !accept", args)));
                                out.push((Some(args.clone()), format!("[Duel] {} challenged you! Type !accept to fight", player)));
                            }
                        }
                        "!accept" => {
                            match st.pending.remove(&player.to_lowercase()) {
                                None => out.push((Some(player.clone()), "[Duel] No pending challenge.".to_string())),
                                Some(duel) => {
                                    st.active.push(ActiveDuel {
                                        player_a: duel.challenger.clone(), player_a_steam: duel.challenger_steam.clone(),
                                        player_b: player.clone(), player_b_steam: steam.clone(), started: Instant::now(),
                                    });
                                    out.push((None, format!("[Duel] {} vs {} - FIGHT! (5 min timer)", duel.challenger, player)));
                                }
                            }
                        }
                        "!duelstats" => {
                            let (wins, losses) = st.stats.get(&steam).copied().unwrap_or((0, 0));
                            let total = wins + losses;
                            let wr = if total == 0 { 0.0 } else { wins as f64 / total as f64 * 100.0 };
                            out.push((Some(player.clone()), format!("[Duel] W:{} L:{} WR:{:.0}%", wins, losses, wr)));
                        }
                        "!decline" => {
                            if let Some(duel) = st.pending.remove(&player.to_lowercase()) {
                                out.push((Some(player.clone()), "[Duel] Challenge declined.".to_string()));
                                out.push((Some(duel.challenger.clone()), format!("[Duel] {} declined your challenge.", player)));
                            }
                        }
                        _ => return Outcome::Ignored,
                    }
                }
                for (rcpt, msg) in &out {
                    match rcpt { Some(r) => reply(msg, r).await, None => broadcast(msg).await }
                }
                Outcome::Handled
            }
            _ => Outcome::Ignored,
        }
    }
}
