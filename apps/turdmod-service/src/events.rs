// Persistent event subscriber — holds a long-lived pipe connection and
// broadcasts game events (chat, login, logout, kill) to all listeners.

use std::path::PathBuf;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::net::windows::named_pipe::ClientOptions;
use tokio::sync::{broadcast, watch};

use crate::engine::SharedState;

const SECURITY_IMPERSONATION: u32 = 0x0002_0000;
const RECONNECT_DELAY: Duration = Duration::from_secs(5);
const MAX_FRAME: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, serde::Deserialize)]
pub struct GameEvent {
    pub event: String,
    pub data: serde_json::Value,
}

fn read_pipe_name() -> Option<String> {
    // Auto-detect the live bridge pipe (enumerate + verify against pipe.txt), so a
    // stale pipe.txt after a restart/update no longer silently kills the event stream.
    // Re-evaluated every reconnect loop, so it self-heals onto the new pipe. @dep pipe_rpc.
    crate::pipe_rpc::discover_pipe().ok()
}

async fn connect_and_read(
    pipe_name: &str,
    tx: &broadcast::Sender<GameEvent>,
) -> anyhow::Result<()> {
    let mut client = {
        let mut attempts = 0;
        loop {
            match ClientOptions::new()
                .security_qos_flags(SECURITY_IMPERSONATION)
                .open(pipe_name)
            {
                Ok(c) => break c,
                Err(e) if e.raw_os_error() == Some(231) && attempts < 40 => {
                    attempts += 1;
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                Err(e) => anyhow::bail!("open pipe: {}", e),
            }
        }
    };

    tracing::info!("events: connected to {}", pipe_name);

    loop {
        let mut len_buf = [0u8; 4];
        client.read_exact(&mut len_buf).await?;
        let len = u32::from_le_bytes(len_buf) as usize;

        if len == 0 || len > MAX_FRAME {
            anyhow::bail!("bad frame length {}", len);
        }

        let mut body = vec![0u8; len];
        client.read_exact(&mut body).await?;

        let val: serde_json::Value = match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if val.get("id").is_some() {
            continue;
        }

        if let Some(name) = val.get("event").and_then(|v| v.as_str()) {
            let ev = GameEvent {
                event: name.to_string(),
                data: val.get("data").cloned().unwrap_or(serde_json::Value::Null),
            };
            tracing::debug!("events: {}", ev.event);
            let _ = tx.send(ev);
        }
    }
}

pub fn start(state: SharedState, mut shutdown_rx: watch::Receiver<bool>) -> broadcast::Sender<GameEvent> {
    let (tx, _) = broadcast::channel::<GameEvent>(256);
    let tx2 = tx.clone();

    tokio::spawn(async move {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(15)) => {}
            _ = shutdown_rx.changed() => return,
        }

        loop {
            tokio::select! {
                _ = shutdown_rx.changed() => {
                    tracing::info!("events: shutdown");
                    return;
                }
                _ = async {
                    // Don't gate on state.running — the server may be running
                    // from a previous service instance after a service restart
                    let Some(pipe_name) = read_pipe_name() else {
                        tokio::time::sleep(RECONNECT_DELAY).await;
                        return;
                    };
                    if let Err(e) = connect_and_read(&pipe_name, &tx2).await {
                        tracing::warn!("events: disconnected: {}", e);
                    }
                    tokio::time::sleep(RECONNECT_DELAY).await;
                } => {}
            }
        }
    });

    tx
}
