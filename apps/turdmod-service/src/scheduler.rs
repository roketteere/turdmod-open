// Server restart scheduler + weather cycle.
// Auto-restart every N hours with countdown warnings.
// Weather cycles through patterns on a configurable schedule.

use std::time::Duration;
use tokio::sync::broadcast;

use crate::events::GameEvent;
use crate::pipe_rpc;

// Ramp-down plan: start at 6h while the server is young/low-pop; as it grows we
// shorten this toward the 2h target (more frequent PvP-zone rotation + fresher loot).
// @ctx: Joel 2026-06-08 — "6h now, shorten until we reach 2h as the server grows."
const RESTART_INTERVAL_HOURS: u64 = 6; // rotating PvP zones cycle each restart
const WARNING_MINUTES: &[u64] = &[15, 10, 5, 4, 3, 2, 1];

async fn broadcast(msg: &str) {
    let params = serde_json::json!({ "text": msg });
    pipe_rpc::call("broadcastChat", Some(params)).await.ok();
}

pub async fn restart_scheduler(tx: broadcast::Sender<GameEvent>) {
    let _ = tx; // keep the sender alive
    let interval = Duration::from_secs(RESTART_INTERVAL_HOURS * 3600);
    let mut next_restart = tokio::time::Instant::now() + interval;

    loop {
        let remaining = next_restart - tokio::time::Instant::now();
        let remaining_mins = remaining.as_secs() / 60;

        // Find next warning threshold
        let next_warning = WARNING_MINUTES.iter()
            .filter(|&&m| m < remaining_mins)
            .max()
            .copied();

        let sleep_until = if let Some(warn_min) = next_warning {
            // Sleep until that warning threshold
            let warn_remaining = Duration::from_secs(warn_min * 60);
            next_restart - warn_remaining
        } else if remaining_mins > 0 {
            // Within final minute, count down in 30s increments
            tokio::time::Instant::now() + Duration::from_secs(30)
        } else {
            next_restart
        };

        tokio::time::sleep_until(sleep_until).await;

        let now = tokio::time::Instant::now();
        if now >= next_restart {
            broadcast("[ScummyMap] Restarting NOW — back in ~2 minutes. Reconnect shortly! www.ScummyMap.com").await;
            tokio::time::sleep(Duration::from_secs(5)).await;
            pipe_rpc::call("shutdownServer", Some(serde_json::json!({ "reason": "scheduled" }))).await.ok();
            // Reset timer
            next_restart = tokio::time::Instant::now() + interval;
            // Wait for server to come back up before resuming warnings
            tokio::time::sleep(Duration::from_secs(180)).await;
        } else {
            let left = (next_restart - now).as_secs() / 60;
            if left > 0 {
                broadcast(&format!("[ScummyMap] Server restart in {} minute{} — wrap up and get to safety.", left, if left == 1 { "" } else { "s" })).await;
            } else {
                let secs = (next_restart - now).as_secs();
                broadcast(&format!("[ScummyMap] Server restart in {} seconds!", secs)).await;
            }
        }
    }
}

pub async fn weather_cycle(_tx: broadcast::Sender<GameEvent>) {
    // Weather cycle: clear → building → storm → clearing → clear
    // Each phase lasts 30-45 min (real time)
    let phases: Vec<(&str, f64, u64)> = vec![
        ("clear",    0.0,  2400), // 40 min clear
        ("building", 0.3,  1200), // 20 min light clouds
        ("storm",    0.8,  1800), // 30 min heavy rain
        ("clearing", 0.2,  900),  // 15 min dying down
    ];

    // Initial delay so server is fully booted
    tokio::time::sleep(Duration::from_secs(120)).await;

    loop {
        for (name, severity, duration_secs) in &phases {
            let params = serde_json::json!({ "severity": severity });
            if pipe_rpc::call("setWeather", Some(params)).await.is_ok() {
                pipe_rpc::call("forceWeatherSnapshot", None).await.ok();
                tracing::info!("weather_cycle: phase={} severity={}", name, severity);
            }
            tokio::time::sleep(Duration::from_secs(*duration_secs)).await;
        }
    }
}
