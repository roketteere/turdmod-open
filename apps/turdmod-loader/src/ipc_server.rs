//! Inbound JSON-RPC over named pipe — lets the Manager push custom HUD
//! panels into the running ImGui render queue from outside SCUM.exe.
//!
//! Mirrors the protocol used by `turdmod-server-loader::admin_api`:
//!
//!   - Pipe name: `\\.\pipe\turdmod-loader` (static for V1; documented
//!     limitation = single SCUM client per machine).
//!   - Discovery file: `%LOCALAPPDATA%/TurdMOD/loader/pipe.txt` — contains
//!     the pipe name as a one-liner so the Manager can find it without
//!     hardcoding.
//!   - Frame layout: 4-byte little-endian length prefix + JSON body.
//!   - Request: `{ "id": "...", "method": "...", "params": {...} }`
//!   - Response: `{ "id": "...", "result": {...} }` or
//!                `{ "id": "...", "error": { "code": N, "message": "..." } }`
//!
//! Methods supported in V1:
//!   - `ping`            → returns `{ "ok": true, "version": "...", "pid": N }`
//!   - `enqueuePanel`    → params = panel JSON (passes straight through
//!                         to `hooks::enqueue_panel`); returns `{ "queued": true }`
//!
//! NULL-DACL security: matches the bridge's pipe. The loader runs inside
//! SCUM.exe (user integrity); the Manager runs at the same level; allow
//! any local user to connect. The pipe carries panel JSON, not secrets.
//!
//! Threading: one blocking thread per pipe instance. We create a new
//! pipe instance on every accept so back-to-back connections don't
//! ERROR_PIPE_BUSY each other. Each worker reads → dispatches →
//! writes → disconnects in a loop until the client closes.

#![allow(clippy::missing_safety_doc)]

use std::ffi::OsString;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::ptr::null_mut;
use std::thread;

use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Security::{
    InitializeSecurityDescriptor, SetSecurityDescriptorDacl, SECURITY_ATTRIBUTES,
    SECURITY_DESCRIPTOR,
};
// PIPE_ACCESS_DUPLEX lives in Storage::FileSystem (it's a FILE_FLAGS_AND_ATTRIBUTES
// type alias); ReadFile/WriteFile also live there but cfg-gated on
// Win32_System_IO because their signatures reference OVERLAPPED.
use windows_sys::Win32::Storage::FileSystem::{PIPE_ACCESS_DUPLEX, ReadFile, WriteFile};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe,
    PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};

use crate::{hooks, logging};

const PIPE_NAME: &str = r"\\.\pipe\turdmod-loader";
const MAX_FRAME_BYTES: u32 = 4 * 1024 * 1024;

static STARTED: OnceCell<()> = OnceCell::new();

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

impl RpcResponse {
    fn ok(id: String, result: Json) -> Self {
        Self { id, result: Some(result), error: None }
    }
    fn err(id: String, code: i32, message: String) -> Self {
        Self { id, result: None, error: Some(RpcErrorBody { code, message }) }
    }
}

/// Stand up the IPC server. Idempotent; safe to call from `init_thread`
/// once the runtime + hooks are wired. Returns immediately — the listen
/// loop runs on its own thread.
pub fn start() {
    if STARTED.set(()).is_err() {
        return;
    }
    if let Err(e) = write_discovery_file() {
        logging::log(&format!("[ipc_server] discovery write failed: {e} (continuing — clients can use static pipe name)"));
    }
    thread::Builder::new()
        .name("turdmod-loader-ipc".to_string())
        .spawn(accept_loop)
        .ok();
    logging::log(&format!("[ipc_server] listening on {PIPE_NAME}"));
}

fn write_discovery_file() -> std::io::Result<()> {
    let dir = loader_state_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("pipe.txt");
    std::fs::write(&path, PIPE_NAME)?;
    logging::log(&format!("[ipc_server] discovery file: {}", path.display()));
    Ok(())
}

fn loader_state_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Users\Public"));
    base.join("TurdMOD").join("loader")
}

fn accept_loop() {
    loop {
        let server = unsafe { create_pipe_instance() };
        if server == INVALID_HANDLE_VALUE {
            logging::log("[ipc_server] CreateNamedPipeW failed; retrying in 1s");
            thread::sleep(std::time::Duration::from_secs(1));
            continue;
        }
        let connected = unsafe { ConnectNamedPipe(server, null_mut()) };
        // ConnectNamedPipe returns non-zero on success. ERROR_PIPE_CONNECTED
        // (535) is also OK — a client beat us to the connect.
        if connected == 0 {
            let err = unsafe { windows_sys::Win32::Foundation::GetLastError() };
            const ERROR_PIPE_CONNECTED: u32 = 535;
            if err != ERROR_PIPE_CONNECTED {
                logging::log(&format!("[ipc_server] ConnectNamedPipe failed: {err}"));
                unsafe { CloseHandle(server) };
                continue;
            }
        }
        // HANDLE is `*mut c_void` — not Send. The kernel doesn't care
        // which thread issues IO against the handle, so smuggle it
        // across as a usize and rebuild on the worker side. (Newtype
        // + unsafe Send doesn't help because edition-2021 disjoint
        // captures look through the wrapper.)
        let handle_addr = server as usize;
        thread::Builder::new()
            .name("turdmod-loader-ipc-conn".to_string())
            .spawn(move || {
                serve_connection(handle_addr as HANDLE);
            })
            .ok();
    }
}

unsafe fn create_pipe_instance() -> HANDLE {
    let wide: Vec<u16> = OsString::from(PIPE_NAME)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // NULL DACL — matches the bridge's pipe so unprivileged Manager
    // processes can connect regardless of integrity boundary.
    let mut sd: SECURITY_DESCRIPTOR = std::mem::zeroed();
    const SECURITY_DESCRIPTOR_REVISION: u32 = 1;
    InitializeSecurityDescriptor(&mut sd as *mut _ as *mut _, SECURITY_DESCRIPTOR_REVISION);
    SetSecurityDescriptorDacl(&mut sd as *mut _ as *mut _, 1, null_mut(), 0);
    let mut sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: &mut sd as *mut _ as *mut _,
        bInheritHandle: 0,
    };

    CreateNamedPipeW(
        wide.as_ptr(),
        PIPE_ACCESS_DUPLEX,
        PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
        PIPE_UNLIMITED_INSTANCES,
        64 * 1024,
        64 * 1024,
        0,
        &mut sa,
    )
}

fn serve_connection(pipe: HANDLE) {
    loop {
        let frame = match read_frame(pipe) {
            Ok(f) => f,
            Err(e) => {
                // EOF / broken pipe = peer disconnected; quiet exit.
                let kind = e.kind();
                if kind != std::io::ErrorKind::UnexpectedEof
                    && kind != std::io::ErrorKind::BrokenPipe
                {
                    logging::log(&format!("[ipc_server] read frame: {e}"));
                }
                break;
            }
        };

        let response = match serde_json::from_slice::<RpcRequest>(&frame) {
            Ok(req) => dispatch(req),
            Err(e) => RpcResponse::err(
                String::new(),
                -32700,
                format!("parse error: {e}"),
            ),
        };

        let body = match serde_json::to_vec(&response) {
            Ok(b) => b,
            Err(e) => {
                logging::log(&format!("[ipc_server] encode response: {e}"));
                continue;
            }
        };
        if let Err(e) = write_frame(pipe, &body) {
            logging::log(&format!("[ipc_server] write response: {e}"));
            break;
        }
    }
    unsafe {
        DisconnectNamedPipe(pipe);
        CloseHandle(pipe);
    }
}

fn dispatch(req: RpcRequest) -> RpcResponse {
    match req.method.as_str() {
        "ping" => RpcResponse::ok(
            req.id,
            serde_json::json!({
                "ok": true,
                "version": env!("CARGO_PKG_VERSION"),
                "pid": std::process::id(),
            }),
        ),
        "enqueuePanel" => {
            // Params shape is whatever hooks::enqueue_panel understands —
            // for V1 it accepts arbitrary JSON, which the render loop
            // pattern-matches at draw time. So just pass it through.
            hooks::enqueue_panel(req.params);
            RpcResponse::ok(req.id, serde_json::json!({ "queued": true }))
        }
        // Phase 1 perception (AI-as-player track): read the local player's
        // world transform by walking GEngine -> Pawn -> RootComponent.
        "pullPlayerState" => match crate::player::pull_player_state() {
            Ok(v) => RpcResponse::ok(req.id, v),
            Err(e) => RpcResponse::err(req.id, -32000, e),
        },
        // Phase 2 movement (drive ControlInputVector — no ProcessEvent).
        "moveForward" => {
            let scale = req.params.get("scale").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
            let dur = req.params.get("durationMs").and_then(|v| v.as_u64()).unwrap_or(1500);
            RpcResponse::ok(req.id, crate::player::move_forward(scale, dur))
        }
        "moveInput" => {
            let g = |k: &str, d: f64| req.params.get(k).and_then(|v| v.as_f64()).unwrap_or(d) as f32;
            let (dx, dy, dz) = (g("dx", 0.0), g("dy", 0.0), g("dz", 0.0));
            let scale = g("scale", 1.0);
            let dur = req.params.get("durationMs").and_then(|v| v.as_u64()).unwrap_or(1500);
            RpcResponse::ok(req.id, crate::player::move_input(dx, dy, dz, scale, dur))
        }
        "moveStop" => RpcResponse::ok(req.id, crate::player::move_stop()),
        "engineProbe" => RpcResponse::ok(req.id, crate::player::engine_probe()),
        "showWelcome" => match crate::native_ui::request_show() {
            Ok(()) => RpcResponse::ok(req.id, serde_json::json!({ "requested": true })),
            Err(e) => RpcResponse::err(req.id, -32000, e),
        },
        "mountMech" => match crate::native_ui::request_mount() {
            Ok(()) => RpcResponse::ok(req.id, serde_json::json!({ "requested": true })),
            Err(e) => RpcResponse::err(req.id, -32000, e),
        },
        "dismountMech" => match crate::native_ui::request_dismount() {
            Ok(()) => RpcResponse::ok(req.id, serde_json::json!({ "requested": true })),
            Err(e) => RpcResponse::err(req.id, -32000, e),
        },
        "becomeMech" => match crate::native_ui::request_become_mech() {
            Ok(()) => RpcResponse::ok(req.id, serde_json::json!({ "requested": true })),
            Err(e) => RpcResponse::err(req.id, -32000, e),
        },
        "mechAnim" => match crate::native_ui::request_mech_anim() {
            Ok(()) => RpcResponse::ok(req.id, serde_json::json!({ "requested": true })),
            Err(e) => RpcResponse::err(req.id, -32000, e),
        },
        "revertMech" => match crate::native_ui::request_revert_mech() {
            Ok(()) => RpcResponse::ok(req.id, serde_json::json!({ "requested": true })),
            Err(e) => RpcResponse::err(req.id, -32000, e),
        },
        "spawnMech" => match crate::native_ui::request_spawn_mech() {
            Ok(()) => RpcResponse::ok(req.id, serde_json::json!({ "requested": true })),
            Err(e) => RpcResponse::err(req.id, -32000, e),
        },
        "spawnHeli" => match crate::native_ui::request_spawn_heli() {
            Ok(()) => RpcResponse::ok(req.id, serde_json::json!({ "requested": true })),
            Err(e) => RpcResponse::err(req.id, -32000, e),
        },
        "reskinVehicle" => match crate::native_ui::request_reskin_vehicle() {
            Ok(()) => RpcResponse::ok(req.id, serde_json::json!({ "requested": true })),
            Err(e) => RpcResponse::err(req.id, -32000, e),
        },
        "heliCamera" => {
            let follow = req.params.get("follow").and_then(|v| v.as_bool()).unwrap_or(true);
            match crate::native_ui::request_heli_camera(follow) {
                Ok(()) => RpcResponse::ok(req.id, serde_json::json!({ "heliCamera": follow })),
                Err(e) => RpcResponse::err(req.id, -32000, e),
            }
        },
        "heliTune" => {
            let f = |k: &str| req.params.get(k).and_then(|v| v.as_f64()).map(|n| n as f32);
            match crate::native_ui::request_heli_tune(f("thrust"), f("fwd"), f("strafe"), f("yaw")) {
                Ok(()) => RpcResponse::ok(req.id, serde_json::json!({
                    "thrust": f("thrust"), "fwd": f("fwd"), "strafe": f("strafe"), "yaw": f("yaw")
                })),
                Err(e) => RpcResponse::err(req.id, -32000, e),
            }
        },
        "heliMount" => {
            let mount = req.params.get("mount").and_then(|v| v.as_bool()).unwrap_or(true);
            match crate::native_ui::request_heli_mount(mount) {
                Ok(()) => RpcResponse::ok(req.id, serde_json::json!({ "heliMount": mount })),
                Err(e) => RpcResponse::err(req.id, -32000, e),
            }
        },
        "heliFlight" => {
            let enable = req.params.get("enable").and_then(|v| v.as_bool())
                .or_else(|| req.params.get("enable").and_then(|v| v.as_i64()).map(|n| n != 0))
                .unwrap_or(true);
            match crate::native_ui::request_heli_flight(enable) {
                Ok(()) => RpcResponse::ok(req.id, serde_json::json!({ "heliFlight": enable })),
                Err(e) => RpcResponse::err(req.id, -32000, e),
            }
        }
        "connectLocal" => match crate::native_ui::request_connect_local() {
            Ok(()) => RpcResponse::ok(req.id, serde_json::json!({ "requested": true })),
            Err(e) => RpcResponse::err(req.id, -32000, e),
        },
        "enterHeli" => {
            let seat = req.params.get("seat").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            match crate::native_ui::request_enter_heli(seat) {
                Ok(()) => RpcResponse::ok(req.id, serde_json::json!({ "requested": true, "seat": seat })),
                Err(e) => RpcResponse::err(req.id, -32000, e),
            }
        }
        "exitHeli" => match crate::native_ui::request_exit_heli() {
            Ok(()) => RpcResponse::ok(req.id, serde_json::json!({ "requested": true })),
            Err(e) => RpcResponse::err(req.id, -32000, e),
        },
        "paintVehicle" => match crate::native_ui::request_paint() {
            Ok(()) => RpcResponse::ok(req.id, serde_json::json!({ "requested": true })),
            Err(e) => RpcResponse::err(req.id, -32000, e),
        },
        "probeVehicleMeshes" => match crate::native_ui::request_probe_meshes() {
            Ok(()) => RpcResponse::ok(req.id, serde_json::json!({ "requested": true })),
            Err(e) => RpcResponse::err(req.id, -32000, e),
        },
        // Radio → Apple Music transport (OS media keys; works on unfocused browser).
        "mediaPlayPause" => { crate::media::play_pause(); RpcResponse::ok(req.id, serde_json::json!({ "ok": true })) }
        "mediaNext" => { crate::media::next_track(); RpcResponse::ok(req.id, serde_json::json!({ "ok": true })) }
        "mediaPrev" => { crate::media::prev_track(); RpcResponse::ok(req.id, serde_json::json!({ "ok": true })) }
        "probeDump" => RpcResponse::ok(req.id, crate::player::probe_dump()),
        "listPrisoners" => RpcResponse::ok(req.id, crate::player::list_prisoners()),
        "driveProbe" => RpcResponse::ok(req.id, crate::player::drive_probe()),
        other => RpcResponse::err(
            req.id,
            -32601,
            format!("unknown method: {other}"),
        ),
    }
}

fn read_frame(pipe: HANDLE) -> std::io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    read_exact(pipe, &mut len_buf)?;
    let len = u32::from_le_bytes(len_buf);
    if len > MAX_FRAME_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("frame too large ({len} > {MAX_FRAME_BYTES})"),
        ));
    }
    let mut body = vec![0u8; len as usize];
    if !body.is_empty() {
        read_exact(pipe, &mut body)?;
    }
    Ok(body)
}

fn write_frame(pipe: HANDLE, body: &[u8]) -> std::io::Result<()> {
    let len = (body.len() as u32).to_le_bytes();
    write_all(pipe, &len)?;
    write_all(pipe, body)?;
    Ok(())
}

fn read_exact(pipe: HANDLE, buf: &mut [u8]) -> std::io::Result<()> {
    let mut total = 0usize;
    while total < buf.len() {
        let mut got: u32 = 0;
        let ok = unsafe {
            ReadFile(
                pipe,
                buf[total..].as_mut_ptr() as *mut _,
                (buf.len() - total) as u32,
                &mut got,
                null_mut(),
            )
        };
        if ok == 0 {
            let err = unsafe { windows_sys::Win32::Foundation::GetLastError() };
            // ERROR_BROKEN_PIPE (109) = peer closed.
            return Err(std::io::Error::from_raw_os_error(err as i32));
        }
        if got == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "pipe EOF",
            ));
        }
        total += got as usize;
    }
    Ok(())
}

fn write_all(pipe: HANDLE, buf: &[u8]) -> std::io::Result<()> {
    let mut total = 0usize;
    while total < buf.len() {
        let mut wrote: u32 = 0;
        let ok = unsafe {
            WriteFile(
                pipe,
                buf[total..].as_ptr() as *const _,
                (buf.len() - total) as u32,
                &mut wrote,
                null_mut(),
            )
        };
        if ok == 0 {
            let err = unsafe { windows_sys::Win32::Foundation::GetLastError() };
            return Err(std::io::Error::from_raw_os_error(err as i32));
        }
        if wrote == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WriteZero,
                "pipe write zero",
            ));
        }
        total += wrote as usize;
    }
    Ok(())
}

