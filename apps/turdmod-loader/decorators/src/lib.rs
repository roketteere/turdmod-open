//! turdmod-rich-decorators — small DLL that upgrades SCUM's engine-stock
//! URichTextBlock decorator classes via function-detour, adding three new
//! markup tag behaviours:
//!
//!   * `<img src="https://..."/>`     — URL-fetched inline images
//!   * `<a href="..." key="...">x</a>` — clickable hyperlinks (Steam Overlay)
//!   * `<dismiss key="Escape"/>`       — keybind to close the parent panel
//!
//! Sibling crate to the kitchen-sink turdmod-loader. Intentionally minimal:
//! no DXGI hook, no ImGui, no UE4 game-object hooks, no Lua runtime. The
//! only runtime work is sigscanning UE 4.27 internals and installing
//! function-detours on three engine-stock decorator classes.
//!
//! Targets SCUM build 23128448 (private/modded servers; BE-off).
//!
//! ## Build status — Phase A (boot-and-log)
//!
//! Today this DLL does nothing beyond log "attached" to
//! %LOCALAPPDATA%/TurdMOD/decorators.log. Phase B (real work) lands in a
//! follow-up commit:
//!
//!   1. sigscan UE 4.27 globals (GUObjectArray, decorator class CDOs)
//!   2. resolve URichTextBlockImageDecorator / KeyPromptDecorator /
//!      ActionPromptDecorator vtable entries
//!   3. install function-detours via vendored MinHook
//!   4. detour bodies: parse markup attributes, do the upgraded rendering
//!
//! Phase A's job is just to prove the DLL loads cleanly into SCUM.exe via
//! the existing turdmod-launcher injector before any engine-internal work
//! starts. Failure modes here (load fails, attach crashes, log file not
//! written) are easy to debug; failure modes once we start touching UClass
//! memory are not.

#![allow(non_snake_case)]
#![allow(clippy::missing_safety_doc)]

mod dispatch;
mod engine_resolve;
mod fname;
mod guarray;
mod hooks;
mod image_fetch;
mod logging;
mod uobject;
mod vtable;
// Share the existing UE4 4.27 sigscan implementation with the kitchen-sink
// loader rather than duplicating it. The `#[path]` attribute pulls the file
// in directly; both crates have compatible `logging` modules so
// `crate::logging::log(...)` resolves cleanly in either context. If this
// path-share grows into a maintenance burden, extract to a
// `packages/scum-sigscan` crate.
#[path = "../../src/sigscan.rs"]
mod sigscan;
mod sigscan_ext;
mod whitelist;

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use windows_sys::Win32::Foundation::{BOOL, HINSTANCE, TRUE};
use windows_sys::Win32::System::LibraryLoader::DisableThreadLibraryCalls;
use windows_sys::Win32::System::SystemServices::{DLL_PROCESS_ATTACH, DLL_PROCESS_DETACH};

/// Set once init_thread has logged its startup line. Used by the test
/// harness to confirm the DLL actually attached without depending on log
/// timing.
static INIT_DONE: AtomicBool = AtomicBool::new(false);

/// Branding banner — same shared "TurdMOD is running" art the other
/// processes log. Kept in sync with apps/turdmod-loader/src/lib.rs:BANNER
/// so every TurdMOD process logs the same moment in its respective sink.
// `concat!` preserves leading whitespace on each line. Don't switch to a
// multi-line `\` continuation — Rust's escape rules eat leading
// whitespace on the next line and the T-block's middle column drifts.
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
    "              decorators v0.1.0 — engine-stock detour\n",
    "                  github.com/roketteere/scummymap\n",
);

#[no_mangle]
pub extern "system" fn DllMain(
    hinst: HINSTANCE,
    reason: u32,
    _reserved: *mut c_void,
) -> BOOL {
    match reason {
        DLL_PROCESS_ATTACH => {
            // Same loader-lock-safety dance as turdmod-loader: disable
            // thread attach/detach noise and spawn a detached worker so
            // we never call LoadLibrary / open files / touch the heap on
            // a non-calling thread inside DllMain itself.
            unsafe { DisableThreadLibraryCalls(hinst); }
            thread::spawn(|| {
                if let Err(e) = init_thread() {
                    logging::log(&format!("[decorators] init failed: {e}"));
                }
            });
        }
        DLL_PROCESS_DETACH => {
            hooks::uninstall();
            logging::log("[decorators] DLL_PROCESS_DETACH");
        }
        _ => {}
    }
    TRUE
}

fn init_thread() -> Result<(), String> {
    for line in BANNER.lines() {
        logging::log(line);
    }
    logging::log(&format!(
        "[decorators] DllMain attach — turdmod-rich-decorators v{} pid={} module={:?}",
        env!("CARGO_PKG_VERSION"),
        std::process::id(),
        std::env::current_exe().ok(),
    ));
    logging::log("[decorators] phase A — boot-and-log only; engine-internal detours land in phase B");

    // Install the detour set. Today this is a Phase-A stub returning Ok;
    // Phase B fills in the engine-side resolution.
    if let Err(e) = hooks::install() {
        logging::log(&format!("[decorators] hooks::install failed (continuing): {e}"));
    }

    INIT_DONE.store(true, Ordering::SeqCst);
    Ok(())
}

/// Test hook: the launcher's smoke test reads this through GetProcAddress
/// to confirm load succeeded without depending on log timing.
#[no_mangle]
pub extern "C" fn turdmod_decorators_is_ready() -> BOOL {
    if INIT_DONE.load(Ordering::SeqCst) { TRUE } else { 0 }
}

/// Returns the decorator DLL version as a UTF-8 C string. Static lifetime;
/// do not free.
#[no_mangle]
pub extern "C" fn turdmod_decorators_version() -> *const u8 {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr()
}
