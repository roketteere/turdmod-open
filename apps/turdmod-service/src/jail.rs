// Jail system - !jail <player> <minutes>, !unjail <player>, !jailstatus.
// Teleports to jail coords, re-teleports escapees + auto-releases on a 10s interval tick.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const RATE_LIMIT: Duration = Duration::from_secs(3);
const JAIL_X: f64 = 0.0;
const JAIL_Y: f64 = 0.0;
const JAIL_Z: f64 = 500.0;

#[derive(Clone)]
struct Inmate {
    name: String,
    #[allow(dead_code)]
    steam: String,
    jailed_at: Instant,
    duration: Duration,
    reason: String,
}

fn is_owner(steam: &str, player: &str) -> bool {
    crate::owner::is_owner(steam, player)
}

async fn reply(msg: &str, player: &str) {
    let params = serde_json::json!({ "message": msg, "playerName": player, "channel": "4" });
    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
}

async fn broadcast(msg: &str) {
    let params = serde_json::json!({ "text": msg });
    pipe_rpc::call("broadcastChat", Some(params)).await.ok();
}

async fn teleport_to_jail(player: &str) {
    // coords as strings - bridge mis-parses large bare numbers (see teleport.rs).
    let params = serde_json::json!({ "name": player, "x": JAIL_X.to_string(), "y": JAIL_Y.to_string(), "z": JAIL_Z.to_string() });
    pipe_rpc::call("teleportPlayer", Some(params)).await.ok();
}

pub struct Jail { inmates: Mutex<HashMap<String, Inmate>>, rate: Mutex<HashMap<String, Instant>> }
impl Jail {
    pub fn new() -> Self { Self { inmates: Mutex::new(HashMap::new()), rate: Mutex::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl Mod for Jail {
    fn name(&self) -> &'static str { "jail" }
    fn commands(&self) -> &'static [&'static str] { &["!jail", "!unjail", "!jailstatus"] }
    fn interval(&self) -> Option<Duration> { Some(Duration::from_secs(10)) }

    // Dormant unless someone is jailed.
    async fn active(&self) -> bool { !self.inmates.lock().await.is_empty() }

    // Auto-release expired inmates + re-teleport escapees (the old release check + timeout branch).
    async fn tick(&self, _ctx: &ModCtx) {
        let (released, still_jailed): (Vec<String>, Vec<String>) = {
            let mut inm = self.inmates.lock().await;
            let done: Vec<String> = inm.iter().filter(|(_, i)| i.jailed_at.elapsed() > i.duration).map(|(k, _)| k.clone()).collect();
            let mut released = Vec::new();
            for steam in &done {
                if let Some(i) = inm.remove(steam) { released.push(i.name.clone()); }
            }
            let still: Vec<String> = inm.values().map(|i| i.name.clone()).collect();
            (released, still)
        };
        for name in &released {
            broadcast(&format!("[Jail] {} has been released.", name)).await;
            reply("[Jail] You have been released. Don't do it again.", name).await;
        }
        for name in &still_jailed {
            teleport_to_jail(name).await;
        }
    }

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if ev.event != "chat" { return Outcome::Ignored; }
        let text = ev.data.get("text").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let steam = ev.data.get("steam").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let parts: Vec<&str> = text.split_whitespace().collect();
        let cmd = parts.first().map(|s| s.to_lowercase()).unwrap_or_default();
        if !matches!(cmd.as_str(), "!jail" | "!unjail" | "!jailstatus") { return Outcome::Ignored; }

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
            "!jail" => {
                if !is_owner(&steam, &player) { reply("[Jail] Admin only.", &player).await; return Outcome::Handled; }
                if parts.len() < 2 { reply("[Jail] Usage: !jail <player> [minutes] [reason]", &player).await; return Outcome::Handled; }
                let target = parts[1].to_string();
                let minutes: u64 = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(5);
                let reason = if parts.len() > 3 { parts[3..].join(" ") } else { "misbehavior".to_string() };
                let target_steam = format!("jail_{}", target.to_lowercase());
                {
                    let mut inm = self.inmates.lock().await;
                    inm.insert(target_steam.clone(), Inmate {
                        name: target.clone(), steam: target_steam, jailed_at: Instant::now(),
                        duration: Duration::from_secs(minutes * 60), reason: reason.clone(),
                    });
                }
                teleport_to_jail(&target).await;
                broadcast(&format!("[Jail] {} jailed for {}min - {}", target, minutes, reason)).await;
                Outcome::Handled
            }
            "!unjail" => {
                if !is_owner(&steam, &player) { reply("[Jail] Admin only.", &player).await; return Outcome::Handled; }
                if parts.len() < 2 { reply("[Jail] Usage: !unjail <player>", &player).await; return Outcome::Handled; }
                let target = parts[1].to_string();
                let key = format!("jail_{}", target.to_lowercase());
                let removed = self.inmates.lock().await.remove(&key).is_some();
                if removed { broadcast(&format!("[Jail] {} released early by admin.", target)).await; }
                else { reply(&format!("[Jail] {} is not in jail.", target), &player).await; }
                Outcome::Handled
            }
            "!jailstatus" => {
                let lines: Vec<String> = {
                    let inm = self.inmates.lock().await;
                    if inm.is_empty() {
                        vec!["[Jail] No inmates.".to_string()]
                    } else {
                        inm.values().map(|i| {
                            let remaining = i.duration.checked_sub(i.jailed_at.elapsed()).map(|d| d.as_secs() / 60).unwrap_or(0);
                            format!("[Jail] {} - {}min left ({})", i.name, remaining, i.reason)
                        }).collect()
                    }
                };
                for l in lines { reply(&l, &player).await; }
                Outcome::Handled
            }
            _ => Outcome::Ignored,
        }
    }
}
