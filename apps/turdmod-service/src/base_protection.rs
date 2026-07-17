// Raid window management - !raidstatus !raidtimes !setraidtimes !raidoff !raidon

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const OWNER_STEAM_ID: &str = "YOUR_STEAM_ID_1";
const RATE_LIMIT: Duration = Duration::from_secs(3);
const DAYS: &[&str] = &["Monday","Tuesday","Wednesday","Thursday","Friday","Saturday","Sunday"];

#[derive(Clone, serde::Serialize, serde::Deserialize, Default)]
struct RaidConfig {
    #[serde(rename = "RaidTimes")]
    windows: Vec<RaidWindow>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct RaidWindow {
    #[serde(rename = "DayOfWeek")]
    day: String,
    #[serde(rename = "StartHour")]
    start: u8,
    #[serde(rename = "EndHour")]
    end: u8,
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

async fn read_raid_config() -> Option<RaidConfig> {
    let params = serde_json::json!({ "name": "RaidTimes.json" });
    let resp = pipe_rpc::call("readConfigFile", Some(params)).await.ok()?;
    let content = resp.get("content").and_then(|v| v.as_str())?;
    serde_json::from_str(content).ok()
}

async fn write_raid_config(cfg: &RaidConfig) -> bool {
    let Ok(content) = serde_json::to_string_pretty(cfg) else { return false };
    let params = serde_json::json!({ "name": "RaidTimes.json", "content": content });
    pipe_rpc::call("writeConfigFile", Some(params)).await.is_ok()
}

pub struct BaseProtection {
    cached: Mutex<Option<RaidConfig>>,
    rate: Mutex<HashMap<String, Instant>>,
}

impl BaseProtection {
    pub fn new() -> Self {
        Self { cached: Mutex::new(None), rate: Mutex::new(HashMap::new()) }
    }
}

#[async_trait::async_trait]
impl Mod for BaseProtection {
    fn name(&self) -> &'static str { "base_protection" }
    fn commands(&self) -> &'static [&'static str] {
        &["!raidstatus", "!raidtimes", "!setraidtimes", "!raidoff", "!raidon"]
    }

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

        let (cmd, args) = match text.find(' ') {
            Some(i) => (text[..i].to_lowercase(), text[i + 1..].trim().to_string()),
            None => (text.to_lowercase(), String::new()),
        };

        let is_owner = steam == OWNER_STEAM_ID || steam == "YOUR_STEAM_ID_2";

        // Lazy-load cache on first use
        {
            let mut cached = self.cached.lock().await;
            if cached.is_none() {
                *cached = read_raid_config().await;
            }
        }

        match cmd.as_str() {
            "!raidstatus" | "!raidtimes" => {
                let cached = self.cached.lock().await;
                match cached.as_ref() {
                    Some(c) if !c.windows.is_empty() => {
                        let w = &c.windows[0];
                        reply(&format!("[Raid] ENABLED - {}:00 to {}:00 daily", w.start, w.end), &player).await;
                    }
                    _ => reply("[Raid] Raiding is currently DISABLED", &player).await,
                }
                Outcome::Handled
            }

            "!setraidtimes" => {
                if !is_owner { reply("[Raid] Owner only.", &player).await; return Outcome::Handled; }
                let parts: Vec<&str> = args.split_whitespace().collect();
                if parts.len() != 2 {
                    reply("[Raid] Usage: !setraidtimes <start> <end> (e.g. 18 22)", &player).await;
                    return Outcome::Handled;
                }
                let (Ok(start), Ok(end)) = (parts[0].parse::<u8>(), parts[1].parse::<u8>()) else {
                    reply("[Raid] Hours must be 0-23", &player).await;
                    return Outcome::Handled;
                };
                if start >= end || end > 24 {
                    reply("[Raid] Invalid window", &player).await;
                    return Outcome::Handled;
                }
                let cfg = RaidConfig {
                    windows: DAYS.iter().map(|d| RaidWindow { day: d.to_string(), start, end }).collect(),
                };
                if write_raid_config(&cfg).await {
                    let msg = format!("[Raid] Set: {}:00 - {}:00 daily", start, end);
                    *self.cached.lock().await = Some(cfg);
                    reply(&msg, &player).await;
                } else {
                    reply("[Raid] Failed to write config", &player).await;
                }
                Outcome::Handled
            }

            "!raidoff" => {
                if !is_owner { reply("[Raid] Owner only.", &player).await; return Outcome::Handled; }
                let cfg = RaidConfig { windows: vec![] };
                if write_raid_config(&cfg).await {
                    *self.cached.lock().await = Some(cfg);
                    reply("[Raid] Raiding DISABLED", &player).await;
                } else {
                    reply("[Raid] Failed to disable", &player).await;
                }
                Outcome::Handled
            }

            "!raidon" => {
                if !is_owner { reply("[Raid] Owner only.", &player).await; return Outcome::Handled; }
                reply("[Raid] Use !setraidtimes <start> <end> to set hours", &player).await;
                Outcome::Handled
            }

            _ => Outcome::Ignored,
        }
    }
}
