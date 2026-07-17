// DIMs — Dumb Intelligent Mutants — NPC relationship system

pub mod config;
pub mod state;
pub mod chat_router;
pub mod ollama_queue;
pub mod prompt;
pub mod services;

use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::broadcast;

use crate::events::GameEvent;

pub async fn npc_loop(tx: broadcast::Sender<GameEvent>) {
    tokio::time::sleep(Duration::from_secs(30)).await;

    let cfg = config::NpcConfig::load();
    if cfg.npcs.is_empty() {
        tracing::info!("npc: no DIMs configured, disabled");
        let mut rx = tx.subscribe();
        loop { if rx.recv().await.is_err() { return; } }
    }

    tracing::info!("npc: {} DIMs loaded — {}", cfg.npcs.len(),
        cfg.npcs.iter().map(|n| n.display_name.as_str()).collect::<Vec<_>>().join(", "));

    let mut db = state::RelationshipDb::new(PathBuf::from(r"C:\TurdMOD\npc\relationships"));
    let ollama = ollama_queue::OllamaQueue::new();
    let mut router = chat_router::ChatRouter::new();

    let mut rx = tx.subscribe();
    loop {
        let ev = match rx.recv().await {
            Ok(ev) => ev,
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("npc: lagged {}", n);
                continue;
            }
            Err(_) => return,
        };

        match ev.event.as_str() {
            "chat" => {
                router.handle_chat(&ev, &cfg, &mut db, &ollama).await;
            }
            "login" => {
                let player = ev.data.get("player").and_then(|v| v.as_str()).unwrap_or("");
                if player.is_empty() { continue; }
                for npc in &cfg.npcs {
                    if npc.greet_on_enter {
                        if let Some(greet) = &npc.greet_template {
                            tokio::time::sleep(Duration::from_secs(8)).await;
                            let params = serde_json::json!({ "text": greet });
                            crate::pipe_rpc::call("broadcastChat", Some(params)).await.ok();
                        }
                    }
                }
            }
            _ => {}
        }
    }
}
