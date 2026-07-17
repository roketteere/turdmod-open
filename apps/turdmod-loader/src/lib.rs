//! TurdMOD in-game loader DLL.
//!
//! Strategy C: this DLL gets loaded into SCUM.exe via the `dxgi.dll` proxy
//! technique — Windows resolves DLL imports relative to the EXE's directory
//! before falling back to System32, so a `dxgi.dll` placed next to SCUM.exe
//! is loaded in place of the system one. We forward all the legitimate
//! dxgi.dll exports to the real one in System32 (see `proxy.rs`) and use
//! `DllMain` as our foothold to spin up the loader thread.
//!
//! Workflow once injected:
//!   1. DllMain → spawn detached `init_thread`
//!   2. init_thread → write a startup line to the persistent loader log
//!      (matches the format the CLI's detection layer already uses)
//!   3. init_thread → run the BattlEye / official-server check; if either
//!      is true, refuse to do anything beyond logging (we never inject
//!      anywhere BattlEye is watching)
//!   4. init_thread → wait for the game's UE4 internals to settle, then
//!      hand control to the layer-2 runtime (LuaJIT) — TODO #168
//!
//! This file is the foundation; the actual UE4 hooking lives in `hooks.rs`
//! (TODO #169) and the Lua runtime in `runtime.rs` (TODO #168).

#![allow(non_snake_case)]
#![allow(clippy::missing_safety_doc)]

mod api;
mod detect;
mod hooks;
mod ipc;
mod ipc_server;
mod logging;
mod media;
mod native_ui;
mod player;
mod proxy;
mod runtime;
mod sigscan;

use once_cell::sync::OnceCell;
use parking_lot::Mutex;

static RUNTIME: OnceCell<Mutex<Option<runtime::Runtime>>> = OnceCell::new();

/// Returns the live Lua runtime, if `init_thread` brought it up. Used by
/// the IPC layer (and by `turdmod_loader_dispatch` for testing) to fire
/// events into the mod sandbox from outside.
pub(crate) fn runtime() -> Option<runtime::Runtime> {
    RUNTIME.get().and_then(|m| m.lock().clone())
}

/// Crate-internal alias used by `runtime::get_runtime_handle`.
pub(crate) fn runtime_handle() -> Option<runtime::Runtime> {
    runtime()
}

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use windows_sys::Win32::Foundation::{BOOL, HINSTANCE, TRUE};
use windows_sys::Win32::System::SystemServices::{DLL_PROCESS_ATTACH, DLL_PROCESS_DETACH};
use windows_sys::Win32::System::LibraryLoader::DisableThreadLibraryCalls;

/// Set when init_thread has logged its startup line. Used by the test
/// harness to know we've actually been loaded.
static INIT_DONE: AtomicBool = AtomicBool::new(false);

#[no_mangle]
pub extern "system" fn DllMain(
    hinst: HINSTANCE,
    reason: u32,
    _reserved: *mut c_void,
) -> BOOL {
    match reason {
        DLL_PROCESS_ATTACH => {
            // We don't care about thread attach/detach noise — every UE4
            // game spins up dozens of worker threads and forwarding those
            // events through DllMain just slows them down.
            unsafe {
                DisableThreadLibraryCalls(hinst);
            }

            // Real proxy DLLs do as little as possible inside DllMain
            // because the loader lock is held — anything that calls
            // LoadLibrary, opens a file, or touches the heap on a non-
            // calling thread can deadlock. Spawn a detached worker.
            thread::spawn(|| {
                if let Err(e) = init_thread() {
                    logging::log(&format!("[loader] init failed: {e}"));
                }
            });
        }
        DLL_PROCESS_DETACH => {
            logging::log("[loader] DLL_PROCESS_DETACH");
        }
        _ => {}
    }
    TRUE
}

/// Branding banner. Kept in sync with companion/src/index.ts:BANNER and
/// guard/src/main.rs:BANNER so every TurdMOD process logs the same
/// "TurdMOD is running" moment in its respective audit log.
// `concat!` preserves leading whitespace per line. The naive
// "\n\\\n   ..." form gets eaten by Rust's string-escape rules and the
// T-block drifts left.
const BANNER: &str = concat!(
    "\n",
    "████████╗██╗   ██╗██████╗ ██████╗ ███╗   ███╗ ██████╗ ██████╗\n",
    "╚══██╔══╝██║   ██║██╔══██╗██╔══██╗████╗ ████║██╔═══██╗██╔══██╗\n",
    "   ██║   ██║   ██║██████╔╝██║  ██║██╔████╔██║██║   ██║██║  ██║\n",
    "   ██║   ██║   ██║██╔══██╗██║  ██║██║╚██╔╝██║██║   ██║██║  ██║\n",
    "   ██║   ╚██████╔╝██║  ██║██████╔╝██║ ╚═╝ ██║╚██████╔╝██████╔╝\n",
    "   ╚═╝    ╚═════╝ ╚═╝  ╚═╝╚═════╝ ╚═╝     ╚═╝ ╚═════╝ ╚═════╝\n",
    "\n",
    "                  >>> TurdMOD is running! <<<\n",
    "                 loader v0.1.0 — Strategy C in-game\n",
    "                  github.com/roketteere/scummymap\n",
);

fn init_thread() -> Result<(), String> {
    for line in BANNER.lines() {
        logging::log(line);
    }
    logging::log(&format!(
        "[loader] DllMain attach — turdmod-loader v{} pid={} module={:?}",
        env!("CARGO_PKG_VERSION"),
        std::process::id(),
        std::env::current_exe().ok(),
    ));

    // Detection: refuse to do anything beyond log if BE is on or this
    // looks like an official server. The CLI already runs this check at
    // launcher time; we re-run it here because a player can launch SCUM
    // through Steam directly (bypassing our launcher) and we still want
    // the proxy to no-op safely.
    let mode = detect::infer_mode();
    logging::log(&format!("[loader] detected mode: {mode:?}"));
    if matches!(mode, detect::Mode::PrivateBeOn | detect::Mode::Unknown) {
        logging::log(
            "[loader] BE is on or environment unknown — staying inert. \
             Only dxgi-proxy forwarders will run; no UE4 hooks, no Lua, \
             no UI injection.",
        );
        INIT_DONE.store(true, Ordering::SeqCst);
        return Ok(());
    }

    // Game-specific initialization hooks can be installed here.
    // See the project documentation for how to add custom pak or
    // signature hooks for your UE4 game.

    // MINIMAL AI-PLAYER MODE (2026-06-07): for the "AI becomes a real player"
    // track we need ONLY the inbound IPC server + in-process memory access
    // (player.rs). We DELIBERATELY skip the Lua runtime, the DXGI/ImGui
    // overlay hooks (hudhook), and the companion subscriber — those hook
    // Present/world systems and are the prime suspect for breaking SCUM's
    // sandbox world-load (every capture showed the player stuck pre-spawn).
    // A bare loader that only listens on the pipe can't interfere with the
    // game loading + the character becoming walkable.
    logging::log("[loader] minimal AI-player mode — IPC only (no Lua, no overlay hooks)");
    ipc_server::start();

    // NATIVE-PANEL FOUNDATION: once the engine boots, resolve UObject::ProcessEvent
    // (GEngine->vtable[68], Dumper-7 idx 0x44) and log the call surface. GEngine is
    // null until UEngine init, so poll. This proves the un-staled RVAs + the
    // ProcessEvent vtable approach before we wire CreateWidget/AddToViewport.
    thread::spawn(|| {
        for _ in 0..150 {
            if let Some(pe) = unsafe { player::resolve_process_event() } {
                logging::log(&format!(
                    "[engine] ProcessEvent RESOLVED @ {:#x} — call surface: {}",
                    pe,
                    player::engine_probe()
                ));
                return;
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
        logging::log("[engine] ProcessEvent NOT resolved after 300s (GEngine never populated?)");
    });

    // C-GUI TEST (opt-in, 2026-06-14): re-enable the hudhook ImGui overlay, but
    // DEFER its install until the player is confirmed in-world (resolve_pawn Ok),
    // so it CANNOT interfere with SCUM's world-load — the reason the overlay was
    // parked. Gated behind %LOCALAPPDATA%\TurdMOD\overlay.enabled so normal play
    // is untouched. @brk if a player STILL sticks pre-spawn with this ON, the
    // overlay itself is the culprit (next: bisect Lua/companion). If you spawn AND
    // see the panel, C-GUI is unblocked.
    if overlay_test_enabled() {
        logging::log("[loader] overlay test flag ON — deferring ImGui overlay install (spawn OR 60s fallback)");
        thread::spawn(|| {
            // resolve_pawn is unreliable on some SCUM builds (parked at a version
            // gate), so don't depend on it alone: install when the player is detected
            // in-world OR after a 60s fallback (by then the game has loaded, so the
            // install can't disrupt world-load). @brk if spawn breaks WITH this, the
            // overlay itself is the culprit; if GUI shows, C-GUI is unblocked.
            let mut reason = "60s fallback";
            for _ in 0..60 {
                if player::resolve_pawn().is_ok() {
                    reason = "in-world detected";
                    break;
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
            logging::log(&format!("[loader] installing ImGui overlay now ({reason})"));
            match hooks::install() {
                Ok(()) => {
                    hooks::enqueue_default_welcome_panel(&[], env!("CARGO_PKG_VERSION"));
                    logging::log("[loader] overlay installed — GUI panel should appear in-game");
                }
                Err(e) => logging::log(&format!("[loader] overlay install FAILED: {e}")),
            }
        });
    }

    INIT_DONE.store(true, Ordering::SeqCst);
    Ok(())
}

/// Opt-in gate for the C-GUI overlay test: %LOCALAPPDATA%\TurdMOD\overlay.enabled.
fn overlay_test_enabled() -> bool {
    std::env::var_os("LOCALAPPDATA")
        .map(|l| {
            let mut p = std::path::PathBuf::from(l);
            p.push("TurdMOD");
            p.push("overlay.enabled");
            p.exists()
        })
        .unwrap_or(false)
}

/// Test hook: fire an event into the running runtime from outside Lua.
/// `channel` is a NUL-terminated UTF-8 string; `payload_json` is too. The
/// CLI's smoke-test harness calls this via GetProcAddress to confirm the
/// dispatch chain works end-to-end without needing a live companion.
#[no_mangle]
pub unsafe extern "C" fn turdmod_loader_dispatch(
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

/// Test hook: returns true once init_thread has finished its initial run.
/// The CLI's `turdmod doctor` reads this through a tiny named pipe to
/// confirm a successful injection without depending on log timing.
#[no_mangle]
pub extern "C" fn turdmod_loader_is_ready() -> BOOL {
    if INIT_DONE.load(Ordering::SeqCst) { TRUE } else { 0 }
}

/// Returns the loader version as a UTF-8 C string for diagnostics. Static
/// lifetime; do not free.
#[no_mangle]
pub extern "C" fn turdmod_loader_version() -> *const u8 {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr()
}
