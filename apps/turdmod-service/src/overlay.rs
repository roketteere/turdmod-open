// In-game HUD overlay (Phase 6 MVP) - pushes tasteful on-screen notifications via
// the bridge's sendHudMessage. Deliberately low-spam: a per-player welcome on
// login, and server-wide banners only for NOTABLE kills (headshot or long range).
// The Discord kill feed (live-feed.ts) already carries the full firehose; the
// in-game HUD is reserved for high-signal moments so it doesn't bury the screen.
//
// Env-gated: OVERLAY_ENABLED=1 to turn on (default OFF - the HUD is prime screen
// real estate; the operator opts in). OVERLAY_MIN_DISTANCE_M tunes the "notable"
// range threshold (default 100). @dep: bridge sendHudMessage {text, playerName?}.

use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const SERVER_HUD_GAP: Duration = Duration::from_secs(4); // rate-limit broadcasts

fn enabled() -> bool {
    std::env::var("OVERLAY_ENABLED").map(|v| v == "1" || v.eq_ignore_ascii_case("true")).unwrap_or(false)
}

fn min_distance() -> f64 {
    std::env::var("OVERLAY_MIN_DISTANCE_M").ok().and_then(|v| v.parse().ok()).unwrap_or(100.0)
}

async fn hud_broadcast(text: &str) {
    let params = serde_json::json!({ "text": text });
    pipe_rpc::call("sendHudMessage", Some(params)).await.ok();
}

async fn hud_player(text: &str, player: &str) {
    let params = serde_json::json!({ "text": text, "playerName": player });
    pipe_rpc::call("sendHudMessage", Some(params)).await.ok();
}

pub struct Overlay {
    last_server_hud: Mutex<Option<Instant>>,
}

impl Overlay {
    pub fn new() -> Self {
        Self { last_server_hud: Mutex::new(None) }
    }
}

#[async_trait::async_trait]
impl Mod for Overlay {
    fn name(&self) -> &'static str { "overlay" }
    // event-driven: login + kill

    async fn handle(&self, ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        if !enabled() { return Outcome::Ignored; }

        match ev.event.as_str() {
            "login" => {
                let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if !player.is_empty() {
                    hud_player(&format!("Welcome to the server, {}!", player), &player).await;
                    Outcome::Handled
                } else {
                    Outcome::Ignored
                }
            }

            "kill" => {
                let victim = ev.data.get("victim").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let killer = ev.data.get("killer").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if victim.is_empty() || killer.is_empty() { return Outcome::Ignored; }

                let headshot = ev.data.get("headshot").and_then(|v| v.as_bool()).unwrap_or(false);
                let dist = ev.data.get("distanceM").and_then(|v| v.as_f64()).unwrap_or(0.0);
                // Notable only: headshot OR long-range. Skip the rest (Discord has them).
                if !headshot && dist < min_distance() { return Outcome::Ignored; }

                // Rate-limit server-wide HUD so a firefight doesn't flood the screen.
                // @inv: lock not held across await — capture decision, drop lock, then send.
                let should_send = {
                    let mut last = self.last_server_hud.lock().await;
                    let now = Instant::now();
                    if let Some(prev) = *last {
                        if now.duration_since(prev) < SERVER_HUD_GAP {
                            false
                        } else {
                            *last = Some(now);
                            true
                        }
                    } else {
                        *last = Some(now);
                        true
                    }
                };
                if !should_send { return Outcome::Ignored; }

                let tag = if headshot { "headshot" } else { "long-range" };
                let dist_txt = if dist > 0.0 { format!(" ({:.0}m)", dist) } else { String::new() };
                hud_broadcast(&format!("[{}] {} eliminated {}{}", tag, killer, victim, dist_txt)).await;
                Outcome::Handled
            }

            _ => Outcome::Ignored,
        }
    }
}
