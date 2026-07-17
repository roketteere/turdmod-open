// Scheduled announcements + !announce — rotating server messages on a timer

use std::time::Duration;
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const OWNER_STEAM_ID: &str = "YOUR_STEAM_ID_1";
const CFG_PATH: &str = r"C:\TurdMOD\data\announcements.json";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct AnnounceCfg {
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default = "default_interval")]
    interval_minutes: u64,
    #[serde(default = "default_messages")]
    messages: Vec<String>,
    #[serde(default = "default_channel")]
    channel: String,
}

fn default_true() -> bool { true }
fn default_interval() -> u64 { 15 }
fn default_channel() -> String { "6".into() }
fn default_messages() -> Vec<String> {
    vec![
        "[ScummyMap] Type !help for available commands".into(),
        "[ScummyMap] Visit www.ScummyMap.com for mods and community".into(),
    ]
}

impl Default for AnnounceCfg {
    fn default() -> Self {
        Self { enabled: true, interval_minutes: 15, messages: default_messages(), channel: "6".into() }
    }
}

fn load_cfg() -> AnnounceCfg {
    std::fs::read_to_string(CFG_PATH).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| {
            let cfg = AnnounceCfg::default();
            save_cfg(&cfg);
            cfg
        })
}

fn save_cfg(cfg: &AnnounceCfg) {
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    if let Ok(json) = serde_json::to_string_pretty(cfg) {
        let tmp = format!("{}.tmp", CFG_PATH);
        if std::fs::write(&tmp, &json).is_ok() {
            let _ = std::fs::rename(&tmp, CFG_PATH);
        }
    }
}

async fn broadcast_msg(text: &str, channel: &str) {
    let params = serde_json::json!({ "text": text, "channel": channel });
    pipe_rpc::call("broadcastChat", Some(params)).await.ok();
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

// @inv: idx wraps on overflow; messages rotated in order.
pub struct GameEvents {
    cfg: Mutex<AnnounceCfg>,
    idx: Mutex<usize>,
}

impl GameEvents {
    pub fn new() -> Self {
        let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
        Self {
            cfg: Mutex::new(load_cfg()),
            idx: Mutex::new(0),
        }
    }
}

#[async_trait::async_trait]
impl Mod for GameEvents {
    fn name(&self) -> &'static str { "game_events" }
    fn commands(&self) -> &'static [&'static str] { &["!announce", "!announcements"] }

    // Interval drives the rotating broadcast. interval_minutes loaded from cfg at tick time.
    // @brk: interval() is fixed at 1 min; we gate inside tick() on the configured interval.
    // This is necessary because interval() can't read async state. We track elapsed ticks instead.
    fn interval(&self) -> Option<Duration> { Some(Duration::from_secs(60)) }

    async fn tick(&self, _ctx: &ModCtx) {
        let (enabled, interval_mins, messages, channel, idx) = {
            let c = self.cfg.lock().await;
            let mut i = self.idx.lock().await;
            // Only fire every interval_minutes ticks (each tick = 1 min)
            // We re-use idx as a combined tick counter + message index.
            // Approach: store raw tick count, modulo interval_minutes to gate, modulo messages.len() for which msg.
            let tick_count = *i;
            *i = i.wrapping_add(1);
            (c.enabled, c.interval_minutes, c.messages.clone(), c.channel.clone(), tick_count)
        };
        if !enabled || messages.is_empty() { return; }
        if interval_mins == 0 { return; }
        // Fire only on ticks that are multiples of interval_mins (first tick = minute 1 onwards)
        if idx == 0 { return; } // skip the immediate-boot tick
        if idx % (interval_mins as usize) != 0 { return; }
        let msg_idx = (idx / interval_mins as usize) - 1;
        let msg = &messages[msg_idx % messages.len()];
        broadcast_msg(msg, &channel).await;
    }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("");
        if steam != OWNER_STEAM_ID && steam != "YOUR_STEAM_ID_2" { return Outcome::Ignored; }

        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();

        if let Some(msg) = text.strip_prefix("!announce ") {
            let msg = msg.trim();
            if !msg.is_empty() {
                let ch = { self.cfg.lock().await.channel.clone() };
                broadcast_msg(msg, &ch).await;
                reply("[ScummyMap] Announcement sent.", &player).await;
                return Outcome::Handled;
            }
            return Outcome::Ignored;
        }

        if text.starts_with("!announcements") {
            let arg = text.strip_prefix("!announcements").unwrap_or("").trim().to_ascii_lowercase();
            match arg.as_str() {
                "on" => {
                    let msg = {
                        let mut c = self.cfg.lock().await;
                        c.enabled = true;
                        save_cfg(&c);
                        "[ScummyMap] Announcements enabled".to_string()
                    };
                    reply(&msg, &player).await;
                }
                "off" => {
                    let msg = {
                        let mut c = self.cfg.lock().await;
                        c.enabled = false;
                        save_cfg(&c);
                        "[ScummyMap] Announcements disabled".to_string()
                    };
                    reply(&msg, &player).await;
                }
                _ => {
                    let msg = {
                        let c = self.cfg.lock().await;
                        let state = if c.enabled { "ON" } else { "OFF" };
                        format!("[ScummyMap] Announcements: {} | {} msgs | every {}min", state, c.messages.len(), c.interval_minutes)
                    };
                    reply(&msg, &player).await;
                }
            }
            return Outcome::Handled;
        }

        Outcome::Ignored
    }
}
