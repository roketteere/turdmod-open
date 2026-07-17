// Auto-welcome: polls getOnlinePlayers, welcomes NEW players only (once per server session)

use crate::pipe_rpc;
use std::collections::HashSet;
use std::time::Duration;
use tokio::time::sleep;

const POLL_INTERVAL: Duration = Duration::from_secs(10);

pub async fn watch_loop(mut shutdown_rx: tokio::sync::watch::Receiver<bool>) {
    let mut known: HashSet<String> = HashSet::new();

    // Pre-populate with anyone already online so we don't re-welcome on service restart
    tokio::select! {
        _ = sleep(Duration::from_secs(20)) => {}
        _ = shutdown_rx.changed() => return,
    }

    if let Ok(result) = pipe_rpc::call("getOnlinePlayers", None).await {
        if let Some(players) = result.get("players").and_then(|p| p.as_array()) {
            for p in players {
                if let Some(name) = p.get("name").and_then(|n| n.as_str()) {
                    known.insert(name.to_string());
                    tracing::info!("welcome: pre-populated {}", name);
                }
            }
        }
    }

    loop {
        tokio::select! {
            _ = sleep(POLL_INTERVAL) => {}
            _ = shutdown_rx.changed() => {
                tracing::info!("welcome: shutdown");
                return;
            }
        }

        let Ok(result) = pipe_rpc::call("getOnlinePlayers", None).await else {
            continue;
        };

        let Some(players) = result.get("players").and_then(|p| p.as_array()) else {
            continue;
        };

        for player in players {
            let Some(name) = player.get("name").and_then(|n| n.as_str()) else {
                continue;
            };

            if !known.contains(name) {
                known.insert(name.to_string());
                tracing::info!("welcome: {}", name);
                let lines = [
                    format!("[ScummyMap] Welcome, {}!", name),
                    "Visit www.ScummyMap.com Official Site".into(),
                    "Type !help for commands | !mods for installed mods".into(),
                ];
                for line in &lines {
                    let params = serde_json::json!({ "message": line, "playerName": name, "channel": "4" });
                    pipe_rpc::call("sendChatLineToPlayer", Some(params)).await.ok();
                }
            }
        }

        let current: HashSet<String> = players
            .iter()
            .filter_map(|p| p.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()))
            .collect();
        known.retain(|n| current.contains(n));
    }
}
