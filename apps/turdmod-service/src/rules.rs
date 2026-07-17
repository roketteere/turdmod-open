// Server rules + MOTD + reports â€” community management tools.
// !rules â€” display server rules
// !motd â€” message of the day
// !report <player> <reason> â€” report a player (logged)
// !reports â€” admin view of pending reports

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const OWNER_STEAM_ID: &str = "YOUR_STEAM_ID_1";
const OWNER_NAME: &str = "YOUR_OWNER_NAME";
const RATE_LIMIT: Duration = Duration::from_secs(5);
const REPORT_PATH: &str = r"C:\TurdMOD\data\reports.jsonl";

const RULES: &[&str] = &[
    "1. No cheating or exploiting",
    "2. No hate speech or harassment",
    "3. English in global chat",
    "4. No base camping spawn points",
    "5. PvP is allowed outside safe zones",
    "6. Respect admin decisions",
    "7. Report bugs â€” don't exploit them",
];

const MOTD: &str = "Welcome to ScummyMap! Type !help for commands. Friendly zombies, taming, duels, economy, and more. Have fun!";

fn is_owner(steam: &str, player: &str) -> bool {
    steam == OWNER_STEAM_ID || steam == "YOUR_STEAM_ID_2" || player == OWNER_NAME
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

fn log_report(reporter: &str, reporter_steam: &str, target: &str, reason: &str) {
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_secs();
    let line = serde_json::json!({
        "ts": ts, "reporter": reporter, "reporterSteam": reporter_steam,
        "target": target, "reason": reason
    });
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(REPORT_PATH) {
        use std::io::Write;
        writeln!(f, "{}", serde_json::to_string(&line).unwrap_or_default()).ok();
    }
}

fn read_reports(limit: usize) -> Vec<serde_json::Value> {
    let Ok(data) = std::fs::read_to_string(REPORT_PATH) else { return vec![] };
    data.lines().rev().take(limit)
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

pub struct Rules { rate: Mutex<HashMap<String, Instant>> }
impl Rules { pub fn new() -> Self { Self { rate: Mutex::new(HashMap::new()) } } }

#[async_trait::async_trait]
impl Mod for Rules {
    fn name(&self) -> &'static str { "rules" }
    fn commands(&self) -> &'static [&'static str] { &["!rules", "!motd", "!report", "!reports", "!warn"] }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }

        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        if !text.starts_with('!') { return Outcome::Ignored; }

        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let rate_key = if steam.is_empty() { player.clone() } else { steam.clone() };

        // per-player rate limit (router rate_limit() is per-mod; we want per-player)
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

        match cmd.as_str() {
            "!rules" => {
                reply("[Server Rules]", &player).await;
                for rule in RULES { reply(rule, &player).await; }
                Outcome::Handled
            }
            "!motd" => {
                reply(&format!("[MOTD] {}", MOTD), &player).await;
                Outcome::Handled
            }
            "!report" => {
                let parts: Vec<&str> = args.splitn(2, ' ').collect();
                if parts.is_empty() || parts[0].is_empty() {
                    reply("[Report] Usage: !report <player> <reason>", &player).await;
                    return Outcome::Handled;
                }
                let target = parts[0];
                let reason = parts.get(1).unwrap_or(&"no reason given");
                log_report(&player, &steam, target, reason);
                reply(&format!("[Report] Report filed against {}. Admins will review.", target), &player).await;
                tracing::info!("REPORT: {} reported {} - {}", player, target, reason);
                Outcome::Handled
            }
            "!reports" => {
                if !is_owner(&steam, &player) { reply("[Report] Admin only.", &player).await; return Outcome::Handled; }
                let reports = read_reports(10);
                if reports.is_empty() {
                    reply("[Report] No reports.", &player).await;
                } else {
                    for r in &reports {
                        let reporter = r.get("reporter").and_then(|v| v.as_str()).unwrap_or("?");
                        let target = r.get("target").and_then(|v| v.as_str()).unwrap_or("?");
                        let reason = r.get("reason").and_then(|v| v.as_str()).unwrap_or("?");
                        reply(&format!("[Report] {} -> {} - {}", reporter, target, reason), &player).await;
                    }
                }
                Outcome::Handled
            }
            "!warn" => {
                if !is_owner(&steam, &player) { reply("[Warn] Admin only.", &player).await; return Outcome::Handled; }
                if args.is_empty() { reply("[Warn] Usage: !warn <player> [message]", &player).await; return Outcome::Handled; }
                let parts: Vec<&str> = args.splitn(2, ' ').collect();
                let target = parts[0];
                let msg = parts.get(1).unwrap_or(&"You have been warned by an admin");
                reply(&format!("[WARNING] {}", msg), target).await;
                reply(&format!("[Warn] Warning sent to {}", target), &player).await;
                Outcome::Handled
            }
            _ => Outcome::Ignored,
        }
    }
}
