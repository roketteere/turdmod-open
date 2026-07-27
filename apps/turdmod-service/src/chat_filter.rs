// Chat filter - auto-detect profanity/toxic language + spam, warn + escalate to a 5-min mute.
// Event-driven: inspects ALL chat (not a command verb), so no commands().

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const MUTE_DURATION: Duration = Duration::from_secs(300); // 5 min
const STRIKE_THRESHOLD: u32 = 3;

const BLOCKED_WORDS: &[&str] = &[
    // Slurs and hate speech - server admin can extend via config file.
];

const TOXIC_PATTERNS: &[&str] = &[
    "kys", "kill yourself", "go die", "neck yourself",
];

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

#[derive(Default)]
struct FilterState {
    strikes: HashMap<String, u32>,
    muted: HashMap<String, Instant>,
    rate: HashMap<String, Instant>,
}

pub struct ChatFilter { state: Mutex<FilterState> }
impl ChatFilter {
    pub fn new() -> Self { Self { state: Mutex::new(FilterState::default()) } }
}

#[async_trait::async_trait]
impl Mod for ChatFilter {
    fn name(&self) -> &'static str { "chat_filter" }
    // event-driven (no commands()): inspects every chat message.

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if steam.is_empty() || crate::owner::is_owner_steam(&steam) { return Outcome::Ignored; }
        if text.starts_with('!') { return Outcome::Ignored; } // don't filter commands

        let mut replies: Vec<String> = Vec::new();
        {
            let mut st = self.state.lock().await;

            // Muted? (silently drop while still muted)
            let still_muted = match st.muted.get(&steam) {
                Some(m) if m.elapsed() < MUTE_DURATION => true,
                Some(_) => { st.muted.remove(&steam); false }
                None => false,
            };
            if still_muted { return Outcome::Ignored; }

            let text_lower = text.to_lowercase();
            let has_blocked = BLOCKED_WORDS.iter().any(|w| text_lower.contains(w));
            let has_toxic = TOXIC_PATTERNS.iter().any(|p| text_lower.contains(p));
            if has_blocked || has_toxic {
                let count = st.strikes.entry(steam.clone()).or_insert(0);
                *count += 1;
                let s = *count;
                if s >= STRIKE_THRESHOLD {
                    st.muted.insert(steam.clone(), Instant::now());
                    st.strikes.remove(&steam);
                    replies.push("[Filter] You have been muted for 5 minutes. Repeated violations may result in a ban.".to_string());
                    tracing::warn!("chat_filter: muted {} ({}) for toxic chat after {} strikes", player, steam, s);
                } else {
                    replies.push(format!("[Filter] Warning {}/{}. Keep it civil.", s, STRIKE_THRESHOLD));
                }
            }

            // Spam: very rapid messages.
            let now = Instant::now();
            if let Some(prev) = st.rate.get(&steam) {
                if now.duration_since(*prev) < Duration::from_millis(500) {
                    let count = st.strikes.entry(format!("spam_{}", steam)).or_insert(0);
                    *count += 1;
                    if *count >= 5 {
                        replies.push("[Filter] Slow down - anti-spam triggered.".to_string());
                        *count = 0;
                    }
                }
            }
            st.rate.insert(steam.clone(), now);
        }

        if replies.is_empty() { return Outcome::Ignored; }
        for m in &replies { reply(m, &player).await; }
        Outcome::Handled
    }
}
