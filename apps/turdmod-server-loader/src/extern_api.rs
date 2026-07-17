//! C-ABI surface for in-process C++ UE4SS mods to plug into the engine.
//!
//! The C++ mod (see `apps/turdmod-engine-bridge/`) loads into the same
//! GameServer.exe process as this DLL. It calls our exported
//! `turdmod_engine_register_handler` to register handler function pointers
//! for RPC methods, and calls `turdmod_engine_emit_event` to push UE4
//! events outbound via the named-pipe `EventBroadcaster`.
//!
//! ## Memory ownership
//!
//! No malloc crosses the FFI boundary. The convention is:
//!
//!   - **Inbound (params)**: Rust passes a NUL-terminated C string that
//!     is valid only for the duration of the handler call. The handler
//!     must copy if it needs the data afterward.
//!   - **Outbound (result / error)**: the handler writes a pointer to
//!     its OWN buffer (e.g. a `thread_local std::string` member). Rust
//!     copies the bytes during the call; the handler may reuse / free
//!     the buffer on the next invocation.
//!
//! This sidesteps every cross-DLL allocator-mismatch class of bug.
//!
//! ## Threading
//!
//! Handlers are called from Tokio worker threads (`admin_api::dispatch`).
//! Handlers that touch UE4 actor state MUST marshal to the game thread
//! via UE4SS's `ExecuteInGameThread` — same constraint as the Lua
//! community-mod template.

use std::collections::HashMap;
use std::ffi::{c_char, CStr, CString};
use std::sync::OnceLock;

use parking_lot::Mutex;
use serde_json::Value as Json;
use tracing::{info, warn};

use crate::admin_api::{EngineError, EventBroadcaster};

/// Handler signature for C++ UE4SS mods.
///
/// - `params_json`: NUL-terminated C string with the request params, valid
///   only for the duration of this call. Copy if needed.
/// - `result_out`: out-param. Handler writes a pointer to its OWN buffer
///   (NUL-terminated C string holding the JSON result). Set to NULL for
///   `null` result. Buffer must remain valid until the next call.
/// - `error_out`: same conventions; non-NULL means error.
///
/// Returns: 0 = success, non-zero = error (set `error_out`).
pub type ExternHandlerFn = unsafe extern "C" fn(
    params_json: *const c_char,
    result_out: *mut *const c_char,
    error_out: *mut *const c_char,
) -> i32;

static REGISTERED: OnceLock<Mutex<HashMap<String, ExternHandlerFn>>> = OnceLock::new();
static GLOBAL_BROADCASTER: OnceLock<EventBroadcaster> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<String, ExternHandlerFn>> {
    REGISTERED.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Install the broadcaster so the `emit_event` export can reach it.
/// Called from `lib.rs` right after `admin_api::start_rpc` returns.
pub fn install_broadcaster(bc: EventBroadcaster) {
    if GLOBAL_BROADCASTER.set(bc).is_err() {
        warn!("[extern_api] install_broadcaster called more than once — ignored");
    }
}

/// Look up a registered handler. Used by `admin_api::dispatch` to check
/// for an external implementation before falling back to the trait.
pub fn lookup_handler(method: &str) -> Option<ExternHandlerFn> {
    registry().lock().get(method).copied()
}

/// Invoke a registered handler with `params`. Owns the FFI boundary —
/// converts to/from C strings, drives the out-params, returns a typed
/// `Result<Json, EngineError>` for the dispatch layer.
pub fn call_handler(
    method: &str,
    fn_ptr: ExternHandlerFn,
    params: Json,
) -> Result<Json, EngineError> {
    let params_cstr = CString::new(params.to_string())
        .map_err(|e| EngineError::InvalidParams { msg: format!("params contains NUL: {e}") })?;
    let mut result_ptr: *const c_char = std::ptr::null();
    let mut error_ptr:  *const c_char = std::ptr::null();

    // SAFETY: fn_ptr is a function pointer registered by the C++ mod.
    // We assume the C++ side honors the documented contract (handler
    // returns to caller before its result buffer is freed). All four
    // pointers are valid for the call.
    let status = unsafe { fn_ptr(params_cstr.as_ptr(), &mut result_ptr, &mut error_ptr) };

    let result_str = unsafe { cstr_to_string(result_ptr) };
    let error_str  = unsafe { cstr_to_string(error_ptr) };

    if status == 0 {
        match result_str {
            Some(s) => serde_json::from_str(&s)
                .map_err(|e| EngineError::HookFailed {
                    msg: format!("handler '{method}' returned non-JSON: {e}"),
                }),
            None => Ok(Json::Null),
        }
    } else {
        let msg = error_str.unwrap_or_else(|| {
            format!("handler '{method}' returned status {status} without an error body")
        });
        // Try to parse the error_json as { code, message }; if it doesn't,
        // wrap it in a HookFailed with the raw message.
        if let Ok(Json::Object(obj)) = serde_json::from_str::<Json>(&msg) {
            if let (Some(_code), Some(message)) = (obj.get("code"), obj.get("message").and_then(Json::as_str)) {
                return Err(EngineError::HookFailed { msg: message.to_string() });
            }
        }
        Err(EngineError::HookFailed { msg })
    }
}

/// Copy a NUL-terminated C string into an owned Rust `String`.
/// Returns `None` for null pointers or invalid UTF-8.
unsafe fn cstr_to_string(p: *const c_char) -> Option<String> {
    if p.is_null() { return None; }
    CStr::from_ptr(p).to_str().ok().map(|s| s.to_string())
}

// ─── C-ABI exports ──────────────────────────────────────────────────────────

/// Register a handler for the named RPC method. Replaces any prior
/// registration for the same method.
///
/// Returns 0 on success, -1 on bad input (null method, invalid UTF-8).
#[no_mangle]
pub extern "C" fn turdmod_engine_register_handler(
    method: *const c_char,
    fn_ptr: ExternHandlerFn,
) -> i32 {
    if method.is_null() { return -1; }
    let name = match unsafe { CStr::from_ptr(method) }.to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return -1,
    };
    registry().lock().insert(name.clone(), fn_ptr);
    info!("[extern_api] handler registered: {name}");
    0
}

/// Unregister a previously-registered handler. Returns 0 if removed,
/// -1 if no handler was registered for that method, -2 on bad input.
#[no_mangle]
pub extern "C" fn turdmod_engine_unregister_handler(method: *const c_char) -> i32 {
    if method.is_null() { return -2; }
    let name = match unsafe { CStr::from_ptr(method) }.to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return -2,
    };
    if registry().lock().remove(&name).is_some() {
        info!("[extern_api] handler unregistered: {name}");
        0
    } else {
        -1
    }
}

/// Emit an engine event. `json_data` is parsed as JSON; on parse failure
/// the event is dropped and -1 returned.
///
/// Returns 0 on success, -1 if the broadcaster is not yet installed or
/// the input is malformed.
#[no_mangle]
pub extern "C" fn turdmod_engine_emit_event(
    event_name: *const c_char,
    json_data: *const c_char,
) -> i32 {
    let bc = match GLOBAL_BROADCASTER.get() {
        Some(b) => b,
        None => {
            warn!("[extern_api] emit_event called before broadcaster installed");
            return -1;
        }
    };
    if event_name.is_null() { return -1; }
    let name = match unsafe { CStr::from_ptr(event_name) }.to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let data: Json = if json_data.is_null() {
        Json::Null
    } else {
        match unsafe { CStr::from_ptr(json_data) }.to_str() {
            Ok(s) => serde_json::from_str(s).unwrap_or(Json::Null),
            Err(_) => return -1,
        }
    };
    bc.emit(name, data);
    0
}

/// Version probe — C++ side can check that the loaded DLL is compatible.
/// Returns a static NUL-terminated UTF-8 string; do not free.
#[no_mangle]
pub extern "C" fn turdmod_engine_extern_api_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr() as *const c_char
}
