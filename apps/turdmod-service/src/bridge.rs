//! Command model + bridge transports — the shared core of the router (interpreter).
//!
//! chat_cmds parses in-game input into a `Command`; `router::dispatch` executes it by
//! routing to one of the `Bridge`s. Phase 2 implements `EnginePipe`; `Rcon`/`Ollama`/
//! `FileState` arrive in Phases 3-4. The reply/format helpers live here so every input
//! source and the router share one implementation.

use std::time::Duration;

use crate::pipe_rpc;

pub const OWNER_STEAM_ID: &str = "YOUR_STEAM_ID_1";
pub const OWNER_NAME: &str = "YOUR_OWNER_NAME";

/// Normalized command — the "bytecode" the router interprets.
#[derive(Clone, Debug)]
pub struct Command {
    pub action: String, // verb, lowercased, no leading '!'
    pub args: String,   // raw arg string after the verb
    pub player: String,
    pub steam: String,
}

/// The communication bridges the router can target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bridge {
    EnginePipe, // in-process engine bridge over the named pipe
    Rcon,       // BattlEye RCON (Phase 3)
    Ollama,     // local LLM (Phase 4)
    FileState,  // json state stores (Phase 4)
}

/// True if this caller is the server owner/admin.
pub fn is_admin(steam: &str, player: &str) -> bool {
    steam == OWNER_STEAM_ID || steam == "YOUR_STEAM_ID_2" || player == OWNER_NAME
}

/// EnginePipe bridge: invoke a bridge handler over the named pipe.
pub async fn engine(method: &str, params: serde_json::Value) -> anyhow::Result<serde_json::Value> {
    pipe_rpc::call(method, Some(params)).await
}

/// Private yellow reply to one player (channel 4), split to SCUM's ~200-char line limit.
pub async fn reply(msg: &str, player: &str) {
    for line in split_chat_lines(msg, 180) {
        let params = serde_json::json!({ "message": line, "playerName": player, "channel": "4" });
        if let Err(e) = pipe_rpc::call("sendChatLineToPlayer", Some(params)).await {
            tracing::warn!("bridge::reply to {} failed: {}", player, e);
            break;
        }
    }
}

/// Standard boxed reply: `[ModName]` / `==Title==` / lines / `==END==`.
pub async fn fmt_reply(player: &str, mod_name: &str, title: &str, lines: &[&str]) {
    let p = |m: String| serde_json::json!({ "message": m, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(p(format!("[{}]", mod_name)))).await.ok();
    pipe_rpc::call("sendChatLineToPlayer", Some(p(format!("=={}==", title)))).await.ok();
    for line in lines {
        pipe_rpc::call("sendChatLineToPlayer", Some(p((*line).to_string()))).await.ok();
    }
    pipe_rpc::call("sendChatLineToPlayer", Some(p("==END==".to_string()))).await.ok();
}

fn split_chat_lines(msg: &str, max_len: usize) -> Vec<&str> {
    if msg.len() <= max_len {
        return vec![msg];
    }
    let mut lines = Vec::new();
    let mut remaining = msg;
    while !remaining.is_empty() {
        if remaining.len() <= max_len {
            lines.push(remaining);
            break;
        }
        let split_at = remaining[..max_len].rfind(' ').unwrap_or(max_len);
        lines.push(&remaining[..split_at]);
        remaining = remaining[split_at..].trim_start();
    }
    lines
}

/// Walk SCUM's time-of-day to `target` hour (0..24) in crash-safe steps.
/// @inv: the bridge `setTimeOfDay` handler hard-clamps to ±2h/call ON PURPOSE — a
/// single large jump crosses OnRep_NighttimeDarkness and crashes SCUMServer ~50s
/// later. So we RAMP: call repeatedly (each ≤2h) with a pause until it arrives
/// (`clamped:false`). Honors up to 24h.
/// @brk: SINGLE runner, LATEST target. Two overlapping !day/!night used to spawn two
///   ramps that fought (one to noon, one to midnight) so time never settled. Now a new
///   call just updates the shared target and the in-flight runner redirects to it.
pub async fn ramp_time_of_day(target: f64) -> bool {
    RAMP_TARGET.store(target.to_bits(), std::sync::atomic::Ordering::SeqCst);
    // If a ramp is already walking, it'll pick up the new target — don't start a second.
    if RAMP_ACTIVE.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return true;
    }
    let mut ok = true;
    for _ in 0..32 {
        let t = f64::from_bits(RAMP_TARGET.load(std::sync::atomic::Ordering::SeqCst));
        match pipe_rpc::call("setTimeOfDay", Some(serde_json::json!({ "hours": t }))).await {
            Ok(resp) => {
                if !resp.get("clamped").and_then(|v| v.as_bool()).unwrap_or(false) {
                    break; // arrived at the latest target
                }
            }
            Err(_) => { ok = false; break; }
        }
        tokio::time::sleep(Duration::from_millis(1500)).await;
    }
    RAMP_ACTIVE.store(false, std::sync::atomic::Ordering::SeqCst);
    ok
}
static RAMP_TARGET: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static RAMP_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
