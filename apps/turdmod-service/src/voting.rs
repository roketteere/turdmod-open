// Voting system - !vote <topic>, !yes, !no, auto-execute results (weather/time/restart/custom).
// commands() = !vote/!yes/!no; a 5s tick closes expired votes.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const VOTE_DURATION: Duration = Duration::from_secs(60);
const COOLDOWN: Duration = Duration::from_secs(120);
const RATE_LIMIT: Duration = Duration::from_secs(3);

#[derive(Clone)]
struct ActiveVote {
    topic: String,
    action: VoteAction,
    started: Instant,
    yes: HashSet<String>,
    no: HashSet<String>,
    #[allow(dead_code)]
    started_by: String,
}

#[derive(Clone)]
enum VoteAction {
    Day,
    Night,
    Storm,
    Clear,
    Restart,
    Custom(String),
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

async fn broadcast(msg: &str) {
    let params = serde_json::json!({ "text": msg });
    pipe_rpc::call("broadcastChat", Some(params)).await.ok();
}

async fn execute_action(action: &VoteAction) {
    match action {
        VoteAction::Day => { pipe_rpc::call("setTimeOfDay", Some(serde_json::json!({ "hours": 12.0 }))).await.ok(); }
        VoteAction::Night => { pipe_rpc::call("setTimeOfDay", Some(serde_json::json!({ "hours": 0.0 }))).await.ok(); }
        VoteAction::Storm => {
            pipe_rpc::call("setWeather", Some(serde_json::json!({ "severity": 1.0 }))).await.ok();
            pipe_rpc::call("forceWeatherSnapshot", None).await.ok();
        }
        VoteAction::Clear => {
            pipe_rpc::call("setWeather", Some(serde_json::json!({ "severity": 0.0 }))).await.ok();
            pipe_rpc::call("forceWeatherSnapshot", None).await.ok();
        }
        VoteAction::Restart => {
            broadcast("[Server] Restart vote passed - restarting in 60 seconds!").await;
            tokio::time::sleep(Duration::from_secs(60)).await;
            pipe_rpc::call("shutdownServer", Some(serde_json::json!({ "reason": "vote" }))).await.ok();
        }
        VoteAction::Custom(_) => {}
    }
}

#[derive(Default)]
struct VotingState {
    active: Option<ActiveVote>,
    last_vote: Option<Instant>,
}

pub struct Voting { state: Mutex<VotingState>, rate: Mutex<HashMap<String, Instant>> }
impl Voting {
    pub fn new() -> Self { Self { state: Mutex::new(VotingState::default()), rate: Mutex::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl Mod for Voting {
    fn name(&self) -> &'static str { "voting" }
    fn commands(&self) -> &'static [&'static str] { &["!vote", "!yes", "!no"] }
    fn interval(&self) -> Option<Duration> { Some(Duration::from_secs(5)) }

    // Dormant unless a vote is open.
    async fn active(&self) -> bool { self.state.lock().await.active.is_some() }

    // Close + tally an expired vote (the old top-of-loop expiry check).
    async fn tick(&self, _ctx: &ModCtx) {
        let result = {
            let mut st = self.state.lock().await;
            let expired = st.active.as_ref().map(|v| v.started.elapsed() > VOTE_DURATION).unwrap_or(false);
            if expired {
                let vote = st.active.take().unwrap();
                let yes = vote.yes.len();
                let no = vote.no.len();
                st.last_vote = Some(Instant::now());
                Some((vote.topic.clone(), vote.action.clone(), yes, no, yes > no && yes >= 2))
            } else { None }
        };
        if let Some((topic, action, yes, no, passed)) = result {
            if passed {
                broadcast(&format!("[Vote] '{}' PASSED ({} yes / {} no)", topic, yes, no)).await;
                tokio::spawn(async move { execute_action(&action).await; });
            } else {
                broadcast(&format!("[Vote] '{}' FAILED ({} yes / {} no)", topic, yes, no)).await;
            }
        }
    }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let parts: Vec<&str> = text.split_whitespace().collect();
        let cmd = parts.first().map(|s| s.to_lowercase()).unwrap_or_default();
        if !matches!(cmd.as_str(), "!vote" | "!yes" | "!no") { return Outcome::Ignored; }

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
            "!vote" => {
                let mut st = self.state.lock().await;
                if st.active.is_some() { drop(st); reply("[Vote] A vote is already active. Use !yes or !no", &player).await; return Outcome::Handled; }
                if let Some(lv) = st.last_vote {
                    if lv.elapsed() < COOLDOWN {
                        let remaining = (COOLDOWN - lv.elapsed()).as_secs();
                        drop(st);
                        reply(&format!("[Vote] Cooldown: {}s remaining", remaining), &player).await;
                        return Outcome::Handled;
                    }
                }
                let topic = parts.get(1).map(|s| s.to_lowercase()).unwrap_or_default();
                if topic.is_empty() { drop(st); reply("[Vote] Usage: !vote day|night|storm|clear|restart", &player).await; return Outcome::Handled; }
                let action = match topic.as_str() {
                    "day" => VoteAction::Day,
                    "night" => VoteAction::Night,
                    "storm" => VoteAction::Storm,
                    "clear" => VoteAction::Clear,
                    "restart" => VoteAction::Restart,
                    other => VoteAction::Custom(other.into()),
                };
                let mut yes = HashSet::new();
                yes.insert(steam.clone());
                st.active = Some(ActiveVote { topic: topic.clone(), action, started: Instant::now(), yes, no: HashSet::new(), started_by: player.clone() });
                drop(st);
                broadcast(&format!("[Vote] {} started a vote: '{}' - !yes or !no (60s)", player, topic)).await;
                Outcome::Handled
            }
            "!yes" => {
                let msg = {
                    let mut st = self.state.lock().await;
                    if let Some(ref mut vote) = st.active {
                        vote.no.remove(&steam);
                        vote.yes.insert(steam.clone());
                        format!("[Vote] Counted. ({} yes / {} no)", vote.yes.len(), vote.no.len())
                    } else { "[Vote] No active vote.".to_string() }
                };
                reply(&msg, &player).await;
                Outcome::Handled
            }
            "!no" => {
                let msg = {
                    let mut st = self.state.lock().await;
                    if let Some(ref mut vote) = st.active {
                        vote.yes.remove(&steam);
                        vote.no.insert(steam.clone());
                        format!("[Vote] Counted. ({} yes / {} no)", vote.yes.len(), vote.no.len())
                    } else { "[Vote] No active vote.".to_string() }
                };
                reply(&msg, &player).await;
                Outcome::Handled
            }
            _ => Outcome::Ignored,
        }
    }
}
