//! UE4 / Win32 function hooks for the SCUM server loader.
//!
//! Currently shipping: the **pak signature bypass guard window** (Path B
//! of the bypass investigation — see memory `scum-bypass-investigation-state`
//! and `scum-pak-signature-wall`).
//!
//! Why a guard window:
//! SCUM's pak loader hard-crashes on boot if any pak in Content/Paks/
//! lacks a `.sig` sidecar. The crash path goes through SCUM's "fatal exit
//! with reason" helper → `FPlatformMisc::RequestExit(0)` → eventually
//! `kernel32!ExitProcess` / `kernel32!TerminateProcess`. By hooking those
//! two kernel32 functions and no-op'ing them during the first N seconds
//! after the loader attaches, we let SCUM boot past pak validation. After
//! the guard window, the hooks pass through so normal shutdowns (Ctrl-C,
//! console close) still work.
//!
//! This is over-broad — it suppresses ALL exit attempts during the guard.
//! For Path A (surgical), see the SCUMServer.exe string offsets in
//! memory `scum-bypass-investigation-state` and use a disassembler to
//! locate SCUM's specific helper function. Hook only that.
//!
//! TIMING NOTE: this module's `install()` MUST be called from DllMain
//! (BEFORE the suspended main thread resumes), not from a spawned thread.
//! If called from `init_thread()` (the worker that lib.rs spawns), the
//! main thread can race ahead and reach pak enumeration before our hook
//! is installed. Calling from DllMain guarantees the hooks are in place
//! before any SCUMServer.exe code runs.
//!
//! STORAGE NOTE: retour 0.3.1's `GenericDetour` and `RawDetour` don't
//! expose a Send+Sync-safe path to the trampoline from a static, and the
//! `static_detour!` macro requires nightly. Workaround: store just the
//! trampoline pointer (which is a raw machine address inside the
//! detour's heap-allocated trampoline buffer) in an AtomicPtr, and
//! `Box::leak` the detour itself to keep the buffer alive for the
//! lifetime of the process. Detours never get uninstalled at runtime,
//! so this is fine.

use std::sync::atomic::{AtomicI64, AtomicPtr, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use retour::RawDetour;
use windows_sys::Win32::Foundation::{BOOL, HMODULE};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

use crate::logging;

// ─── Function pointer types we're hooking ──────────────────────────────────

type ExitProcessFn = unsafe extern "system" fn(u32);
type TerminateProcessFn = unsafe extern "system" fn(*mut std::ffi::c_void, u32) -> BOOL;

// ─── Trampoline storage ────────────────────────────────────────────────────
//
// AtomicPtr<()> holds the raw address of the trampoline buffer that retour
// allocates inside the detour object. Once the detour is leaked (Box::leak),
// the trampoline pointer is stable for the lifetime of the process and can
// be transmuted to the original function pointer type when needed.

static EXIT_PROCESS_TRAMPOLINE: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
static TERMINATE_PROCESS_TRAMPOLINE: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

// ─── Guard-window state ────────────────────────────────────────────────────

static INSTALL_TIMESTAMP_MS: AtomicI64 = AtomicI64::new(0);

/// How long after install() to suppress process-exit calls.
const GUARD_WINDOW_SECONDS: i64 = 60;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn within_guard_window() -> bool {
    let ts = INSTALL_TIMESTAMP_MS.load(Ordering::Relaxed);
    if ts == 0 {
        return false;
    }
    (now_ms() - ts) / 1000 < GUARD_WINDOW_SECONDS
}

// ─── Detour bodies ─────────────────────────────────────────────────────────

unsafe extern "system" fn exit_process_detour(exit_code: u32) {
    if within_guard_window() {
        logging::log(&format!(
            "[server_hooks] ExitProcess({exit_code}) SUPPRESSED — guard window active ({GUARD_WINDOW_SECONDS}s)"
        ));
        return;
    }
    logging::log(&format!(
        "[server_hooks] ExitProcess({exit_code}) passing through (post-guard)"
    ));
    let tramp = EXIT_PROCESS_TRAMPOLINE.load(Ordering::Acquire);
    if tramp.is_null() {
        return;
    }
    let original: ExitProcessFn = std::mem::transmute(tramp);
    original(exit_code);
}

unsafe extern "system" fn terminate_process_detour(
    handle: *mut std::ffi::c_void,
    exit_code: u32,
) -> BOOL {
    if within_guard_window() {
        logging::log(&format!(
            "[server_hooks] TerminateProcess(handle={:?}, code={exit_code}) SUPPRESSED — guard window active",
            handle
        ));
        // Return 1 (TRUE) so callers think it worked.
        return 1;
    }
    logging::log(&format!(
        "[server_hooks] TerminateProcess(handle={:?}, code={exit_code}) passing through (post-guard)",
        handle
    ));
    let tramp = TERMINATE_PROCESS_TRAMPOLINE.load(Ordering::Acquire);
    if tramp.is_null() {
        return 1;
    }
    let original: TerminateProcessFn = std::mem::transmute(tramp);
    original(handle, exit_code)
}

// ─── Install ───────────────────────────────────────────────────────────────

/// Install the pak-signature-bypass guard hooks. Call from DllMain.
///
/// Safe to call from DllMain: only touches already-loaded kernel32 via
/// `GetModuleHandleA` + `GetProcAddress`, no LoadLibrary, no thread
/// synchronization beyond atomic stores. retour's detour install is
/// itself just VirtualProtect + memcpy on already-resident memory.
pub fn install() {
    INSTALL_TIMESTAMP_MS.store(now_ms(), Ordering::Relaxed);

    let kernel32 = unsafe { GetModuleHandleA(b"kernel32.dll\0".as_ptr()) };
    if kernel32.is_null() {
        logging::log("[server_hooks] FATAL: kernel32.dll not loaded — cannot install bypass hooks");
        return;
    }

    install_exit_process(kernel32);
    install_terminate_process(kernel32);

    logging::log(&format!(
        "[server_hooks] install complete — pak-signature bypass guard window {GUARD_WINDOW_SECONDS}s starts now"
    ));
}

fn install_exit_process(kernel32: HMODULE) {
    let Some(raw_ptr) = (unsafe { GetProcAddress(kernel32, b"ExitProcess\0".as_ptr()) }) else {
        logging::log("[server_hooks] ERROR: GetProcAddress(ExitProcess) returned None");
        return;
    };
    let target_ptr = raw_ptr as *const ();
    let detour_ptr = exit_process_detour as *const ();
    let detour = match unsafe { RawDetour::new(target_ptr, detour_ptr) } {
        Ok(d) => d,
        Err(e) => {
            logging::log(&format!(
                "[server_hooks] RawDetour::new(ExitProcess) failed: {e:?}"
            ));
            return;
        }
    };
    if let Err(e) = unsafe { detour.enable() } {
        logging::log(&format!(
            "[server_hooks] ExitProcess detour.enable() failed: {e:?}"
        ));
        return;
    }
    // Stash the trampoline pointer, then leak the detour so the trampoline
    // buffer it owns stays alive for the lifetime of the process.
    let tramp_ptr = detour.trampoline() as *const () as *mut ();
    EXIT_PROCESS_TRAMPOLINE.store(tramp_ptr, Ordering::Release);
    Box::leak(Box::new(detour));
    logging::log("[server_hooks] ExitProcess detour active");
}

fn install_terminate_process(kernel32: HMODULE) {
    let Some(raw_ptr) = (unsafe { GetProcAddress(kernel32, b"TerminateProcess\0".as_ptr()) }) else {
        logging::log("[server_hooks] ERROR: GetProcAddress(TerminateProcess) returned None");
        return;
    };
    let target_ptr = raw_ptr as *const ();
    let detour_ptr = terminate_process_detour as *const ();
    let detour = match unsafe { RawDetour::new(target_ptr, detour_ptr) } {
        Ok(d) => d,
        Err(e) => {
            logging::log(&format!(
                "[server_hooks] RawDetour::new(TerminateProcess) failed: {e:?}"
            ));
            return;
        }
    };
    if let Err(e) = unsafe { detour.enable() } {
        logging::log(&format!(
            "[server_hooks] TerminateProcess detour.enable() failed: {e:?}"
        ));
        return;
    }
    let tramp_ptr = detour.trampoline() as *const () as *mut ();
    TERMINATE_PROCESS_TRAMPOLINE.store(tramp_ptr, Ordering::Release);
    Box::leak(Box::new(detour));
    logging::log("[server_hooks] TerminateProcess detour active");
}
