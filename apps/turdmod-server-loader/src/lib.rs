//! TurdMOD server-side loader DLL.
//!
//! This DLL is injected into `GameServer.exe` via the existing
//! `turdmod-launcher` binary (pass `--scum path/to/GameServer.exe` and
//! `--dll path/to/turdmod_server_loader.dll`). It does NOT use the
//! dxgi-proxy technique — `GameServer.exe` has no rendering pipeline and
//! therefore no DXGI imports. Instead the launcher uses the standard
//! `CreateProcessW(CREATE_SUSPENDED) + inject_dll() + ResumeThread` path.
//!
//! Differences from the client loader (`apps/turdmod-loader/`):
//!   - No hudhook / ImGui / DXGI proxy — server has no display.
//!   - No BattlEye check — BE is a client-only component.
//!   - Mods root: `%PROGRAMDATA%\TurdMOD\server-mods\` (not LOCALAPPDATA).
//!   - Log path: `%PROGRAMDATA%\TurdMOD\server-loader.log`.
//!   - IPC role: outbound push to companion `/ingest` (not SSE subscribe).
//!   - `admin_api` (Part B) is RPC-ready; `server_hooks` (Part C) is a stub.

#![allow(non_snake_case)]
#![allow(clippy::missing_safety_doc)]

mod admin_api;
mod api;
mod detect;
mod extern_api;
mod ipc;
mod logging;
mod runtime;
mod server_hooks;
// sigscan is shared directly from the client loader crate source via
// #[path]; no copy needed. Both crates expose a `logging::log(msg)` fn
// with the same signature, so the shared file compiles cleanly in both.
#[path = "../../turdmod-loader/src/sigscan.rs"]
mod sigscan;

use once_cell::sync::OnceCell;
use parking_lot::Mutex;

static RUNTIME: OnceCell<Mutex<Option<runtime::Runtime>>> = OnceCell::new();

pub(crate) fn runtime() -> Option<runtime::Runtime> {
    RUNTIME.get().and_then(|m| m.lock().clone())
}

pub(crate) fn runtime_handle() -> Option<runtime::Runtime> {
    runtime()
}

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use windows_sys::Win32::Foundation::{BOOL, HINSTANCE, TRUE};
use windows_sys::Win32::System::SystemServices::{DLL_PROCESS_ATTACH, DLL_PROCESS_DETACH};
use windows_sys::Win32::System::LibraryLoader::DisableThreadLibraryCalls;

static INIT_DONE: AtomicBool = AtomicBool::new(false);

#[no_mangle]
pub extern "system" fn DllMain(
    hinst: HINSTANCE,
    reason: u32,
    _reserved: *mut c_void,
) -> BOOL {
    match reason {
        DLL_PROCESS_ATTACH => {
            unsafe { DisableThreadLibraryCalls(hinst); }
            // CRITICAL: install bypass hooks SYNCHRONOUSLY before DllMain
            // returns. The suspended main thread can't resume until DllMain
            // exits, so installing here guarantees the hooks are in place
            // before any GameServer.exe code (including pak enumeration)
            // runs. If we moved this to init_thread, the spawned thread
            // would race the main thread and lose on some boots.
            server_hooks::install();
            thread::spawn(|| {
                if let Err(e) = init_thread() {
                    logging::log(&format!("[server-loader] init failed: {e}"));
                }
            });
        }
        DLL_PROCESS_DETACH => {
            logging::log("[server-loader] DLL_PROCESS_DETACH");
        }
        _ => {}
    }
    TRUE
}

const BANNER: &str = concat!(
    "\n",
    "████████╗██╗   ██╗██████╗ ██████╗ ███╗   ███╗ ██████╗ ██████╗\n",
    "╚══██╔══╝██║   ██║██╔══██╗██╔══██╗████╗ ████║██╔═══██╗██╔══██╗\n",
    "   ██║   ██║   ██║██████╔╝██║  ██║██╔████╔██║██║   ██║██║  ██║\n",
    "   ██║   ██║   ██║██╔══██╗██║  ██║██║╚██╔╝██║██║   ██║██║  ██║\n",
    "   ██║   ╚██████╔╝██║  ██║██████╔╝██║ ╚═╝ ██║╚██████╔╝██████╔╝\n",
    "   ╚═╝    ╚═════╝ ╚═╝  ╚═╝╚═════╝ ╚═╝     ╚═╝ ╚═════╝ ╚═════╝\n",
    "\n",
    "               >>> TurdMOD SERVER-LOADER is running! <<<\n",
    "              server-loader v0.1.0 — GameServer.exe in-process\n",
    "               github.com/roketteere/turdmod\n",
);

fn init_thread() -> Result<(), String> {
    for line in BANNER.lines() {
        logging::log(line);
    }
    logging::log(&format!(
        "[server-loader] DllMain attach — turdmod-server-loader v{} pid={} exe={:?}",
        env!("CARGO_PKG_VERSION"),
        std::process::id(),
        std::env::current_exe().ok(),
    ));

    // Process guard: refuse to do engine work if we're not inside SCUMServer.
    // TURDMOD_FORCE_TEST=1 bypasses this for smoke-test harnesses.
    if std::env::var_os("TURDMOD_FORCE_TEST").is_none() {
        if !detect::is_scum_server_process() {
            logging::log(
                "[server-loader] not running inside GameServer.exe — staying inert. \
                 Set TURDMOD_FORCE_TEST=1 to bypass in test harnesses.",
            );
            INIT_DONE.store(true, Ordering::SeqCst);
            return Ok(());
        }
    } else {
        logging::log(
            "[server-loader] TURDMOD_FORCE_TEST set — skipping process check (TEST MODE)",
        );
    }

    logging::log("[server-loader] process check passed — bringing up Lua runtime");

    let rt = runtime::Runtime::start().map_err(|e| format!("runtime start: {e}"))?;

    admin_api::init(&rt);
    // server_hooks::install() is now called from DllMain (above) so the
    // bypass hooks land before the main thread resumes. Don't double-install.

    // Bring up the named-pipe RPC server (admin_api::start_rpc) on a
    // dedicated Tokio runtime, then install the broadcaster into
    // extern_api so the C++ UE4SS mod can emit events through it.
    //
    // We use a stub EngineApi trait impl — every method returns
    // NotImplemented. Real method handling comes through extern_api's
    // registered C-ABI handlers, which dispatch() checks first.
    //
    // The Tokio runtime is leaked into a static so the spawned accept
    // loop survives — dropping the runtime would terminate it.
    start_rpc_server();

    rt.discover_and_load();
    logging::log(&format!(
        "[server-loader] runtime ready — {} mod(s) loaded",
        rt.loaded_mods().len()
    ));
    let _ = RUNTIME.set(Mutex::new(Some(rt)));

    ipc::start_outbound();
    logging::log("[server-loader] ipc outbound publisher started");

    INIT_DONE.store(true, Ordering::SeqCst);
    logging::log("[server-loader] ready");
    Ok(())
}

#[no_mangle]
pub unsafe extern "C" fn turdmod_server_loader_dispatch(
    channel: *const u8,
    payload_json: *const u8,
) -> BOOL {
    if channel.is_null() { return 0; }
    let channel_s = match cstr_utf8(channel) {
        Some(s) => s,
        None => return 0,
    };
    let payload: serde_json::Value = if payload_json.is_null() {
        serde_json::Value::Null
    } else {
        match cstr_utf8(payload_json) {
            Some(s) => serde_json::from_str(&s).unwrap_or(serde_json::Value::Null),
            None => serde_json::Value::Null,
        }
    };
    let rt = match runtime() {
        Some(r) => r,
        None => return 0,
    };
    rt.dispatch(&channel_s, payload);
    TRUE
}

unsafe fn cstr_utf8(p: *const u8) -> Option<String> {
    let mut len = 0;
    while *p.add(len) != 0 { len += 1; }
    let slice = std::slice::from_raw_parts(p, len);
    std::str::from_utf8(slice).ok().map(|s| s.to_string())
}

#[no_mangle]
pub extern "C" fn turdmod_server_loader_is_ready() -> BOOL {
    if INIT_DONE.load(Ordering::SeqCst) { TRUE } else { 0 }
}

#[no_mangle]
pub extern "C" fn turdmod_server_loader_version() -> *const u8 {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr()
}

// ─── RPC server bootstrap ────────────────────────────────────────────────────

/// Leaked Tokio runtime owning the RPC accept loop. Lives until process exit.
static TOKIO_RT: OnceCell<tokio::runtime::Runtime> = OnceCell::new();

struct StubEngineApi;

#[async_trait::async_trait]
impl admin_api::EngineApi for StubEngineApi {}

fn start_rpc_server() {
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .thread_name("turdmod-rpc")
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            logging::log(&format!(
                "[server-loader] tokio runtime build failed: {e}; RPC disabled"
            ));
            return;
        }
    };

    let stub: std::sync::Arc<dyn admin_api::EngineApi + Send + Sync> =
        std::sync::Arc::new(StubEngineApi);

    match rt.block_on(admin_api::start_rpc(stub)) {
        Ok((bc, _handle)) => {
            extern_api::install_broadcaster(bc);
            logging::log("[server-loader] admin RPC up; extern_api broadcaster installed");
        }
        Err(e) => {
            logging::log(&format!(
                "[server-loader] start_rpc failed: {e}; companion will not be able to connect"
            ));
            return;
        }
    }

    if TOKIO_RT.set(rt).is_err() {
        logging::log("[server-loader] TOKIO_RT already set — duplicate init?");
    }
}
