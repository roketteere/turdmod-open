//! RPC server — bridges the companion process (Node.js, separate PID) to the
//! engine DLL living inside SCUMServer.exe.
//!
//! ## Transport
//!
//! Windows named pipe: `\\.\pipe\turdmod-engine-<server-pid>`.
//! Chosen over TCP because:
//!   - We're Windows-only (SCUMServer.exe is Win64).
//!   - Named pipes are kernel-mediated; no port collision, no firewall.
//!   - Pipe name encodes the server PID so multiple SCUM instances on the
//!     same box don't collide.
//!
//! On startup the chosen pipe name is written to
//! `%LOCALAPPDATA%\TurdMOD\engine\pipe.txt` so the companion can discover
//! it without hard-coding anything.
//!
//! ## Wire protocol
//!
//! Length-prefixed JSON frames over the pipe (uint32 LE byte count, then UTF-8
//! JSON body). Bidirectional. Three message shapes:
//!
//! ```text
//! Request:  { "id": "<uuid>", "method": "<name>", "params": <json> }
//! Response: { "id": "<uuid>", "result": <json> }
//!        OR { "id": "<uuid>", "error": { "code": <int>, "message": "<str>" } }
//! Event:    { "event": "<name>", "data": <json> }   (engine → companion, no id)
//! ```
//!
//! ## Lifecycle
//!
//! Two entry points:
//!   - [`init`] — the lib.rs init thread shim. Logs status; awaits Part C
//!     to call [`start_rpc`] with a real [`EngineApi`] implementor.
//!   - [`start_rpc`] — once Part C provides its hook-backed `EngineApi`,
//!     call this from a Tokio runtime to bring the pipe online.

use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};

/// One-shot marker so we log the first successful pipe bind to the visible sink (diagnostics).
static FIRST_BOUND: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
use tokio::sync::{broadcast, Mutex};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::logging;
use crate::runtime::Runtime;

/// Lib.rs init-thread shim. Part C will replace this with a call that
/// constructs `HookApi`, builds a Tokio runtime, and calls [`start_rpc`].
pub fn init(_runtime: &Runtime) {
    logging::log(
        "[admin_api] ready — awaiting Part C HookApi to call start_rpc()",
    );
}

// ─── Public error type ───────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum EngineError {
    NotImplemented,
    InvalidParams { msg: String },
    NoSuchPlayer,
    HookFailed { msg: String },
}

impl EngineError {
    fn code(&self) -> i32 {
        match self {
            EngineError::NotImplemented => -32601,
            EngineError::InvalidParams { .. } => -32602,
            EngineError::NoSuchPlayer => 1001,
            EngineError::HookFailed { .. } => 1002,
        }
    }

    fn message(&self) -> String {
        match self {
            EngineError::NotImplemented => "not implemented".into(),
            EngineError::InvalidParams { msg } => format!("invalid params: {msg}"),
            EngineError::NoSuchPlayer => "no such player".into(),
            EngineError::HookFailed { msg } => format!("hook failed: {msg}"),
        }
    }
}

// ─── EngineApi trait ─────────────────────────────────────────────────────────

#[async_trait]
pub trait EngineApi: Send + Sync {
    async fn broadcast_chat(&self, params: Json) -> Result<Json, EngineError> {
        let _ = params;
        Err(EngineError::NotImplemented)
    }

    async fn send_private_message(&self, params: Json) -> Result<Json, EngineError> {
        let _ = params;
        Err(EngineError::NotImplemented)
    }

    async fn teleport_player(&self, params: Json) -> Result<Json, EngineError> {
        let _ = params;
        Err(EngineError::NotImplemented)
    }

    async fn get_online_players(&self, params: Json) -> Result<Json, EngineError> {
        let _ = params;
        Err(EngineError::NotImplemented)
    }

    async fn get_player_position(&self, params: Json) -> Result<Json, EngineError> {
        let _ = params;
        Err(EngineError::NotImplemented)
    }

    async fn spawn_vehicle(&self, params: Json) -> Result<Json, EngineError> {
        let _ = params;
        Err(EngineError::NotImplemented)
    }

    async fn execute_admin_command(&self, params: Json) -> Result<Json, EngineError> {
        let _ = params;
        Err(EngineError::NotImplemented)
    }

    async fn ping(&self, _params: Json) -> Result<Json, EngineError> {
        Ok(serde_json::json!({
            "pong": true,
            "engineVersion": env!("CARGO_PKG_VERSION"),
        }))
    }
}

// ─── Wire types ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct RpcRequest {
    id: String,
    method: String,
    #[serde(default)]
    params: Json,
}

#[derive(Debug, Serialize)]
struct RpcResponse {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Json>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcErrorBody>,
}

#[derive(Debug, Serialize)]
struct RpcErrorBody {
    code: i32,
    message: String,
}

#[derive(Debug, Serialize)]
struct RpcEvent {
    event: String,
    data: Json,
}

impl RpcResponse {
    fn ok(id: String, result: Json) -> Self {
        Self { id, result: Some(result), error: None }
    }

    fn err(id: String, e: EngineError) -> Self {
        Self {
            id,
            result: None,
            error: Some(RpcErrorBody { code: e.code(), message: e.message() }),
        }
    }
}

// ─── EventBroadcaster ────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct EventBroadcaster {
    tx: broadcast::Sender<Arc<Vec<u8>>>,
}

impl EventBroadcaster {
    pub fn emit(&self, event: &str, data: Json) {
        let envelope = RpcEvent { event: event.to_string(), data };
        match frame_encode(&envelope) {
            Ok(bytes) => {
                let _ = self.tx.send(Arc::new(bytes));
            }
            Err(e) => error!("[admin_api] failed to encode event {event}: {e}"),
        }
    }
}

// ─── Entry point ─────────────────────────────────────────────────────────────

pub async fn start_rpc(
    api: Arc<dyn EngineApi + Send + Sync>,
) -> Result<(EventBroadcaster, JoinHandle<()>), io::Error> {
    let pipe_name = build_pipe_name();
    write_discovery_file(&pipe_name)?;
    info!("[admin_api] listening on {pipe_name}");

    let (event_tx, _) = broadcast::channel::<Arc<Vec<u8>>>(256);
    let broadcaster = EventBroadcaster { tx: event_tx.clone() };

    let handle = tokio::spawn(accept_loop(pipe_name, api, event_tx));
    Ok((broadcaster, handle))
}

/// Build a SECURITY_ATTRIBUTES with a NULL DACL — "allow any client on this
/// machine to open the pipe regardless of integrity level".
///
/// Without this, the loader runs inside SCUMServer.exe (elevated; created
/// via UAC), and the kernel applies the default DACL from SCUMServer's
/// token — which restricts non-admin clients' access to pipe data. Tokio's
/// named-pipe client gets ERROR_ACCESS_DENIED opening the pipe from the
/// non-elevated Manager.  Node's libuv happens to bypass the check via
/// the access flags it uses; tokio doesn't. Fix it at the source: create
/// the pipe with no ACL filtering so any local user can connect.
///
/// Security: the pipe accepts JSON-RPC for SCUM admin commands. Anyone
/// who can reach the named pipe (which by default any local user can,
/// given the name) is implicitly trusted. This is the same trust model
/// the test mjs script already operates under.
///
/// SAFETY: caller must keep `sd` (and the underlying `SECURITY_DESCRIPTOR`
/// it points at) alive until after the CreateNamedPipe call returns.
unsafe fn build_permissive_security_attributes(
    sd: &mut windows_sys::Win32::Security::SECURITY_DESCRIPTOR,
) -> windows_sys::Win32::Security::SECURITY_ATTRIBUTES {
    use windows_sys::Win32::Security::{
        InitializeSecurityDescriptor, SetSecurityDescriptorDacl,
        SECURITY_ATTRIBUTES,
    };
    // SECURITY_DESCRIPTOR_REVISION = 1 — its location moves between
    // windows-sys feature flag combos; hardcode the documented value.
    const SECURITY_DESCRIPTOR_REVISION: u32 = 1;
    InitializeSecurityDescriptor(
        sd as *mut _ as *mut _,
        SECURITY_DESCRIPTOR_REVISION,
    );
    SetSecurityDescriptorDacl(
        sd as *mut _ as *mut _,
        1,                        // bDaclPresent = TRUE
        std::ptr::null_mut(),     // pDacl = NULL → no DACL → allow everyone
        0,                        // bDaclDefaulted = FALSE
    );
    SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd as *mut _ as *mut _,
        bInheritHandle: 0,
    }
}

async fn accept_loop(
    pipe_name: String,
    api: Arc<dyn EngineApi + Send + Sync>,
    event_tx: broadcast::Sender<Arc<Vec<u8>>>,
) {
    // POOL of concurrent acceptors. @brk: with a single accept loop only ONE
    // pipe instance is ever listening; the moment a client connects there's a
    // window (until the next create()) with NO listening instance. A burst of
    // connections — the service's persistent event stream + per-request RPC +
    // EVERY background mod's pipe_rpc call at boot — outruns one-at-a-time accept
    // and the extras fail with ERROR_PIPE_BUSY (os err 231). That blocked
    // external engine RPC (loadAsset) on a busy server (OVH, 2026-06-13). N
    // acceptors keep N instances listening simultaneously.
    const POOL: usize = 16;
    let mut handles = Vec::with_capacity(POOL);
    for _ in 0..POOL {
        handles.push(tokio::spawn(accept_one(
            pipe_name.clone(),
            api.clone(),
            event_tx.clone(),
        )));
    }
    crate::logging::log(&format!(
        "[admin_api] acceptor pool spawned ({POOL}) for {pipe_name}"
    ));
    for h in handles {
        let _ = h.await;
    }
}

async fn accept_one(
    pipe_name: String,
    api: Arc<dyn EngineApi + Send + Sync>,
    event_tx: broadcast::Sender<Arc<Vec<u8>>>,
) {
    loop {
        let server = unsafe {
            let mut sd: windows_sys::Win32::Security::SECURITY_DESCRIPTOR =
                std::mem::zeroed();
            let mut sa = build_permissive_security_attributes(&mut sd);
            ServerOptions::new()
                .first_pipe_instance(false)
                .create_with_security_attributes_raw(
                    &pipe_name,
                    &mut sa as *mut _ as *mut _,
                )
        };
        let server = match server {
            Ok(s) => s,
            Err(e) => {
                error!("[admin_api] create pipe: {e}; retrying in 1s");
                crate::logging::log(&format!("[admin_api] create pipe FAILED: {e} (retrying 1s)"));
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                continue;
            }
        };
        if !FIRST_BOUND.swap(true, std::sync::atomic::Ordering::SeqCst) {
            crate::logging::log(&format!("[admin_api] pipe instance BOUND: {pipe_name}"));
        }

        if let Err(e) = server.connect().await {
            warn!("[admin_api] connect await: {e}");
            crate::logging::log(&format!("[admin_api] connect await FAILED: {e}"));
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            continue;
        }

        info!("[admin_api] client connected");
        let api_clone = api.clone();
        let event_rx = event_tx.subscribe();
        tokio::spawn(serve_connection(server, api_clone, event_rx));
    }
}

async fn serve_connection(
    pipe: NamedPipeServer,
    api: Arc<dyn EngineApi + Send + Sync>,
    mut event_rx: broadcast::Receiver<Arc<Vec<u8>>>,
) {
    // Split the pipe into independent read and write halves. Without this,
    // both the request-reader and the event-writer task contend for a
    // single Mutex<NamedPipeServer>. The reader holds the mutex while
    // awaiting a frame, so the writer can't push events to a subscriber
    // that doesn't send requests (the CLI tail / scumpilot subscribe-only
    // pattern). Bug observed 2026-05-22: tail connected 6s, smoke.tick
    // firing every 1s, zero frames delivered. After split, R and W are
    // independent — events flow even on a silent reader.
    //
    // The write half is still shared between RPC responses and event
    // pushes, so we keep a Mutex on it for write ordering.
    let (mut reader, writer) = tokio::io::split(pipe);
    let writer = Arc::new(Mutex::new(writer));
    let event_writer = writer.clone();

    let writer_task = tokio::spawn(async move {
        loop {
            match event_rx.recv().await {
                Ok(frame) => {
                    let mut guard = event_writer.lock().await;
                    if let Err(e) = guard.write_all(&frame).await {
                        debug!("[admin_api] event write: {e} — client likely disconnected");
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("[admin_api] event subscriber lagged, dropped {n} events");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    loop {
        let req_bytes = match read_frame(&mut reader).await {
            Ok(b) => b,
            Err(e) => {
                if e.kind() != io::ErrorKind::UnexpectedEof && e.kind() != io::ErrorKind::BrokenPipe {
                    warn!("[admin_api] read frame: {e}");
                }
                info!("[admin_api] client disconnected");
                break;
            }
        };

        let req: RpcRequest = match serde_json::from_slice(&req_bytes) {
            Ok(r) => r,
            Err(e) => {
                warn!("[admin_api] malformed request: {e}");
                continue;
            }
        };

        debug!("[admin_api] → {} (id={})", req.method, req.id);
        let response = dispatch(&*api, req).await;
        debug!("[admin_api] ← id={}", response.id);

        let frame = match frame_encode(&response) {
            Ok(f) => f,
            Err(e) => { error!("[admin_api] encode response: {e}"); continue; }
        };

        let mut guard = writer.lock().await;
        if let Err(e) = guard.write_all(&frame).await {
            warn!("[admin_api] write response: {e}");
            break;
        }
    }

    writer_task.abort();
}

async fn dispatch(api: &dyn EngineApi, req: RpcRequest) -> RpcResponse {
    // First, check for an externally-registered C-ABI handler (the C++
    // UE4SS engine bridge plugs in here via `extern_api`). External
    // handlers take precedence over the trait implementation so the
    // C++ mod can override any method.
    if let Some(fn_ptr) = crate::extern_api::lookup_handler(&req.method) {
        debug!("[admin_api] dispatching {} via extern_api handler", req.method);
        let result = crate::extern_api::call_handler(&req.method, fn_ptr, req.params);
        return match result {
            Ok(v)  => RpcResponse::ok(req.id, v),
            Err(e) => RpcResponse::err(req.id, e),
        };
    }

    let result = match req.method.as_str() {
        "broadcastChat"       => api.broadcast_chat(req.params).await,
        "sendPrivateMessage"  => api.send_private_message(req.params).await,
        "teleportPlayer"      => api.teleport_player(req.params).await,
        "getOnlinePlayers"    => api.get_online_players(req.params).await,
        "getPlayerPosition"   => api.get_player_position(req.params).await,
        "spawnVehicle"        => api.spawn_vehicle(req.params).await,
        "executeAdminCommand" => api.execute_admin_command(req.params).await,
        "ping"                => api.ping(req.params).await,
        other => {
            warn!("[admin_api] unknown method: {other}");
            Err(EngineError::InvalidParams {
                msg: format!("unknown method: {other}"),
            })
        }
    };

    match result {
        Ok(v)  => RpcResponse::ok(req.id, v),
        Err(e) => RpcResponse::err(req.id, e),
    }
}

async fn read_frame<R: AsyncReadExt + Unpin>(r: &mut R) -> io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf).await?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > 4 * 1024 * 1024 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large (> 4 MiB)"));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).await?;
    Ok(body)
}

fn frame_encode<T: Serialize>(v: &T) -> io::Result<Vec<u8>> {
    let json = serde_json::to_vec(v)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let len = json.len() as u32;
    let mut buf = Vec::with_capacity(4 + json.len());
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(&json);
    Ok(buf)
}

fn build_pipe_name() -> String {
    let pid = std::process::id();
    format!(r"\\.\pipe\turdmod-engine-{pid}")
}

fn write_discovery_file(pipe_name: &str) -> io::Result<()> {
    let dir = engine_state_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("pipe.txt");
    std::fs::write(&path, pipe_name)?;
    info!("[admin_api] discovery file: {}", path.display());
    Ok(())
}

fn engine_state_dir() -> PathBuf {
    let local = std::env::var_os("LOCALAPPDATA").unwrap_or_default();
    PathBuf::from(local).join("TurdMOD").join("engine")
}

// ─── Unit tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    struct StubApi;

    #[async_trait]
    impl EngineApi for StubApi {
        async fn ping(&self, _params: Json) -> Result<Json, EngineError> {
            Ok(serde_json::json!({ "pong": true, "engineVersion": "0.0.0-test" }))
        }

        async fn broadcast_chat(&self, params: Json) -> Result<Json, EngineError> {
            let text = params
                .get("text")
                .and_then(Json::as_str)
                .ok_or_else(|| EngineError::InvalidParams { msg: "missing text".into() })?;
            Ok(serde_json::json!({ "sent": text }))
        }
    }

    #[tokio::test]
    async fn frame_codec_roundtrip() {
        let original = serde_json::json!({ "hello": "world", "n": 42 });
        let encoded = frame_encode(&original).expect("encode");

        let mut cursor = Cursor::new(encoded);
        let decoded_bytes = read_frame(&mut cursor).await.expect("decode");
        let decoded: Json = serde_json::from_slice(&decoded_bytes).expect("json");

        assert_eq!(original, decoded);
    }

    #[tokio::test]
    async fn dispatch_ping_returns_pong() {
        let api = StubApi;
        let req = RpcRequest {
            id: Uuid::new_v4().to_string(),
            method: "ping".into(),
            params: Json::Null,
        };
        let resp = dispatch(&api, req).await;
        assert!(resp.error.is_none(), "expected no error, got {:?}", resp.error);
        let result = resp.result.expect("result");
        assert_eq!(result["pong"], Json::Bool(true));
    }

    #[tokio::test]
    async fn dispatch_unknown_method_returns_error() {
        let api = StubApi;
        let req = RpcRequest {
            id: Uuid::new_v4().to_string(),
            method: "doesNotExist".into(),
            params: Json::Null,
        };
        let resp = dispatch(&api, req).await;
        assert!(resp.result.is_none());
        let err = resp.error.expect("error body");
        assert_eq!(err.code, -32602);
    }

    #[tokio::test]
    async fn dispatch_broadcast_chat_missing_text() {
        let api = StubApi;
        let req = RpcRequest {
            id: Uuid::new_v4().to_string(),
            method: "broadcastChat".into(),
            params: serde_json::json!({}),
        };
        let resp = dispatch(&api, req).await;
        assert!(resp.result.is_none());
        let err = resp.error.expect("error body");
        assert_eq!(err.code, -32602);
        assert!(err.message.contains("missing text"), "message: {}", err.message);
    }
}
