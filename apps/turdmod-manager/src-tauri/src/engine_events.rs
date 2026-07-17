// Long-lived listener that subscribes to the engine bridge's named-pipe
// event firehose and re-emits events to the React UI as Tauri events.
//
// Why this exists, given engine_rpc.rs already speaks the pipe:
//   - `engine_rpc::call()` is request/reply only — it opens a connection,
//     sends one request, reads frames until it sees its own response id,
//     then closes. Unsolicited event frames are silently dropped.
//   - The server-loader's pipe server broadcasts events to EVERY connected
//     client (see admin_api.rs::serve_connection — each connection gets its
//     own broadcast::Receiver via event_tx.subscribe()).
//   - To consume events we need a SEPARATE long-lived connection that just
//     reads forever. That's this module.
//
// Wire format on the pipe (matches turdmod-server-loader/src/admin_api.rs):
//   [4 bytes LE length][JSON body]
// Response frames are `{ id, result?, error? }`.
// Event frames are `{ event, data }`.
// We discriminate by presence of `event` (vs `id` for responses).
//
// Emitted to the React side as Tauri event `engine://event` with payload:
//   { event: string, data: any, receivedAtMs: number }

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value as Json;
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncReadExt;
use tokio::net::windows::named_pipe::ClientOptions;

const PIPE_DISCOVERY_REL: &str = r"TurdMOD\engine\pipe.txt";
const EVENT_CHANNEL: &str = "engine://event";

// Reconnect cadence. The engine may not be running when Manager boots, so
// we retry forever with a bounded backoff so logs don't spam too fast.
const RECONNECT_DELAY: Duration = Duration::from_secs(2);
const PIPE_BUSY_DELAY: Duration = Duration::from_millis(50);
const PIPE_BUSY_MAX_ATTEMPTS: u32 = 40;

// Frame size sanity guard. Anything larger than this on the event channel
// is almost certainly a bug; we'd rather log + drop than OOM.
const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024; // 16 MiB

#[derive(Serialize, Clone)]
struct EnginePushedEvent {
    event: String,
    data: Json,
    #[serde(rename = "receivedAtMs")]
    received_at_ms: u64,
}

fn pipe_discovery_path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("APPDATA"))
        .map(|base| PathBuf::from(base).join(PIPE_DISCOVERY_REL))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Spawn the listener as a background Tauri async task. Runs for the
/// lifetime of the process. Safe to call once during setup().
pub fn spawn_listener(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        run_forever(app).await;
    });
}

async fn run_forever(app: AppHandle) {
    loop {
        match try_connect_and_read(&app).await {
            Ok(()) => {
                tracing::info!("[engine-events] pipe stream ended cleanly — reconnecting");
            }
            Err(e) => {
                tracing::debug!("[engine-events] {} (will retry)", e);
            }
        }
        tokio::time::sleep(RECONNECT_DELAY).await;
    }
}

async fn try_connect_and_read(app: &AppHandle) -> Result<(), String> {
    // Resolve pipe name from discovery file. Missing file = engine not
    // running yet, which is the common path; quiet log + retry.
    let disco = pipe_discovery_path().ok_or_else(|| "LOCALAPPDATA not set".to_string())?;
    if !disco.exists() {
        return Err(format!("pipe.txt missing at {} (engine not running)", disco.display()));
    }
    let pipe_name = std::fs::read_to_string(&disco)
        .map_err(|e| format!("read {}: {}", disco.display(), e))?
        .trim()
        .to_string();

    // Same gotchas as engine_rpc::call:
    //   - ERROR_PIPE_BUSY when all instances are in use; retry briefly
    //   - SECURITY_IMPERSONATION required to authenticate against the
    //     server-loader's permissive-DACL pipe
    const ERROR_PIPE_BUSY: i32 = 231;
    const SECURITY_IMPERSONATION: u32 = 0x00020000;
    let mut sock = {
        let mut attempts = 0;
        loop {
            match ClientOptions::new()
                .security_qos_flags(SECURITY_IMPERSONATION)
                .open(&pipe_name)
            {
                Ok(s) => break s,
                Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY)
                        && attempts < PIPE_BUSY_MAX_ATTEMPTS => {
                    attempts += 1;
                    tokio::time::sleep(PIPE_BUSY_DELAY).await;
                }
                Err(e) => return Err(format!("open pipe {}: {}", pipe_name, e)),
            }
        }
    };

    tracing::info!("[engine-events] connected to {}", pipe_name);

    // Read frames forever. We never send anything on this pipe — the
    // broadcaster pushes events automatically to every connected client.
    let mut buf: Vec<u8> = Vec::with_capacity(8 * 1024);
    let mut frames_read: u64 = 0;
    let mut events_emitted: u64 = 0;

    loop {
        // Fill until we have at least the 4-byte length prefix.
        while buf.len() < 4 {
            let mut chunk = [0u8; 4096];
            let n = sock.read(&mut chunk).await
                .map_err(|e| format!("read header: {}", e))?;
            if n == 0 {
                return Err(format!(
                    "pipe closed after {} frames ({} events emitted)",
                    frames_read, events_emitted
                ));
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        let frame_len = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
        if frame_len == 0 || frame_len > MAX_FRAME_BYTES {
            return Err(format!("absurd frame_len={frame_len} — pipe out of sync"));
        }

        // Fill until we have the whole body.
        while buf.len() < 4 + frame_len {
            let mut chunk = vec![0u8; 4096];
            let n = sock.read(&mut chunk).await
                .map_err(|e| format!("read body: {}", e))?;
            if n == 0 {
                return Err("pipe closed mid-frame".to_string());
            }
            buf.extend_from_slice(&chunk[..n]);
        }

        let body = &buf[4..4 + frame_len];
        frames_read += 1;

        // Parse generic JSON; discriminate event vs response by presence
        // of `event` field. Anything we don't recognize is silently
        // dropped (responses to requests we didn't make = noise on this
        // connection, since we never sent any).
        match serde_json::from_slice::<Json>(body) {
            Ok(Json::Object(obj)) => {
                if let Some(Json::String(event_name)) = obj.get("event") {
                    let data = obj.get("data").cloned().unwrap_or(Json::Null);
                    let payload = EnginePushedEvent {
                        event: event_name.clone(),
                        data,
                        received_at_ms: now_ms(),
                    };
                    if let Err(e) = app.emit(EVENT_CHANNEL, &payload) {
                        tracing::warn!("[engine-events] emit failed: {}", e);
                    } else {
                        events_emitted += 1;
                    }
                }
                // else: response frame (has `id`), or some other shape — ignore.
            }
            Ok(_) => {
                tracing::debug!("[engine-events] non-object frame, ignoring");
            }
            Err(e) => {
                tracing::warn!("[engine-events] frame parse error: {}", e);
            }
        }

        buf.drain(..4 + frame_len);
    }
}
