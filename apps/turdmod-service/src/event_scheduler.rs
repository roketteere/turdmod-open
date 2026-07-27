// Event scheduler - admin schedules recurring or one-time server events.
// !event create <name> <minutes> - start event in N minutes
// !event list - show upcoming events
// !event cancel <name>
// Events broadcast countdown and trigger actions (warzone, horde, airdrop, etc.)

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const RATE_LIMIT: Duration = Duration::from_secs(3);
// Tick interval for countdown checking
const TICK_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Clone)]
struct ScheduledEvent {
    name: String,
    fires_at: Instant,
    action: String,
    announced_5min: bool,
    announced_1min: bool,
    fired: bool,
}

fn is_owner(steam: &str, player: &str) -> bool {
    crate::owner::is_owner(steam, player)
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

async fn broadcast_msg(msg: &str) {
    // Server-wide event countdowns now fire the prominent zero-admin #Announce
    // banner, plus a chat line for scrollback. @dep crate::auto_announce::announce.
    crate::auto_announce::announce(msg).await;
    let params = serde_json::json!({ "text": msg });
    pipe_rpc::call("broadcastChat", Some(params)).await.ok();
}

pub struct EventScheduler {
    rate: Mutex<HashMap<String, Instant>>,
    events: Mutex<Vec<ScheduledEvent>>,
}
impl EventScheduler {
    pub fn new() -> Self {
        Self {
            rate: Mutex::new(HashMap::new()),
            events: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait::async_trait]
impl Mod for EventScheduler {
    fn name(&self) -> &'static str { "event_scheduler" }
    fn commands(&self) -> &'static [&'static str] { &["!event"] }
    fn interval(&self) -> Option<Duration> { Some(TICK_INTERVAL) }

    // Periodic tick: advance countdown announcements and fire due events.
    // @inv: no bridge calls while holding self.events lock
    async fn tick(&self, _ctx: &ModCtx) {
        // Collect announcements/fires under lock, send after.
        let mut to_broadcast: Vec<String> = Vec::new();
        {
            let mut events = self.events.lock().await;
            for ev in events.iter_mut() {
                if ev.fired { continue; }
                let remaining = ev.fires_at.saturating_duration_since(Instant::now());
                let secs = remaining.as_secs();

                if secs <= 300 && secs > 60 && !ev.announced_5min {
                    to_broadcast.push(format!("[Event] '{}' starts in 5 minutes!", ev.name));
                    ev.announced_5min = true;
                }
                if secs <= 60 && secs > 0 && !ev.announced_1min {
                    to_broadcast.push(format!("[Event] '{}' starts in 1 minute!", ev.name));
                    ev.announced_1min = true;
                }
                if secs == 0 || Instant::now() >= ev.fires_at {
                    to_broadcast.push(format!("[Event] '{}' has STARTED!", ev.name));
                    ev.fired = true;
                }
            }
            events.retain(|e| !e.fired || e.fires_at.elapsed() < Duration::from_secs(60));
        }
        for msg in &to_broadcast { broadcast_msg(msg).await; }
    }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        if !text.starts_with("!event") { return Outcome::Ignored; }

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

        let parts: Vec<&str> = text.split_whitespace().collect();
        let sub = parts.get(1).map(|s| s.to_lowercase()).unwrap_or_default();

        // Build reply strings under lock; send after.
        // @inv: never hold events lock across pipe_rpc::call await
        let mut out: Vec<(String, String)> = Vec::new();
        let mut broadcasts: Vec<String> = Vec::new();

        {
            let mut events = self.events.lock().await;

            match sub.as_str() {
                "create" | "schedule" => {
                    if !is_owner(&steam, &player) {
                        out.push((player.clone(), "[Event] Admin only.".into()));
                    } else if parts.len() < 4 {
                        out.push((player.clone(), "[Event] Usage: !event create <name> <minutes>".into()));
                    } else {
                        let name = parts[2].to_string();
                        let minutes: u64 = parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(10);
                        events.push(ScheduledEvent {
                            name: name.clone(),
                            fires_at: Instant::now() + Duration::from_secs(minutes * 60),
                            action: "custom".into(),
                            announced_5min: minutes <= 5,
                            announced_1min: minutes <= 1,
                            fired: false,
                        });
                        broadcasts.push(format!("[Event] '{}' scheduled in {} minutes!", name, minutes));
                    }
                }

                "list" => {
                    let active: Vec<(String, u64)> = events.iter()
                        .filter(|e| !e.fired)
                        .map(|e| (e.name.clone(), e.fires_at.saturating_duration_since(Instant::now()).as_secs() / 60))
                        .collect();
                    if active.is_empty() {
                        out.push((player.clone(), "[Event] No upcoming events.".into()));
                    } else {
                        for (name, remaining) in &active {
                            out.push((player.clone(), format!("[Event] {} - in {}min", name, remaining)));
                        }
                    }
                }

                "cancel" => {
                    if !is_owner(&steam, &player) {
                        out.push((player.clone(), "[Event] Admin only.".into()));
                    } else {
                        let name = parts.get(2).map(|s| s.to_lowercase()).unwrap_or_default();
                        let before = events.len();
                        events.retain(|e| e.name.to_lowercase() != name);
                        if events.len() < before {
                            broadcasts.push(format!("[Event] '{}' cancelled.", name));
                        } else {
                            out.push((player.clone(), format!("[Event] '{}' not found.", name)));
                        }
                    }
                }

                _ => {
                    out.push((player.clone(), "[Event] Commands: create/list/cancel".into()));
                }
            }
        } // lock dropped here

        for (rcpt, msg) in &out { reply(msg, rcpt).await; }
        for msg in &broadcasts { broadcast_msg(msg).await; }
        Outcome::Handled
    }
}
