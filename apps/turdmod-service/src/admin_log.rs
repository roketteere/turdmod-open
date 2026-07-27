// Admin action audit log — every owner !command logged to JSONL

use std::fs::OpenOptions;
use std::io::Write;
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const LOG_PATH: &str = r"C:\TurdMOD\data\admin-actions.log";

fn append_entry(admin: &str, steam: &str, action: &str, args: &str) {
    let _ = std::fs::create_dir_all(r"C:\TurdMOD\data");
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let entry = serde_json::json!({
        "ts": ts, "admin": admin, "steam": steam, "action": action, "args": args
    });
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(LOG_PATH) {
        let _ = writeln!(f, "{}", entry);
    }
}

fn read_tail(n: usize) -> Vec<String> {
    let Ok(content) = std::fs::read_to_string(LOG_PATH) else { return vec![] };
    content.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.to_string())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .take(n.min(50))
        .rev()
        .collect()
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

pub struct AdminLog;

#[async_trait::async_trait]
impl Mod for AdminLog {
    fn name(&self) -> &'static str { "admin_log" }
    // event-driven: needs all chat events to filter by owner steam id

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }

        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if !crate::owner::is_owner_steam(&steam) { return Outcome::Ignored; }

        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        if !text.starts_with('!') { return Outcome::Ignored; }

        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let (cmd, args) = match text.find(' ') {
            Some(i) => (text[..i].to_lowercase(), text[i + 1..].trim().to_string()),
            None => (text.to_lowercase(), String::new()),
        };

        if cmd == "!adminlog" {
            let n: usize = args.trim().parse().unwrap_or(10);
            let lines = read_tail(n);
            if lines.is_empty() {
                reply("[AdminLog] No entries", &player).await;
            } else {
                reply(&format!("[AdminLog] Last {} actions:", lines.len()), &player).await;
                for line in &lines {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                        let action = v.get("action").and_then(|x| x.as_str()).unwrap_or("?");
                        let a = v.get("args").and_then(|x| x.as_str()).unwrap_or("");
                        reply(&format!("  {} {}", action, a), &player).await;
                    }
                }
            }
            return Outcome::Handled;
        }

        // Log every other owner command
        append_entry(&player, &steam, &cmd, &args);
        Outcome::Handled
    }
}
