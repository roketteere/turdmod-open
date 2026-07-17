// turdmod_engine_api.h
//
// C-ABI header for the in-process surface exported by
// turdmod_server_loader.dll. The C++ UE4SS engine bridge mod includes
// this header and links against the loader's exports at runtime via
// GetProcAddress (no .lib needed — the loader is already in-process by
// the time the bridge's start_mod() runs).
//
// Source of truth: apps/turdmod-server-loader/src/extern_api.rs
// Keep the two files in sync — when one changes, change the other.

#ifndef TURDMOD_ENGINE_API_H
#define TURDMOD_ENGINE_API_H

#ifdef __cplusplus
extern "C" {
#endif

#include <stdint.h>

// =============================================================================
// Handler signature
// =============================================================================
//
// Handlers are called from the Rust DLL's Tokio worker threads whenever
// the companion sends an RPC request whose method name matches a
// registered handler.
//
// params_json:  NUL-terminated UTF-8 C string with request params. Valid
//               only for the duration of this call. Copy if needed.
// result_out:   The handler writes a pointer to its OWN buffer here
//               (NUL-terminated UTF-8 holding the JSON result body).
//               Set to NULL for `null` result. Buffer must remain valid
//               until the next invocation of this handler.
// error_out:    Same conventions as result_out; non-NULL means error.
//
// Return value: 0 on success, non-zero on error (set error_out).
//
// Memory ownership convention: NO malloc crosses the FFI boundary. The
// handler owns its result/error buffers — use a thread_local std::string
// member or similar. Rust copies the bytes before the handler returns
// from the dispatching tokio task.
typedef int32_t (*TurdmodEngineHandlerFn)(
    const char*  params_json,
    const char** result_out,
    const char** error_out
);

// =============================================================================
// Exported functions (resolved via GetProcAddress on turdmod_server_loader.dll)
// =============================================================================

// Returns the loader's CARGO_PKG_VERSION. Static buffer; do not free.
typedef const char* (*PFN_turdmod_engine_extern_api_version)(void);

// Register a handler for the named RPC method. Replaces any prior
// registration. Returns 0 on success, -1 on bad input.
typedef int32_t (*PFN_turdmod_engine_register_handler)(
    const char* method,
    TurdmodEngineHandlerFn fn_ptr
);

// Unregister a previously-registered handler. Returns 0 if removed,
// -1 if no handler was registered, -2 on bad input.
typedef int32_t (*PFN_turdmod_engine_unregister_handler)(
    const char* method
);

// Emit an engine event to all connected companion subscribers.
// json_data is parsed as JSON; on parse failure the event is dropped.
// Returns 0 on success, -1 if broadcaster not yet installed or bad input.
typedef int32_t (*PFN_turdmod_engine_emit_event)(
    const char* event_name,
    const char* json_data
);

// =============================================================================
// Convenience binder
// =============================================================================

// Holds resolved function pointers from turdmod_server_loader.dll. The
// C++ mod calls turdmod_engine_api_resolve() once during start_mod() and
// uses the struct fields thereafter. Returns 0 on success, non-zero if
// any required symbol is missing (loader DLL not present / wrong version).

typedef struct TurdmodEngineApi {
    PFN_turdmod_engine_extern_api_version  version;
    PFN_turdmod_engine_register_handler    register_handler;
    PFN_turdmod_engine_unregister_handler  unregister_handler;
    PFN_turdmod_engine_emit_event          emit_event;
} TurdmodEngineApi;

#ifdef __cplusplus
} // extern "C"
#endif

#endif // TURDMOD_ENGINE_API_H
