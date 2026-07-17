// Named-pipe JSON-RPC client for the turdmod_server_loader.dll RPC surface.
//
// The bridge cppmod registers handlers (broadcastChat, getOnlinePlayers,
// dumpWidgets, describeWidget, ...) on the named pipe whose name lives in
// %LOCALAPPDATA%\TurdMOD\engine\pipe.txt. Each request is a length-prefixed
// JSON frame:
//
//   [4 bytes LE length][JSON body bytes]
//
// Request: { id: string, method: string, params?: any }
// Response: { id: string, result?: any, error?: any }
//
// Matched by uuid id. The Schema Browser UI uses this to call any RPC
// (dumpWidgets / describeWidget / findFunctions / dumpClasses / ...)
// from the React frontend through a single generic Tauri command.

use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::ClientOptions;
use tokio::time::timeout;
use uuid::Uuid;

const PIPE_DISCOVERY_REL: &str = r"TurdMOD\engine\pipe.txt";
const RPC_LOG_REL: &str = r"TurdMOD\engine-rpc.log";
const RPC_TIMEOUT: Duration = Duration::from_secs(30);

fn pipe_discovery_path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("APPDATA"))
        .map(|base| PathBuf::from(base).join(PIPE_DISCOVERY_REL))
}

/// Path to the persistent RPC log. Each request + response gets one
/// JSON line appended. Exposed via the manager_engine_rpc_log_path
/// Tauri command so the UI can offer an "Open log" button.
pub fn rpc_log_path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("APPDATA"))
        .map(|base| PathBuf::from(base).join(RPC_LOG_REL))
}

/// Append one JSON line to the RPC log. Failures are swallowed —
/// logging must never break the call path.
fn log_rpc(method: &str, params: Option<&serde_json::Value>, outcome: &str, payload: &str) {
    let Some(path) = rpc_log_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let ts_millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let entry = serde_json::json!({
        "tsMs": ts_millis,
        "method": method,
        "params": params,
        "outcome": outcome,   // "ok" or "error"
        "payload": serde_json::from_str::<serde_json::Value>(payload)
            .unwrap_or_else(|_| serde_json::Value::String(payload.to_string())),
    });
    let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) else {
        return;
    };
    let _ = writeln!(f, "{}", entry);
}

#[derive(Serialize)]
struct RpcRequest<'a> {
    id: &'a str,
    method: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<&'a serde_json::Value>,
}

#[derive(Deserialize)]
struct RpcResponse {
    id: String,
    #[serde(default)]
    result: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<serde_json::Value>,
}

/// Send one JSON-RPC request to the engine pipe and return its `result`
/// (or a typed error). The request id is randomized; we keep reading
/// framed messages until we see one with our id, so unsolicited events
/// emitted on the pipe in the meantime are ignored.
pub async fn call(
    method: &str,
    params: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let pipe_file = pipe_discovery_path()
        .ok_or_else(|| "LOCALAPPDATA/APPDATA not set".to_string())?;
    if !pipe_file.exists() {
        return Err(format!(
            "no pipe.txt at {} — is the engine running?",
            pipe_file.display()
        ));
    }
    let pipe_name = std::fs::read_to_string(&pipe_file)
        .map_err(|e| format!("read {}: {}", pipe_file.display(), e))?
        .trim()
        .to_string();

    let id = Uuid::new_v4().to_string();
    let body = serde_json::to_vec(&RpcRequest {
        id: &id,
        method,
        params: params.as_ref(),
    })
    .map_err(|e| format!("serialize: {}", e))?;
    if body.len() > u32::MAX as usize {
        return Err("request body exceeds 4 GiB frame limit".to_string());
    }
    let len = (body.len() as u32).to_le_bytes();

    let work = async move {
        // Two gotchas opening a Windows named pipe from tokio that Node's
        // net.createConnection handles invisibly:
        //
        //   1) ERROR_PIPE_BUSY (231) — all pipe instances in use; wait + retry.
        //      The bridge runs the pipe with a small max-instances cap and
        //      the companion holds one slot, so the first attempt routinely
        //      hits this.
        //
        //   2) Access flags — by default tokio's ClientOptions::open does
        //      NOT set SECURITY_SQOS_PRESENT + SECURITY_IMPERSONATION,
        //      which some pipe servers require. Without it the open
        //      returns ERROR_ACCESS_DENIED even with a permissive DACL.
        //      Node sets these by default; tokio leaves it to the caller.
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
                            && attempts < 40 => {
                        attempts += 1;
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                    Err(e) => {
                        return Err(format!("open pipe {}: {}", pipe_name, e));
                    }
                }
            }
        };
        sock.write_all(&len).await.map_err(|e| format!("write len: {}", e))?;
        sock.write_all(&body).await.map_err(|e| format!("write body: {}", e))?;
        sock.flush().await.map_err(|e| format!("flush: {}", e))?;

        let mut buf = Vec::<u8>::with_capacity(4096);
        loop {
            // Frame header.
            while buf.len() < 4 {
                let mut chunk = [0u8; 4096];
                let n = sock
                    .read(&mut chunk)
                    .await
                    .map_err(|e| format!("read header: {}", e))?;
                if n == 0 {
                    return Err("pipe closed before response".to_string());
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            let frame_len = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
            // Frame body.
            while buf.len() < 4 + frame_len {
                let mut chunk = vec![0u8; 4096];
                let n = sock
                    .read(&mut chunk)
                    .await
                    .map_err(|e| format!("read body: {}", e))?;
                if n == 0 {
                    return Err("pipe closed mid-frame".to_string());
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            let frame = &buf[4..4 + frame_len];
            let resp: serde_json::Result<RpcResponse> = serde_json::from_slice(frame);
            let frame_owned = frame.to_vec();
            // Drain whether we matched or not.
            buf.drain(..4 + frame_len);
            match resp {
                Ok(r) if r.id == id => {
                    if let Some(err) = r.error {
                        return Err(format!("rpc error: {}", err));
                    }
                    return Ok(r.result.unwrap_or(serde_json::Value::Null));
                }
                Ok(_) => {
                    // Unrelated event frame on the same pipe. Ignore and keep reading.
                    continue;
                }
                Err(_) => {
                    // Couldn't parse as RpcResponse — most likely an event frame
                    // with a different shape. Skip it.
                    let _ = frame_owned;
                    continue;
                }
            }
        }
    };

    let result = match timeout(RPC_TIMEOUT, work).await {
        Ok(r) => r,
        Err(_) => Err(format!("rpc timeout after {:?}", RPC_TIMEOUT)),
    };
    match &result {
        Ok(v) => log_rpc(method, params.as_ref(), "ok", &v.to_string()),
        Err(e) => log_rpc(method, params.as_ref(), "error", e),
    }
    result
}

