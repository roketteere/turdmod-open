// TurdMODEngineBridge.hpp
//
// UE4SS C++ mod that bridges the in-process side of the TurdMOD Engine.
// Loads alongside SCUMServer.exe, registers C-ABI handlers with
// turdmod_server_loader.dll, and forwards companion RPC into UE4SS's
// reflection / hook APIs.
//
// This is the production engine core. Community-authored mods use the
// Lua template at examples/turdmod-engine-bridge/ — Lua is the right
// surface for low-barrier community work. C++ is the right surface for
// the engine core that needs compile-time UE4 type safety and direct
// in-process FFI with the Rust loader (no file polling).

#pragma once

#include <Mod/CppUserModBase.hpp>
#include "turdmod_engine_api.h"

namespace TurdMOD {

class EngineBridge : public RC::CppUserModBase
{
public:
    EngineBridge();
    ~EngineBridge() override = default;

    // UE4SS lifecycle hooks.
    auto on_unreal_init()  -> void override;
    auto on_program_start() -> void override;
    auto on_update()        -> void override;

private:
    // Resolved entry points into turdmod_server_loader.dll.
    // Populated by `resolve_engine_api()` during on_unreal_init.
    TurdmodEngineApi m_api{};
    bool m_api_resolved{false};

    // Resolve the C-ABI exports from turdmod_server_loader.dll (already
    // loaded in-process by the launcher). Returns true on success;
    // false logs and leaves m_api_resolved = false.
    bool resolve_engine_api();

    // Register all our handlers with the loader.
    void register_handlers();

    // Install UE4 function hooks for outbound events (chat, login, etc.).
    void install_hooks();

    // ── Handler implementations ─────────────────────────────────────────────
    // Each handler returns a result via a thread_local buffer it owns.
    // Signatures match TurdmodEngineHandlerFn in turdmod_engine_api.h.
    static int32_t handle_ping(const char* params_json,
                               const char** result_out,
                               const char** error_out);

    static int32_t handle_broadcast_chat(const char* params_json,
                                         const char** result_out,
                                         const char** error_out);

    static int32_t handle_teleport_player(const char* params_json,
                                          const char** result_out,
                                          const char** error_out);

    static int32_t handle_get_online_players(const char* params_json,
                                             const char** result_out,
                                             const char** error_out);

    static int32_t handle_spawn_vehicle(const char* params_json,
                                        const char** result_out,
                                        const char** error_out);

    // dumpUFunctions — first real reflection call. Walks GUObjectArray
    // via UObjectGlobals::ForEachUObject, returns total UObject count
    // plus a JSON sample of the first 100 with their FName indices and
    // their ClassPrivate's FName index. The sample's class-index
    // distribution tells us which ComparisonIndex maps to "Function" so
    // we can filter in iteration 2 (full name resolution).
    static int32_t handle_dump_ufunctions(const char* params_json,
                                          const char** result_out,
                                          const char** error_out);
};

} // namespace TurdMOD
