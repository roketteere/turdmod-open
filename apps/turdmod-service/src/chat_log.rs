// In-memory ring buffer of recent in-game chat. Captured at the registry dispatch
// hub (every "chat" event, not just ones a mod handles) so the hosted admin map can
// read live chat without a bridge change. @dep: registry.rs dispatch calls record();
// server.rs exposes GET /chat/recent. @inv: bounded to CAP — never grows.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::events::GameEvent;

const CAP: usize = 250;

#[derive(Clone, serde::Serialize)]
pub struct ChatEntry {
    pub at: u64, // epoch ms
    pub player: String,
    pub steam: String,
    pub text: String,
    pub channel: String,
}

static LOG: Mutex<VecDeque<ChatEntry>> = Mutex::new(VecDeque::new());

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Record one chat event. Call for every `ev.event == "chat"`.
pub fn record(ev: &GameEvent) {
    let g = |k: &str| ev.data.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let text = g("text");
    if text.trim().is_empty() { return; }
    let entry = ChatEntry {
        at: now_ms(),
        player: g("player"),
        steam: g("steam"),
        text,
        channel: ev.data.get("channel").and_then(|v| v.as_str()).unwrap_or("Global").to_string(),
    };
    if let Ok(mut q) = LOG.lock() {
        q.push_back(entry);
        while q.len() > CAP {
            q.pop_front();
        }
    }
}

/// Most recent `n` chat lines, oldest → newest.
pub fn recent(n: usize) -> Vec<ChatEntry> {
    LOG.lock()
        .map(|q| {
            let len = q.len();
            let start = len.saturating_sub(n);
            q.iter().skip(start).cloned().collect()
        })
        .unwrap_or_default()
}
