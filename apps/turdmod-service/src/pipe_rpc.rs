// Named-pipe JSON-RPC proxy — connects to the bridge pipe on the local
// machine and forwards requests from the HTTP API.
//
// Protocol: length-prefixed frames (4 bytes LE + JSON body).
// Request:  { id, method, params? }
// Response: { id, result?, error? }

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::ClientOptions;
use tokio::sync::Semaphore;
use tokio::time::timeout;

const RPC_TIMEOUT: Duration = Duration::from_secs(30);

// ── Global bridge gate ───────────────────────────────────────────────────────
// The engine bridge is a SINGLE named pipe served by the SINGLE game thread.
// Firing concurrent RPCs has crashed SCUMServer. Every pipe_rpc::call (88 callers
// across the service) acquires this 1-permit semaphore first, so bridge access is
// strictly one-at-a-time and FIRST-COME-FIRST-SERVE (tokio grants permits FIFO).
// No more "1000 things at once" — calls queue in arrival order and drain smoothly.
static BRIDGE_GATE: Semaphore = Semaphore::const_new(1);
const SECURITY_IMPERSONATION: u32 = 0x00020000;
const ERROR_PIPE_BUSY: i32 = 231;

#[derive(Serialize)]
struct RpcReq<'a> {
    id: &'a str,
    method: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<&'a serde_json::Value>,
}

#[derive(Deserialize)]
struct RpcResp {
    id: String,
    #[serde(default)]
    result: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<serde_json::Value>,
}

/// Live `\\.\pipe\turdmod-engine-*` entries (read_dir lists only EXISTING pipes,
/// so a dead/stale entry simply won't appear). Empty if the namespace can't be
/// enumerated — callers fall back to the pipe.txt hint.
pub fn list_turdmod_pipes() -> Vec<String> {
    std::fs::read_dir(r"\\.\pipe\")
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| n.starts_with("turdmod-engine-"))
                .map(|n| format!(r"\\.\pipe\{}", n))
                .collect()
        })
        .unwrap_or_default()
}

/// The pipe name the bridge last wrote to pipe.txt (a HINT — can go stale across
/// restarts/updates, which is why we cross-check it against the live list).
pub fn pipe_txt_hint() -> Option<String> {
    let appdata = std::env::var("LOCALAPPDATA").ok()?;
    let path = PathBuf::from(appdata).join("TurdMOD").join("engine").join("pipe.txt");
    let name = std::fs::read_to_string(&path).ok()?
        .trim().trim_start_matches('\u{feff}').to_string();
    if name.is_empty() { None } else { Some(name) }
}

/// Auto-detect the live bridge pipe. Robust across restarts/updates: enumerate the
/// live pipes and prefer pipe.txt's value ONLY if it's actually live; otherwise pick
/// a live one (highest id ≈ newest). Falls back to the raw hint if enumeration is
/// unavailable. @inv: never trust a stale pipe.txt over an enumerated live pipe.
pub fn discover_pipe() -> Result<String> {
    let hint = pipe_txt_hint();
    let live = list_turdmod_pipes();
    if let Some(h) = &hint {
        if live.iter().any(|p| p.eq_ignore_ascii_case(h)) {
            return Ok(h.clone());
        }
    }
    if let Some(p) = live.into_iter().max() {
        return Ok(p);
    }
    if let Some(h) = hint {
        return Ok(h); // last resort: enumeration unavailable, trust the file
    }
    bail!("no live turdmod-engine pipe found (none enumerated, no pipe.txt)");
}

pub async fn call(
    method: &str,
    params: Option<serde_json::Value>,
) -> Result<serde_json::Value> {
    let pipe_name = discover_pipe()?;
    let id = uuid::Uuid::new_v4().to_string();

    let body = serde_json::to_vec(&RpcReq {
        id: &id,
        method,
        params: params.as_ref(),
    })?;
    let len = (body.len() as u32).to_le_bytes();

    let work = async {
        // Open pipe with retry on PIPE_BUSY
        let mut sock = {
            let mut attempts = 0;
            loop {
                match ClientOptions::new()
                    .security_qos_flags(SECURITY_IMPERSONATION)
                    .open(&pipe_name)
                {
                    Ok(s) => break s,
                    Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) && attempts < 40 => {
                        attempts += 1;
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                    Err(e) => bail!("open pipe {}: {}", pipe_name, e),
                }
            }
        };

        sock.write_all(&len).await?;
        sock.write_all(&body).await?;
        sock.flush().await?;

        // Read frames until we find our response
        let mut buf = Vec::with_capacity(4096);
        loop {
            while buf.len() < 4 {
                let mut chunk = [0u8; 4096];
                let n = sock.read(&mut chunk).await?;
                if n == 0 { bail!("pipe closed before response"); }
                buf.extend_from_slice(&chunk[..n]);
            }
            let frame_len = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
            while buf.len() < 4 + frame_len {
                let mut chunk = vec![0u8; 4096];
                let n = sock.read(&mut chunk).await?;
                if n == 0 { bail!("pipe closed mid-frame"); }
                buf.extend_from_slice(&chunk[..n]);
            }
            let frame = &buf[4..4 + frame_len];
            if let Ok(r) = serde_json::from_slice::<RpcResp>(frame) {
                if r.id == id {
                    buf.drain(..4 + frame_len);
                    if let Some(err) = r.error {
                        bail!("rpc error: {}", err);
                    }
                    return Ok(r.result.unwrap_or(serde_json::Value::Null));
                }
            }
            // Skip unrelated event frames
            buf.drain(..4 + frame_len);
        }
    };

    // FCFS gate: hold a single global permit for the duration of the bridge
    // interaction so only ONE RPC touches the pipe/game thread at a time, in
    // arrival order. Dropped when this fn returns. @inv: prevents the concurrent-
    // RPC crash; turns the free-for-all into an orderly queue.
    let _permit = BRIDGE_GATE.acquire().await.expect("bridge gate never closed");
    match timeout(RPC_TIMEOUT, work).await {
        Ok(r) => r,
        Err(_) => bail!("rpc timeout after {:?}", RPC_TIMEOUT),
    }
}
