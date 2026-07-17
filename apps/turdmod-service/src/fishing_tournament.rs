// Fishing tournament - timed competitions with leaderboard and prizes.
// !fish start - admin starts tournament (30 min). !fish cast - attempt a catch.
// RNG-based catch with size/rarity. !fish board - tournament leaderboard.
// Winner gets economy reward. Catches broadcast server-wide.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const OWNER_STEAM_ID: &str = "YOUR_STEAM_ID_1";
const OWNER_NAME: &str = "YOUR_OWNER_NAME";
const ECON_PATH: &str = r"C:\TurdMOD\data\economy.json";
const RATE_LIMIT: Duration = Duration::from_secs(5);
const CAST_COOLDOWN: Duration = Duration::from_secs(15);
const TOURNAMENT_DURATION: Duration = Duration::from_secs(1800);
const FIRST_PLACE: i64 = 500;
const SECOND_PLACE: i64 = 250;
const THIRD_PLACE: i64 = 100;

const FISH: &[(&str, f64, f64)] = &[
    ("Sardine",     0.30, 0.5),
    ("Mackerel",    0.25, 1.5),
    ("Bass",        0.15, 3.0),
    ("Trout",       0.10, 4.0),
    ("Salmon",      0.08, 6.0),
    ("Tuna",        0.05, 10.0),
    ("Swordfish",   0.04, 15.0),
    ("Shark",       0.02, 25.0),
    ("Golden Carp", 0.01, 50.0),
];

#[derive(Clone)]
struct Catch {
    fish: String,
    weight: f64,
    score: f64,
}

#[derive(Clone)]
struct Tournament {
    started: Instant,
    scores: HashMap<String, Vec<Catch>>,
    steam_map: HashMap<String, String>,
}

struct Inner {
    tournament: Option<Tournament>,
    rate: HashMap<String, Instant>,
    cast_cooldown: HashMap<String, Instant>,
}

fn is_owner(steam: &str, player: &str) -> bool {
    steam == OWNER_STEAM_ID || steam == "YOUR_STEAM_ID_2" || player == OWNER_NAME
}

fn rng() -> f64 {
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_nanos();
    ((seed % 10000) as f64) / 10000.0
}

fn pick_fish() -> Option<(&'static str, f64)> {
    let roll = rng();
    if roll < 0.20 { return None; }
    let mut cumulative = 0.20;
    for (name, chance, base_score) in FISH {
        cumulative += chance;
        if roll < cumulative {
            let weight = 0.5 + rng() * 4.5;
            let score = base_score * weight;
            return Some((name, score));
        }
    }
    Some(("Sardine", 0.5))
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

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

async fn broadcast_msg(msg: &str) {
    let params = serde_json::json!({ "text": msg });
    pipe_rpc::call("broadcastChat", Some(params)).await.ok();
}

fn total_score(catches: &[Catch]) -> f64 {
    catches.iter().map(|c| c.score).sum()
}

pub struct FishingTournament { inner: Mutex<Inner> }
impl FishingTournament {
    pub fn new() -> Self {
        Self { inner: Mutex::new(Inner {
            tournament: None,
            rate: HashMap::new(),
            cast_cooldown: HashMap::new(),
        })}
    }
}

#[async_trait::async_trait]
impl Mod for FishingTournament {
    fn name(&self) -> &'static str { "fishing_tournament" }
    fn commands(&self) -> &'static [&'static str] { &["!fish"] }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        if !text.starts_with("!fish") { return Outcome::Ignored; }

        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let rate_key = if steam.is_empty() { player.clone() } else { steam.clone() };

        let parts: Vec<&str> = text.split_whitespace().collect();
        let sub = parts.get(1).map(|s| s.to_lowercase()).unwrap_or_default();

        // Check + expire tournament, enforce rate limit, all under one lock.
        // Collect reply strings; send after lock drops.
        enum Action {
            Ignore,
            Replies(Vec<String>),       // (player, msg) pairs flattened: [player0,msg0,player1,msg1,...]
            BroadcastAndReplies { bcast: Vec<String>, replies: Vec<(String,String)> },
            CreditAndBroadcast { credits: Vec<(String, i64)>, msgs: Vec<String> },
        }

        let action = {
            let mut inner = self.inner.lock().await;

            // Expire tournament if time's up — record results for crediting outside lock.
            if let Some(ref t) = inner.tournament {
                if t.started.elapsed() > TOURNAMENT_DURATION {
                    let mut results: Vec<(String, f64)> = t.scores.iter()
                        .map(|(name, catches)| (name.clone(), total_score(catches)))
                        .collect();
                    results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

                    let mut msgs = vec!["[Fishing] TOURNAMENT OVER! Results:".to_string()];
                    let mut credits: Vec<(String, i64)> = Vec::new();
                    for (i, (name, score)) in results.iter().enumerate().take(5) {
                        let prize = match i { 0 => FIRST_PLACE, 1 => SECOND_PLACE, 2 => THIRD_PLACE, _ => 0 };
                        if prize > 0 {
                            if let Some(st) = t.steam_map.get(name) {
                                credits.push((st.clone(), prize));
                            }
                        }
                        let prize_str = if prize > 0 { format!(" +{}c!", prize) } else { String::new() };
                        msgs.push(format!("[Fishing] {}. {} - {:.0} pts{}", i + 1, name, score, prize_str));
                    }
                    inner.tournament = None;
                    // Continue processing the current event after clearing.
                    // Fall through — tournament is now None.
                    // We still need to reply the results; handle below by dropping action.
                    // Use a nested early return via a flag.
                    return {
                        for (st, amt) in credits { credit(&st, amt); }
                        for m in &msgs { broadcast_msg(m).await; }
                        Outcome::Handled
                    };
                }
            }

            // Rate limit
            {
                let now = Instant::now();
                if let Some(prev) = inner.rate.get(&rate_key) {
                    if now.duration_since(*prev) < RATE_LIMIT { return Outcome::Ignored; }
                }
                inner.rate.insert(rate_key.clone(), now);
            }

            match sub.as_str() {
                "start" => {
                    if !is_owner(&steam, &player) {
                        Action::Replies(vec![player.clone(), "[Fishing] Admin only.".to_string()])
                    } else if inner.tournament.is_some() {
                        Action::Replies(vec![player.clone(), "[Fishing] Tournament already active!".to_string()])
                    } else {
                        inner.tournament = Some(Tournament {
                            started: Instant::now(),
                            scores: HashMap::new(),
                            steam_map: HashMap::new(),
                        });
                        Action::BroadcastAndReplies {
                            bcast: vec!["[FISHING TOURNAMENT] Started! 30 minutes! !fish cast to catch! Prizes: 500/250/100 coins!".to_string()],
                            replies: vec![],
                        }
                    }
                }
                "cast" => {
                    if inner.tournament.is_none() {
                        Action::Replies(vec![player.clone(), "[Fishing] No tournament active.".to_string()])
                    } else {
                        let now = Instant::now();
                        if let Some(last) = inner.cast_cooldown.get(&steam) {
                            if last.elapsed() < CAST_COOLDOWN {
                                let wait = (CAST_COOLDOWN - last.elapsed()).as_secs();
                                return {
                                    reply(&format!("[Fishing] Wait {}s before casting again.", wait), &player).await;
                                    Outcome::Handled
                                };
                            }
                        }
                        inner.cast_cooldown.insert(steam.clone(), now);
                        match pick_fish() {
                            Some((fish, score)) => {
                                let t = inner.tournament.as_mut().unwrap();
                                t.steam_map.insert(player.clone(), steam.clone());
                                let catches = t.scores.entry(player.clone()).or_default();
                                catches.push(Catch { fish: fish.into(), weight: score / 2.0, score });
                                let total = total_score(catches);
                                Action::BroadcastAndReplies {
                                    bcast: vec![format!("[Fishing] {} caught a {}! ({:.0} pts, total: {:.0})", player, fish, score, total)],
                                    replies: vec![],
                                }
                            }
                            None => Action::Replies(vec![player.clone(), "[Fishing] Nothing biting... try again in 15s.".to_string()]),
                        }
                    }
                }
                "board" | "scores" => {
                    if let Some(ref t) = inner.tournament {
                        let remaining = TOURNAMENT_DURATION.checked_sub(t.started.elapsed())
                            .map(|d| d.as_secs() / 60).unwrap_or(0);
                        let mut results: Vec<(String, f64)> = t.scores.iter()
                            .map(|(name, catches)| (name.clone(), total_score(catches)))
                            .collect();
                        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                        let mut msgs = vec![(player.clone(), format!("[Fishing] {}min remaining", remaining))];
                        for (i, (name, score)) in results.iter().enumerate().take(5) {
                            msgs.push((player.clone(), format!("[Fishing] {}. {} - {:.0} pts", i + 1, name, score)));
                        }
                        Action::BroadcastAndReplies { bcast: vec![], replies: msgs }
                    } else {
                        Action::Replies(vec![player.clone(), "[Fishing] No tournament active.".to_string()])
                    }
                }
                "end" => {
                    if !is_owner(&steam, &player) {
                        Action::Replies(vec![player.clone(), "[Fishing] Admin only.".to_string()])
                    } else if let Some(ref mut t) = inner.tournament {
                        t.started = Instant::now() - TOURNAMENT_DURATION - Duration::from_secs(1);
                        Action::Ignore
                    } else {
                        Action::Ignore
                    }
                }
                _ => Action::Replies(vec![player.clone(), "[Fishing] Commands: cast / board / Admin: start / end".to_string()]),
            }
        };

        match action {
            Action::Ignore => Outcome::Ignored,
            Action::Replies(pairs) => {
                let mut i = 0;
                while i + 1 < pairs.len() {
                    reply(&pairs[i + 1], &pairs[i]).await;
                    i += 2;
                }
                Outcome::Handled
            }
            Action::BroadcastAndReplies { bcast, replies } => {
                for m in &bcast { broadcast_msg(m).await; }
                for (rcpt, msg) in &replies { reply(msg, rcpt).await; }
                Outcome::Handled
            }
            Action::CreditAndBroadcast { credits, msgs } => {
                for (st, amt) in credits { credit(&st, amt); }
                for m in &msgs { broadcast_msg(m).await; }
                Outcome::Handled
            }
        }
    }
}
