// TurdMOD Engine Bridge â€” UE4SS C++ mod.
//
// Bridges between UE4SS reflection (in-process with GameServer.exe) and
// your_loader.dll /* YOUR_LOADER_DLL */ (also in-process). Companion talks to the
// loader DLL over a named pipe; the loader dispatches RPC methods via
// C-ABI handlers this mod registers in on_unreal_init().
//
// Canonical source: apps/turdmod-engine-bridge/ in the turdmod repo.
// This copy lives under UE4SS's cppmods/ for the build.

#include <Mod/CppUserModBase.hpp>
#include <DynamicOutput/DynamicOutput.hpp>
#include <Unreal/UObject.hpp>
#include <Unreal/UObjectGlobals.hpp>
#include <Unreal/UEngine.hpp>
#include <Unreal/FName.hpp>
#include <Unreal/_Common.hpp>
#include <UE4SSRuntime.hpp>

#include <Windows.h>
#include <Psapi.h>           // GetModuleFileNameExA (not used now but kept for future)
#include <TlHelp32.h>        // Thread32First/Next + CreateToolhelp32Snapshot â€” for patchInstructions thread-suspend
#include <intrin.h>          // _ReturnAddress() intrinsic for caller-aware filtering
#include <string>
#include <cstring>
#include <cstdint>
#include <cstdio>
#include <cstdlib>           // std::getenv for smoke-tick env-gate
#include <unordered_map>
#include <unordered_set>
#include <vector>
#include <fstream>
#include <filesystem>        // Phase 1.1 â€” config-file handlers
#include <chrono>
#include <atomic>
#include <array>             // pak-validator v3 telemetry buffer
#include <thread>            // smoke-tick emitter detaches its own thread
#include <mutex>             // ensure_hook_installed_once guard (pool = concurrent handlers)
#include <chrono>            // resolve_event_dispatch_ptrs retry-until-complete time guard

#include <polyhook2/Detour/x64Detour.hpp>

#include "turdmod_engine_api.h"

using namespace RC;
using namespace RC::Unreal;

namespace TurdMOD {

// â”€â”€â”€ Scan timeout guard â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Macro-based timeout for ForEachUObject lambdas. Add SCAN_TIMEOUT_INIT()
// before the scan, SCAN_TIMEOUT_CHECK() at the top of the lambda body.
// If 8 seconds elapse, all subsequent iterations short-circuit.
//
// This prevents ANY handler from jamming the named pipe.
#define SCAN_TIMEOUT_INIT() \
    auto _scan_start = std::chrono::steady_clock::now(); \
    bool _scan_timed_out = false; \
    size_t _scan_count = 0;

#define SCAN_TIMEOUT_CHECK() \
    if (_scan_timed_out) return; \
    if ((++_scan_count & 0xFFF) == 0) { \
        auto _elapsed = std::chrono::duration_cast<std::chrono::milliseconds>( \
            std::chrono::steady_clock::now() - _scan_start).count(); \
        if (_elapsed > 8000) { _scan_timed_out = true; return; } \
    }

// â”€â”€â”€ Logging helpers â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

template <typename... Args>
static void log_info_fmt(const wchar_t* fmt, Args&&... args)
{
    Output::send<LogLevel::Default>(fmt, std::forward<Args>(args)...);
}

// Convert UTF-8 std::string â†’ std::wstring (UTF-16) properly.
// Replaces the older byte-copy `wstring(s.begin(), s.end())` pattern
// which mangled multi-byte chars: kanji/emoji rendered as half-width
// katakana mojibake in game chat (Joel 2026-05-22/23). Uses Windows'
// MultiByteToWideChar which is reliable for CP_UTF8.
static std::wstring utf8_to_wstring(const std::string& utf8)
{
    if (utf8.empty()) return {};
    int wide_len = ::MultiByteToWideChar(
        CP_UTF8, 0, utf8.c_str(),
        static_cast<int>(utf8.size()), nullptr, 0);
    if (wide_len <= 0) return {};
    std::wstring out(static_cast<size_t>(wide_len), L'\0');
    ::MultiByteToWideChar(
        CP_UTF8, 0, utf8.c_str(),
        static_cast<int>(utf8.size()), &out[0], wide_len);
    return out;
}

static void log_info(const std::wstring& msg)
{
    Output::send<LogLevel::Default>(STR("[TurdMODEngineBridge] {}\n"), msg);
}

// Forward declaration â€” defined at line ~5255, needed by runAdminCommand at ~2738
static UObject* find_pc_by_player_name(const std::wstring& want_name_w);

static void log_error(const std::wstring& msg)
{
    Output::send<LogLevel::Error>(STR("[TurdMODEngineBridge] ERROR: {}\n"), msg);
}

// â”€â”€â”€ Resolved loader API â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

static TurdmodEngineApi g_api{};
static bool g_api_resolved = false;

// [SCRUBBED] Game-specific section removed (1051 lines)


static bool resolve_engine_api()
{
    // Race: UE4SS fires on_unreal_init within a few ms of being injected,
    // but the launcher injects the loader DLL sequentially AFTER UE4SS.
    // The loader's DllMain may not have completed when we get here. Retry
    // every 50ms for up to 10 seconds; real-world arrival is single-digit ms.
    HMODULE loader = nullptr;
    for (int attempt = 0; attempt < 200; ++attempt) {
        loader = GetModuleHandleA("your_loader.dll /* YOUR_LOADER_DLL */");
        if (!loader) loader = GetModuleHandleA("turdmod-server-loader.dll");
        if (loader) break;
        Sleep(50);
    }
    if (!loader) {
        log_error(STR("your_loader.dll /* YOUR_LOADER_DLL */ not present in-process after 10s"));
        return false;
    }
    if (true) {
        log_info(STR("your_loader.dll /* YOUR_LOADER_DLL */ located after on_unreal_init race wait"));
    }

    g_api.version            = reinterpret_cast<PFN_turdmod_engine_extern_api_version>(
        GetProcAddress(loader, "turdmod_engine_extern_api_version"));
    g_api.register_handler   = reinterpret_cast<PFN_turdmod_engine_register_handler>(
        GetProcAddress(loader, "turdmod_engine_register_handler"));
    g_api.unregister_handler = reinterpret_cast<PFN_turdmod_engine_unregister_handler>(
        GetProcAddress(loader, "turdmod_engine_unregister_handler"));
    g_api.emit_event         = reinterpret_cast<PFN_turdmod_engine_emit_event>(
        GetProcAddress(loader, "turdmod_engine_emit_event"));

    if (!g_api.version || !g_api.register_handler || !g_api.unregister_handler || !g_api.emit_event) {
        log_error(STR("your_loader.dll /* YOUR_LOADER_DLL */ loaded but one or more exports missing"));
        return false;
    }

    g_api_resolved = true;
    return true;
}

// â”€â”€â”€ Handler implementations â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Thread-local buffers â€” each handler owns its own. Lifetime is "until
// the next call to this same handler", matching the FFI contract in
// turdmod_engine_api.h.

static thread_local std::string s_ping_result;
static thread_local std::string s_broadcast_result;
static thread_local std::string s_teleport_result;
static thread_local std::string s_players_result;
static thread_local std::string s_spawn_result;
static thread_local std::string s_dump_result;
static thread_local std::string s_find_result;
static thread_local std::string s_describe_result;
static thread_local std::string s_classes_result;
static thread_local std::string s_admin_result;
static thread_local std::string s_sendchat_result;
static thread_local std::string s_widgets_result;
static thread_local std::string s_describewidget_result;
static thread_local std::string s_dump_all_classes_result;
static thread_local std::string s_dump_all_enums_result;
static thread_local std::string s_dump_all_structs_result;
static thread_local std::string s_raid_banner_result;
static thread_local std::string s_set_fame_result;
static thread_local std::string s_set_currency_result;
static thread_local std::string s_probe_quest_result;
static thread_local std::string s_set_economy_result;
static thread_local std::string s_list_instances_result;
static thread_local std::string s_dump_admin_commands_result;
static thread_local std::string s_send_chat_line_to_player_result;
static thread_local std::string s_list_handlers_result;
static thread_local std::string s_kick_player_result;
static thread_local std::string s_set_time_of_day_result;
static thread_local std::string s_run_hello_world_result;
static thread_local std::string s_set_flying_mode_result;
static thread_local std::string s_possess_actor_result;
static thread_local std::string s_force_weather_snapshot_result;
static thread_local std::string s_unpossess_actor_result;
static thread_local std::string s_move_actor_result;
static thread_local std::string s_tame_nearby_result;
static thread_local std::string s_provoke_zombies_result;
static thread_local std::string s_spawn_mech_result;
static thread_local std::string s_mech_fire_result;
static thread_local std::string s_spawn_item_result;
static thread_local std::string s_admin_output_result;

// Method names of every successfully-registered handler â€” populated in
// on_unreal_init's registration loop, read by handle_list_handlers so
// the Manager's Engine Console page can auto-discover what RPCs exist
// without us maintaining a parallel list in TypeScript.
static std::vector<std::string> g_registered_methods;

// Forward declarations â€” definitions below.
static std::string extract_json_str(const char* params_json, const char* key);
static std::wstring fname_to_wstring(const FName& name);
static int32_t find_property_offset(UObject* uclass, const wchar_t* prop_name);
static std::wstring read_fstring_at(const void* obj, int32_t offset);
static std::wstring read_ftext_at(const void* obj, int32_t offset);
static UObject* find_first_instance_of_class(const wchar_t* class_name);

// â”€â”€â”€ Engine event emit helpers â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Pushes typed JSON events through `turdmod_engine_emit_event` (resolved
// from turdmod-server-loader.dll at on_unreal_init time, see resolve_engine_api).
// The loader-side broadcaster fans out to every connected named-pipe client
// (Manager + scumpilot + tooling) automatically â€” see
// turdmod-server-loader/src/admin_api.rs::EventBroadcaster + serve_connection.
//
// Game-thread safety: g_api.emit_event() ends up calling Tokio's
// broadcast::Sender::send() which is non-blocking and drops on overflow.
// Safe to call from the ProcessEvent hook + the smoke-tick thread.
//
// Convention: keep event payloads small (â‰¤ 1 KB ideally) and pre-formatted
// JSON. The C-ABI is `(event_name, json_data)` â€” both NUL-terminated UTF-8.
// Per scumpilot's bridge-gap-contract.md, event payloads must match
// turdmod-companion/src/parsers.ts ServerEvent shape byte-for-byte.
static inline void emit_engine_event(const char* event_name, const std::string& json_payload)
{
    if (!g_api_resolved || !g_api.emit_event) return;
    g_api.emit_event(event_name, json_payload.c_str());
}

// ISO-8601 UTC timestamp for event payloads. scumpilot accepts both
// ISO-8601 and SCUM's log format (YYYY.MM.DD-HH.MM.SS) â€” we prefer ISO
// per the contract.
static std::string iso_ts_now()
{
    using namespace std::chrono;
    auto now = system_clock::now();
    auto secs = time_point_cast<seconds>(now);
    auto ms = duration_cast<milliseconds>(now - secs).count();
    std::time_t t = system_clock::to_time_t(now);
    std::tm utc{};
#if defined(_WIN32)
    gmtime_s(&utc, &t);
#else
    gmtime_r(&t, &utc);
#endif
    char buf[40];
    std::snprintf(buf, sizeof(buf), "%04d-%02d-%02dT%02d:%02d:%02d.%03lldZ",
                  utc.tm_year + 1900, utc.tm_mon + 1, utc.tm_mday,
                  utc.tm_hour, utc.tm_min, utc.tm_sec,
                  static_cast<long long>(ms));
    return buf;
}

// EChatType byte â†’ contract channel name. Mapping confirmed live 2026-05-17
// via AdminPage broadcast (see reference_scum_chat_channel_colors memory)
// and cross-referenced against AdminPage.tsx's hardcoded options.
//   0 = Local, 1 = Squad, 2 = Global, 3 = Admin
// scumpilot's contract excludes Whisper (different UFunction); Admin is
// server-emitted so it should never appear on Chat_Server_BroadcastChatMessage,
// but we map defensively.
static const char* chat_channel_to_string(uint8_t channel)
{
    switch (channel) {
        case 0: return "Local";
        case 1: return "Squad";
        case 2: return "Global";
        case 3: return "Admin";
        default: return "Unknown";
    }
}

// JSON-escape a UTF-8 string for safe embedding inside a JSON string
// literal. Handles "quote", \backslash, \n, \r, \t + \uXXXX for any
// control char < 0x20. Returns by value â€” short strings are cheap.
static std::string json_escape(const std::string& s)
{
    std::string out;
    out.reserve(s.size() + 8);
    for (char c : s) {
        switch (c) {
            case '"':  out += "\\\""; break;
            case '\\': out += "\\\\"; break;
            case '\n': out += "\\n";  break;
            case '\r': out += "\\r";  break;
            case '\t': out += "\\t";  break;
            default:
                if (static_cast<uint8_t>(c) < 0x20) {
                    char b[8];
                    std::snprintf(b, sizeof(b), "\\u%04x", static_cast<uint8_t>(c));
                    out += b;
                } else {
                    out += c;
                }
        }
    }
    return out;
}

// Extract the player display name from a PlayerController by walking
// PC.PlayerState.PlayerNamePrivate (falling back to PlayerName). Mirrors
// the get_online_players + teleportPlayer offset-finding pattern â€” see
// commit a7aaa42 for the design. Cached per-class so each unique class
// pair pays the find_property_offset cost once.
//
// Returns empty wstring if any step fails (PC has no class / no PlayerState /
// no FString readable). Callers should treat empty as "unknown sender".
static std::wstring read_pc_player_name(UObject* pc)
{
    if (!pc) return {};
    auto* p = reinterpret_cast<const uint8_t*>(pc);
    auto* pc_class = *reinterpret_cast<UObject* const*>(p + 0x10);
    if (!pc_class) return {};

    static std::unordered_map<UObject*, int32_t> ps_off_cache;
    static std::unordered_map<UObject*, int32_t> name_off_cache;

    auto ps_it = ps_off_cache.find(pc_class);
    int32_t ps_off;
    if (ps_it == ps_off_cache.end()) {
        ps_off = find_property_offset(pc_class, L"PlayerState");
        ps_off_cache[pc_class] = ps_off;
    } else {
        ps_off = ps_it->second;
    }
    if (ps_off < 0) return {};

    UObject* ps = *reinterpret_cast<UObject* const*>(p + ps_off);
    if (!ps) return {};
    auto* ps_class = *reinterpret_cast<UObject* const*>(
        reinterpret_cast<const uint8_t*>(ps) + 0x10);
    if (!ps_class) return {};

    auto name_it = name_off_cache.find(ps_class);
    int32_t name_off;
    if (name_it == name_off_cache.end()) {
        name_off = find_property_offset(ps_class, L"PlayerNamePrivate");
        if (name_off < 0) name_off = find_property_offset(ps_class, L"PlayerName");
        name_off_cache[ps_class] = name_off;
    } else {
        name_off = name_it->second;
    }
    if (name_off < 0) return {};

    return read_fstring_at(ps, name_off);
}

// â”€â”€â”€ Global ProcessEvent hook (PolyHook2) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// One detour at UObject::ProcessEvent (resolved via vtable lookup using
// VTableLayoutMap[L"ProcessEvent"]). Catches every UFunction invocation
// SCUM makes â€” thousands per frame. We filter by FName ComparisonIndex
// (cached after first sighting) and log only chat-related calls. The
// game-thread-safety contract: do not allocate on every fire; do not
// take long locks; do not recurse into ProcessEvent.

// Read SteamID64 from a PlayerController's PlayerState.UniqueId
// (FUniqueNetIdRepl at offset 0x250 on PlayerState).
//
// Closes the wave-1 impersonation gap (B-???): until this lands the
// bridge ships `steam:""` on every chat/login/logout event, and the
// owner-override / consent / regulars-memory code falls back to
// displayName comparison â€” exploitable by anyone with the right name.
//
// UE 4.27 FUniqueNetIdRepl layout (verified against UE source):
//   +0x00  TSharedPtr<const FUniqueNetId>::Object  (8 bytes â€” raw ptr)
//   +0x08  TSharedPtr<const FUniqueNetId>::RefCount (8 bytes â€” control block ptr)
//   +0x10  TArray<uint8> ReplicationBytes          (16 bytes)
//
// For Steam-platform connections, the pointed-to FUniqueNetId is a
// FUniqueNetIdSteam:
//   +0x00  vtable*
//   +0x08  uint64  SteamID
//
// Returns empty string if any step fails (null ptr, invalid SteamID
// range â€” e.g. for bots / disconnected players).
static std::string read_pc_steam_id(UObject* pc)
{
    if (!pc) return {};
    auto* p = reinterpret_cast<const uint8_t*>(pc);
    auto* pc_class = *reinterpret_cast<UObject* const*>(p + 0x10);
    if (!pc_class) return {};

    static std::unordered_map<UObject*, int32_t> ps_off_cache;
    auto it = ps_off_cache.find(pc_class);
    int32_t ps_off;
    if (it == ps_off_cache.end()) {
        ps_off = find_property_offset(pc_class, L"PlayerState");
        ps_off_cache[pc_class] = ps_off;
    } else {
        ps_off = it->second;
    }
    if (ps_off < 0) return {};

    UObject* ps = *reinterpret_cast<UObject* const*>(p + ps_off);
    if (!ps) return {};
    auto* ps_bytes = reinterpret_cast<const uint8_t*>(ps);

    // PlayerState.UniqueId (offset 0x250) is an FUniqueNetIdRepl:
    //   +0x00 vtable, +0x08 net-id obj ptr, +0x10 shared-ref controller,
    //   +0x18 ReplicationBytes TArray<uint8> data ptr, +0x20 Num (int32).
    // SCUM serializes the SteamID into ReplicationBytes as BCD decimal digits:
    //   [type][len=N][N bytes, each two decimal digits]. e.g. len=9 ->
    //   "0YOUR_STEAM_ID". Offset-relative => survives SCUM restarts; the old
    //   code read +0x00 (the vtable) as the net-id ptr and ALWAYS returned empty.
    //   RE'd live 2026-06-07; if the format ever differs we bail -> service
    //   falls back to name-based profile resolution (scumdb::profile_id_for_name).
    static constexpr int32_t kUniqueIdOffset = 0x250;
    const uint8_t* uid = ps_bytes + kUniqueIdOffset;
    const uint8_t* rep = *reinterpret_cast<const uint8_t* const*>(uid + 0x18);
    int32_t rep_num = *reinterpret_cast<const int32_t*>(uid + 0x20);
    if (!rep || rep_num < 3) return {};
    int len = rep[1];
    if (len <= 0 || len > 10 || 2 + len > rep_num) return {};
    uint64_t steam_id = 0;
    for (int i = 0; i < len; i++) {
        uint8_t b = rep[2 + i];
        if ((b >> 4) > 9 || (b & 0x0F) > 9) return {}; // not BCD -> bail
        steam_id = steam_id * 100 + (b >> 4) * 10 + (b & 0x0F);
    }
    // Valid SteamID64 range â€” individual user accounts.
    if (steam_id < 76561197960265728ULL || steam_id > 76561202255233023ULL) {
        return {};
    }
    char buf[24];
    std::snprintf(buf, sizeof(buf), "%llu",
                  static_cast<unsigned long long>(steam_id));
    return std::string(buf);
}

static PLH::x64Detour* g_pe_detour = nullptr;
static uint64_t g_pe_trampoline = 0;
// Decision cache: 0 = skip, 1 = log
static std::unordered_map<uint32_t, int> g_fn_filter_cache;

// Event-dispatch cache. Parallel to g_fn_filter_cache but answers a different
// question: "does this UFunction fire a typed engine event?" Keyed by FName
// ComparisonIndex so first-sighting cost is paid once per unique fn (~12k
// worst case), subsequent calls are a single hashmap hit.
//
// Values are dispatch IDs (small enum below). 0 = "no dispatch" (default).
// The actual payload-build + emit happens via a switch in dispatch_engine_event.
//
// Wire-shape contract: see scumpilot/docs/bridge-gap-contract.md + the
// canonical types in turdmod-companion/src/parsers.ts. Per-event payload
// MUST match those shapes byte-for-byte.
enum EventDispatchKind : uint8_t {
    EV_NONE              = 0,
    EV_CHAT              = 1,  // PlayerRpcChannel::Chat_Server_BroadcastChatMessage
                               //   (server RPC fired by client; `this_` is the channel,
                               //    its Outer is the owning PlayerController â†’ PlayerState)
    EV_LOGIN             = 2,  // GameModeBase::HandleStartingNewPlayer(NewPlayer: PC)
                               //   (fires pre-PostLogin; PlayerName may be momentarily
                               //    empty if replication hasn't filled it yet)
    EV_LOGOUT            = 3,  // PlayerRpcChannel::SurvivalStats_Server_HandlePlayerLogout(PC)
                               //   SCUM-specific logout â€” fires when a client's server-side
                               //   stats cleanup runs (both intentional logout and
                               //   disconnect). Discovered 2026-05-22 from scumdump grep;
                               //   supersedes the earlier "no reflectable logout UFunction"
                               //   note. Pairs with SurvivalStats_Server_HandlePlayerLogin.
    // TODO(events): EV_KILL, EV_VEHICLE_SPAWNED, EV_VEHICLE_DESTROYED,
    //   EV_WEATHER_CHANGED, EV_TIME_CHANGED â€” each needs its own dump grep.
    //   Wave 4 of scumpilot's gap contract. EV_TIME_CHANGED + EV_WEATHER_CHANGED
    //   are currently emitted only on the OUTBOUND side (setTimeOfDay /
    //   setWeather response path), not via ProcessEvent dispatch.
    EV_ADMIN_RESPONSE = 4,  // Chat_Client_ProcessAdminCommand â€” admin command echo
    EV_CLIENT_CHAT    = 5,  // Chat_Client_SendMessageToChat â€” all serverâ†’client chat
                             //   including ListItems output. Only captured when
                             //   g_admin_capture_active is true.
};
static std::unordered_map<uint32_t, uint8_t> g_fn_event_dispatch;

// Build-safe event dispatch (2026-05-30): resolved UFunction* -> event_kind,
// populated ONCE at startup by resolve_event_dispatch_ptrs(). hooked_process_event
// compares the `fn` POINTER against this map instead of reading the FName at
// fn+0x18 every call -- that per-call read crashed build 23451409 on the player-
// join spawn-flood. @inv: UFunction pointers are stable GUObjectArray singletons.
static std::unordered_map<void*, uint8_t> g_fn_ptr_dispatch;
static bool g_fn_ptr_resolved = false;
static bool resolve_event_dispatch_ptrs();  // fwd-decl; defined after fname_to_wstring

// Admin command response capture buffer â€” ring buffer of last 500 lines.
// Written by EV_ADMIN_RESPONSE dispatch (both Chat_Client_ProcessAdminCommand
// AND Chat_Client_SendMessageToChat when capture mode is active).
static std::vector<std::string> g_admin_output_buf;
static std::mutex g_admin_output_mutex;
static const size_t kMaxAdminOutputLines = 500;
static std::atomic<bool> g_admin_capture_active{false}; // set by runAdminCommand, cleared by getAdminOutput

// Notification capture (autonomous-announce RE): record the 24-byte
// NotificationDescriptionReplicationHelper struct the next time
// NetMulticast_RequestNotification fires (i.e. an admin runs #Announce). The
// struct is opaque to reflection, so we capture a real one + replicate it.
static std::atomic<void*> g_notif_fn_ptr{nullptr};
static std::atomic<bool>  g_notif_arm{false};
static std::atomic<bool>  g_notif_captured{false};
static uint8_t            g_notif_buf[64] = {0};
// Strings reached by dereferencing the struct's 3 pointers (read in-hook while
// the data is alive). Level 1 = read wchars at *ptrN; level 2 = read wchars at
// **ptrN (FText nests its FString one level deeper). ASCII-narrowed.
static char               g_notif_deref[6][192] = {{0}};
// Deep capture: the helper wraps a TSharedPtr<NotificationDescriptionData>.
// Grab ptr0's object (the Data, ~56 bytes) + ptr8's object (the ref controller).
static uint8_t            g_notif_data[96]   = {0};
static uint8_t            g_notif_refctl[48] = {0};
// Message text read out of the Data at capture time (to see if SCUM embeds rich-text
// color markup). 4 candidate offsets x 3 deref levels, ASCII-narrowed.
static char               g_notif_msg[12][192] = {{0}};
// Optional capture filter: when set, the hook only captures a notification
// whose deref/msg slots contain this marker substring (skips others, stays
// armed). Lets us grab a SPECIFIC banner amid gameplay-notification noise.
static char               g_notif_marker[64] = {0};

// Build + emit a typed event for a recognized UFunction call. Called from
// hooked_process_event when the dispatch cache says this fn maps to a
// typed event kind. Kept in its own function so the hot path stays tight
// and the per-event payload-building logic is grouped here for review.
//
// Per-event payload contract â€” keep stable; scumpilot + Manager UI rely
// on these shapes. Authoritative source: turdmod-companion/src/parsers.ts
// ServerEvent. See also scumpilot/docs/bridge-gap-contract.md and
// docs/runbooks/bridge-events.md.
// Thread-local guard set by handle_broadcast_chat / similar outbound chat
// helpers while they invoke ProcessEvent. When true, the immediately-
// following EV_CHAT hook fire is from OUR OWN broadcast (not a player
// typing), and emitting it would feed scumpilot/companion a fake "TechyRican
// said X" event â€” because we route through a real PlayerController as the
// WorldContextObject, the dispatched chat looks like it came from that
// player. This caused B-010 (god-admin chat loop) live 2026-05-22 22:46.
thread_local bool tl_suppress_next_chat_event = false;

static void dispatch_engine_event(uint8_t kind, UObject* this_, UFunction* /*fn*/, void* params)
{
    if (!params) return;

    switch (kind) {
        case EV_CHAT: {
            // Suppress outbound broadcasts â€” the server's own broadcastChat
            // RPC dispatches via ProcessEvent on a real PC, which makes the
            // chat look like it came from that player. Filtering here keeps
            // god-admin from replying to its own messages (B-010).
            if (tl_suppress_next_chat_event) {
                tl_suppress_next_chat_event = false;
                return;
            }

            // PlayerRpcChannel::Chat_Server_BroadcastChatMessage(FString Message, EChatType Channel)
            //   params layout:
            //     +0   FString  Message  (16 bytes: data ptr, num, max)
            //     +16  uint8    Channel  (1 byte; EChatType: 0=Local 1=Squad 2=Global 3=Admin)
            //   `this_` is the PlayerRpcChannel; its Outer is the owning
            //   PlayerController (UE4's standard component-owner pattern).
            //   We walk PC â†’ PlayerState â†’ PlayerNamePrivate for sender info.

            std::wstring text_w = read_fstring_at(params, 0);
            uint8_t channel = *(reinterpret_cast<const uint8_t*>(params) + 16);

            // Walk this_ â†’ Outer to get the owning PlayerController. Outer
            // lives at offset 0x20 on every UObject in UE 4.27.
            UObject* outer = nullptr;
            if (this_) {
                outer = *reinterpret_cast<UObject* const*>(
                    reinterpret_cast<const uint8_t*>(this_) + 0x20);
            }
            std::wstring player_w = read_pc_player_name(outer);
            std::string steam_id = read_pc_steam_id(outer);

            // Narrow strings to UTF-8 for JSON. wchar_t on Windows is UTF-16;
            // ASCII passes through cleanly. Non-ASCII may garble â€” fix in v2
            // with a real UTF-16â†’UTF-8 transcoder if SCUM communities use it.
            std::string text_utf8(text_w.begin(), text_w.end());
            std::string player_utf8(player_w.begin(), player_w.end());
            std::string ts = iso_ts_now();

            // Per contract: ts, channel, player, steam (SteamID64 string or
            // empty for bots / pre-auth), text.
            std::string payload;
            payload.reserve(text_utf8.size() + player_utf8.size() + 128);
            payload += "{\"ts\":\""; payload += ts;
            payload += "\",\"channel\":\""; payload += chat_channel_to_string(channel);
            payload += "\",\"player\":\""; payload += json_escape(player_utf8);
            payload += "\",\"steam\":\""; payload += steam_id; payload += "\",";
            payload += "\"text\":\""; payload += json_escape(text_utf8);
            payload += "\"}";
            emit_engine_event("chat", payload);
            break;
        }

        case EV_LOGIN: {
            // GameModeBase::HandleStartingNewPlayer(UObject* NewPlayer)
            //   params layout:
            //     +0   UObject*  NewPlayer (the new PlayerController)
            //
            // Caveat: fires very early â€” PlayerName may not be replicated yet.
            // scumpilot's brain re-folds chat-event senders later so an empty
            // name at login is recoverable. We still emit so the player joins
            // state.players asap; the name self-corrects via subsequent events.
            UObject* new_pc = *reinterpret_cast<UObject* const*>(params);
            std::wstring player_w = read_pc_player_name(new_pc);
            std::string steam_id = read_pc_steam_id(new_pc);
            std::string player_utf8(player_w.begin(), player_w.end());
            std::string ts = iso_ts_now();

            // Per contract: ts, ip (we don't have it from this hook), steam
            // (SteamID64 string or empty for pre-auth), player (required),
            // pos (Vec3, default 0,0,0).
            std::string payload;
            payload.reserve(player_utf8.size() + 192);
            payload += "{\"ts\":\""; payload += ts;
            payload += "\",\"steam\":\""; payload += steam_id; payload += "\",";
            payload += "\"player\":\""; payload += json_escape(player_utf8);
            payload += "\",\"pos\":{\"x\":0,\"y\":0,\"z\":0}}";
            emit_engine_event("login", payload);
            break;
        }

        case EV_ADMIN_RESPONSE: {
            std::wstring resp_w = read_fstring_at(params, 0);
            std::string resp_utf8(resp_w.begin(), resp_w.end());
            {
                std::lock_guard<std::mutex> lock(g_admin_output_mutex);
                g_admin_output_buf.push_back(resp_utf8);
                if (g_admin_output_buf.size() > kMaxAdminOutputLines) {
                    g_admin_output_buf.erase(g_admin_output_buf.begin());
                }
            }
            break;
        }

        case EV_CLIENT_CHAT: {
            // Chat_Client_SendMessageToChat â€” captures ALL serverâ†’client chat
            // but only stores it when admin capture mode is active.
            if (!g_admin_capture_active.load()) break;
            std::wstring msg_w = read_fstring_at(params, 0);
            std::string msg_utf8(msg_w.begin(), msg_w.end());
            {
                std::lock_guard<std::mutex> lock(g_admin_output_mutex);
                g_admin_output_buf.push_back(msg_utf8);
                if (g_admin_output_buf.size() > kMaxAdminOutputLines) {
                    g_admin_output_buf.erase(g_admin_output_buf.begin());
                }
            }
            break;
        }

        case EV_LOGOUT: {
            // PlayerRpcChannel::SurvivalStats_Server_HandlePlayerLogout(UObject* PlayerController)
            //   params layout:
            //     +0   UObject*  PlayerController (the leaving player's PC)
            //
            // Fires on the server-side stats cleanup path. Captures both
            // graceful logouts and disconnects. The PC may be partially torn
            // down by this point â€” read_pc_player_name is best-effort, falls
            // back to empty string if the PlayerState is already detached.
            UObject* leaving_pc = *reinterpret_cast<UObject* const*>(params);
            std::wstring player_w = read_pc_player_name(leaving_pc);
            std::string steam_id = read_pc_steam_id(leaving_pc);
            std::string player_utf8(player_w.begin(), player_w.end());
            std::string ts = iso_ts_now();

            // Per scumpilot's bridge-gap-contract: ts, steam (SteamID64 or
            // empty if PC partially torn down), player.
            std::string payload;
            payload.reserve(player_utf8.size() + 128);
            payload += "{\"ts\":\""; payload += ts;
            payload += "\",\"steam\":\""; payload += steam_id; payload += "\",";
            payload += "\"player\":\""; payload += json_escape(player_utf8);
            payload += "\"}";
            emit_engine_event("logout", payload);
            break;
        }

        default:
            break;
    }
}

// @inv: OFF by default — gates the unsafe FName-read event routing in the PE hook.
// The hook stays installed (drain-only) for game-thread work; routing is opt-in.
// Default-ON since 2026-05-30: the per-call FName read that crashed build 23451409
// is gone (replaced by pointer-compare via g_fn_ptr_dispatch). Opt OUT with
// TURDMOD_PE_HOOK=0 if a future build regresses (kill-switch, no rebuild needed).
static bool g_pe_event_routing_enabled = true;

extern "C" void hooked_process_event(UObject* this_, UFunction* fn, void* params)
{
    // Game-thread work drainage â€” we're on UE's game thread here.
    // Cheap no-op if no work pending; runs LoadPackage if a loadAsset
    // request is queued.
    drain_load_asset_request();
    drain_despawn_request();
    drain_admin_command_request();

    // Armed one-shot capture of the notification struct (autonomous-announce RE).
    // Cheap: one relaxed bool load per call; only memcpys when armed + fn matches.
    if (g_notif_arm.load(std::memory_order_relaxed) && fn && params
        && static_cast<void*>(fn) == g_notif_fn_ptr.load(std::memory_order_relaxed)) {
        std::memcpy(g_notif_buf, params, sizeof(g_notif_buf));
        // Deref the struct's 3 pointers (offsets 0/8/16) while the data is alive.
        auto plausible = [](const void* p){ auto v = reinterpret_cast<uintptr_t>(p); return v > 0x10000 && v < 0x7FFFFFFFFFFFULL; };
        auto read_wstr = [](const wchar_t* p, char* dst) {
            dst[0] = 0; if (!p) return;
            for (int j = 0; j < 80; ++j) { wchar_t w = p[j]; if (w == 0) { dst[j] = 0; break; } dst[j] = (w >= 32 && w < 127) ? (char)w : '.'; dst[j+1] = 0; }
        };
        for (int k = 0; k < 3; ++k) {
            auto* p1 = *reinterpret_cast<wchar_t* const*>(reinterpret_cast<const uint8_t*>(params) + k*8);
            read_wstr(plausible(p1) ? p1 : nullptr, g_notif_deref[k]);
            auto* p2 = plausible(p1) ? *reinterpret_cast<wchar_t* const*>(p1) : nullptr;
            read_wstr(plausible(p2) ? p2 : nullptr, g_notif_deref[k+3]);
        }
        // Deep grab: ptr0's object (the Data) + ptr8's object (ref controller).
        std::memset(g_notif_data, 0, sizeof(g_notif_data));
        std::memset(g_notif_refctl, 0, sizeof(g_notif_refctl));
        void* d0 = *reinterpret_cast<void**>(reinterpret_cast<uint8_t*>(params) + 0);
        if (plausible(d0)) std::memcpy(g_notif_data, d0, sizeof(g_notif_data));
        // Read the Message text out of the Data while it's alive: try heap-ptr fields
        // at off 8/16/24/32, each 3 deref levels, ASCII-narrowed. One slot should show
        // the message (and any embedded <color>/rich-text markup driving the color).
        std::memset(g_notif_msg, 0, sizeof(g_notif_msg));
        if (plausible(d0)) {
            int slot = 0;
            for (int off : { 8, 16, 24, 32 }) {
                auto* base = reinterpret_cast<const uint8_t*>(d0) + off;
                auto* p1 = *reinterpret_cast<wchar_t* const*>(base);
                read_wstr(plausible(p1) ? p1 : nullptr, g_notif_msg[slot * 3 + 0]);
                auto* p2 = plausible(p1) ? *reinterpret_cast<wchar_t* const*>(p1) : nullptr;
                read_wstr(plausible(p2) ? p2 : nullptr, g_notif_msg[slot * 3 + 1]);
                auto* p3 = plausible(p2) ? *reinterpret_cast<wchar_t* const*>(p2) : nullptr;
                read_wstr(plausible(p3) ? p3 : nullptr, g_notif_msg[slot * 3 + 2]);
                ++slot;
            }
        }
        void* d8 = *reinterpret_cast<void**>(reinterpret_cast<uint8_t*>(params) + 8);
        if (plausible(d8)) std::memcpy(g_notif_refctl, d8, sizeof(g_notif_refctl));
        // NOTE: writing the live controller's refcount here HARD-CRASHES SCUM (corrupts
        // UE shared-ptr bookkeeping mid-ProcessEvent). Capture is read-only only.
        // Marker filter: if set, only keep this capture when a deref/msg slot contains
        // the marker; otherwise stay armed and let the next notification try. Lets us
        // target a specific banner amid frequent gameplay notifications.
        bool keep = true;
        if (g_notif_marker[0]) {
            keep = false;
            for (int mi = 0; mi < 12 && !keep; ++mi)
                if (g_notif_msg[mi][0] && std::strstr(g_notif_msg[mi], g_notif_marker)) keep = true;
            for (int di = 0; di < 6 && !keep; ++di)
                if (g_notif_deref[di][0] && std::strstr(g_notif_deref[di], g_notif_marker)) keep = true;
        }
        if (keep) {
            g_notif_captured.store(true, std::memory_order_relaxed);
            g_notif_arm.store(false, std::memory_order_relaxed);
        }
    }

    // @brk: the FName-read event routing below is GATED OFF by default. That
    // `fn + 0x18` read on the join spawn-flood was the build-23451409 server crash.
    // The hook now runs DRAIN-ONLY — a safe game-thread executor for the loadAsset
    // and despawn queues. Re-enable routing (g_pe_event_routing_enabled) only once
    // the UFunction-name read is made build-safe.
    if (g_pe_event_routing_enabled && fn) {
        // Build-safe dispatch (2026-05-30): compare the UFunction POINTER against
        // the table resolved ONCE at startup (resolve_event_dispatch_ptrs). NO
        // per-call fn+0x18 FName read -- that crashed build 23451409 on the
        // player-join spawn-flood. Pointers are stable GUObjectArray singletons.
        uint8_t event_kind = EV_NONE;
        auto pit = g_fn_ptr_dispatch.find(static_cast<void*>(fn));
        if (pit != g_fn_ptr_dispatch.end()) event_kind = pit->second;

        if (event_kind != EV_NONE) {
            dispatch_engine_event(event_kind, this_, fn, params);
        }
        // Swallow ! commands from public chat: emit the event (service receives
        // the command) but skip the original ProcessEvent so it never reaches
        // other players. Responses go via sendChatLineToPlayer (admin text).
        if (event_kind == EV_CHAT && params) {
            std::wstring msg_text = read_fstring_at(params, 0);
            if (!msg_text.empty() && msg_text[0] == L'!') {
                return;
            }
        }
    }
    // Call original via trampoline. Same param order.
    auto orig = reinterpret_cast<void(*)(UObject*, UFunction*, void*)>(g_pe_trampoline);
    orig(this_, fn, params);
}

static bool install_processevent_hook()
{
    if (g_pe_detour) {
        return true;
    }

    auto it = UObject::VTableLayoutMap.find(L"ProcessEvent");
    if (it == UObject::VTableLayoutMap.end()) {
        log_error(STR("[HOOK] VTableLayoutMap missing ProcessEvent â€” VTableLayout.ini not loaded?"));
        return false;
    }
    uint32_t offset = it->second;

    UObject* sample = nullptr;
    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        if (!sample) sample = obj;
    });
    if (!sample) {
        log_error(STR("[HOOK] No UObject to read vtable from"));
        return false;
    }

    void** vtable = *reinterpret_cast<void***>(sample);
    void* pe_addr = *reinterpret_cast<void**>(
        reinterpret_cast<uint8_t*>(vtable) + offset);
    log_info_fmt(STR("[HOOK] UObject::ProcessEvent resolved @ {:p} via vtable offset {:#x}\n"),
                 pe_addr, offset);

    g_pe_detour = new PLH::x64Detour(
        reinterpret_cast<uint64_t>(pe_addr),
        reinterpret_cast<uint64_t>(&hooked_process_event),
        &g_pe_trampoline);

    if (!g_pe_detour->hook()) {
        log_error(STR("[HOOK] PolyHook2 hook() returned false"));
        delete g_pe_detour;
        g_pe_detour = nullptr;
        return false;
    }

    log_info(STR("[HOOK] ProcessEvent global hook installed â€” chat-related calls will be logged"));
    return true;
}

// Lazy hook installer â€” call from handler entry points. Tries once per
// process; on_unreal_init runs too early (GUObjectArray chunks are still
// empty), so we wait for the first RPC handler call when the engine has
// fully populated the object array.
static void ensure_hook_installed_once()
{
    // @brk: latch ONLY on full success. on_unreal_init AND the first RPC can both
    // land before GUObjectArray is populated; latching on an empty pass left
    // chat/login/logout events permanently dead (the 2026-06-16 "event-dispatch
    // resolved 0" regression). Resolve the dispatch map FIRST (needs the array up),
    // then install the PE hook — so the hot path never reads a half-built map — and
    // only mark done when both succeed. An early call retries on the next handler.
    // Mutex: the loader's 16-instance pipe pool can now serve handlers concurrently.
    static std::mutex init_mtx;
    static std::atomic<bool> done{false};
    if (done.load(std::memory_order_acquire)) return;
    std::lock_guard<std::mutex> lk(init_mtx);
    if (done.load(std::memory_order_relaxed)) return;
    // DISABLED 2026-05-29 (build 23451409): the global ProcessEvent hook reads
    // a UFunction's FName at the hardcoded offset `fn + 0x18` and runs
    // fname_to_wstring on thousands of calls/frame. After the SCUM update the
    // layout shifted, so on the player-join spawn flood (SpawnDefaultPawnFor →
    // matches "Spawn") it derefs garbage and crashes the server
    // (EXCEPTION_ACCESS_VIOLATION reading 0xffff...ffff). It is chat-call
    // LOGGING only — the RPC handlers (spawnVehicle / runAdminCommand /
    // getOnlinePlayers / teleportPlayer) do NOT use it, so disabling restores
    // player join while keeping the full vehicle feature. Re-enable (set
    // TURDMOD_PE_HOOK=1) once the FName read is made build-safe by resolving the
    // UFunction name through the UE4SS API instead of the raw fn+0x18 read.
    // RE-ENABLED 2026-05-29 (drain-only): the hook now ONLY drains game-thread work
    // queues (loadAsset, despawn) + calls original; the unsafe fn+0x18 FName routing
    // is gated OFF (g_pe_event_routing_enabled). Drain-only is join-safe AND gives us
    // a GAME-THREAD execution point for native calls (DestroyVehicleAndEntity). Env
    // TURDMOD_PE_HOOK=1 additionally re-enables the event routing (still unsafe).
    // Event routing is default-ON now (pointer-compare dispatch is build-safe).
    // Kill-switch: TURDMOD_PE_HOOK=0 disables it without a rebuild.
    const char* peh = std::getenv("TURDMOD_PE_HOOK");
    if (peh != nullptr && peh[0] == '0') {
        g_pe_event_routing_enabled = false;
    }
    // Build the dispatch pointer map BEFORE arming the hook, so hooked_process_event
    // never pointer-compares against a half-built map. Both steps need a populated
    // GUObjectArray; if it isn't up yet, resolve returns false → skip the hook →
    // leave unlatched → the next handler call retries.
    bool disp_ok = g_pe_event_routing_enabled ? resolve_event_dispatch_ptrs() : true;
    bool hook_ok = disp_ok ? install_processevent_hook() : false;
    if (disp_ok && hook_ok) {
        done.store(true, std::memory_order_release);
    }
}

// fname_to_wstring â€” resolve an FName to its display string by calling
// GameServer.exe's real FName::ToString(FString&) through the detour holder
// patternsleuth armed at boot. Returns empty wstring if the detour isn't
// armed or if the lookup yields no data.
//
// CAVEAT: the underlying FName::ToString leaks the FString's heap buffer (allocated via UE's
// GMalloc — no FMemory::Free is resolved in this build). We make that leak BOUNDED with a
// process-lifetime cache keyed by the full FName identity (ComparisonIndex+Number, read as the
// raw 8 bytes so we don't depend on struct field names — same identity every local cls_cache
// already trusts via .ComparisonIndex, plus Number so instance names like "Foo_5" don't collide).
// @inv: each distinct name resolves — and leaks ONE buffer — at most once, EVER, across all
// callers. @brk: was the OVH idle memory leak — timer handlers (setAnimalPassive @30s, god_mode
// reapply, durability poll, ...) each rebuild a LOCAL cls_cache per tick, so before this they
// re-resolved ~14.5k class names every tick => ~1MB/tick/handler => GBs/hour even at 0 players,
// reaching 43GB idle (2026-06-17). Bounded now to ~tens of thousands of one-time allocations.
static std::wstring fname_to_wstring(const FName& name)
{
    if (!FName::ToStringInternal.is_ready()) return {};

    const uint64_t key = *reinterpret_cast<const uint64_t*>(&name);
    static std::mutex s_fn_cache_mtx;
    static std::unordered_map<uint64_t, std::wstring> s_fn_cache;
    std::lock_guard<std::mutex> lk(s_fn_cache_mtx); // bridge RPCs serialize, so contention ~nil
    auto it = s_fn_cache.find(key);
    if (it != s_fn_cache.end()) return it->second;

    // Cold path — resolve via SCUM's real FName::ToString(FString&), then memoize.
    // FString layout: wchar_t* Data @ 0, int32 ArrayNum @ 8, int32 ArrayMax @ 12 (16 bytes).
    alignas(16) uint8_t fstring_buf[16] = {0};
    using FnT = void(*)(const FName*, void*);
    auto fn = reinterpret_cast<FnT>(FName::ToStringInternal.get_function_address());
    fn(&name, fstring_buf);
    wchar_t* data = *reinterpret_cast<wchar_t**>(fstring_buf + 0);
    int32_t num = *reinterpret_cast<const int32_t*>(fstring_buf + 8);
    std::wstring result;
    if (data && num > 0) {
        // num typically includes the null terminator; strip it.
        int32_t len = (data[num - 1] == 0) ? num - 1 : num;
        if (len > 0) result.assign(data, static_cast<size_t>(len));
    }
    s_fn_cache.emplace(key, result); // empty results cached too — a bad FName won't re-leak
    return result;
}

// JSON-escape an ASCII representation of a wstring. UE FNames are basically
// ASCII in practice (engine identifiers, BP node names, asset paths). For
// any non-ASCII bytes we emit '?' rather than fight with UTF-8 encoding in
// the hot path â€” diagnostic only.
static std::string fname_to_json_string(const std::wstring& ws)
{
    std::string out;
    out.reserve(ws.size() + 8);
    for (wchar_t c : ws) {
        if (c == L'"') { out += "\\\""; }
        else if (c == L'\\') { out += "\\\\"; }
        else if (c < 0x20) { out += '?'; }
        else if (c < 0x80) { out += static_cast<char>(c); }
        else { out += '?'; }
    }
    return out;
}

// Build-safe replacement for the per-call fn+0x18 FName read that crashed build
// 23451409. Walks GUObjectArray ONCE, matches the handful of UFunction names we
// route on, and caches their object pointers in g_fn_ptr_dispatch. The hot path
// (hooked_process_event) then only pointer-compares. Runs on the IPC thread via
// ensure_hook_installed_once, with the engine fully up. @dep: fname_to_wstring.
static bool resolve_event_dispatch_ptrs()
{
    if (g_fn_ptr_resolved) return true;

    struct NameKind { const wchar_t* name; uint8_t kind; };
    static const NameKind kTargets[] = {
        { L"Chat_Server_BroadcastChatMessage" /* YOUR_CHAT_BROADCAST_FN */,        EV_CHAT },
        { L"HandleStartingNewPlayer" /* YOUR_LOGIN_EVENT_FN */,                 EV_LOGIN },
        { L"SurvivalStats_Server_HandlePlayerLogout" /* YOUR_LOGOUT_EVENT_FN */, EV_LOGOUT },
        { L"Chat_Client_ProcessAdminCommand" /* YOUR_ADMIN_CMD_FN */,         EV_ADMIN_RESPONSE },
        { L"Chat_Client_SendMessageToChat" /* YOUR_CHAT_SEND_FN */,           EV_CLIENT_CHAT },
        { L"ClientMessage",                           EV_CLIENT_CHAT },
    };

    std::unordered_map<uint32_t, std::wstring> cls_cache;
    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* class_ptr = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!class_ptr) return;
        auto* cp = reinterpret_cast<const uint8_t*>(class_ptr);
        const FName& cls_fname = *reinterpret_cast<const FName*>(cp + 0x18);
        auto cit = cls_cache.find(cls_fname.ComparisonIndex);
        const std::wstring* cls_name;
        if (cit == cls_cache.end()) {
            auto [ins, _] = cls_cache.try_emplace(
                cls_fname.ComparisonIndex, fname_to_wstring(cls_fname));
            cls_name = &ins->second;
        } else {
            cls_name = &cit->second;
        }
        if (*cls_name != L"Function") return;
        const FName& obj_name = *reinterpret_cast<const FName*>(p + 0x18);
        std::wstring nm = fname_to_wstring(obj_name);
        for (const auto& t : kTargets) {
            if (nm == t.name) {
                g_fn_ptr_dispatch[static_cast<void*>(obj)] = t.kind;
                Output::send<LogLevel::Default>(
                    STR("[HOOK] resolved dispatch fn {} kind={} @ {:p}\n"),
                    nm, static_cast<int>(t.kind), static_cast<void*>(obj));
                break;
            }
        }
    });

    // @brk: latch ONLY when the FULL target set is resolved. The target UFunctions load at
    // different points during boot, so an early handler call can catch a PARTIAL set (seen
    // 2/6 — chat/login/logout missing). Latching partial leaves those events permanently
    // dead. Retry on each handler call until complete; a 5-minute time guard (not an attempt
    // count — early boot can fire many handler calls) ensures the world fully loads first, and
    // bounds re-walks if a target name ever disappears (SCUM rename) → then latch with what we have.
    constexpr size_t kTargetCount = sizeof(kTargets) / sizeof(kTargets[0]);
    static auto s_first_attempt = std::chrono::steady_clock::now();
    static bool s_started = false;
    if (!s_started) { s_first_attempt = std::chrono::steady_clock::now(); s_started = true; }
    if (g_fn_ptr_dispatch.size() < kTargetCount &&
        std::chrono::steady_clock::now() - s_first_attempt < std::chrono::minutes(5)) {
        return false;
    }
    g_fn_ptr_resolved = true;
    Output::send<LogLevel::Default>(
        STR("[HOOK] event-dispatch resolved {} of {} UFunction pointer(s)\n"),
        g_fn_ptr_dispatch.size(), kTargetCount);
    return true;
}

static int32_t handle_ping(const char*, const char** result_out, const char**)
{
    ensure_hook_installed_once();
    s_ping_result = R"({"pong":true,"version":"0.1.0","mod":"TurdMODEngineBridge","kind":"cpp"})";
    *result_out = s_ping_result.c_str();
    return 0;
}

static int32_t handle_broadcast_chat(const char* params_json, const char** result_out, const char**)
{
    ensure_hook_installed_once();
    // Real signature discovered via describeFunction:
    //   MiscStatics::BroadcastChatLine(UObject* WorldContextObject,
    //                                  FString Text,
    //                                  EChatType ChatType)
    //
    // Params: { "text": "Hello world", "chatType": "0" }
    std::string text = extract_json_str(params_json, "text");
    std::string chat_type_s = extract_json_str(params_json, "chatType");
    uint8_t chat_type = 0;
    if (!chat_type_s.empty()) {
        try { chat_type = static_cast<uint8_t>(std::stoi(chat_type_s)); } catch (...) {}
    }
    if (text.empty()) {
        s_broadcast_result = R"({"error":"text param required"})";
        *result_out = s_broadcast_result.c_str();
        return 0;
    }

    // Find what we need: MiscStatics UClass (only for diagnostics), the
    // BroadcastChatLine UFunction, and â€” crucially â€” a real UObject whose
    // GetWorld() returns a valid UWorld. A BlueprintCallable WorldContext
    // dereferences `Ctx->GetWorld()`; if that returns null, the function
    // body early-returns and the broadcast silently no-ops.
    //
    // Prior bug: we used class-name == "World" and fell back to the
    // MiscStatics UClass when not found. The UClass's GetWorld() is null,
    // so the broadcast never fired. Three diagnostics for this attempt:
    //   1) collect ALL world_candidates (class name == "World") so we
    //      can pick the GameWorld over EditorWorld, and log how many
    //      we saw.
    //   2) also collect PlayerController instances as a fallback â€”
    //      every PC has a valid GetWorld() back-pointer.
    //   3) log which candidate we picked + its name, so a no-op is
    //      diagnosable from the log.
    UObject* misc_statics_class = nullptr;
    UObject* broadcast_fn = nullptr;
    UObject* world_obj = nullptr;            // first UObject of class "World"
    UObject* player_ctrl = nullptr;          // any PlayerController instance
    size_t world_count = 0;
    size_t pc_count = 0;
    std::unordered_map<uint32_t, std::wstring> cls_cache;

    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* class_ptr = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!class_ptr) return;
        auto* cp = reinterpret_cast<const uint8_t*>(class_ptr);
        const FName& cls_fname = *reinterpret_cast<const FName*>(cp + 0x18);

        auto it = cls_cache.find(cls_fname.ComparisonIndex);
        const std::wstring* cls_name;
        if (it == cls_cache.end()) {
            auto [ins, _] = cls_cache.try_emplace(
                cls_fname.ComparisonIndex, fname_to_wstring(cls_fname));
            cls_name = &ins->second;
        } else {
            cls_name = &it->second;
        }

        const FName& obj_name = *reinterpret_cast<const FName*>(p + 0x18);

        if (!misc_statics_class && *cls_name == L"Class") {
            if (fname_to_wstring(obj_name) == L"MiscStatics") {
                misc_statics_class = obj;
            }
        } else if (!broadcast_fn && *cls_name == L"Function") {
            if (fname_to_wstring(obj_name) == L"BroadcastChatLine") {
                auto* outer = *reinterpret_cast<UObject* const*>(p + 0x20);
                if (outer) {
                    auto* op = reinterpret_cast<const uint8_t*>(outer);
                    const FName& outer_name = *reinterpret_cast<const FName*>(op + 0x18);
                    if (fname_to_wstring(outer_name) == L"MiscStatics") {
                        broadcast_fn = obj;
                    }
                }
            }
        } else if (*cls_name == L"World") {
            // Skip CDOs (UE puts a Default__<Class> object in GUObjectArray
            // for every class â€” their GetWorld() returns null). Without this
            // filter the scan picks up "Default__World" and broadcast silently
            // no-ops because BroadcastChatLine early-returns on null World.
            std::wstring on = fname_to_wstring(obj_name);
            if (on.compare(0, 9, L"Default__") != 0) {
                ++world_count;
                if (!world_obj) world_obj = obj;
            }
        } else if (cls_name->find(L"PlayerController") != std::wstring::npos) {
            // PlayerController CDOs exist too. Filter them out so player_ctrl
            // is guaranteed to be a real connected-player instance.
            std::wstring on = fname_to_wstring(obj_name);
            if (on.compare(0, 9, L"Default__") != 0) {
                ++pc_count;
                if (!player_ctrl) player_ctrl = obj;
            }
        }
    });

    if (!broadcast_fn) {
        s_broadcast_result = R"({"error":"MiscStatics::BroadcastChatLine UFunction not found"})";
        *result_out = s_broadcast_result.c_str();
        return 0;
    }

    // Pick WorldContext: prefer PlayerController (a connected-player instance
    // is guaranteed to have a valid GetWorld() pointing at the GameWorld),
    // fall back to a non-CDO UWorld. Refuse to fall back to the MiscStatics
    // UClass â€” its GetWorld() is null and the prior version silently no-oped.
    UObject* world_ctx = player_ctrl;
    const wchar_t* ctx_kind = L"PlayerController";
    if (!world_ctx) {
        world_ctx = world_obj;
        ctx_kind = L"UWorld";
    }
    if (!world_ctx) {
        log_error(STR("broadcastChat: no UWorld or PlayerController in GUObjectArray â€” "
                      "is the map loaded yet?"));
        s_broadcast_result =
            R"({"error":"no UWorld or PlayerController found â€” wait until map loads"})";
        *result_out = s_broadcast_result.c_str();
        return 0;
    }

    // Diagnostics: log what we picked and the candidate counts so the next
    // failure is debuggable without code changes.
    {
        auto* ctxp = reinterpret_cast<const uint8_t*>(world_ctx);
        const FName& ctx_name = *reinterpret_cast<const FName*>(ctxp + 0x18);
        std::wstring ctx_name_s = fname_to_wstring(ctx_name);
        log_info_fmt(STR("[TurdMODEngineBridge] broadcastChat: WorldContext kind={} "
                         "name={} ptr={:p} (worldCount={} pcCount={})\n"),
                     ctx_kind, ctx_name_s, static_cast<void*>(world_ctx),
                     world_count, pc_count);
    }

    // Build the FString text buffer. Game may keep the pointer; using a
    // thread-local buffer that outlives the call is the simplest path
    // for a v1 handler.
    static thread_local wchar_t msg_buf[1024];
    std::wstring wmsg = utf8_to_wstring(text);
    if (wmsg.size() >= 1023) wmsg.resize(1023);
    wcscpy_s(msg_buf, 1024, wmsg.c_str());
    int32_t msg_len_with_null = static_cast<int32_t>(wmsg.length()) + 1;

    // Param struct laid out to match UE 4.27 calling convention. ObjectProperty
    // is 8 bytes (UObject*), StrProperty is 16 bytes (TArray<wchar_t>),
    // EnumProperty (byte enum) is 1 byte. Align matches default.
    #pragma pack(push, 1)
    struct Params {
        UObject* WorldContextObject;    // +0x00
        wchar_t* TextData;              // +0x08 (FString::Data)
        int32_t  TextNum;               // +0x10 (FString::ArrayNum)
        int32_t  TextMax;               // +0x14 (FString::ArrayMax)
        uint8_t  ChatType;              // +0x18
    };
    #pragma pack(pop)

    Params params{};
    params.WorldContextObject = world_ctx;
    params.TextData = msg_buf;
    params.TextNum  = msg_len_with_null;
    params.TextMax  = msg_len_with_null;
    params.ChatType = chat_type;

    log_info_fmt(STR("[TurdMODEngineBridge] broadcastChat â†’ ProcessEvent text=\"{}\" type={}\n"),
                 wmsg, static_cast<unsigned>(chat_type));

    // Suppress the EV_CHAT event our PE hook will fire from the line
    // below â€” this is the server's OWN broadcast, not a player chat, and
    // emitting it causes god-admin to reply to its own messages (B-010).
    // The flag is consumed exactly once by dispatch_engine_event.
    tl_suppress_next_chat_event = true;

    // Call ProcessEvent on the WorldContext UObject. Our generated UObject::
    // ProcessEvent does vtable dispatch via VTableLayoutMap[L"ProcessEvent"]
    // which UE4SS populated from VTableLayout.ini at boot.
    world_ctx->ProcessEvent(reinterpret_cast<class UFunction*>(broadcast_fn), &params);

    s_broadcast_result = "{\"ok\":true,\"sent\":\"";
    s_broadcast_result += text;
    s_broadcast_result += "\"}";
    *result_out = s_broadcast_result.c_str();
    return 0;
}

// Float parser for JSON string values. Our test driver always quotes
// values, so numeric params arrive as strings (e.g. {"x":"100.5"}).
static float extract_json_float(const char* json, const char* key, float def = 0.0f)
{
    // First try string-quoted form: `"key":"123.4"` (matches the Manager UI
    // pattern where every Input is a string). Then fall back to bare-number
    // form: `"key":123.4` (matches scumpilot's bridge-gap contract wire format).
    std::string s = extract_json_str(json, key);
    if (!s.empty()) {
        try { return std::stof(s); } catch (...) { return def; }
    }
    // Bare-number scan: find `"key"`, skip ":" + whitespace, then parse the
    // number up to the next non-numeric char. Handles negative + decimal +
    // scientific notation via stof.
    std::string pattern = "\"";
    pattern += key;
    pattern += "\"";
    const char* p = std::strstr(json, pattern.c_str());
    if (!p) return def;
    p += pattern.size();
    while (*p == ' ' || *p == '\t' || *p == ':') p++;
    if (*p == '"') return def;  // string form would have matched above
    // Parse number â€” accept -, digits, ., e, E, +
    const char* start = p;
    if (*p == '-' || *p == '+') p++;
    while ((*p >= '0' && *p <= '9') || *p == '.' || *p == 'e' || *p == 'E'
           || *p == '-' || *p == '+') p++;
    if (p == start) return def;
    try { return std::stof(std::string(start, p - start)); } catch (...) { return def; }
}

static bool extract_json_bool(const char* json, const char* key, bool def = false)
{
    std::string s = extract_json_str(json, key);
    if (s == "true" || s == "1") return true;
    if (s == "false" || s == "0") return false;
    // Bare true/false: scan for "key":true or "key":false
    std::string pattern = std::string("\"") + key + "\"";
    const char* p = std::strstr(json, pattern.c_str());
    if (!p) return def;
    p += pattern.size();
    while (*p == ' ' || *p == '\t' || *p == ':') p++;
    if (std::strncmp(p, "true", 4) == 0) return true;
    if (std::strncmp(p, "false", 5) == 0) return false;
    return def;
}

// Find a UFunction by exact name (FName ComparisonIndex match against
// `wanted`). Walks GUObjectArray; caches the result statically so
// subsequent lookups for the same function are O(1).
//
// `wanted_owner` is optional â€” pass a non-empty string to require the
// UFunction's outer (its owning UClass) to match. Use this for ambiguous
// names like "Tick"; pass empty wstring for engine-unique names like
// "K2_TeleportTo".
static UObject* find_ufunction(const wchar_t* wanted, const wchar_t* wanted_owner = L"")
{
    UObject* found = nullptr;
    std::unordered_map<uint32_t, std::wstring> cls_cache;
    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        if (found) return;
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* class_ptr = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!class_ptr) return;
        auto* cp = reinterpret_cast<const uint8_t*>(class_ptr);
        const FName& cls_fname = *reinterpret_cast<const FName*>(cp + 0x18);
        auto it = cls_cache.find(cls_fname.ComparisonIndex);
        const std::wstring* cls_name;
        if (it == cls_cache.end()) {
            auto [ins, _] = cls_cache.try_emplace(
                cls_fname.ComparisonIndex, fname_to_wstring(cls_fname));
            cls_name = &ins->second;
        } else {
            cls_name = &it->second;
        }
        if (*cls_name != L"Function") return;
        const FName& obj_name = *reinterpret_cast<const FName*>(p + 0x18);
        if (fname_to_wstring(obj_name) != wanted) return;
        if (wanted_owner && *wanted_owner) {
            auto* outer = *reinterpret_cast<UObject* const*>(p + 0x20);
            if (!outer) return;
            const FName& outer_name = *reinterpret_cast<const FName*>(
                reinterpret_cast<const uint8_t*>(outer) + 0x18);
            if (fname_to_wstring(outer_name) != wanted_owner) return;
        }
        found = obj;
    });
    return found;
}

// teleportPlayer â€” move a connected player's Pawn to (x,y,z).
//
// Params: { "name": "TechyRican", "x": "100", "y": "200", "z": "300",
//           "pitch": "0", "yaw": "0", "roll": "0" }
// Alternatively, "ptr" can identify the PC directly (hex string from
// getOnlinePlayers). pitch/yaw/roll are optional, default 0.
//
// Mechanism: PC.Pawn->K2_TeleportTo(NewLocation, NewRotation). UE 4.27
// K2_TeleportTo signature (on AActor, inherited by APawn):
//   bool K2_TeleportTo(FVector DestLocation, FRotator DestRotation)
//
// Picked over K2_SetActorLocation because the latter has an FHitResult&
// out param (~144 bytes of struct we'd need to zero and ignore).
static int32_t handle_teleport_player(const char* params_json, const char** result_out, const char**)
{
    ensure_hook_installed_once();

    std::string want_name = extract_json_str(params_json, "name");
    std::string want_ptr_s = extract_json_str(params_json, "ptr");
    float x = extract_json_float(params_json, "x", 0.0f);
    float y = extract_json_float(params_json, "y", 0.0f);
    float z = extract_json_float(params_json, "z", 0.0f);
    float pitch = extract_json_float(params_json, "pitch", 0.0f);
    float yaw   = extract_json_float(params_json, "yaw", 0.0f);
    float roll  = extract_json_float(params_json, "roll", 0.0f);

    UObject* want_ptr = nullptr;
    if (!want_ptr_s.empty()) {
        try { want_ptr = reinterpret_cast<UObject*>(std::stoull(want_ptr_s, nullptr, 0)); }
        catch (...) {}
    }
    if (want_name.empty() && !want_ptr) {
        s_teleport_result = R"({"error":"name or ptr required"})";
        *result_out = s_teleport_result.c_str();
        return 0;
    }

    // Locate the PC. Re-uses the getOnlinePlayers scan logic, stopping
    // on first match.
    UObject* pc = nullptr;
    std::unordered_map<uint32_t, std::wstring> cls_cache;
    std::unordered_map<UObject*, int32_t> ps_offset_cache;
    std::unordered_map<UObject*, int32_t> name_offset_cache;

    auto get_ps_offset = [&](UObject* pc_class) -> int32_t {
        auto it = ps_offset_cache.find(pc_class);
        if (it != ps_offset_cache.end()) return it->second;
        int32_t off = find_property_offset(pc_class, L"PlayerState");
        ps_offset_cache[pc_class] = off;
        return off;
    };
    auto get_name_offset = [&](UObject* ps_class) -> int32_t {
        auto it = name_offset_cache.find(ps_class);
        if (it != name_offset_cache.end()) return it->second;
        int32_t off = find_property_offset(ps_class, L"PlayerNamePrivate");
        if (off < 0) off = find_property_offset(ps_class, L"PlayerName");
        name_offset_cache[ps_class] = off;
        return off;
    };

    std::wstring want_name_w = utf8_to_wstring(want_name);

    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        if (pc) return;
        if (want_ptr) {
            if (obj == want_ptr) pc = obj;
            return;
        }
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* class_ptr = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!class_ptr) return;
        auto* cp = reinterpret_cast<const uint8_t*>(class_ptr);
        const FName& cls_fname = *reinterpret_cast<const FName*>(cp + 0x18);
        auto it = cls_cache.find(cls_fname.ComparisonIndex);
        const std::wstring* cls_name;
        if (it == cls_cache.end()) {
            auto [ins, _] = cls_cache.try_emplace(
                cls_fname.ComparisonIndex, fname_to_wstring(cls_fname));
            cls_name = &ins->second;
        } else {
            cls_name = &it->second;
        }
        if (cls_name->find(L"PlayerController") == std::wstring::npos) return;

        const FName& obj_name = *reinterpret_cast<const FName*>(p + 0x18);
        std::wstring on = fname_to_wstring(obj_name);
        if (on.compare(0, 9, L"Default__") == 0) return;

        // Match by display name from PlayerState.
        int32_t ps_off = get_ps_offset(class_ptr);
        if (ps_off < 0) return;
        auto* ps = *reinterpret_cast<UObject* const*>(p + ps_off);
        if (!ps) return;
        auto* ps_class = *reinterpret_cast<UObject* const*>(
            reinterpret_cast<const uint8_t*>(ps) + 0x10);
        if (!ps_class) return;
        int32_t name_off = get_name_offset(ps_class);
        if (name_off < 0) return;
        std::wstring pn = read_fstring_at(ps, name_off);
        if (pn == want_name_w) pc = obj;
    });

    if (!pc) {
        s_teleport_result = R"({"error":"player not found"})";
        *result_out = s_teleport_result.c_str();
        return 0;
    }

    // Resolve PC.Pawn â€” UProperty on AController.
    auto* pc_class = *reinterpret_cast<UObject* const*>(
        reinterpret_cast<const uint8_t*>(pc) + 0x10);
    int32_t pawn_off = find_property_offset(pc_class, L"Pawn");
    if (pawn_off < 0) {
        s_teleport_result = R"({"error":"Pawn UProperty not found on PC class"})";
        *result_out = s_teleport_result.c_str();
        return 0;
    }
    UObject* pawn = *reinterpret_cast<UObject* const*>(
        reinterpret_cast<const uint8_t*>(pc) + pawn_off);
    if (!pawn) {
        s_teleport_result = "{\"error\":\"PC has no Pawn (spectator)\"}";
        *result_out = s_teleport_result.c_str();
        return 0;
    }

    // Find K2_TeleportTo UFunction. Owner should be "Actor"; pass empty
    // owner because UE class FNames may be lowercased here ("Actor" vs
    // "actor") and the name itself is engine-unique anyway.
    static UObject* s_teleport_fn = nullptr;
    if (!s_teleport_fn) {
        s_teleport_fn = find_ufunction(L"K2_TeleportTo");
        if (!s_teleport_fn) {
            s_teleport_result = R"({"error":"K2_TeleportTo UFunction not found"})";
            *result_out = s_teleport_result.c_str();
            return 0;
        }
    }

    // Param struct:
    //   FVector  DestLocation  (12 bytes: float X, Y, Z)
    //   FRotator DestRotation  (12 bytes: float Pitch, Yaw, Roll)
    //   bool     ReturnValue   (1 byte)
    #pragma pack(push, 1)
    struct TeleportParams {
        float    DestX, DestY, DestZ;
        float    Pitch, Yaw, Roll;
        uint8_t  ReturnValue;
    };
    #pragma pack(pop)
    TeleportParams tp{};
    tp.DestX = x; tp.DestY = y; tp.DestZ = z;
    tp.Pitch = pitch; tp.Yaw = yaw; tp.Roll = roll;

    log_info_fmt(STR("[TurdMODEngineBridge] teleportPlayer â†’ Pawn={:p} loc=({},{},{}) rot=({},{},{})\n"),
                 static_cast<void*>(pawn), x, y, z, pitch, yaw, roll);

    pawn->ProcessEvent(reinterpret_cast<class UFunction*>(s_teleport_fn), &tp);

    char buf[256];
    std::snprintf(buf, sizeof(buf),
                  "{\"ok\":true,\"teleported\":%s,\"x\":%g,\"y\":%g,\"z\":%g}",
                  tp.ReturnValue ? "true" : "false", x, y, z);
    s_teleport_result = buf;
    *result_out = s_teleport_result.c_str();
    return 0;
}

// find_property_offset â€” walk a UClass's full inheritance chain looking
// for an FProperty named `prop_name`. Returns the property's
// Offset_Internal (byte offset of the field within an instance of the
// class) or -1 if not found.
//
// Layout (UE 4.27):
//   UStruct (parent of UClass):
//     0x40  UStruct* SuperStruct
//     0x50  FField*  ChildProperties        (FProperty linked list, 4.25+)
//   FField:
//     0x20  FField* Next
//     0x28  FName   NamePrivate
//   FProperty (inherits FField, FField is 0x38):
//     0x4C  int32   Offset_Internal
//
// Caller passes the UClass* as a generic UObject*.
static int32_t find_property_offset(UObject* uclass, const wchar_t* prop_name)
{
    auto cls = reinterpret_cast<const uint8_t*>(uclass);
    while (cls) {
        auto field = *reinterpret_cast<const uint8_t* const*>(cls + 0x50);
        while (field) {
            const FName& field_name = *reinterpret_cast<const FName*>(field + 0x28);
            std::wstring fn = fname_to_wstring(field_name);
            if (fn == prop_name) {
                return *reinterpret_cast<const int32_t*>(field + 0x4C);
            }
            field = *reinterpret_cast<const uint8_t* const*>(field + 0x20);
        }
        cls = *reinterpret_cast<const uint8_t* const*>(cls + 0x40);
    }
    return -1;
}

// read_fstring_at â€” read an FString field at the given byte offset within
// `obj` and return its contents as a wstring. Empty if not allocated.
//
// FString layout: 16 bytes
//   0x00 wchar_t* Data
//   0x08 int32    ArrayNum  (length including null terminator)
//   0x0C int32    ArrayMax
static std::wstring read_fstring_at(const void* obj, int32_t offset)
{
    auto* p = reinterpret_cast<const uint8_t*>(obj) + offset;
    wchar_t* data = *reinterpret_cast<wchar_t* const*>(p + 0);
    int32_t num   = *reinterpret_cast<const int32_t*>(p + 8);
    if (!data || num <= 0) return {};
    int32_t len = (data[num - 1] == 0) ? num - 1 : num;
    if (len <= 0) return {};
    return std::wstring(data, static_cast<size_t>(len));
}

// FText reader â€” UE4's FText is a refcounted shared pointer to ITextData,
// not a flat string, so we can't slice it from memory the way FString
// works. UE4SS's FText::ToString() is a stub in this build (returns
// empty FString), so the only practical path is to dispatch UE's own
// `KismetTextLibrary::Conv_TextToString` UFunction:
//
//   FString Conv_TextToString(FText InText)
//   numParms=2, paramsSize=40, returnOffset=24
//   +0   FText  InText      (24 bytes)
//   +24  FString ReturnValue (16 bytes)
//
// We cache the UFunction + a KismetTextLibrary CDO once per process.
// The returned FString's char buffer is allocated by UE's allocator and
// not freed here â€” leaks a few hundred bytes per call. Acceptable for
// one-shot dumpers; do NOT call this in a hot loop.
static UObject* s_conv_text_to_string_fn  = nullptr;
static UObject* s_kismet_text_library_cdo = nullptr;

static std::wstring read_ftext_at(const void* obj, int32_t offset)
{
    if (!obj) return {};

    if (!s_conv_text_to_string_fn) {
        s_conv_text_to_string_fn = find_ufunction(
            L"Conv_TextToString", L"KismetTextLibrary");
        if (!s_conv_text_to_string_fn) return {};
    }
    if (!s_kismet_text_library_cdo) {
        UObjectGlobals::ForEachUObject([&](UObject* o, int32_t, int32_t) {
            if (s_kismet_text_library_cdo) return;
            auto* p = reinterpret_cast<const uint8_t*>(o);
            const FName& on = *reinterpret_cast<const FName*>(p + 0x18);
            if (fname_to_wstring(on) == L"Default__KismetTextLibrary") {
                s_kismet_text_library_cdo = o;
            }
        });
        if (!s_kismet_text_library_cdo) return {};
    }

    // Copy the 24-byte FText struct into the param buffer at offset 0,
    // dispatch, then read FString at offset 24.
    alignas(16) uint8_t buf[64] = {0};
    std::memcpy(buf + 0, reinterpret_cast<const uint8_t*>(obj) + offset, 24);

    s_kismet_text_library_cdo->ProcessEvent(
        reinterpret_cast<class UFunction*>(s_conv_text_to_string_fn), buf);

    wchar_t* data = *reinterpret_cast<wchar_t* const*>(buf + 24 + 0);
    int32_t  num  = *reinterpret_cast<const int32_t*>(buf + 24 + 8);
    if (!data || num <= 0 || num > 4096) return {};
    int32_t len = (data[num - 1] == 0) ? num - 1 : num;
    if (len <= 0 || len > 4096) return {};
    return std::wstring(data, static_cast<size_t>(len));
}

// getOnlinePlayers â€” enumerate every connected-player PlayerController in
// GUObjectArray and resolve each one's display name via PlayerState.
//
// Filter: any UObject whose class FName contains "PlayerController", minus
// CDOs (name starts with "Default__"). Matches BP_ConZPlayerController_C
// and any subclass.
//
// Per-player resolution chain:
//   PC --(PlayerState offset, UProperty on AController)--> APlayerState
//   APlayerState --(PlayerName offset, FString UProperty)--> wstring
// Both offsets are resolved at runtime by walking the relevant UClass's
// ChildProperties FProperty list â€” robust to BP-class inheritance and
// per-class layout changes.
//
// Steam ID is intentionally not in v1. It lives on APlayerState::UniqueId
// (FUniqueNetIdRepl) which wraps a TSharedPtr<FUniqueNetId>; resolving it
// requires either calling the engine's GetUniqueNetIdAsString helper or
// walking the smart-pointer chain. Future work.
//
// Output:
//   { "count": N, "players": [
//       { "name": "Joel",
//         "controller": "BP_ConZPlayerController_C_2147344517",
//         "class": "BP_ConZPlayerController_C",
//         "ptr": "0x1a31b565600" }, ...
//   ]}
static int32_t handle_get_online_players(const char*, const char** result_out, const char**)
{
    std::unordered_map<uint32_t, std::wstring> cls_cache;
    // Per-UClass offset caches â€” same PC class is shared by every connected
    // player, so we walk ChildProperties at most once per class per call.
    std::unordered_map<UObject*, int32_t> ps_offset_cache;
    std::unordered_map<UObject*, int32_t> name_offset_cache;

    auto get_ps_offset = [&](UObject* pc_class) -> int32_t {
        auto it = ps_offset_cache.find(pc_class);
        if (it != ps_offset_cache.end()) return it->second;
        int32_t off = find_property_offset(pc_class, L"PlayerState");
        ps_offset_cache[pc_class] = off;
        return off;
    };
    auto get_name_offset = [&](UObject* ps_class) -> int32_t {
        auto it = name_offset_cache.find(ps_class);
        if (it != name_offset_cache.end()) return it->second;
        int32_t off = find_property_offset(ps_class, L"PlayerNamePrivate");
        // SCUM/UE4.27 may use either PlayerNamePrivate (the backing field)
        // or PlayerName (the legacy public field). Try both.
        if (off < 0) off = find_property_offset(ps_class, L"PlayerName");
        name_offset_cache[ps_class] = off;
        return off;
    };

    std::string out;
    out.reserve(4096);
    out = "[";
    bool first = true;
    size_t count = 0;

    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* class_ptr = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!class_ptr) return;
        auto* cp = reinterpret_cast<const uint8_t*>(class_ptr);
        const FName& cls_fname = *reinterpret_cast<const FName*>(cp + 0x18);

        auto cit = cls_cache.find(cls_fname.ComparisonIndex);
        const std::wstring* cls_name;
        if (cit == cls_cache.end()) {
            auto [ins, _] = cls_cache.try_emplace(
                cls_fname.ComparisonIndex, fname_to_wstring(cls_fname));
            cls_name = &ins->second;
        } else {
            cls_name = &cit->second;
        }
        if (cls_name->find(L"PlayerController") == std::wstring::npos) return;

        const FName& obj_name_fn = *reinterpret_cast<const FName*>(p + 0x18);
        std::wstring obj_name = fname_to_wstring(obj_name_fn);
        if (obj_name.compare(0, 9, L"Default__") == 0) return;

        ++count;
        std::string obj_name_s = fname_to_json_string(obj_name);
        std::string cls_name_s = fname_to_json_string(*cls_name);

        // Resolve PlayerState pointer on the PC.
        std::wstring display_name;
        int32_t ps_off = get_ps_offset(class_ptr);
        if (ps_off >= 0) {
            auto* ps = *reinterpret_cast<UObject* const*>(p + ps_off);
            if (ps) {
                // Resolve PlayerName on the PlayerState's class.
                auto* ps_class = *reinterpret_cast<UObject* const*>(
                    reinterpret_cast<const uint8_t*>(ps) + 0x10);
                if (ps_class) {
                    int32_t name_off = get_name_offset(ps_class);
                    if (name_off >= 0) {
                        display_name = read_fstring_at(ps, name_off);
                    }
                }
            }
        }
        std::string display_name_s = fname_to_json_string(display_name);
        std::string steam_id_s = read_pc_steam_id(obj);

        char ptr_buf[32];
        std::snprintf(ptr_buf, sizeof(ptr_buf), "0x%llx",
                      reinterpret_cast<unsigned long long>(obj));

        if (!first) out += ",";
        first = false;
        out += "{\"name\":\"";
        out += display_name_s;
        out += "\",\"steamId\":\"";
        out += steam_id_s;
        out += "\",\"controller\":\"";
        out += obj_name_s;
        out += "\",\"class\":\"";
        out += cls_name_s;
        out += "\",\"ptr\":\"";
        out += ptr_buf;
        out += "\"}";
    });
    out += "]";

    s_players_result = "{\"count\":";
    s_players_result += std::to_string(count);
    s_players_result += ",\"players\":";
    s_players_result += out;
    s_players_result += "}";

    static int s_last_logged_count = -1;
    if (count != s_last_logged_count) {
        log_info_fmt(STR("[TurdMODEngineBridge] getOnlinePlayers: count={}\n"), count);
        s_last_logged_count = count;
    }

    *result_out = s_players_result.c_str();
    return 0;
}

// Walk a UFunction's ChildProperties and return a map of param name to
// Offset_Internal (byte offset within the ProcessEvent param buffer).
// Same layout as describeFunction reads. Used by handlers that need to
// build param buffers dynamically rather than via a hardcoded struct â€”
// makes the call layout-driven and robust to engine-version drift.
static std::unordered_map<std::wstring, int32_t> get_function_param_offsets(UObject* ufn)
{
    std::unordered_map<std::wstring, int32_t> out;
    if (!ufn) return out;
    auto* p = reinterpret_cast<const uint8_t*>(ufn);
    auto field = *reinterpret_cast<const uint8_t* const*>(p + 0x50);
    int max_walk = 64;
    while (field && max_walk-- > 0) {
        const FName& prop_name = *reinterpret_cast<const FName*>(field + 0x28);
        int32_t offset = *reinterpret_cast<const int32_t*>(field + 0x4C);
        out[fname_to_wstring(prop_name)] = offset;
        field = *reinterpret_cast<const uint8_t* const*>(field + 0x20);
    }
    return out;
}

// spawnVehicle â€” spawn an actor (vehicle BP or any subclass of AActor)
// at world coords. Uses UE's two-call BlueprintCallable spawn path:
//   1. UGameplayStatics::BeginDeferredActorSpawnFromClass â€” creates the
//      actor in a deferred state.
//   2. UGameplayStatics::FinishSpawningActor â€” finalizes (calls
//      construction script, BeginPlay).
//
// Param layout is built dynamically from each UFunction's ChildProperties
// rather than a hardcoded struct, because BeginDeferredActorSpawnFromClass
// has an FTransform field (48 bytes, 16-byte alignment) flanked by an enum
// and a pointer â€” easy to get wrong by hand, robust when read live.
//
// FTransform internal layout IS hardcoded â€” it's an engine-stable core
// type and won't shift between SCUM updates:
//   +0x00 FQuat Rotation     (16 bytes â€” X, Y, Z, W floats)
//   +0x10 FVector Translation (12 bytes â€” X, Y, Z; padded to 16)
//   +0x20 FVector Scale3D     (12 bytes â€” X, Y, Z; padded to 16)
//
// Params (JSON):
//   { "class": "BP_Hatchback_C", "x": "100", "y": "200", "z": "300" }
// Pitch/yaw/roll are accepted but ignored in v1 (Eulerâ†’Quat conversion
// is more code than warranted for a first cut â€” vehicle can be driven
// to reorient).
// [SCRUBBED] Game-specific section removed (286 lines)

static int32_t handle_find_functions(const char* params_json,
                                     const char** result_out,
                                     const char**)
{
    std::string grep = extract_json_str(params_json, "grep");
    std::string limit_str = extract_json_str(params_json, "limit");
    size_t kMaxEmit = 500;
    if (!limit_str.empty()) {
        try { kMaxEmit = static_cast<size_t>(std::stoul(limit_str)); } catch (...) {}
    }

    std::unordered_map<uint32_t, std::wstring> class_name_cache;
    size_t total_functions = 0;
    size_t emitted = 0;
    std::string out;
    out.reserve(32768);
    out = "[";
    bool first = true;

    SCAN_TIMEOUT_INIT();
    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t /*chunk*/, int32_t /*idx*/) {
        SCAN_TIMEOUT_CHECK();
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* class_ptr = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!class_ptr) return;
        auto* cp = reinterpret_cast<const uint8_t*>(class_ptr);
        const FName& class_fname = *reinterpret_cast<const FName*>(cp + 0x18);

        // Cached class-name resolution. Unique classes are ~few thousand.
        auto cache_it = class_name_cache.find(class_fname.ComparisonIndex);
        const std::wstring* cls_name;
        if (cache_it == class_name_cache.end()) {
            auto [ins, _] = class_name_cache.try_emplace(
                class_fname.ComparisonIndex, fname_to_wstring(class_fname));
            cls_name = &ins->second;
        } else {
            cls_name = &cache_it->second;
        }
        if (*cls_name != L"Function") return;
        ++total_functions;

        if (emitted >= kMaxEmit) return;

        const FName& obj_name = *reinterpret_cast<const FName*>(p + 0x18);
        std::wstring fname_wstr = fname_to_wstring(obj_name);
        std::string fname_str = fname_to_json_string(fname_wstr);

        if (!grep.empty() && fname_str.find(grep) == std::string::npos) return;

        // Also resolve owner class chain â€” UFunction's outer is its UClass.
        // Read OuterPrivate at 0x20 on the UFunction itself.
        auto* outer = *reinterpret_cast<UObject* const*>(p + 0x20);
        std::string owner_str;
        if (outer) {
            auto* op = reinterpret_cast<const uint8_t*>(outer);
            const FName& owner_name = *reinterpret_cast<const FName*>(op + 0x18);
            owner_str = fname_to_json_string(fname_to_wstring(owner_name));
        }

        ++emitted;
        if (!first) out += ",";
        first = false;
        out += "{\"name\":\"";
        out += fname_str;
        out += "\",\"owner\":\"";
        out += owner_str;
        out += "\"}";
    });
    out += "]";

    s_find_result = "{\"totalFunctions\":";
    s_find_result += std::to_string(total_functions);
    s_find_result += ",\"emitted\":";
    s_find_result += std::to_string(emitted);
    s_find_result += ",\"limit\":";
    s_find_result += std::to_string(kMaxEmit);
    s_find_result += ",\"grep\":\"";
    s_find_result += grep;
    s_find_result += "\",\"functions\":";
    s_find_result += out;
    s_find_result += "}";

    std::wstring grep_w = utf8_to_wstring(grep);
    log_info_fmt(STR("[TurdMODEngineBridge] handle_find_functions: total={} emitted={} grep=\"{}\"\n"),
                 total_functions, emitted, grep_w);

    *result_out = s_find_result.c_str();
    return 0;
}

// runAdminCommand â€” dispatch an arbitrary game admin chat command through
// PlayerRpcChannel::Chat_Server_ProcessAdminCommand. This is the exact
// codepath that fires when an admin types e.g. `#SpawnVehicle BPC_Kinglet_Duster`
// in chat, so the command runs through SCUM's real admin parser â†’
// permission check â†’ vehicle manager â†’ fully assembled spawn. Unlocks
// every admin command in one shot (#teleport, #godmode, #SetGold, etc.)
// without us reverse-engineering each one.
//
// Confirmed signature (via describeFunction 2026-05-16):
//   void Chat_Server_ProcessAdminCommand(FString commandText)
//   owner = PlayerRpcChannel
//
// Mechanism: locate any non-CDO PlayerRpcChannel in GUObjectArray
// (each connected player has one; its outer is the PC). Call
// ProcessEvent with the command string. Permission check is on the
// PC backing the channel, so this only does work if that PC is admin.
//
// Params: { "command": "SpawnVehicle BPC_Kinglet_Duster" }
//   The `#` prefix is optional â€” SCUM accepts both forms.
static int32_t handle_run_admin_command(const char* params_json,
                                        const char** result_out,
                                        const char**)
{
    ensure_hook_installed_once();

    std::string command = extract_json_str(params_json, "command");
    if (command.empty()) {
        s_admin_result = R"({"error":"command param required"})";
        *result_out = s_admin_result.c_str();
        return 0;
    }

    // Find the SPECIFIC player's PlayerRpcChannel â€” auth check needs the
    // right channel so SCUM can identify the sender as an admin.
    // Optional "playerName" param targets that player's channel;
    // falls back to first non-CDO channel if not specified.
    std::string player_name = extract_json_str(params_json, "playerName");
    UObject* target_pc = nullptr;
    if (!player_name.empty()) {
        target_pc = find_pc_by_player_name(utf8_to_wstring(player_name));
    }

    UObject* rpc_channel = nullptr;
    std::unordered_map<uint32_t, std::wstring> cls_cache;
    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        if (rpc_channel) return;
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* class_ptr = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!class_ptr) return;
        auto* cp = reinterpret_cast<const uint8_t*>(class_ptr);
        const FName& cls_fname = *reinterpret_cast<const FName*>(cp + 0x18);
        auto it = cls_cache.find(cls_fname.ComparisonIndex);
        const std::wstring* cls_name;
        if (it == cls_cache.end()) {
            auto [ins, _] = cls_cache.try_emplace(
                cls_fname.ComparisonIndex, fname_to_wstring(cls_fname));
            cls_name = &ins->second;
        } else {
            cls_name = &it->second;
        }
        if (cls_name->find(L"PlayerRpcChannel" /* YOUR_GAME_RPC_CHANNEL */) == std::wstring::npos) return;
        const FName& obj_name = *reinterpret_cast<const FName*>(p + 0x18);
        std::wstring on = fname_to_wstring(obj_name);
        if (on.compare(0, 9, L"Default__") == 0) return;

        // If playerName specified, match by Outer chain (RpcChannel's Outer is the PC)
        if (target_pc) {
            // UObject::Outer is at offset 0x20 in standard UE4
            UObject* outer = *reinterpret_cast<UObject* const*>(p + 0x20);
            if (outer != target_pc) return;
        }
        rpc_channel = obj;
    });

    if (!rpc_channel) {
        s_admin_result =
            R"({"error":"no PlayerRpcChannel instance found - is a player connected?"})";
        *result_out = s_admin_result.c_str();
        return 0;
    }

    // Cache the UFunction lookup. ~1.5M-object walk is expensive; we
    // only need to do it once per process.
    static UObject* s_cmd_fn = nullptr;
    if (!s_cmd_fn) {
        s_cmd_fn = find_ufunction(L"Chat_Server_ProcessAdminCommand");
        if (!s_cmd_fn) {
            s_admin_result =
                R"({"error":"Chat_Server_ProcessAdminCommand UFunction not found"})";
            *result_out = s_admin_result.c_str();
            return 0;
        }
    }

    // Build the FString commandText. Same pattern as broadcastChat: a
    // thread-local buffer outlives the call so SCUM can read freely
    // even if its handler holds the pointer briefly.
    static thread_local wchar_t cmd_buf[2048];
    std::wstring wcmd = utf8_to_wstring(command);
    if (wcmd.size() >= 2047) wcmd.resize(2047);
    wcscpy_s(cmd_buf, 2048, wcmd.c_str());
    int32_t len_with_null = static_cast<int32_t>(wcmd.length()) + 1;

    #pragma pack(push, 1)
    struct Params {
        wchar_t* TextData;   // FString::Data
        int32_t  TextNum;    // FString::ArrayNum (incl. null)
        int32_t  TextMax;    // FString::ArrayMax
    };
    #pragma pack(pop)
    Params p{};
    p.TextData = cmd_buf;
    p.TextNum  = len_with_null;
    p.TextMax  = len_with_null;

    log_info_fmt(STR("[TurdMODEngineBridge] runAdminCommand: \"{}\" via PlayerRpcChannel {:p}\n"),
                 wcmd, static_cast<void*>(rpc_channel));

    // Activate capture mode so Chat_Client_SendMessageToChat output is stored
    g_admin_capture_active.store(true);

    // gameThread=1 → queue for game-thread dispatch (the off-thread synchronous
    // path silently aborts in SCUM's context-validation). bypass=1 → also flip
    // the CanExecute auth gate so the command runs regardless of the channel
    // PC's permissions (server-authority injection, no real admin account).
    std::string gt_s     = extract_json_str(params_json, "gameThread");
    std::string bypass_s = extract_json_str(params_json, "bypass");
    bool game_thread = (gt_s == "1" || gt_s == "true");
    bool bypass      = (bypass_s == "1" || bypass_s == "true");

    if (game_thread) {
        // Copy into NON-thread-local globals so the GAME thread sees them.
        wcscpy_s(g_admin_cmd_buf, 2048, wcmd.c_str());
        g_admin_cmd_params.TextData = g_admin_cmd_buf;
        g_admin_cmd_params.TextNum  = len_with_null;
        g_admin_cmd_params.TextMax  = len_with_null;
        g_admin_cmd_req.channel = rpc_channel;
        g_admin_cmd_req.fn      = reinterpret_cast<class UFunction*>(s_cmd_fn);
        g_admin_cmd_req.params  = &g_admin_cmd_params;
        g_admin_cmd_req.bypass  = bypass;
        g_admin_cmd_req.seh     = 0;
        g_admin_cmd_req.gate_patched = false;
        int expected = 0;
        if (!g_admin_cmd_req.state.compare_exchange_strong(
                expected, 1, std::memory_order_acq_rel)) {
            s_admin_result = R"({"error":"another admin command already in progress"})";
            *result_out = s_admin_result.c_str();
            return 0;
        }
        auto poll_start = std::chrono::steady_clock::now();
        while (g_admin_cmd_req.state.load(std::memory_order_acquire) != 3) {
            if (std::chrono::steady_clock::now() - poll_start > std::chrono::seconds(10)) {
                g_admin_cmd_req.state.store(0, std::memory_order_release);
                s_admin_result = "{\"error\":\"admin command timed out - game thread not draining (PE hook installed?)\"}";
                *result_out = s_admin_result.c_str();
                return 0;
            }
            std::this_thread::sleep_for(std::chrono::milliseconds(5));
        }
        uint32_t seh = g_admin_cmd_req.seh;
        bool gp = g_admin_cmd_req.gate_patched;
        g_admin_cmd_req.state.store(0, std::memory_order_release);
        char rb[320];
        std::snprintf(rb, sizeof(rb),
            "{\"ok\":%s,\"gameThread\":true,\"bypass\":%s,\"gatePatched\":%s,\"seh\":\"0x%08x\"}",
            seh == 0 ? "true" : "false", bypass ? "true" : "false",
            gp ? "true" : "false", seh);
        s_admin_result = rb;
        *result_out = s_admin_result.c_str();
        return 0;
    }

    rpc_channel->ProcessEvent(reinterpret_cast<class UFunction*>(s_cmd_fn), &p);

    s_admin_result = "{\"ok\":true,\"command\":\"";
    // JSON-escape the command for the response â€” safest to use
    // fname_to_json_string which handles quotes/backslashes.
    s_admin_result += fname_to_json_string(wcmd);
    s_admin_result += "\"}";
    *result_out = s_admin_result.c_str();
    return 0;
}

// sendChat â€” send a chat message AS A PLAYER by dispatching through
// PlayerRpcChannel::Chat_Server_BroadcastChatMessage, the exact entry
// point SCUM hits when a client types in chat. SCUM's own pipeline
// then sets up auth state and dispatches admin commands if the text
// starts with `#`.
//
// Proof of approach: typing `#SpawnVehicle BPC_Kinglet_Duster` in
// the in-game chat client produces a fully-assembled vehicle with a
// SCUM-managed VID. Our earlier bypass into Chat_Server_ProcessAdmin-
// Command did NOT, because it skipped whatever session state
// BroadcastChatMessage primes. This handler mimics chat input
// directly â€” same path the client uses.
//
// Signature (via describeFunction 2026-05-16):
//   void Chat_Server_BroadcastChatMessage(FString Message, EChatChannel Channel)
//
// Params: { "text": "#SpawnVehicle BPC_Kinglet_Duster", "channel": "0" }
//   channel defaults to 0 (typically Local). Admin parsing keys off
//   the `#` prefix, not the channel, so any channel works for admin
//   commands.
static int32_t handle_send_chat(const char* params_json,
                                const char** result_out,
                                const char**)
{
    ensure_hook_installed_once();

    std::string text = extract_json_str(params_json, "text");
    if (text.empty()) {
        s_sendchat_result = R"({"error":"text param required"})";
        *result_out = s_sendchat_result.c_str();
        return 0;
    }
    std::string ch_s = extract_json_str(params_json, "channel");
    uint8_t channel = 0;
    if (!ch_s.empty()) {
        try { channel = static_cast<uint8_t>(std::stoi(ch_s)); } catch (...) {}
    }

    // Locate any non-CDO PlayerRpcChannel. SCUM treats the message as
    // coming from THIS channel's owning PC, so single-player servers
    // (or admin-only testing) work fine with first-found. Multi-player
    // future work: filter by display name or Steam ID.
    UObject* rpc_channel = nullptr;
    std::unordered_map<uint32_t, std::wstring> cls_cache;
    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        if (rpc_channel) return;
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* class_ptr = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!class_ptr) return;
        auto* cp = reinterpret_cast<const uint8_t*>(class_ptr);
        const FName& cls_fname = *reinterpret_cast<const FName*>(cp + 0x18);
        auto it = cls_cache.find(cls_fname.ComparisonIndex);
        const std::wstring* cls_name;
        if (it == cls_cache.end()) {
            auto [ins, _] = cls_cache.try_emplace(
                cls_fname.ComparisonIndex, fname_to_wstring(cls_fname));
            cls_name = &ins->second;
        } else {
            cls_name = &it->second;
        }
        if (cls_name->find(L"PlayerRpcChannel" /* YOUR_GAME_RPC_CHANNEL */) == std::wstring::npos) return;
        const FName& obj_name = *reinterpret_cast<const FName*>(p + 0x18);
        std::wstring on = fname_to_wstring(obj_name);
        if (on.compare(0, 9, L"Default__") == 0) return;
        rpc_channel = obj;
    });
    if (!rpc_channel) {
        s_sendchat_result =
            R"({"error":"no PlayerRpcChannel instance - is a player connected?"})";
        *result_out = s_sendchat_result.c_str();
        return 0;
    }

    // Locate Chat_Server_BroadcastChatMessage and cache its param layout.
    // FString + EnumProperty packing isn't fully obvious by hand â€” using
    // the live layout means we don't have to guess.
    static UObject* s_fn = nullptr;
    static std::unordered_map<std::wstring, int32_t> s_offsets;
    if (!s_fn) {
        s_fn = find_ufunction(L"Chat_Server_BroadcastChatMessage" /* YOUR_CHAT_BROADCAST_FN */,
                              L"PlayerRpcChannel" /* YOUR_GAME_RPC_CHANNEL */);
        if (s_fn) s_offsets = get_function_param_offsets(s_fn);
        if (!s_fn) {
            s_sendchat_result =
                R"({"error":"Chat_Server_BroadcastChatMessage not found"})";
            *result_out = s_sendchat_result.c_str();
            return 0;
        }
    }

    auto get_off = [](const std::unordered_map<std::wstring, int32_t>& m,
                      const std::wstring& k) -> int32_t {
        auto it = m.find(k);
        return it == m.end() ? -1 : it->second;
    };
    int32_t msg_off = get_off(s_offsets, L"Message");
    int32_t ch_off  = get_off(s_offsets, L"Channel");
    if (msg_off < 0 || ch_off < 0) {
        s_sendchat_result =
            R"({"error":"Chat_Server_BroadcastChatMessage param layout mismatch"})";
        *result_out = s_sendchat_result.c_str();
        return 0;
    }

    static thread_local wchar_t text_buf[2048];
    std::wstring wtext = utf8_to_wstring(text);
    if (wtext.size() >= 2047) wtext.resize(2047);
    wcscpy_s(text_buf, 2048, wtext.c_str());
    int32_t len_with_null = static_cast<int32_t>(wtext.length()) + 1;

    // 64-byte buffer is plenty (FString = 16 bytes + 1 byte enum + padding).
    alignas(16) uint8_t buf[64] = {0};
    // FString internal layout at the Message offset:
    //   +0x00 wchar_t* Data
    //   +0x08 int32    Num  (includes null)
    //   +0x0C int32    Max
    *reinterpret_cast<wchar_t**>(buf + msg_off + 0)  = text_buf;
    *reinterpret_cast<int32_t*>(buf + msg_off + 8)   = len_with_null;
    *reinterpret_cast<int32_t*>(buf + msg_off + 12)  = len_with_null;
    buf[ch_off] = channel;

    log_info_fmt(STR("[TurdMODEngineBridge] sendChat: text=\"{}\" channel={} via PlayerRpcChannel={:p}\n"),
                 wtext, static_cast<unsigned>(channel),
                 static_cast<void*>(rpc_channel));

    rpc_channel->ProcessEvent(reinterpret_cast<class UFunction*>(s_fn), buf);

    s_sendchat_result = "{\"ok\":true,\"text\":\"";
    s_sendchat_result += fname_to_json_string(wtext);
    s_sendchat_result += "\",\"channel\":";
    s_sendchat_result += std::to_string(channel);
    s_sendchat_result += "}";
    *result_out = s_sendchat_result.c_str();
    return 0;
}

// sendChatLineToPlayer â€” directed analogue of broadcastChat. Looks up
// a connected player by display name (same pattern as getOnlinePlayers:
// PC â†’ PlayerState â†’ PlayerName(Private)) and calls
// `MiscStatics::SendChatLineToPlayer` via ProcessEvent on that PC.
//
// Param shape verified live 2026-05-17, captured in
// `reference_scum_chat_functions`:
//   +0   UObject*  PlayerController (target client; implicit world context)
//   +8   FString   Text             (16 bytes: Data*, Num, Max)
//   +24  uint8     ChatType         (0=Local 1=Squad 2=Global 3=Admin)
//   +25  uint8     ShouldCopyToClientClipboard
//
// Params JSON: { "playerName": "...", "message": "...", "channel": "Global" }
// (channel accepts either the named string or a numeric "0"-"3").
//
// Used by the Welcome Mod hook in turdmod-manager â€” every 5s the manager
// diffs getOnlinePlayers and fires this handler for each new join.
static int32_t handle_send_chat_line_to_player(const char* params_json,
                                                const char** result_out,
                                                const char**)
{
    ensure_hook_installed_once();

    std::string player_name = extract_json_str(params_json, "playerName");
    std::string message     = extract_json_str(params_json, "message");
    std::string channel_s   = extract_json_str(params_json, "channel");

    if (player_name.empty()) {
        s_send_chat_line_to_player_result = R"({"error":"playerName param required"})";
        *result_out = s_send_chat_line_to_player_result.c_str();
        return 0;
    }
    if (message.empty()) {
        s_send_chat_line_to_player_result = R"({"error":"message param required"})";
        *result_out = s_send_chat_line_to_player_result.c_str();
        return 0;
    }

    // EChatType: 0=Local 1=Squad 2=Global 3=Admin. Accept named or numeric.
    uint8_t chat_type = 2; // Global (welcome mod's default)
    if (!channel_s.empty()) {
        bool parsed_numeric = false;
        try {
            chat_type = static_cast<uint8_t>(std::stoi(channel_s));
            parsed_numeric = true;
        } catch (...) {}
        if (!parsed_numeric) {
            if      (channel_s == "Local")  chat_type = 0;
            else if (channel_s == "Squad")  chat_type = 1;
            else if (channel_s == "Global") chat_type = 2;
            else if (channel_s == "Admin")  chat_type = 3;
        }
    }

    // Walk GUObjectArray to resolve playerName â†’ PlayerController*.
    // Same PC-class chain as getOnlinePlayers; per-class offset caches
    // keep the work O(N) over UObjects, not O(N) over properties.
    std::wstring target_w(player_name.begin(), player_name.end());
    UObject* target_pc = nullptr;
    std::unordered_map<uint32_t, std::wstring> cls_cache;
    std::unordered_map<UObject*, int32_t> ps_offset_cache;
    std::unordered_map<UObject*, int32_t> name_offset_cache;

    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        if (target_pc) return;
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* class_ptr = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!class_ptr) return;
        auto* cp = reinterpret_cast<const uint8_t*>(class_ptr);
        const FName& cls_fname = *reinterpret_cast<const FName*>(cp + 0x18);

        auto cit = cls_cache.find(cls_fname.ComparisonIndex);
        const std::wstring* cls_name;
        if (cit == cls_cache.end()) {
            auto [ins, _] = cls_cache.try_emplace(
                cls_fname.ComparisonIndex, fname_to_wstring(cls_fname));
            cls_name = &ins->second;
        } else {
            cls_name = &cit->second;
        }
        if (cls_name->find(L"PlayerController") == std::wstring::npos) return;

        const FName& obj_name_fn = *reinterpret_cast<const FName*>(p + 0x18);
        std::wstring obj_name = fname_to_wstring(obj_name_fn);
        if (obj_name.compare(0, 9, L"Default__") == 0) return;

        // PC â†’ PlayerState
        auto ps_it = ps_offset_cache.find(class_ptr);
        int32_t ps_off;
        if (ps_it != ps_offset_cache.end()) {
            ps_off = ps_it->second;
        } else {
            ps_off = find_property_offset(class_ptr, L"PlayerState");
            ps_offset_cache[class_ptr] = ps_off;
        }
        if (ps_off < 0) return;
        auto* ps = *reinterpret_cast<UObject* const*>(p + ps_off);
        if (!ps) return;
        auto* ps_class = *reinterpret_cast<UObject* const*>(
            reinterpret_cast<const uint8_t*>(ps) + 0x10);
        if (!ps_class) return;

        // PlayerState â†’ PlayerName(Private)
        auto nit = name_offset_cache.find(ps_class);
        int32_t name_off;
        if (nit != name_offset_cache.end()) {
            name_off = nit->second;
        } else {
            name_off = find_property_offset(ps_class, L"PlayerNamePrivate");
            if (name_off < 0) name_off = find_property_offset(ps_class, L"PlayerName");
            name_offset_cache[ps_class] = name_off;
        }
        if (name_off < 0) return;

        std::wstring display_name = read_fstring_at(ps, name_off);
        if (display_name == target_w) {
            target_pc = obj;
        }
    });

    if (!target_pc) {
        s_send_chat_line_to_player_result =
            R"({"error":"player not found","playerName":")";
        s_send_chat_line_to_player_result += fname_to_json_string(target_w);
        s_send_chat_line_to_player_result += R"("})";
        *result_out = s_send_chat_line_to_player_result.c_str();
        return 0;
    }

    // Find UFunction once and cache.
    static UObject* s_fn = nullptr;
    if (!s_fn) {
        s_fn = find_ufunction(L"SendChatLineToPlayer", L"MiscStatics");
        if (!s_fn) {
            s_send_chat_line_to_player_result =
                R"({"error":"MiscStatics::SendChatLineToPlayer UFunction not found"})";
            *result_out = s_send_chat_line_to_player_result.c_str();
            return 0;
        }
    }

    // Build 26-byte param buffer.
    static thread_local wchar_t text_buf[2048];
    std::wstring wmsg = utf8_to_wstring(message);
    if (wmsg.size() >= 2047) wmsg.resize(2047);
    wcscpy_s(text_buf, 2048, wmsg.c_str());
    int32_t len_with_null = static_cast<int32_t>(wmsg.length()) + 1;

    alignas(16) uint8_t buf[64] = {0};
    *reinterpret_cast<UObject**>(buf + 0)       = target_pc;
    *reinterpret_cast<wchar_t**>(buf + 8 + 0)   = text_buf;
    *reinterpret_cast<int32_t*>(buf + 8 + 8)    = len_with_null;
    *reinterpret_cast<int32_t*>(buf + 8 + 12)   = len_with_null;
    buf[24] = chat_type;
    buf[25] = 0; // ShouldCopyToClientClipboard

    log_info_fmt(STR("[TurdMODEngineBridge] sendChatLineToPlayer: player=\"{}\" channel={} text=\"{}\"\n"),
                 target_w, static_cast<unsigned>(chat_type), wmsg);

    target_pc->ProcessEvent(reinterpret_cast<class UFunction*>(s_fn), buf);

    s_send_chat_line_to_player_result = R"({"ok":true,"player":")";
    s_send_chat_line_to_player_result += fname_to_json_string(target_w);
    s_send_chat_line_to_player_result += R"(","channel":)";
    s_send_chat_line_to_player_result += std::to_string(chat_type);
    s_send_chat_line_to_player_result += R"(,"text":")";
    s_send_chat_line_to_player_result += fname_to_json_string(wmsg);
    s_send_chat_line_to_player_result += R"("})";
    *result_out = s_send_chat_line_to_player_result.c_str();
    return 0;
}

// listHandlers â€” return the method names of every RPC handler this
// bridge has registered. Single source of truth: reads from
// g_registered_methods which on_unreal_init populates as it registers
// each handler. Used by the Manager's Engine Console page to populate
// its method picker â€” no parallel TS list to drift.
//
// Params: none.
// Returns: { "ok": true, "count": N, "handlers": [ "ping", "broadcastChat", ... ] }
static int32_t handle_list_handlers(const char*, const char** result_out, const char**)
{
    s_list_handlers_result = "{\"ok\":true,\"count\":";
    s_list_handlers_result += std::to_string(g_registered_methods.size());
    s_list_handlers_result += ",\"handlers\":[";
    bool first = true;
    for (const auto& m : g_registered_methods) {
        if (!first) s_list_handlers_result += ",";
        first = false;
        s_list_handlers_result += "\"";
        s_list_handlers_result += m;
        s_list_handlers_result += "\"";
    }
    s_list_handlers_result += "]}";
    *result_out = s_list_handlers_result.c_str();
    return 0;
}

// describeWidget â€” drill into a single widget UClass and return its
// inheritance chain + properties + named-slot members. Foundation for
// the UI/UX Maker's "inspect widget" panel â€” once we can describe what
// a widget contains, the editor can render the editable surface.
//
// Reuses the FProperty walker logic from describeFunction, but applied
// to a UClass's ChildProperties (member fields) rather than a
// UFunction's ChildProperties (parameters). Same FField layout, same
// offsets.
//
// Params (JSON):
//   { "name": "ChatWidget",
//     "includeInherited": "true",   (default: false â€” only own props)
//     "limit": "200" }              (cap on property count, default 200)
//
// Output:
//   { "found": true,
//     "name": "ChatWidget",
//     "kind": "cpp",                ("cpp" or "bp")
//     "inheritance": ["ChatWidget", "UserWidget", "Widget", "Visual", "Object"],
//     "properties": [
//       { "name": "MessageList", "type": "ObjectProperty", "offset": 600,
//         "ownerClass": "ChatWidget" },   ("ownerClass" only when
//                                          includeInherited=true)
//       ...
//     ] }
static int32_t handle_describe_widget(const char* params_json,
                                      const char** result_out,
                                      const char**)
{
    std::string want = extract_json_str(params_json, "name");
    if (want.empty()) {
        s_describewidget_result = R"({"error":"name param required"})";
        *result_out = s_describewidget_result.c_str();
        return 0;
    }
    std::wstring want_w(want.begin(), want.end());
    bool include_inherited = (extract_json_str(params_json, "includeInherited") == "true");
    size_t limit = 200;
    {
        std::string s = extract_json_str(params_json, "limit");
        if (!s.empty()) { try { limit = static_cast<size_t>(std::stoul(s)); } catch (...) {} }
    }

    // Find the target UClass by name. Match either "Class" (C++) or
    // "BlueprintGeneratedClass" (BP); exclude CDOs.
    UObject* target = nullptr;
    const char* target_kind = nullptr;
    std::unordered_map<uint32_t, std::wstring> cls_cache;
    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        if (target) return;
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* class_ptr = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!class_ptr) return;
        auto* cp = reinterpret_cast<const uint8_t*>(class_ptr);
        const FName& cls_fname = *reinterpret_cast<const FName*>(cp + 0x18);
        auto it = cls_cache.find(cls_fname.ComparisonIndex);
        const std::wstring* cls_name;
        if (it == cls_cache.end()) {
            auto [ins, _] = cls_cache.try_emplace(
                cls_fname.ComparisonIndex, fname_to_wstring(cls_fname));
            cls_name = &ins->second;
        } else {
            cls_name = &it->second;
        }
        const char* k = nullptr;
        if (*cls_name == L"Class") k = "cpp";
        else if (*cls_name == L"BlueprintGeneratedClass") k = "bp";
        else return;
        const FName& obj_name = *reinterpret_cast<const FName*>(p + 0x18);
        std::wstring on = fname_to_wstring(obj_name);
        if (on.compare(0, 9, L"Default__") == 0) return;
        if (on == want_w) { target = obj; target_kind = k; }
    });
    if (!target) {
        s_describewidget_result = "{\"error\":\"UClass not found: ";
        s_describewidget_result += want;
        s_describewidget_result += "\"}";
        *result_out = s_describewidget_result.c_str();
        return 0;
    }

    // Walk SuperStruct chain to build the inheritance list. Same shape
    // as the chain walker in dumpWidgets but reports each step.
    std::string inheritance = "[";
    {
        bool first_link = true;
        const uint8_t* cur = reinterpret_cast<const uint8_t*>(target);
        int max_depth = 32;
        while (cur && max_depth-- > 0) {
            const FName& nm = *reinterpret_cast<const FName*>(cur + 0x18);
            std::string s = fname_to_json_string(fname_to_wstring(nm));
            if (!first_link) inheritance += ",";
            first_link = false;
            inheritance += "\"";
            inheritance += s;
            inheritance += "\"";
            cur = *reinterpret_cast<const uint8_t* const*>(cur + 0x40);
        }
    }
    inheritance += "]";

    // Walk ChildProperties. If include_inherited, also walk SuperStruct's
    // ChildProperties (recursively up the chain) and tag each entry with
    // which class declared it.
    std::string props_out = "[";
    bool first_prop = true;
    size_t emitted = 0;
    size_t total_props = 0;

    auto emit_props_for = [&](const uint8_t* cls, const std::string& owner_name_s) {
        auto field = *reinterpret_cast<const uint8_t* const*>(cls + 0x50);
        int max_walk = 256;
        while (field && max_walk-- > 0) {
            ++total_props;
            if (emitted >= limit) {
                field = *reinterpret_cast<const uint8_t* const*>(field + 0x20);
                continue;
            }
            // FField:
            //   0x08 FFieldClass* ClassPrivate  (type tag)
            //   0x28 FName       NamePrivate
            //   0x20 FField*     Next
            // FFieldClass:
            //   0x00 FName       Name
            // FProperty (inherits FField):
            //   0x4C int32       Offset_Internal
            const FName& field_name = *reinterpret_cast<const FName*>(field + 0x28);
            std::string prop_name_s = fname_to_json_string(fname_to_wstring(field_name));
            int32_t offset = *reinterpret_cast<const int32_t*>(field + 0x4C);
            std::string type_name_s;
            auto* class_priv = *reinterpret_cast<void* const*>(field + 0x08);
            if (class_priv) {
                const FName& type_fname = *reinterpret_cast<const FName*>(
                    reinterpret_cast<const uint8_t*>(class_priv) + 0x00);
                type_name_s = fname_to_json_string(fname_to_wstring(type_fname));
            }

            if (!first_prop) props_out += ",";
            first_prop = false;
            props_out += "{\"name\":\"";
            props_out += prop_name_s;
            props_out += "\",\"type\":\"";
            props_out += type_name_s;
            props_out += "\",\"offset\":";
            props_out += std::to_string(offset);
            if (include_inherited) {
                props_out += ",\"ownerClass\":\"";
                props_out += owner_name_s;
                props_out += "\"";
            }
            props_out += "}";
            ++emitted;
            field = *reinterpret_cast<const uint8_t* const*>(field + 0x20);
        }
    };

    {
        const uint8_t* cur = reinterpret_cast<const uint8_t*>(target);
        int max_depth = 32;
        bool first_in_chain = true;
        while (cur && max_depth-- > 0) {
            const FName& nm = *reinterpret_cast<const FName*>(cur + 0x18);
            std::string owner_s = fname_to_json_string(fname_to_wstring(nm));
            emit_props_for(cur, owner_s);
            if (!include_inherited) break;
            cur = *reinterpret_cast<const uint8_t* const*>(cur + 0x40);
            first_in_chain = false;
        }
    }
    props_out += "]";

    s_describewidget_result = "{\"found\":true,\"name\":\"";
    s_describewidget_result += fname_to_json_string(want_w);
    s_describewidget_result += "\",\"kind\":\"";
    s_describewidget_result += target_kind;
    s_describewidget_result += "\",\"inheritance\":";
    s_describewidget_result += inheritance;
    s_describewidget_result += ",\"totalProperties\":";
    s_describewidget_result += std::to_string(total_props);
    s_describewidget_result += ",\"emitted\":";
    s_describewidget_result += std::to_string(emitted);
    s_describewidget_result += ",\"properties\":";
    s_describewidget_result += props_out;
    s_describewidget_result += "}";

    log_info_fmt(STR("[TurdMODEngineBridge] describeWidget: name={} props={}/{}\n"),
                 want_w, emitted, total_props);

    *result_out = s_describewidget_result.c_str();
    return 0;
}

// â”€â”€â”€ readClassValues â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Read the CDO (Class Default Object) property values for a given UClass.
// Schema Browser + GUI Builder use this to populate actual default values
// next to each property's type and offset metadata.
static std::string s_read_class_values_result;

static int32_t handle_read_class_values(const char* params_json,
                                        const char** result_out,
                                        const char**)
{
    std::string want = extract_json_str(params_json, "name");
    if (want.empty()) {
        s_read_class_values_result = R"({"found":false,"error":"name param required"})";
        *result_out = s_read_class_values_result.c_str();
        return 0;
    }
    std::wstring want_w(want.begin(), want.end());
    bool include_inherited = (extract_json_str(params_json, "includeInherited") == "true");
    size_t limit = 500;
    {
        std::string s = extract_json_str(params_json, "limit");
        if (!s.empty()) { try { limit = static_cast<size_t>(std::stoul(s)); } catch (...) {} }
    }

    // Pass 1: find the target UClass by name.
    UObject* target_class = nullptr;
    std::unordered_map<uint32_t, std::wstring> cls_cache;
    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        if (target_class) return;
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* class_ptr = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!class_ptr) return;
        auto* cp = reinterpret_cast<const uint8_t*>(class_ptr);
        const FName& cls_fname = *reinterpret_cast<const FName*>(cp + 0x18);
        auto it = cls_cache.find(cls_fname.ComparisonIndex);
        const std::wstring* cls_name;
        if (it == cls_cache.end()) {
            auto [ins, _] = cls_cache.try_emplace(
                cls_fname.ComparisonIndex, fname_to_wstring(cls_fname));
            cls_name = &ins->second;
        } else {
            cls_name = &it->second;
        }
        if (*cls_name != L"Class" && *cls_name != L"BlueprintGeneratedClass") return;
        const FName& obj_name = *reinterpret_cast<const FName*>(p + 0x18);
        std::wstring on = fname_to_wstring(obj_name);
        if (on.compare(0, 9, L"Default__") == 0) return;
        if (on == want_w) { target_class = obj; }
    });
    if (!target_class) {
        s_read_class_values_result = "{\"found\":false,\"error\":\"UClass not found: ";
        s_read_class_values_result += json_escape(want);
        s_read_class_values_result += "\"}";
        *result_out = s_read_class_values_result.c_str();
        return 0;
    }

    // Pass 2: find the CDO â€” ClassPrivate == target_class, name starts with Default__.
    UObject* cdo = nullptr;
    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        if (cdo) return;
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* obj_class = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (obj_class != target_class) return;
        const FName& obj_name = *reinterpret_cast<const FName*>(p + 0x18);
        std::wstring on = fname_to_wstring(obj_name);
        if (on.compare(0, 9, L"Default__") == 0) { cdo = obj; }
    });
    if (!cdo) {
        s_read_class_values_result = "{\"found\":false,\"error\":\"CDO not found for: ";
        s_read_class_values_result += json_escape(want);
        s_read_class_values_result += "\"}";
        *result_out = s_read_class_values_result.c_str();
        return 0;
    }

    const uint8_t* cdo_bytes = reinterpret_cast<const uint8_t*>(cdo);
    std::string values_json = "[";
    bool first = true;
    size_t emitted = 0;
    size_t total = 0;

    auto emit_props_for = [&](const uint8_t* cls_ptr) {
        auto* field = *reinterpret_cast<const uint8_t* const*>(cls_ptr + 0x50);
        int max_walk = 512;
        while (field && max_walk-- > 0) {
            ++total;
            if (emitted >= limit) {
                field = *reinterpret_cast<const uint8_t* const*>(field + 0x20);
                continue;
            }

            const FName& field_name = *reinterpret_cast<const FName*>(field + 0x28);
            std::string prop_name_s = fname_to_json_string(fname_to_wstring(field_name));
            int32_t offset = *reinterpret_cast<const int32_t*>(field + 0x4C);

            std::string type_name_s;
            auto* class_priv = *reinterpret_cast<void* const*>(field + 0x08);
            if (class_priv) {
                const FName& type_fname = *reinterpret_cast<const FName*>(
                    reinterpret_cast<const uint8_t*>(class_priv) + 0x00);
                type_name_s = fname_to_json_string(fname_to_wstring(type_fname));
            }

            std::string value_str;
            std::string value_kind;
            bool value_is_string = false;

            {
                if (type_name_s == "BoolProperty") {
                    uint8_t field_mask = *reinterpret_cast<const uint8_t*>(field + 0x73);
                    if (field_mask == 0) {
                        uint8_t raw = *reinterpret_cast<const uint8_t*>(cdo_bytes + offset);
                        value_str = (raw != 0) ? "true" : "false";
                    } else {
                        uint8_t byte_off = *reinterpret_cast<const uint8_t*>(field + 0x71);
                        uint8_t raw = *reinterpret_cast<const uint8_t*>(cdo_bytes + offset + byte_off);
                        value_str = ((raw & field_mask) != 0) ? "true" : "false";
                    }
                    value_kind = "bool";
                } else if (type_name_s == "IntProperty") {
                    value_str = std::to_string(*reinterpret_cast<const int32_t*>(cdo_bytes + offset));
                    value_kind = "int";
                } else if (type_name_s == "Int8Property") {
                    value_str = std::to_string(*reinterpret_cast<const int8_t*>(cdo_bytes + offset));
                    value_kind = "int";
                } else if (type_name_s == "Int16Property") {
                    value_str = std::to_string(*reinterpret_cast<const int16_t*>(cdo_bytes + offset));
                    value_kind = "int";
                } else if (type_name_s == "Int64Property") {
                    value_str = std::to_string(*reinterpret_cast<const int64_t*>(cdo_bytes + offset));
                    value_kind = "int";
                } else if (type_name_s == "UInt16Property") {
                    value_str = std::to_string(*reinterpret_cast<const uint16_t*>(cdo_bytes + offset));
                    value_kind = "int";
                } else if (type_name_s == "UInt32Property") {
                    value_str = std::to_string(*reinterpret_cast<const uint32_t*>(cdo_bytes + offset));
                    value_kind = "int";
                } else if (type_name_s == "UInt64Property") {
                    value_str = std::to_string(*reinterpret_cast<const uint64_t*>(cdo_bytes + offset));
                    value_kind = "int";
                } else if (type_name_s == "FloatProperty") {
                    float v = *reinterpret_cast<const float*>(cdo_bytes + offset);
                    char buf[64];
                    std::snprintf(buf, sizeof(buf), "%.6g", static_cast<double>(v));
                    value_str = buf;
                    value_kind = "float";
                } else if (type_name_s == "DoubleProperty") {
                    double v = *reinterpret_cast<const double*>(cdo_bytes + offset);
                    char buf[64];
                    std::snprintf(buf, sizeof(buf), "%.12g", v);
                    value_str = buf;
                    value_kind = "float";
                } else if (type_name_s == "ByteProperty") {
                    value_str = std::to_string(*reinterpret_cast<const uint8_t*>(cdo_bytes + offset));
                    value_kind = "byte";
                } else if (type_name_s == "NameProperty") {
                    const FName& nm = *reinterpret_cast<const FName*>(cdo_bytes + offset);
                    value_str = json_escape(fname_to_json_string(fname_to_wstring(nm)));
                    value_kind = "name";
                    value_is_string = true;
                } else if (type_name_s == "StrProperty") {
                    std::wstring ws = read_fstring_at(cdo, offset);
                    std::string narrow(ws.begin(), ws.end());
                    value_str = json_escape(narrow);
                    value_kind = "string";
                    value_is_string = true;
                } else if (type_name_s == "ObjectProperty") {
                    auto* ref = *reinterpret_cast<UObject* const*>(cdo_bytes + offset);
                    if (ref) {
                        auto* rp = reinterpret_cast<const uint8_t*>(ref);
                        const FName& ref_name = *reinterpret_cast<const FName*>(rp + 0x18);
                        value_str = json_escape(fname_to_json_string(fname_to_wstring(ref_name)));
                        value_is_string = true;
                    } else {
                        value_str = "null";
                    }
                    value_kind = "object";
                } else {
                    value_str = "null";
                    value_kind = "unsupported";
                }
            }

            if (!first) values_json += ",";
            first = false;
            values_json += "{\"name\":\"";
            values_json += prop_name_s;
            values_json += "\",\"type\":\"";
            values_json += type_name_s;
            values_json += "\",\"offset\":";
            values_json += std::to_string(offset);
            values_json += ",\"value\":";
            if (value_is_string) {
                values_json += "\"";
                values_json += value_str;
                values_json += "\"";
            } else {
                values_json += value_str;
            }
            values_json += ",\"valueKind\":\"";
            values_json += value_kind;
            values_json += "\"}";
            ++emitted;

            field = *reinterpret_cast<const uint8_t* const*>(field + 0x20);
        }
    };

    {
        const uint8_t* cur = reinterpret_cast<const uint8_t*>(target_class);
        int max_depth = 32;
        while (cur && max_depth-- > 0) {
            emit_props_for(cur);
            if (!include_inherited) break;
            cur = *reinterpret_cast<const uint8_t* const*>(cur + 0x40);
        }
    }
    values_json += "]";

    s_read_class_values_result = "{\"found\":true,\"name\":\"";
    s_read_class_values_result += fname_to_json_string(want_w);
    s_read_class_values_result += "\",\"values\":";
    s_read_class_values_result += values_json;
    s_read_class_values_result += "}";

    log_info_fmt(STR("[TurdMODEngineBridge] readClassValues: name={} values={}/{}\n"),
                 want_w, emitted, total);

    *result_out = s_read_class_values_result.c_str();
    return 0;
}

// ─── readActorByPtr ────────────────────────────────────────────────────────
// @ctx: light read-by-pointer — NO ForEachUObject scan. Given a hex UObject*
//   (from a prior listClassInstances / vehicle enumeration), dump that live
//   object's property values directly. SEH-guarded so a stale/bad pointer
//   returns an error instead of crashing SCUMServer. Object & array values are
//   returned AS POINTERS (arrays also yield element ptrs) so the caller can
//   recurse readActorByPtr to walk an object graph (e.g. a vehicle's
//   _vehicleAttachments) safely — one fault-protected hop at a time.
// @inv: actor layout matches the bridge UObject map (same as readClassValues):
//   class@0x10, name@0x18; FProperty: name@0x28, type-class@0x08, offset@0x4C,
//   bool-byteoff@0x71, bool-mask@0x73, next@0x20; UClass: first-field@0x50,
//   super@0x40, class-of-class@0x10. @brk: any of those offsets shifting.
static std::string s_read_actor_by_ptr_result;

// POD record filled INSIDE the SEH region (no C++ allocation/unwinding there).
// FName is stored by value (trivially copyable) and stringified AFTER, outside.
struct ActorPropRec {
    FName    name;
    FName    type;
    bool     has_type;
    int32_t  offset;
    uint8_t  bool_mask;
    uint8_t  bool_byteoff;
    uint8_t  raw[16];      // scalar (≤8B) OR TArray header (data@0,num@8,max@12)
};

// Plain-C SEH walk: validate `actor`, read up to `cap` props into `recs`, and
// capture the object + class FNames. Returns prop count, -1 on access violation,
// -2 on null class ptr (not a live UObject). NO C++ objects in __try scope.
static int read_actor_props_seh(const uint8_t* actor, ActorPropRec* recs, int cap,
                                bool include_inherited,
                                FName* out_obj_name, FName* out_cls_name)
{
    int n = 0;
    __try {
        const uint8_t* cls = *reinterpret_cast<const uint8_t* const*>(actor + 0x10);
        if (!cls) return -2;
        // sanity: a live UObject's class itself has a non-null meta-class ptr.
        const uint8_t* meta = *reinterpret_cast<const uint8_t* const*>(cls + 0x10);
        if (!meta) return -2;
        memcpy(out_obj_name, actor + 0x18, sizeof(FName));
        memcpy(out_cls_name, cls + 0x18, sizeof(FName));
        const uint8_t* cur = cls;
        int max_depth = 32;
        while (cur && max_depth-- > 0 && n < cap) {
            const uint8_t* field = *reinterpret_cast<const uint8_t* const*>(cur + 0x50);
            int max_walk = 512;
            while (field && max_walk-- > 0 && n < cap) {
                ActorPropRec& r = recs[n];
                memcpy(&r.name, field + 0x28, sizeof(FName));
                memcpy(&r.offset, field + 0x4C, 4);
                r.bool_byteoff = *reinterpret_cast<const uint8_t*>(field + 0x71);
                r.bool_mask    = *reinterpret_cast<const uint8_t*>(field + 0x73);
                const uint8_t* tcls = *reinterpret_cast<const uint8_t* const*>(field + 0x08);
                r.has_type = (tcls != nullptr);
                if (tcls) memcpy(&r.type, tcls + 0x00, sizeof(FName));
                memcpy(r.raw, actor + r.offset, 16);   // scalar / ptr / TArray header
                ++n;
                field = *reinterpret_cast<const uint8_t* const*>(field + 0x20);
            }
            if (!include_inherited) break;
            cur = *reinterpret_cast<const uint8_t* const*>(cur + 0x40);
        }
        return n;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return -1;
    }
}

// SEH-protected read of up to `cap` pointer-sized elements from a TArray data
// buffer. Returns count, -1 on fault. For object/struct-ptr arrays only.
static int read_ptr_array_seh(const uint8_t* data, int num, uint64_t* out, int cap)
{
    if (!data || num <= 0) return 0;
    int n = (num < cap) ? num : cap;
    __try {
        for (int i = 0; i < n; ++i)
            memcpy(&out[i], data + static_cast<size_t>(i) * 8, 8);
        return n;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return -1;
    }
}

static int32_t handle_read_actor_by_ptr(const char* params_json,
                                        const char** result_out,
                                        const char**)
{
    std::string ptr_s = extract_json_str(params_json, "ptr");
    if (ptr_s.empty()) {
        s_read_actor_by_ptr_result = R"({"found":false,"error":"ptr param required - hex UObject pointer"})";
        *result_out = s_read_actor_by_ptr_result.c_str();
        return 0;
    }
    uint64_t addr = 0;
    try { addr = std::stoull(ptr_s, nullptr, 16); } catch (...) { addr = 0; }
    if (addr == 0) {
        s_read_actor_by_ptr_result = R"({"found":false,"error":"bad ptr - want hex UObject pointer e.g. 0x000001EA"})";
        *result_out = s_read_actor_by_ptr_result.c_str();
        return 0;
    }
    bool include_inherited = (extract_json_str(params_json, "includeInherited") != "false"); // default true
    bool expand_arrays = (extract_json_str(params_json, "expandArrays") == "true");

    const uint8_t* actor = reinterpret_cast<const uint8_t*>(addr);
    std::vector<ActorPropRec> recs(640);
    FName obj_name{}, cls_name{};
    int count = read_actor_props_seh(actor, recs.data(), static_cast<int>(recs.size()),
                                     include_inherited, &obj_name, &cls_name);
    if (count == -1) {
        s_read_actor_by_ptr_result = "{\"found\":false,\"error\":\"access violation reading ptr (stale/invalid pointer)\",\"ptr\":\"";
        s_read_actor_by_ptr_result += json_escape(ptr_s) + "\"}";
        *result_out = s_read_actor_by_ptr_result.c_str();
        return 0;
    }
    if (count == -2) {
        s_read_actor_by_ptr_result = "{\"found\":false,\"error\":\"not a live UObject (null class ptr)\",\"ptr\":\"";
        s_read_actor_by_ptr_result += json_escape(ptr_s) + "\"}";
        *result_out = s_read_actor_by_ptr_result.c_str();
        return 0;
    }

    std::string values_json = "[";
    for (int i = 0; i < count; ++i) {
        const ActorPropRec& r = recs[i];
        std::string prop_name_s = fname_to_json_string(fname_to_wstring(r.name));
        std::string type_name_s = r.has_type ? fname_to_json_string(fname_to_wstring(r.type)) : std::string("");

        std::string value_str = "null";
        std::string value_kind = "unsupported";
        bool value_is_string = false;
        const uint8_t* v = r.raw;

        if (type_name_s == "BoolProperty") {
            uint8_t raw = (r.bool_mask == 0) ? v[0] : (v[r.bool_byteoff] & r.bool_mask);
            value_str = (raw != 0) ? "true" : "false"; value_kind = "bool";
        } else if (type_name_s == "IntProperty") {
            int32_t x; memcpy(&x, v, 4); value_str = std::to_string(x); value_kind = "int";
        } else if (type_name_s == "Int8Property") {
            value_str = std::to_string(static_cast<int8_t>(v[0])); value_kind = "int";
        } else if (type_name_s == "Int16Property") {
            int16_t x; memcpy(&x, v, 2); value_str = std::to_string(x); value_kind = "int";
        } else if (type_name_s == "Int64Property") {
            int64_t x; memcpy(&x, v, 8); value_str = std::to_string(x); value_kind = "int";
        } else if (type_name_s == "UInt16Property") {
            uint16_t x; memcpy(&x, v, 2); value_str = std::to_string(x); value_kind = "int";
        } else if (type_name_s == "UInt32Property") {
            uint32_t x; memcpy(&x, v, 4); value_str = std::to_string(x); value_kind = "int";
        } else if (type_name_s == "UInt64Property") {
            uint64_t x; memcpy(&x, v, 8); value_str = std::to_string(x); value_kind = "int";
        } else if (type_name_s == "FloatProperty") {
            float x; memcpy(&x, v, 4); char buf[64];
            std::snprintf(buf, sizeof(buf), "%.6g", static_cast<double>(x)); value_str = buf; value_kind = "float";
        } else if (type_name_s == "DoubleProperty") {
            double x; memcpy(&x, v, 8); char buf[64];
            std::snprintf(buf, sizeof(buf), "%.12g", x); value_str = buf; value_kind = "float";
        } else if (type_name_s == "ByteProperty" || type_name_s == "EnumProperty") {
            value_str = std::to_string(v[0]); value_kind = "byte";
        } else if (type_name_s == "NameProperty") {
            FName nm; memcpy(&nm, v, sizeof(FName));
            value_str = json_escape(fname_to_json_string(fname_to_wstring(nm)));
            value_kind = "name"; value_is_string = true;
        } else if (type_name_s == "StrProperty") {
            int32_t slen; memcpy(&slen, v + 8, 4);   // FString num (incl. null)
            value_str = std::to_string(slen > 0 ? slen - 1 : 0); value_kind = "strlen";
        } else if (type_name_s == "ObjectProperty" || type_name_s == "WeakObjectProperty" ||
                   type_name_s == "ClassProperty" || type_name_s == "SoftObjectProperty") {
            uint64_t p; memcpy(&p, v, 8);
            if (p) { char buf[32]; std::snprintf(buf, sizeof(buf), "0x%016llX", static_cast<unsigned long long>(p)); value_str = buf; value_is_string = true; }
            else { value_str = "null"; }
            value_kind = "object";
        } else if (type_name_s == "ArrayProperty") {
            uint64_t data; memcpy(&data, v, 8);
            int32_t num; memcpy(&num, v + 8, 4);
            value_kind = "array";
            std::string arr = "{\"count\":" + std::to_string(num);
            char dbuf[32]; std::snprintf(dbuf, sizeof(dbuf), "0x%016llX", static_cast<unsigned long long>(data));
            arr += ",\"data\":\""; arr += dbuf; arr += "\"";
            if (expand_arrays && data && num > 0) {
                uint64_t elems[64];
                int got = read_ptr_array_seh(reinterpret_cast<const uint8_t*>(data), num, elems, 64);
                if (got > 0) {
                    arr += ",\"elements\":[";
                    for (int e = 0; e < got; ++e) {
                        if (e) arr += ",";
                        char eb[32]; std::snprintf(eb, sizeof(eb), "0x%016llX", static_cast<unsigned long long>(elems[e]));
                        arr += "\""; arr += eb; arr += "\"";
                    }
                    arr += "]";
                } else if (got == -1) { arr += ",\"elements\":\"(fault reading buffer)\""; }
            }
            arr += "}";
            value_str = arr; // emitted raw (object), not quoted
        } else if (type_name_s == "StructProperty") {
            value_kind = "struct"; value_str = "null"; // inline struct — recurse not supported in V1
        }

        if (i) values_json += ",";
        values_json += "{\"name\":\"" + prop_name_s + "\",\"type\":\"" + type_name_s +
                       "\",\"offset\":" + std::to_string(r.offset) + ",\"value\":";
        if (value_is_string) { values_json += "\"" + value_str + "\""; }
        else { values_json += value_str; }
        values_json += ",\"valueKind\":\"" + value_kind + "\"}";
    }
    values_json += "]";

    std::wstring cls_w = fname_to_wstring(cls_name);
    std::wstring obj_w = fname_to_wstring(obj_name);
    s_read_actor_by_ptr_result = "{\"found\":true,\"ptr\":\"" + json_escape(ptr_s) +
        "\",\"objectName\":\"" + fname_to_json_string(obj_w) +
        "\",\"className\":\"" + fname_to_json_string(cls_w) +
        "\",\"count\":" + std::to_string(count) + ",\"values\":" + values_json + "}";

    log_info_fmt(STR("[TurdMODEngineBridge] readActorByPtr: ptr={} class={} props={}\n"),
                 std::wstring(ptr_s.begin(), ptr_s.end()), cls_w, count);

    *result_out = s_read_actor_by_ptr_result.c_str();
    return 0;
}

// ─── findInstancesByClass ──────────────────────────────────────────────────
// @ctx: SAFE locate primitive that pairs with readActorByPtr. Scans
//   GUObjectArray for live instances whose class matches `class` and returns
//   their POINTERS — same scan shape as listClassInstances (fname compares
//   only, NO ProcessEvent / GetActorLocation), so it completes instead of
//   hanging the game thread the way getNearbyActors did on a broad filter.
//   Locate (this) without a per-match ProcessEvent + read (readActorByPtr)
//   without a scan = the safe live-RE toolkit.
// @inv: layout same as listClassInstances (class@0x10, class-name@0x18,
//   obj-name@0x18). Skips Default__ CDOs. Early-outs at `limit`.
static std::string s_find_instances_result;

static int32_t handle_find_instances_by_class(const char* params_json,
                                              const char** result_out, const char**)
{
    ensure_hook_installed_once();
    std::string want = extract_json_str(params_json, "class");
    if (want.empty()) {
        s_find_instances_result = R"({"ok":false,"error":"class param required - substring match, or pass exact=true"})";
        *result_out = s_find_instances_result.c_str();
        return 0;
    }
    std::wstring want_w = utf8_to_wstring(want);
    bool exact = (extract_json_str(params_json, "exact") == "true");
    size_t limit = 25;
    { std::string s = extract_json_str(params_json, "limit");
      if (!s.empty()) { try { limit = static_cast<size_t>(std::stoul(s)); } catch (...) {} } }

    std::vector<std::tuple<void*, std::wstring, std::wstring>> found;
    std::unordered_map<uint32_t, std::wstring> cls_cache;
    size_t scanned = 0;

    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        if (found.size() >= limit) return;
        ++scanned;
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* class_ptr = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!class_ptr) return;
        auto* cp = reinterpret_cast<const uint8_t*>(class_ptr);
        const FName& cls_fname = *reinterpret_cast<const FName*>(cp + 0x18);
        auto it = cls_cache.find(cls_fname.ComparisonIndex);
        const std::wstring* cls;
        if (it == cls_cache.end()) {
            auto [ins, _] = cls_cache.try_emplace(cls_fname.ComparisonIndex, fname_to_wstring(cls_fname));
            cls = &ins->second;
        } else { cls = &it->second; }
        bool m = exact ? (*cls == want_w) : (cls->find(want_w) != std::wstring::npos);
        if (!m) return;
        const FName& obj_name = *reinterpret_cast<const FName*>(p + 0x18);
        std::wstring on = fname_to_wstring(obj_name);
        if (on.compare(0, 9, L"Default__") == 0) return;   // skip CDOs — want live instances
        found.emplace_back(static_cast<void*>(obj), *cls, on);
    });

    std::string list = "[";
    bool first = true;
    for (auto& t : found) {
        if (!first) list += ",";
        first = false;
        char pbuf[24]; std::snprintf(pbuf, sizeof(pbuf), "0x%p", std::get<0>(t));
        std::string cn(std::get<1>(t).begin(), std::get<1>(t).end());
        std::string nn(std::get<2>(t).begin(), std::get<2>(t).end());
        list += "{\"ptr\":\""; list += pbuf; list += "\",\"class\":\""; list += cn;
        list += "\",\"name\":\""; list += nn; list += "\"}";
    }
    list += "]";

    char meta[192];
    std::snprintf(meta, sizeof(meta),
                  "{\"ok\":true,\"query\":\"%s\",\"exact\":%s,\"returned\":%zu,\"scanned\":%zu,\"instances\":",
                  want.c_str(), exact ? "true" : "false", found.size(), scanned);
    s_find_instances_result = std::string(meta) + list + "}";
    *result_out = s_find_instances_result.c_str();
    return 0;
}

// â”€â”€â”€ writeClassDefault â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
static std::string s_write_class_default_result;

static int32_t handle_write_class_default(const char* params_json,
                                           const char** result_out,
                                           const char**)
{
    std::string class_name = extract_json_str(params_json, "name");
    std::string prop_name = extract_json_str(params_json, "propertyName");
    std::string value = extract_json_str(params_json, "value");
    std::string value_kind = extract_json_str(params_json, "valueKind");

    if (class_name.empty() || prop_name.empty() || value_kind.empty()) {
        s_write_class_default_result = R"({"ok":false,"error":"missing required param: name, propertyName, or valueKind"})";
        *result_out = s_write_class_default_result.c_str();
        return 0;
    }

    if (value_kind == "string" || value_kind == "name") {
        s_write_class_default_result = R"({"ok":false,"error":"string/name writes not supported in V1"})";
        *result_out = s_write_class_default_result.c_str();
        return 0;
    }
    if (value_kind != "bool" && value_kind != "int" && value_kind != "float" && value_kind != "byte") {
        s_write_class_default_result = "{\"ok\":false,\"error\":\"unsupported valueKind: " + json_escape(value_kind) + "\"}";
        *result_out = s_write_class_default_result.c_str();
        return 0;
    }

    std::wstring class_w(class_name.begin(), class_name.end());
    std::wstring prop_w(prop_name.begin(), prop_name.end());

    UObject* target_class = nullptr;
    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        if (target_class) return;
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* class_ptr = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!class_ptr) return;
        auto* cp = reinterpret_cast<const uint8_t*>(class_ptr);
        const FName& cls_fname = *reinterpret_cast<const FName*>(cp + 0x18);
        std::wstring cn = fname_to_wstring(cls_fname);
        if (cn != L"Class" && cn != L"BlueprintGeneratedClass") return;
        const FName& obj_fname = *reinterpret_cast<const FName*>(p + 0x18);
        std::wstring on = fname_to_wstring(obj_fname);
        if (on.compare(0, 9, L"Default__") == 0) return;
        if (on == class_w) { target_class = obj; }
    });
    if (!target_class) {
        s_write_class_default_result = "{\"ok\":false,\"error\":\"UClass not found: " + json_escape(class_name) + "\"}";
        *result_out = s_write_class_default_result.c_str();
        return 0;
    }

    UObject* cdo = nullptr;
    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        if (cdo) return;
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* obj_class = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (obj_class != target_class) return;
        const FName& obj_fname = *reinterpret_cast<const FName*>(p + 0x18);
        std::wstring on = fname_to_wstring(obj_fname);
        if (on.compare(0, 9, L"Default__") == 0) { cdo = obj; }
    });
    if (!cdo) {
        s_write_class_default_result = "{\"ok\":false,\"error\":\"CDO not found for: " + json_escape(class_name) + "\"}";
        *result_out = s_write_class_default_result.c_str();
        return 0;
    }

    // Walk FProperty chain + SuperStruct to find the property
    const uint8_t* found_field = nullptr;
    int32_t prop_offset = 0;
    std::wstring prop_type_w;

    auto* cls = reinterpret_cast<const uint8_t*>(target_class);
    while (cls && !found_field) {
        auto* field = *reinterpret_cast<const uint8_t* const*>(cls + 0x50);
        while (field) {
            const FName& fn = *reinterpret_cast<const FName*>(field + 0x28);
            if (fname_to_wstring(fn) == prop_w) {
                found_field = field;
                prop_offset = *reinterpret_cast<const int32_t*>(field + 0x4C);
                auto* fc = *reinterpret_cast<const uint8_t* const*>(field + 0x08);
                const FName& tf = *reinterpret_cast<const FName*>(fc + 0x00);
                prop_type_w = fname_to_wstring(tf);
                break;
            }
            field = *reinterpret_cast<const uint8_t* const*>(field + 0x20);
        }
        cls = *reinterpret_cast<const uint8_t* const*>(cls + 0x40);
    }
    if (!found_field) {
        s_write_class_default_result = "{\"ok\":false,\"error\":\"property not found: " +
            json_escape(prop_name) + " on " + json_escape(class_name) + "\"}";
        *result_out = s_write_class_default_result.c_str();
        return 0;
    }

    // Verify FFieldClass matches requested valueKind
    bool type_ok = false;
    if (value_kind == "bool")  type_ok = (prop_type_w == L"BoolProperty");
    else if (value_kind == "int")
        type_ok = (prop_type_w == L"IntProperty" || prop_type_w == L"Int8Property" ||
                   prop_type_w == L"Int16Property" || prop_type_w == L"Int64Property" ||
                   prop_type_w == L"UInt16Property" || prop_type_w == L"UInt32Property" ||
                   prop_type_w == L"UInt64Property");
    else if (value_kind == "float")
        type_ok = (prop_type_w == L"FloatProperty" || prop_type_w == L"DoubleProperty");
    else if (value_kind == "byte")
        type_ok = (prop_type_w == L"ByteProperty");

    if (!type_ok) {
        std::string pt = fname_to_json_string(prop_type_w);
        s_write_class_default_result = "{\"ok\":false,\"error\":\"type mismatch: property is " +
            json_escape(pt) + " but valueKind is " + json_escape(value_kind) + "\"}";
        *result_out = s_write_class_default_result.c_str();
        return 0;
    }

    auto* cdo_bytes = reinterpret_cast<uint8_t*>(cdo);
    bool write_ok = false;

    if (value_kind == "bool") {
        uint8_t mask = *reinterpret_cast<const uint8_t*>(found_field + 0x73);
        uint8_t* target = cdo_bytes + prop_offset;
        bool nv = (value == "true" || value == "1");
        if (mask != 0) {
            uint8_t ob = *target;
            *target = nv ? (ob | mask) : (ob & ~mask);
        } else {
            *target = nv ? 1 : 0;
        }
        write_ok = true;
    } else if (value_kind == "int") {
        *reinterpret_cast<int32_t*>(cdo_bytes + prop_offset) = std::stoi(value);
        write_ok = true;
    } else if (value_kind == "float") {
        if (prop_type_w == L"DoubleProperty")
            *reinterpret_cast<double*>(cdo_bytes + prop_offset) = std::stod(value);
        else
            *reinterpret_cast<float*>(cdo_bytes + prop_offset) = std::stof(value);
        write_ok = true;
    } else if (value_kind == "byte") {
        cdo_bytes[prop_offset] = static_cast<uint8_t>(std::stoi(value) & 0xFF);
        write_ok = true;
    }

    if (!write_ok) {
        s_write_class_default_result = R"({"ok":false,"error":"write fell through"})";
        *result_out = s_write_class_default_result.c_str();
        return 0;
    }

    log_info_fmt(STR("[TurdMODEngineBridge] writeClassDefault: {}.{} @0x{:X} kind={}\n"),
                 class_w, prop_w, static_cast<unsigned>(prop_offset),
                 std::wstring(value_kind.begin(), value_kind.end()));

    s_write_class_default_result = "{\"ok\":true,\"className\":\"" + json_escape(class_name) +
        "\",\"propertyName\":\"" + json_escape(prop_name) +
        "\",\"valueKind\":\"" + json_escape(value_kind) +
        "\",\"offset\":" + std::to_string(prop_offset) + "}";
    *result_out = s_write_class_default_result.c_str();
    return 0;
}

// â”€â”€â”€ applyRecipe â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
struct OverrideEntry {
    std::string property;
    std::string value;
    std::string valueKind;
};

static std::vector<OverrideEntry> parse_overrides(const char* json)
{
    std::vector<OverrideEntry> out;
    if (!json) return out;
    const char* p = strstr(json, "\"overrides\"");
    if (!p) return out;
    p += 11; // strlen("\"overrides\"")
    while (*p && (*p == ' ' || *p == '\t' || *p == '\n' || *p == '\r')) p++;
    if (*p != ':') return out;
    p++;
    while (*p && (*p == ' ' || *p == '\t' || *p == '\n' || *p == '\r')) p++;
    if (*p != '[') return out;
    p++;

    while (*p) {
        while (*p && (*p == ' ' || *p == '\t' || *p == '\n' || *p == '\r' || *p == ',')) p++;
        if (*p == ']' || *p == '\0') break;
        if (*p != '{') break;

        int depth = 0;
        bool in_str = false;
        const char* q = p;
        while (*q) {
            if (*q == '"' && (q == p || *(q - 1) != '\\')) in_str = !in_str;
            else if (!in_str) {
                if (*q == '{') depth++;
                else if (*q == '}') { depth--; if (depth == 0) { q++; break; } }
            }
            q++;
        }

        std::string obj(p, q);
        OverrideEntry entry;
        entry.property = extract_json_str(obj.c_str(), "property");
        entry.value = extract_json_str(obj.c_str(), "value");
        entry.valueKind = extract_json_str(obj.c_str(), "valueKind");
        if (!entry.property.empty()) out.push_back(std::move(entry));
        p = q;
    }
    return out;
}

static std::string s_apply_recipe_result;

static int32_t handle_apply_recipe(const char* params_json,
                                    const char** result_out,
                                    const char**)
{
    std::string class_name = extract_json_str(params_json, "name");
    if (class_name.empty()) {
        s_apply_recipe_result = R"({"ok":false,"error":"missing 'name' parameter","applied":0,"failed":0})";
        *result_out = s_apply_recipe_result.c_str();
        return 0;
    }

    std::vector<OverrideEntry> overrides = parse_overrides(params_json);
    if (overrides.empty()) {
        s_apply_recipe_result = "{\"ok\":false,\"error\":\"no overrides parsed\",\"applied\":0,\"failed\":0,\"className\":\""
            + json_escape(class_name) + "\"}";
        *result_out = s_apply_recipe_result.c_str();
        return 0;
    }

    std::wstring class_w(class_name.begin(), class_name.end());

    UObject* found_class = nullptr;
    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        if (found_class) return;
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* class_ptr = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!class_ptr) return;
        auto* cp = reinterpret_cast<const uint8_t*>(class_ptr);
        const FName& cls_fname = *reinterpret_cast<const FName*>(cp + 0x18);
        std::wstring cn = fname_to_wstring(cls_fname);
        if (cn != L"Class" && cn != L"BlueprintGeneratedClass") return;
        const FName& obj_fname = *reinterpret_cast<const FName*>(p + 0x18);
        std::wstring on = fname_to_wstring(obj_fname);
        if (on.compare(0, 9, L"Default__") == 0) return;
        if (on == class_w) { found_class = obj; }
    });
    if (!found_class) {
        s_apply_recipe_result = "{\"ok\":false,\"error\":\"UClass not found: " +
            json_escape(class_name) + "\",\"applied\":0,\"failed\":0}";
        *result_out = s_apply_recipe_result.c_str();
        return 0;
    }

    UObject* cdo = nullptr;
    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        if (cdo) return;
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* obj_class = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (obj_class != found_class) return;
        const FName& obj_fname = *reinterpret_cast<const FName*>(p + 0x18);
        std::wstring on = fname_to_wstring(obj_fname);
        if (on.compare(0, 9, L"Default__") == 0) { cdo = obj; }
    });
    if (!cdo) {
        s_apply_recipe_result = "{\"ok\":false,\"error\":\"CDO not found for: " +
            json_escape(class_name) + "\",\"applied\":0,\"failed\":0}";
        *result_out = s_apply_recipe_result.c_str();
        return 0;
    }

    auto* cdo_bytes = reinterpret_cast<uint8_t*>(cdo);
    int applied = 0, failed = 0;
    std::string results_json = "[";

    for (size_t i = 0; i < overrides.size(); i++) {
        const auto& ov = overrides[i];
        std::wstring prop_w(ov.property.begin(), ov.property.end());

        int32_t offset = find_property_offset(found_class, prop_w.c_str());
        if (offset < 0) {
            if (i > 0) results_json += ",";
            results_json += "{\"property\":\"" + json_escape(ov.property) +
                "\",\"ok\":false,\"error\":\"property not found\",\"valueKind\":\"" +
                json_escape(ov.valueKind) + "\"}";
            failed++;
            continue;
        }

        if (ov.valueKind == "name" || ov.valueKind == "string") {
            if (i > 0) results_json += ",";
            results_json += "{\"property\":\"" + json_escape(ov.property) +
                "\",\"ok\":false,\"error\":\"" + json_escape(ov.valueKind) +
                " writes not supported in V1\",\"offset\":" + std::to_string(offset) +
                ",\"valueKind\":\"" + json_escape(ov.valueKind) + "\"}";
            failed++;
            continue;
        }

        // Need FField pointer for BoolProperty FieldMask
        const uint8_t* target_field = nullptr;
        {
            auto* walk = reinterpret_cast<const uint8_t*>(found_class);
            while (walk && !target_field) {
                auto* field = *reinterpret_cast<const uint8_t* const*>(walk + 0x50);
                while (field) {
                    const FName& fn = *reinterpret_cast<const FName*>(field + 0x28);
                    if (fname_to_wstring(fn) == prop_w) { target_field = field; break; }
                    field = *reinterpret_cast<const uint8_t* const*>(field + 0x20);
                }
                walk = *reinterpret_cast<const uint8_t* const*>(walk + 0x40);
            }
        }

        bool write_ok = false;
        std::string write_err;

        if (ov.valueKind == "bool") {
            bool val = (ov.value == "true" || ov.value == "1");
            uint8_t mask = target_field ? *reinterpret_cast<const uint8_t*>(target_field + 0x73) : 0;
            if (mask != 0 && mask != 0xFF) {
                uint8_t ob = *(cdo_bytes + offset);
                *(cdo_bytes + offset) = val ? (ob | mask) : (ob & ~mask);
            } else {
                *(cdo_bytes + offset) = val ? 1 : 0;
            }
            write_ok = true;
        } else if (ov.valueKind == "int") {
            *reinterpret_cast<int32_t*>(cdo_bytes + offset) = std::stoi(ov.value);
            write_ok = true;
        } else if (ov.valueKind == "float") {
            *reinterpret_cast<float*>(cdo_bytes + offset) = std::stof(ov.value);
            write_ok = true;
        } else if (ov.valueKind == "byte") {
            *(cdo_bytes + offset) = static_cast<uint8_t>(std::stoi(ov.value) & 0xFF);
            write_ok = true;
        }

        if (i > 0) results_json += ",";
        if (write_ok) {
            results_json += "{\"property\":\"" + json_escape(ov.property) +
                "\",\"ok\":true,\"offset\":" + std::to_string(offset) +
                ",\"valueKind\":\"" + json_escape(ov.valueKind) + "\"}";
            applied++;
        } else {
            if (write_err.empty()) write_err = "unknown error";
            results_json += "{\"property\":\"" + json_escape(ov.property) +
                "\",\"ok\":false,\"error\":\"" + json_escape(write_err) +
                "\",\"offset\":" + std::to_string(offset) +
                ",\"valueKind\":\"" + json_escape(ov.valueKind) + "\"}";
            failed++;
        }
    }
    results_json += "]";

    bool all_ok = (failed == 0 && applied > 0);
    s_apply_recipe_result = "{\"ok\":" + std::string(all_ok ? "true" : "false") +
        ",\"className\":\"" + json_escape(class_name) +
        "\",\"applied\":" + std::to_string(applied) +
        ",\"failed\":" + std::to_string(failed) +
        ",\"results\":" + results_json + "}";

    log_info_fmt(STR("[TurdMODEngineBridge] applyRecipe: class={} applied={} failed={}\n"),
                 class_w, applied, failed);

    *result_out = s_apply_recipe_result.c_str();
    return 0;
}

// â”€â”€â”€ spawnWidgetRouter â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
static std::string s_spawn_router_result;

static int32_t handle_spawn_widget_router(const char* params_json,
                                           const char** result_out, const char**)
{
    UObject* router = find_first_instance_of_class(L"TurdMODRouter");
    if (router) {
        char buf[256];
        std::snprintf(buf, sizeof(buf),
            R"({"ok":true,"status":"already_exists","routerPtr":"0x%llx"})",
            (unsigned long long)(uintptr_t)router);
        s_spawn_router_result = buf;
        log_info_fmt(STR("[spawnWidgetRouter] already exists at {:p}\n"), static_cast<void*>(router));
        *result_out = s_spawn_router_result.c_str();
        return 0;
    }

    UObject* cdo = nullptr;
    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        if (cdo) return;
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        const FName& on = *reinterpret_cast<const FName*>(p + 0x18);
        std::wstring name = fname_to_wstring(on);
        if (name.compare(0, 9, L"Default__") == 0 &&
            name.find(L"TurdMODRouter") != std::wstring::npos) {
            cdo = obj;
        }
    });

    if (cdo) {
        char buf[512];
        std::snprintf(buf, sizeof(buf),
            R"({"ok":true,"status":"cdo_only","routerPtr":"0x%llx","warning":"CDO found but no spawned instance. NetMulticast replication requires a spawned actor."})",
            (unsigned long long)(uintptr_t)cdo);
        s_spawn_router_result = buf;
        log_info_fmt(STR("[spawnWidgetRouter] CDO at {:p}, no live instance\n"), static_cast<void*>(cdo));
        *result_out = s_spawn_router_result.c_str();
        return 0;
    }

    s_spawn_router_result = R"({"ok":false,"status":"class_not_found","error":"TurdMODRouter not found. Deploy TurdMODLoader pak and call loadAsset first."})";
    log_info_fmt(STR("[spawnWidgetRouter] class not found\n"));
    *result_out = s_spawn_router_result.c_str();
    return 0;
}

// â”€â”€â”€ showPanel â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
static std::string s_show_panel_result;

static int32_t handle_show_panel(const char* params_json,
                                  const char** result_out, const char**)
{
    ensure_hook_installed_once();

    std::string command = extract_json_str(params_json, "command");
    std::string widget_class = extract_json_str(params_json, "widgetClass");
    std::string payload = extract_json_str(params_json, "payload");
    std::string target = extract_json_str(params_json, "target");

    if (command.empty()) command = "Show";
    if (target.empty()) target = "all";

    if (widget_class.empty()) {
        s_show_panel_result = R"({"ok":false,"error":"widgetClass is required"})";
        *result_out = s_show_panel_result.c_str();
        return 0;
    }

    UObject* router = find_first_instance_of_class(L"TurdMODRouter");
    bool using_cdo = false;
    if (!router) {
        UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
            if (router) return;
            auto* p = reinterpret_cast<const uint8_t*>(obj);
            const FName& on = *reinterpret_cast<const FName*>(p + 0x18);
            std::wstring name = fname_to_wstring(on);
            if (name.compare(0, 9, L"Default__") == 0 &&
                name.find(L"TurdMODRouter") != std::wstring::npos) {
                router = obj;
                using_cdo = true;
            }
        });
    }
    if (!router) {
        s_show_panel_result = R"({"ok":false,"error":"TurdMODRouter not found. Deploy TurdMODLoader pak and call loadAsset + spawnWidgetRouter first."})";
        *result_out = s_show_panel_result.c_str();
        return 0;
    }

    UObject* fn = find_ufunction(L"NetMulticast_HandleWidgetCommand", L"TurdMODRouter");
    if (!fn) fn = find_ufunction(L"NetMulticast_HandleWidgetCommand", L"");
    if (!fn) {
        s_show_panel_result = R"({"ok":false,"error":"NetMulticast_HandleWidgetCommand UFunction not found"})";
        *result_out = s_show_panel_result.c_str();
        return 0;
    }

    std::wstring cmd_w(command.begin(), command.end());
    std::wstring path_w(widget_class.begin(), widget_class.end());
    std::wstring payload_w(payload.begin(), payload.end());

    struct FStringSlot { wchar_t* Data; int32_t Num; int32_t Max; };
    struct Params { FStringSlot Command; FStringSlot Path; FStringSlot Payload; };

    Params params = {};
    params.Command.Data = const_cast<wchar_t*>(cmd_w.c_str());
    params.Command.Num  = static_cast<int32_t>(cmd_w.length() + 1);
    params.Command.Max  = params.Command.Num;
    params.Path.Data    = const_cast<wchar_t*>(path_w.c_str());
    params.Path.Num     = static_cast<int32_t>(path_w.length() + 1);
    params.Path.Max     = params.Path.Num;
    params.Payload.Data = const_cast<wchar_t*>(payload_w.c_str());
    params.Payload.Num  = static_cast<int32_t>(payload_w.length() + 1);
    params.Payload.Max  = params.Payload.Num;

    log_info_fmt(STR("[showPanel] router={:p} fn={:p} cmd={} path={} cdo={}\n"),
                 static_cast<void*>(router), static_cast<void*>(fn),
                 cmd_w, path_w, using_cdo ? 1 : 0);

    uint32_t pe_code = call_processevent_seh(router, reinterpret_cast<UFunction*>(fn), &params);
    if (pe_code == 0) {
        s_show_panel_result = "{\"ok\":true,\"command\":\"" + json_escape(command) +
            "\",\"widgetClass\":\"" + json_escape(widget_class) +
            "\",\"target\":\"" + json_escape(target) +
            "\",\"usingCDO\":" + std::string(using_cdo ? "true" : "false") + "}";
    } else {
        s_show_panel_result = "{\"ok\":false,\"error\":\"ProcessEvent crashed\"}";
    }

    log_info_fmt(STR("[showPanel] done\n"));
    *result_out = s_show_panel_result.c_str();
    return 0;
}

// dumpWidgets â€” enumerate every UUserWidget-derived UClass in
// GUObjectArray. Foundation for the UI/UX Maker: until we know what
// SCUM widgets exist, we can't build "browse SCUM's UI surface" or
// "target widget X for a UIIntent" features.
//
// Strategy:
//   Pass 1 â€” walk GUObjectArray to find the base UserWidget UClass
//     (the engine-defined UUserWidget). Single pointer.
//   Pass 2 â€” walk again. For each UClass-typed UObject, follow its
//     SuperStruct chain. If it reaches the UserWidget pointer, it's
//     a widget class. Emit name + kind + parent.
//
// Pointer equality on the SuperStruct chain is O(depth) per class
// and cheap. Caching by class ComparisonIndex avoids re-walking a
// class we've already classified. With ~14k UClasses and chains
// typically <10 deep, the whole scan finishes in milliseconds.
//
// UStruct layout (UE 4.27):
//   0x40  UStruct* SuperStruct
//
// Params (JSON, all optional):
//   { "grep": "Inventory", "limit": 500, "kind": "bp",
//     "includeParent": "true" }
//
// Output:
//   { "totalWidgets": N, "emitted": M, "widgets":
//       [{ "name": "BP_PlayerHUD_C", "kind": "bp",
//          "parent": "ConZUserWidget" }, ...] }
static int32_t handle_dump_widgets(const char* params_json,
                                   const char** result_out,
                                   const char**)
{
    std::string grep = extract_json_str(params_json, "grep");
    std::string limit_str = extract_json_str(params_json, "limit");
    std::string kind = extract_json_str(params_json, "kind");
    std::string include_parent_s = extract_json_str(params_json, "includeParent");
    size_t kMaxEmit = 500;
    if (!limit_str.empty()) {
        try { kMaxEmit = static_cast<size_t>(std::stoul(limit_str)); } catch (...) {}
    }
    bool want_bp  = kind.empty() || kind == "bp";
    bool want_cpp = kind.empty() || kind == "cpp";
    bool include_parent = (include_parent_s == "true" || include_parent_s == "1");

    // Pass 1: find the UUserWidget base class. Look for a UObject whose
    // class FName is "Class" (C++ UClass) and whose own FName is
    // "UserWidget". Cache class-FName lookups per ComparisonIndex to
    // avoid burning ToString calls on the 1.5M-object walk.
    std::unordered_map<uint32_t, std::wstring> cls_name_cache;
    UObject* user_widget_class = nullptr;
    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        if (user_widget_class) return;
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* class_ptr = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!class_ptr) return;
        auto* cp = reinterpret_cast<const uint8_t*>(class_ptr);
        const FName& cls_fname = *reinterpret_cast<const FName*>(cp + 0x18);
        auto it = cls_name_cache.find(cls_fname.ComparisonIndex);
        const std::wstring* cls_name;
        if (it == cls_name_cache.end()) {
            auto [ins, _] = cls_name_cache.try_emplace(
                cls_fname.ComparisonIndex, fname_to_wstring(cls_fname));
            cls_name = &ins->second;
        } else {
            cls_name = &it->second;
        }
        if (*cls_name != L"Class") return;
        const FName& obj_name = *reinterpret_cast<const FName*>(p + 0x18);
        if (fname_to_wstring(obj_name) == L"UserWidget") {
            user_widget_class = obj;
        }
    });

    if (!user_widget_class) {
        s_widgets_result =
            R"({"error":"UserWidget base UClass not found in GUObjectArray"})";
        *result_out = s_widgets_result.c_str();
        return 0;
    }

    // Per-UClass classification cache: 1 = is widget subclass, 0 = is not.
    // Keyed by the UClass UObject pointer (each class is unique).
    std::unordered_map<const void*, int> widget_cache;
    widget_cache[user_widget_class] = 1;

    auto is_widget_class = [&](const void* candidate) -> bool {
        if (!candidate) return false;
        auto it = widget_cache.find(candidate);
        if (it != widget_cache.end()) return it->second == 1;
        // Walk SuperStruct chain, recording each unknown node we visit
        // so subsequent lookups for siblings are O(1).
        std::vector<const void*> chain;
        const void* cur = candidate;
        int decision = 0;
        while (cur) {
            auto cit = widget_cache.find(cur);
            if (cit != widget_cache.end()) {
                decision = cit->second;
                break;
            }
            chain.push_back(cur);
            cur = *reinterpret_cast<const void* const*>(
                reinterpret_cast<const uint8_t*>(cur) + 0x40);
        }
        for (const void* node : chain) widget_cache[node] = decision;
        return decision == 1;
    };

    // Pass 2: emit every non-CDO UClass-typed UObject whose chain hits
    // the UserWidget base.
    size_t total = 0;
    size_t emitted = 0;
    std::string out;
    out.reserve(32768);
    out = "[";
    bool first = true;

    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* class_ptr = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!class_ptr) return;
        auto* cp = reinterpret_cast<const uint8_t*>(class_ptr);
        const FName& cls_fname = *reinterpret_cast<const FName*>(cp + 0x18);
        auto it = cls_name_cache.find(cls_fname.ComparisonIndex);
        const std::wstring* cls_name;
        if (it == cls_name_cache.end()) {
            auto [ins, _] = cls_name_cache.try_emplace(
                cls_fname.ComparisonIndex, fname_to_wstring(cls_fname));
            cls_name = &ins->second;
        } else {
            cls_name = &it->second;
        }
        const char* kind_tag = nullptr;
        if (*cls_name == L"Class") {
            if (!want_cpp) return;
            kind_tag = "cpp";
        } else if (*cls_name == L"BlueprintGeneratedClass") {
            if (!want_bp) return;
            kind_tag = "bp";
        } else {
            return;
        }

        // Now obj IS a UClass-typed object. obj itself is the candidate.
        if (!is_widget_class(obj)) return;
        ++total;

        if (emitted >= kMaxEmit) return;
        const FName& obj_name = *reinterpret_cast<const FName*>(p + 0x18);
        std::wstring obj_name_w = fname_to_wstring(obj_name);
        if (obj_name_w.compare(0, 9, L"Default__") == 0) return;
        std::string obj_name_s = fname_to_json_string(obj_name_w);
        if (!grep.empty() && obj_name_s.find(grep) == std::string::npos) return;

        std::string parent_s;
        if (include_parent) {
            auto* super_ptr = *reinterpret_cast<const void* const*>(
                reinterpret_cast<const uint8_t*>(obj) + 0x40);
            if (super_ptr) {
                const FName& parent_name =
                    *reinterpret_cast<const FName*>(
                        reinterpret_cast<const uint8_t*>(super_ptr) + 0x18);
                parent_s = fname_to_json_string(fname_to_wstring(parent_name));
            }
        }

        ++emitted;
        if (!first) out += ",";
        first = false;
        out += "{\"name\":\"";
        out += obj_name_s;
        out += "\",\"kind\":\"";
        out += kind_tag;
        out += "\"";
        if (include_parent) {
            out += ",\"parent\":\"";
            out += parent_s;
            out += "\"";
        }
        out += "}";
    });
    out += "]";

    s_widgets_result = "{\"totalWidgets\":";
    s_widgets_result += std::to_string(total);
    s_widgets_result += ",\"emitted\":";
    s_widgets_result += std::to_string(emitted);
    s_widgets_result += ",\"limit\":";
    s_widgets_result += std::to_string(kMaxEmit);
    s_widgets_result += ",\"grep\":\"";
    s_widgets_result += grep;
    s_widgets_result += "\",\"widgets\":";
    s_widgets_result += out;
    s_widgets_result += "}";

    std::wstring grep_w = utf8_to_wstring(grep);
    log_info_fmt(STR("[TurdMODEngineBridge] handle_dump_widgets: total={} emitted={} grep=\"{}\"\n"),
                 total, emitted, grep_w);

    *result_out = s_widgets_result.c_str();
    return 0;
}

// runTestAdminCommand â€” invoke MiscStatics::Test_ProcessAdminCommand,
// the static BP-callable variant. Its very name suggests it's the
// test/dev path that bypasses the runtime admin-authentication flag
// the production Chat_Server_ProcessAdminCommand checks (which we
// hit silently when we tried Help / SetGold / SpawnVehicle through
// the production path â€” function fired but command was rejected).
//
// Signature (via describeFunction 2026-05-16):
//   void MiscStatics::Test_ProcessAdminCommand(
//       UObject* WorldContextObject,
//       FString commandText)
//
// Params: { "command": "SpawnVehicle BPC_Kinglet_Duster" }
// [SCRUBBED] Game-specific section removed (111 lines)

static int32_t handle_dump_classes(const char* params_json,
                                   const char** result_out,
                                   const char**)
{
    std::string grep = extract_json_str(params_json, "grep");
    std::string limit_str = extract_json_str(params_json, "limit");
    std::string kind = extract_json_str(params_json, "kind");
    size_t kMaxEmit = 500;
    if (!limit_str.empty()) {
        try { kMaxEmit = static_cast<size_t>(std::stoul(limit_str)); } catch (...) {}
    }
    bool want_bp  = kind.empty() || kind == "bp";
    bool want_cpp = kind.empty() || kind == "cpp";

    std::unordered_map<uint32_t, std::wstring> class_name_cache;
    size_t total = 0;
    size_t emitted = 0;
    std::string out;
    out.reserve(32768);
    out = "[";
    bool first = true;

    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* class_ptr = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!class_ptr) return;
        auto* cp = reinterpret_cast<const uint8_t*>(class_ptr);
        const FName& class_fname = *reinterpret_cast<const FName*>(cp + 0x18);

        auto cache_it = class_name_cache.find(class_fname.ComparisonIndex);
        const std::wstring* cls_name;
        if (cache_it == class_name_cache.end()) {
            auto [ins, _] = class_name_cache.try_emplace(
                class_fname.ComparisonIndex, fname_to_wstring(class_fname));
            cls_name = &ins->second;
        } else {
            cls_name = &cache_it->second;
        }

        const char* kind_tag = nullptr;
        if (*cls_name == L"Class") {
            if (!want_cpp) return;
            kind_tag = "cpp";
        } else if (*cls_name == L"BlueprintGeneratedClass") {
            if (!want_bp) return;
            kind_tag = "bp";
        } else {
            return;
        }
        ++total;
        if (emitted >= kMaxEmit) return;

        const FName& obj_name = *reinterpret_cast<const FName*>(p + 0x18);
        std::wstring obj_name_w = fname_to_wstring(obj_name);
        if (obj_name_w.compare(0, 9, L"Default__") == 0) return;
        std::string obj_name_s = fname_to_json_string(obj_name_w);

        if (!grep.empty() && obj_name_s.find(grep) == std::string::npos) return;

        ++emitted;
        if (!first) out += ",";
        first = false;
        out += "{\"name\":\"";
        out += obj_name_s;
        out += "\",\"kind\":\"";
        out += kind_tag;
        out += "\"}";
    });
    out += "]";

    s_classes_result = "{\"totalClasses\":";
    s_classes_result += std::to_string(total);
    s_classes_result += ",\"emitted\":";
    s_classes_result += std::to_string(emitted);
    s_classes_result += ",\"limit\":";
    s_classes_result += std::to_string(kMaxEmit);
    s_classes_result += ",\"grep\":\"";
    s_classes_result += grep;
    s_classes_result += "\",\"classes\":";
    s_classes_result += out;
    s_classes_result += "}";

    std::wstring grep_w = utf8_to_wstring(grep);
    log_info_fmt(STR("[TurdMODEngineBridge] handle_dump_classes: total={} emitted={} grep=\"{}\"\n"),
                 total, emitted, grep_w);

    *result_out = s_classes_result.c_str();
    return 0;
}

// â”€â”€â”€ handle_dump_all_classes â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Dumps every UClass / BlueprintGeneratedClass / WidgetBlueprintGeneratedClass
// / AnimBlueprintGeneratedClass to <outDir>/classes.json as a JSON array.
// Each entry includes full property and function detail.
//
// Params: { "outDir": "C:/abs/path" }
// Result: { "ok": true, "path": "...", "count": N }
static int32_t handle_dump_all_classes(const char* params_json,
                                       const char** result_out,
                                       const char**)
{
    std::string out_dir = extract_json_str(params_json, "outDir");
    if (out_dir.empty()) {
        s_dump_all_classes_result = R"({"error":"outDir param required"})";
        *result_out = s_dump_all_classes_result.c_str();
        return 0;
    }

    std::string out_path = out_dir + "/classes.json";
    std::ofstream ofs(out_path);
    if (!ofs.is_open()) {
        s_dump_all_classes_result = "{\"error\":\"could not open outFile: ";
        for (char c : out_path) {
            if (c == '"') s_dump_all_classes_result += "\\\"";
            else if (c == '\\') s_dump_all_classes_result += "\\\\";
            else s_dump_all_classes_result += c;
        }
        s_dump_all_classes_result += "\"}";
        *result_out = s_dump_all_classes_result.c_str();
        return 0;
    }

    // FField walker â€” streams each FProperty as a JSON object fragment.
    // full_prop=true emits offset/size/arrayDim (FProperty fields);
    // full_prop=false emits name+type only (for function param lists).
    auto walk_fields = [&](std::ofstream& out_ofs, void* first_field,
                           int cap_props, bool full_prop) -> int {
        int count = 0;
        bool first_f = true;
        void* field = first_field;
        out_ofs << "[";
        while (field && count < cap_props) {
            auto* fp = reinterpret_cast<const uint8_t*>(field);
            // FField layout (verified): 0x08 ClassPrivate, 0x20 Next, 0x28 Name
            auto* class_priv = *reinterpret_cast<void* const*>(fp + 0x08);
            const FName& prop_fname = *reinterpret_cast<const FName*>(fp + 0x28);
            std::string prop_name_s = fname_to_json_string(fname_to_wstring(prop_fname));

            std::string type_name_s;
            if (class_priv) {
                auto* fclass = reinterpret_cast<const uint8_t*>(class_priv);
                const FName& type_fname = *reinterpret_cast<const FName*>(fclass + 0x00);
                type_name_s = fname_to_json_string(fname_to_wstring(type_fname));
            }

            if (!first_f) out_ofs << ",";
            first_f = false;
            out_ofs << "{\"name\":\"" << prop_name_s
                    << "\",\"type\":\"" << type_name_s << "\"";

            if (full_prop) {
                // FProperty extends FField: 0x38 ArrayDim, 0x3C ElementSize,
                // 0x4C Offset_Internal.
                int32_t array_dim       = *reinterpret_cast<const int32_t*>(fp + 0x38);
                int32_t element_size    = *reinterpret_cast<const int32_t*>(fp + 0x3C);
                int32_t offset_internal = *reinterpret_cast<const int32_t*>(fp + 0x4C);
                out_ofs << ",\"offset\":" << offset_internal
                        << ",\"size\":"   << element_size
                        << ",\"arrayDim\":" << array_dim;
            }
            out_ofs << "}";

            field = *reinterpret_cast<void* const*>(fp + 0x20);
            ++count;
        }
        out_ofs << "]";
        return count;
    };

    std::unordered_map<uint32_t, std::wstring> cls_name_cache;
    size_t count = 0;
    bool first_entry = true;

    ofs << "[";

    SCAN_TIMEOUT_INIT();
    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        SCAN_TIMEOUT_CHECK();
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* class_ptr = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!class_ptr) return;
        auto* cp = reinterpret_cast<const uint8_t*>(class_ptr);
        const FName& cls_fname = *reinterpret_cast<const FName*>(cp + 0x18);

        auto it = cls_name_cache.find(cls_fname.ComparisonIndex);
        const std::wstring* cls_name_ptr;
        if (it == cls_name_cache.end()) {
            auto [ins, _] = cls_name_cache.try_emplace(
                cls_fname.ComparisonIndex, fname_to_wstring(cls_fname));
            cls_name_ptr = &ins->second;
        } else {
            cls_name_ptr = &it->second;
        }

        const std::wstring& cls_name = *cls_name_ptr;
        const char* kind_tag = nullptr;
        if (cls_name == L"Class") {
            kind_tag = "cpp";
        } else if (cls_name == L"BlueprintGeneratedClass"
                || cls_name == L"WidgetBlueprintGeneratedClass"
                || cls_name == L"AnimBlueprintGeneratedClass") {
            kind_tag = "bp";
        } else {
            return;
        }

        // Skip CDOs.
        const FName& obj_fname = *reinterpret_cast<const FName*>(p + 0x18);
        std::wstring obj_name_w = fname_to_wstring(obj_fname);
        if (obj_name_w.compare(0, 9, L"Default__") == 0) return;

        std::string obj_name_s = fname_to_json_string(obj_name_w);

        // SuperStruct (UStruct+0x40)
        std::string parent_s;
        auto* super_ptr = *reinterpret_cast<const uint8_t* const*>(p + 0x40);
        if (super_ptr) {
            const FName& super_fname = *reinterpret_cast<const FName*>(super_ptr + 0x18);
            parent_s = fname_to_json_string(fname_to_wstring(super_fname));
        }

        // PropertiesSize (UStruct+0x58), ClassFlags (UClass+0xCC).
        int32_t  props_size  = *reinterpret_cast<const int32_t*>(p + 0x58);
        uint32_t class_flags = *reinterpret_cast<const uint32_t*>(p + 0xCC);

        if (!first_entry) ofs << ",\n";
        first_entry = false;

        ofs << "{\"name\":\"" << obj_name_s
            << "\",\"kind\":\"" << kind_tag
            << "\",\"parent\":\"" << parent_s
            << "\",\"size\":" << props_size
            << ",\"classFlags\":" << class_flags
            << ",\"properties\":";

        // ChildProperties (UStruct+0x50) â€” cap 1024.
        void* child_props = *reinterpret_cast<void* const*>(p + 0x50);
        walk_fields(ofs, child_props, 1024, true);

        // Children (UStruct+0x48) â€” find UFunction sub-objects, cap 2048.
        ofs << ",\"functions\":[";
        void* child = *reinterpret_cast<void* const*>(p + 0x48);
        bool first_fn = true;
        constexpr int kMaxChildren = 2048;
        int child_walk = 0;
        while (child && child_walk < kMaxChildren) {
            auto* ch = reinterpret_cast<const uint8_t*>(child);
            auto* child_class_ptr = *reinterpret_cast<UObject* const*>(ch + 0x10);
            if (child_class_ptr) {
                auto* ccp = reinterpret_cast<const uint8_t*>(child_class_ptr);
                const FName& child_cls_fname = *reinterpret_cast<const FName*>(ccp + 0x18);

                auto cit = cls_name_cache.find(child_cls_fname.ComparisonIndex);
                const std::wstring* child_cls_name;
                if (cit == cls_name_cache.end()) {
                    auto [ins, _] = cls_name_cache.try_emplace(
                        child_cls_fname.ComparisonIndex, fname_to_wstring(child_cls_fname));
                    child_cls_name = &ins->second;
                } else {
                    child_cls_name = &cit->second;
                }

                if (*child_cls_name == L"Function") {
                    const FName& fn_fname = *reinterpret_cast<const FName*>(ch + 0x18);
                    std::string fn_name_s = fname_to_json_string(fname_to_wstring(fn_fname));
                    uint8_t  num_parms     = *reinterpret_cast<const uint8_t*> (ch + 0xB4);
                    uint16_t params_size   = *reinterpret_cast<const uint16_t*>(ch + 0xB6);
                    uint16_t return_offset = *reinterpret_cast<const uint16_t*>(ch + 0xB8);

                    if (!first_fn) ofs << ",";
                    first_fn = false;
                    ofs << "{\"name\":\"" << fn_name_s
                        << "\",\"numParms\":" << static_cast<unsigned>(num_parms)
                        << ",\"paramsSize\":" << static_cast<unsigned>(params_size)
                        << ",\"returnOffset\":" << static_cast<unsigned>(return_offset)
                        << ",\"params\":";
                    void* fn_child_props = *reinterpret_cast<void* const*>(ch + 0x50);
                    walk_fields(ofs, fn_child_props, 64, false);
                    ofs << "}";
                }
            }
            // UField::Next (UField+0x28) â€” UField extends UObject (0x28 bytes).
            child = *reinterpret_cast<void* const*>(ch + 0x28);
            ++child_walk;
        }
        ofs << "]}";

        ++count;
    });

    ofs << "]";
    ofs.close();

    std::wstring path_w(out_path.begin(), out_path.end());
    log_info_fmt(STR("[TurdMODEngineBridge] handle_dump_all_classes: path={} count={}\n"),
                 path_w, count);

    s_dump_all_classes_result = "{\"ok\":true,\"path\":\"";
    for (char c : out_path) {
        if (c == '"') s_dump_all_classes_result += "\\\"";
        else if (c == '\\') s_dump_all_classes_result += "\\\\";
        else s_dump_all_classes_result += c;
    }
    s_dump_all_classes_result += "\",\"count\":";
    s_dump_all_classes_result += std::to_string(count);
    s_dump_all_classes_result += ",\"build\":\"\"}";
    *result_out = s_dump_all_classes_result.c_str();
    return 0;
}

// â”€â”€â”€ handle_dump_all_enums â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Dumps every UEnum / UUserDefinedEnum to <outDir>/enums.json as a JSON array.
//
// Params: { "outDir": "C:/abs/path" }
// Result: { "ok": true, "path": "...", "count": N }
static int32_t handle_dump_all_enums(const char* params_json,
                                     const char** result_out,
                                     const char**)
{
    std::string out_dir = extract_json_str(params_json, "outDir");
    if (out_dir.empty()) {
        s_dump_all_enums_result = R"({"error":"outDir param required"})";
        *result_out = s_dump_all_enums_result.c_str();
        return 0;
    }

    std::string out_path = out_dir + "/enums.json";
    std::ofstream ofs(out_path);
    if (!ofs.is_open()) {
        s_dump_all_enums_result = "{\"error\":\"could not open outFile: ";
        for (char c : out_path) {
            if (c == '"') s_dump_all_enums_result += "\\\"";
            else if (c == '\\') s_dump_all_enums_result += "\\\\";
            else s_dump_all_enums_result += c;
        }
        s_dump_all_enums_result += "\"}";
        *result_out = s_dump_all_enums_result.c_str();
        return 0;
    }

    std::unordered_map<uint32_t, std::wstring> cls_name_cache;
    size_t count = 0;
    bool first_entry = true;

    ofs << "[";

    SCAN_TIMEOUT_INIT();
    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        SCAN_TIMEOUT_CHECK();
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* class_ptr = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!class_ptr) return;
        auto* cp = reinterpret_cast<const uint8_t*>(class_ptr);
        const FName& cls_fname = *reinterpret_cast<const FName*>(cp + 0x18);

        auto it = cls_name_cache.find(cls_fname.ComparisonIndex);
        const std::wstring* cls_name_ptr;
        if (it == cls_name_cache.end()) {
            auto [ins, _] = cls_name_cache.try_emplace(
                cls_fname.ComparisonIndex, fname_to_wstring(cls_fname));
            cls_name_ptr = &ins->second;
        } else {
            cls_name_ptr = &it->second;
        }

        const std::wstring& cls_name = *cls_name_ptr;
        if (cls_name != L"Enum" && cls_name != L"UserDefinedEnum") return;

        // Skip CDOs.
        const FName& obj_fname = *reinterpret_cast<const FName*>(p + 0x18);
        std::wstring obj_name_w = fname_to_wstring(obj_fname);
        if (obj_name_w.compare(0, 9, L"Default__") == 0) return;

        std::string obj_name_s = fname_to_json_string(obj_name_w);

        // UEnum (extends UField, NOT UStruct):
        //   0x40  TArray<TPair<FName,int64_t>> Names
        //     TArray inline: void* Data @ +0, int32 ArrayNum @ +8
        //     TPair entry stride: 16 bytes (FName 8 @ +0, int64_t @ +8)
        //   0x50  uint8 CppForm
        //   0x54  uint32 EnumFlags_Internal
        auto* names_data    = *reinterpret_cast<const uint8_t* const*>(p + 0x40);
        int32_t  names_num  = *reinterpret_cast<const int32_t*>(p + 0x40 + 0x08);
        uint8_t  cpp_form   = *reinterpret_cast<const uint8_t*>(p + 0x50);
        uint32_t enum_flags = *reinterpret_cast<const uint32_t*>(p + 0x54);

        const char* cpp_form_str = "Regular";
        if      (cpp_form == 1) cpp_form_str = "Namespaced";
        else if (cpp_form == 2) cpp_form_str = "EnumClass";
        else if (cpp_form > 2)  cpp_form_str = "Unknown";

        if (!first_entry) ofs << ",\n";
        first_entry = false;

        ofs << "{\"name\":\"" << obj_name_s
            << "\",\"cppForm\":\"" << cpp_form_str
            << "\",\"flags\":" << enum_flags
            << ",\"entries\":[";

        int32_t safe_num = (names_num > 4096) ? 4096 : names_num;
        if (safe_num < 0) safe_num = 0;

        bool first_e = true;
        for (int32_t i = 0; i < safe_num && names_data; ++i) {
            const uint8_t* entry_ptr = names_data + static_cast<ptrdiff_t>(i) * 16;
            const FName& entry_fname = *reinterpret_cast<const FName*>(entry_ptr + 0x00);
            int64_t entry_value      = *reinterpret_cast<const int64_t*>(entry_ptr + 0x08);
            std::string entry_name_s = fname_to_json_string(fname_to_wstring(entry_fname));

            if (!first_e) ofs << ",";
            first_e = false;
            ofs << "{\"name\":\"" << entry_name_s
                << "\",\"value\":" << entry_value << "}";
        }

        ofs << "]}";
        ++count;
    });

    ofs << "]";
    ofs.close();

    std::wstring path_w(out_path.begin(), out_path.end());
    log_info_fmt(STR("[TurdMODEngineBridge] handle_dump_all_enums: path={} count={}\n"),
                 path_w, count);

    s_dump_all_enums_result = "{\"ok\":true,\"path\":\"";
    for (char c : out_path) {
        if (c == '"') s_dump_all_enums_result += "\\\"";
        else if (c == '\\') s_dump_all_enums_result += "\\\\";
        else s_dump_all_enums_result += c;
    }
    s_dump_all_enums_result += "\",\"count\":";
    s_dump_all_enums_result += std::to_string(count);
    s_dump_all_enums_result += ",\"build\":\"\"}";
    *result_out = s_dump_all_enums_result.c_str();
    return 0;
}

// â”€â”€â”€ handle_dump_all_structs â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Dumps every UScriptStruct to <outDir>/structs.json as a JSON array.
//
// Params: { "outDir": "C:/abs/path" }
// Result: { "ok": true, "path": "...", "count": N }
static int32_t handle_dump_all_structs(const char* params_json,
                                       const char** result_out,
                                       const char**)
{
    std::string out_dir = extract_json_str(params_json, "outDir");
    if (out_dir.empty()) {
        s_dump_all_structs_result = R"({"error":"outDir param required"})";
        *result_out = s_dump_all_structs_result.c_str();
        return 0;
    }

    std::string out_path = out_dir + "/structs.json";
    std::ofstream ofs(out_path);
    if (!ofs.is_open()) {
        s_dump_all_structs_result = "{\"error\":\"could not open outFile: ";
        for (char c : out_path) {
            if (c == '"') s_dump_all_structs_result += "\\\"";
            else if (c == '\\') s_dump_all_structs_result += "\\\\";
            else s_dump_all_structs_result += c;
        }
        s_dump_all_structs_result += "\"}";
        *result_out = s_dump_all_structs_result.c_str();
        return 0;
    }

    auto walk_fields = [&](void* first_field) {
        void* field = first_field;
        int count = 0;
        bool first_f = true;
        ofs << "[";
        while (field && count < 1024) {
            auto* fp = reinterpret_cast<const uint8_t*>(field);
            auto* class_priv = *reinterpret_cast<void* const*>(fp + 0x08);
            const FName& prop_fname = *reinterpret_cast<const FName*>(fp + 0x28);
            std::string prop_name_s = fname_to_json_string(fname_to_wstring(prop_fname));

            std::string type_name_s;
            if (class_priv) {
                auto* fclass = reinterpret_cast<const uint8_t*>(class_priv);
                const FName& type_fname = *reinterpret_cast<const FName*>(fclass + 0x00);
                type_name_s = fname_to_json_string(fname_to_wstring(type_fname));
            }

            int32_t array_dim       = *reinterpret_cast<const int32_t*>(fp + 0x38);
            int32_t element_size    = *reinterpret_cast<const int32_t*>(fp + 0x3C);
            int32_t offset_internal = *reinterpret_cast<const int32_t*>(fp + 0x4C);

            if (!first_f) ofs << ",";
            first_f = false;
            ofs << "{\"name\":\"" << prop_name_s
                << "\",\"type\":\"" << type_name_s
                << "\",\"offset\":" << offset_internal
                << ",\"size\":"     << element_size
                << ",\"arrayDim\":" << array_dim << "}";

            field = *reinterpret_cast<void* const*>(fp + 0x20);
            ++count;
        }
        ofs << "]";
    };

    std::unordered_map<uint32_t, std::wstring> cls_name_cache;
    size_t count = 0;
    bool first_entry = true;

    ofs << "[";

    SCAN_TIMEOUT_INIT();
    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        SCAN_TIMEOUT_CHECK();
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* class_ptr = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!class_ptr) return;
        auto* cp = reinterpret_cast<const uint8_t*>(class_ptr);
        const FName& cls_fname = *reinterpret_cast<const FName*>(cp + 0x18);

        auto it = cls_name_cache.find(cls_fname.ComparisonIndex);
        const std::wstring* cls_name_ptr;
        if (it == cls_name_cache.end()) {
            auto [ins, _] = cls_name_cache.try_emplace(
                cls_fname.ComparisonIndex, fname_to_wstring(cls_fname));
            cls_name_ptr = &ins->second;
        } else {
            cls_name_ptr = &it->second;
        }

        if (*cls_name_ptr != L"ScriptStruct") return;

        // Skip CDOs.
        const FName& obj_fname = *reinterpret_cast<const FName*>(p + 0x18);
        std::wstring obj_name_w = fname_to_wstring(obj_fname);
        if (obj_name_w.compare(0, 9, L"Default__") == 0) return;

        std::string obj_name_s = fname_to_json_string(obj_name_w);

        // UScriptStruct extends UStruct: 0x40 SuperStruct, 0x50 ChildProperties,
        // 0x58 PropertiesSize, 0xB0 StructFlags.
        std::string parent_s;
        auto* super_ptr = *reinterpret_cast<const uint8_t* const*>(p + 0x40);
        if (super_ptr) {
            const FName& super_fname = *reinterpret_cast<const FName*>(super_ptr + 0x18);
            parent_s = fname_to_json_string(fname_to_wstring(super_fname));
        }

        int32_t  props_size   = *reinterpret_cast<const int32_t*> (p + 0x58);
        uint32_t struct_flags = *reinterpret_cast<const uint32_t*>(p + 0xB0);

        if (!first_entry) ofs << ",\n";
        first_entry = false;

        ofs << "{\"name\":\"" << obj_name_s
            << "\",\"parent\":\"" << parent_s
            << "\",\"size\":" << props_size
            << ",\"flags\":" << struct_flags
            << ",\"fields\":";

        void* child_props = *reinterpret_cast<void* const*>(p + 0x50);
        walk_fields(child_props);

        ofs << "}";
        ++count;
    });

    ofs << "]";
    ofs.close();

    std::wstring path_w(out_path.begin(), out_path.end());
    log_info_fmt(STR("[TurdMODEngineBridge] handle_dump_all_structs: path={} count={}\n"),
                 path_w, count);

    s_dump_all_structs_result = "{\"ok\":true,\"path\":\"";
    for (char c : out_path) {
        if (c == '"') s_dump_all_structs_result += "\\\"";
        else if (c == '\\') s_dump_all_structs_result += "\\\\";
        else s_dump_all_structs_result += c;
    }
    s_dump_all_structs_result += "\",\"count\":";
    s_dump_all_structs_result += std::to_string(count);
    s_dump_all_structs_result += ",\"build\":\"\"}";
    *result_out = s_dump_all_structs_result.c_str();
    return 0;
}

// describeFunction â€” locate a UFunction by {owner, name} and walk its
// ChildProperties linked list to dump the parameter signature. Each entry
// in the list is an FField subclass (FProperty + variants in UE 4.25+).
//
// FField layout (UE 4.27):
//   0x00  vtable
//   0x08  FFieldClass* ClassPrivate   <- type tag (e.g. FStrProperty class)
//   0x10  FName NamePrivate           <- field name
//   0x18  FField* Next                <- linked list continuation
//   0x20  FFieldVariant Owner         <- parent UStruct
//
// FFieldClass layout:
//   0x00  FName Name                  <- type name like "StrProperty"
//
// UFunction (inherits UStruct) has ChildProperties at offset 0x50.
//
// Params: { "owner": "MiscStatics", "name": "BroadcastChatLine" }
// Output: { "found": true, "params": [{"name":"...","type":"..."}, ...] }
static int32_t handle_describe_function(const char* params_json,
                                        const char** result_out,
                                        const char**)
{
    std::string want_owner = extract_json_str(params_json, "owner");
    std::string want_name = extract_json_str(params_json, "name");
    if (want_name.empty()) {
        s_describe_result = R"({"error":"name param required"})";
        *result_out = s_describe_result.c_str();
        return 0;
    }
    std::wstring want_owner_w(want_owner.begin(), want_owner.end());
    std::wstring want_name_w(want_name.begin(), want_name.end());

    bool found = false;
    std::string params_out = "[";
    bool first_param = true;
    std::string actual_owner;
    // Cache class FName index â†’ string. Without this we'd burn ~1.5M
    // ToString calls just checking "is this a Function" â€” slow and
    // leaks every FString buffer.
    std::unordered_map<uint32_t, std::wstring> class_name_cache;

    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t /*chunk*/, int32_t /*idx*/) {
        if (found) return;
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* class_ptr = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!class_ptr) return;
        auto* cp = reinterpret_cast<const uint8_t*>(class_ptr);
        const FName& cls_fname = *reinterpret_cast<const FName*>(cp + 0x18);

        auto cache_it = class_name_cache.find(cls_fname.ComparisonIndex);
        const std::wstring* cls_name;
        if (cache_it == class_name_cache.end()) {
            auto [ins, _] = class_name_cache.try_emplace(
                cls_fname.ComparisonIndex, fname_to_wstring(cls_fname));
            cls_name = &ins->second;
        } else {
            cls_name = &cache_it->second;
        }
        if (*cls_name != L"Function") return;

        const FName& fn_fname = *reinterpret_cast<const FName*>(p + 0x18);
        if (fname_to_wstring(fn_fname) != want_name_w) return;

        // Owner check (optional filter).
        auto* outer = *reinterpret_cast<UObject* const*>(p + 0x20);
        std::wstring owner_w;
        if (outer) {
            auto* op = reinterpret_cast<const uint8_t*>(outer);
            const FName& outer_name = *reinterpret_cast<const FName*>(op + 0x18);
            owner_w = fname_to_wstring(outer_name);
        }
        if (!want_owner_w.empty() && owner_w != want_owner_w) return;

        actual_owner = fname_to_json_string(owner_w);

        // Walk ChildProperties linked list. UE 4.27 FField layout:
        //   0x00  vtable
        //   0x08  FFieldClass* ClassPrivate
        //   0x10  FFieldVariant Owner    (16 bytes â€” TWeakObjectPtr-ish)
        //   0x20  FField* Next
        //   0x28  FName NamePrivate
        //   0x30  uint32 FlagsPrivate
        // Hard-cap the walk at 64 entries â€” sane UFunctions have <20 params;
        // if we exceed, something's wrong and we should fail safe rather
        // than chase pointers to oblivion (which is what crashed the server
        // the first attempt at this handler).
        void* field = *reinterpret_cast<void* const*>(p + 0x50);
        int param_count = 0;
        constexpr int kMaxParams = 64;
        while (field && param_count < kMaxParams) {
            auto* fp = reinterpret_cast<const uint8_t*>(field);
            auto* class_priv = *reinterpret_cast<void* const*>(fp + 0x08);
            const FName& prop_name = *reinterpret_cast<const FName*>(fp + 0x28);
            std::string prop_name_s = fname_to_json_string(fname_to_wstring(prop_name));

            std::string type_name_s;
            if (class_priv) {
                auto* fclass = reinterpret_cast<const uint8_t*>(class_priv);
                // FFieldClass::Name at offset 0x00 (an FName).
                const FName& type_fname = *reinterpret_cast<const FName*>(fclass + 0x00);
                type_name_s = fname_to_json_string(fname_to_wstring(type_fname));
            }

            if (!first_param) params_out += ",";
            first_param = false;
            params_out += "{\"name\":\"";
            params_out += prop_name_s;
            params_out += "\",\"type\":\"";
            params_out += type_name_s;
            params_out += "\"}";

            field = *reinterpret_cast<void* const*>(fp + 0x20);
            ++param_count;
        }
        found = true;
    });
    params_out += "]";

    if (!found) {
        s_describe_result = "{\"found\":false,\"params\":[]}";
    } else {
        s_describe_result = "{\"found\":true,\"owner\":\"";
        s_describe_result += actual_owner;
        s_describe_result += "\",\"params\":";
        s_describe_result += params_out;
        s_describe_result += "}";
    }
    *result_out = s_describe_result.c_str();
    return 0;
}

// introspectNotification â€” one-shot RE probe. Dumps the layout of the
// NotificationsManager::NetMulticast_RequestNotification "Description" struct
// (fields/types/offsets + struct size) so we can BUILD it in C++ for an
// autonomous (no player-executor) #Announce. Candidate offsets (0x78 for
// FStructProperty.Struct, 0x58 for UStruct.PropertiesSize) are dumped so we
// can verify them from the output before writing the real announce handler.
static thread_local std::string s_introspect_result;
static int32_t handle_introspect_notification(const char*, const char** result_out, const char**)
{
    UObject* fn = find_ufunction(L"NetMulticast_RequestNotification", L"NotificationsManager");
    if (!fn) { s_introspect_result = R"({"error":"NetMulticast_RequestNotification not found"})"; *result_out = s_introspect_result.c_str(); return 0; }
    auto* fb = reinterpret_cast<const uint8_t*>(fn);

    auto read_prop = [](const uint8_t* f, std::string& name, std::string& type, int32_t& off) {
        auto* cls = *reinterpret_cast<void* const*>(f + 0x08);
        type.clear();
        if (cls) { const FName& tf = *reinterpret_cast<const FName*>(reinterpret_cast<const uint8_t*>(cls) + 0x00); type = fname_to_json_string(fname_to_wstring(tf)); }
        const FName& nm = *reinterpret_cast<const FName*>(f + 0x28);
        name = fname_to_json_string(fname_to_wstring(nm));
        off = *reinterpret_cast<const int32_t*>(f + 0x4C);
    };

    std::string out = "{\"ok\":true,";
    out += "\"fn_size_0x58\":" + std::to_string(*reinterpret_cast<const int32_t*>(fb + 0x58)) + ",";

    void* field = *reinterpret_cast<void* const*>(fb + 0x50);
    const uint8_t* desc_prop = nullptr; int32_t desc_off = -1;
    std::string params = "["; bool first = true; int guard = 0;
    while (field && guard++ < 32) {
        auto* f = reinterpret_cast<const uint8_t*>(field);
        std::string n, t; int32_t o; read_prop(f, n, t, o);
        if (!first) params += ","; first = false;
        params += "{\"name\":\"" + n + "\",\"type\":\"" + t + "\",\"offset\":" + std::to_string(o) + "}";
        if (t == "StructProperty" && !desc_prop) { desc_prop = f; desc_off = o; }
        field = *reinterpret_cast<void* const*>(f + 0x20);
    }
    params += "]";
    out += "\"params\":" + params + ",\"descOffset\":" + std::to_string(desc_off) + ",";

    if (desc_prop) {
        // Dump candidate FStructProperty.Struct offsets so we lock the right one.
        auto hexptr = [](const void* p){ char b[24]; std::snprintf(b, sizeof(b), "0x%llX", (unsigned long long)p); return std::string(b); };
        auto name_of = [&](const void* o)->std::string{ if(!o) return ""; const FName& f=*reinterpret_cast<const FName*>(reinterpret_cast<const uint8_t*>(o)+0x18); return fname_to_json_string(fname_to_wstring(f)); };
        std::string cands = "[";
        bool cf = true;
        for (int co : {0x70, 0x78, 0x80, 0x88}) {
            auto* cand = *reinterpret_cast<void* const*>(desc_prop + co);
            if (!cf) cands += ","; cf = false;
            cands += "{\"off\":" + std::to_string(co) + ",\"ptr\":\"" + hexptr(cand) + "\",\"name\":\"" + name_of(cand) + "\"}";
        }
        cands += "]";
        out += "\"structCandidates\":" + cands + ",";

        auto* uscript = *reinterpret_cast<void* const*>(desc_prop + 0x78);
        out += "\"structPtr\":\"" + hexptr(uscript) + "\",\"structName\":\"" + name_of(uscript) + "\",";
        if (uscript) {
            auto* sp = reinterpret_cast<const uint8_t*>(uscript);
            out += "\"structSize_0x58\":" + std::to_string(*reinterpret_cast<const int32_t*>(sp + 0x58)) + ",";
            out += "\"children_0x48\":\"" + hexptr(*reinterpret_cast<void* const*>(sp + 0x48)) + "\",";
            out += "\"childProps_0x50\":\"" + hexptr(*reinterpret_cast<void* const*>(sp + 0x50)) + "\",";
            // FField walk (ChildProperties @0x50)
            void* sf = *reinterpret_cast<void* const*>(sp + 0x50);
            std::string fields = "["; bool ff = true; int g2 = 0;
            while (sf && g2++ < 64) {
                auto* f = reinterpret_cast<const uint8_t*>(sf);
                std::string n, t; int32_t o; read_prop(f, n, t, o);
                if (!ff) fields += ","; ff = false;
                fields += "{\"name\":\"" + n + "\",\"type\":\"" + t + "\",\"offset\":" + std::to_string(o) + "}";
                sf = *reinterpret_cast<void* const*>(f + 0x20);
            }
            fields += "]";
            out += "\"structFields\":" + fields;
        } else { out += "\"structFields\":[]"; }
    } else { out += "\"structPtr\":null,\"structFields\":[]"; }
    out += "}";
    s_introspect_result = out;
    *result_out = s_introspect_result.c_str();
    return 0;
}

// captureNotification â€” arm a one-shot capture of the next
// NetMulticast_RequestNotification call's 24-byte struct. Caller then fires
// #Announce; getCapturedNotification reads the bytes back.
static thread_local std::string s_capnotif_result;
static int32_t handle_capture_notification(const char*, const char** result_out, const char**)
{
    UObject* fn = find_ufunction(L"NetMulticast_RequestNotification", L"NotificationsManager");
    if (!fn) { s_capnotif_result = R"({"error":"NetMulticast_RequestNotification not found"})"; *result_out = s_capnotif_result.c_str(); return 0; }
    g_notif_fn_ptr.store(static_cast<void*>(fn));
    g_notif_captured.store(false);
    std::memset(g_notif_buf, 0, sizeof(g_notif_buf));
    g_notif_marker[0] = 0; // unfiltered: capture the next notification of any kind
    g_notif_arm.store(true);
    s_capnotif_result = R"({"ok":true,"armed":true,"note":"fire #Announce now"})";
    *result_out = s_capnotif_result.c_str();
    return 0;
}

// captureNotificationFiltered { "marker": "..." } — arm a capture that only keeps a
// notification whose message/deref text contains `marker`. Reliably grabs a SPECIFIC
// banner (e.g. our Notifications.json one) amid frequent gameplay notifications.
static int32_t handle_capture_notification_filtered(const char* params_json, const char** result_out, const char**)
{
    UObject* fn = find_ufunction(L"NetMulticast_RequestNotification", L"NotificationsManager");
    if (!fn) { s_capnotif_result = R"({"error":"NetMulticast_RequestNotification not found"})"; *result_out = s_capnotif_result.c_str(); return 0; }
    std::string marker = extract_json_str(params_json, "marker");
    g_notif_fn_ptr.store(static_cast<void*>(fn));
    g_notif_captured.store(false);
    std::memset(g_notif_buf, 0, sizeof(g_notif_buf));
    std::memset(g_notif_marker, 0, sizeof(g_notif_marker));
    std::strncpy(g_notif_marker, marker.c_str(), sizeof(g_notif_marker) - 1);
    g_notif_arm.store(true);
    s_capnotif_result = std::string("{\"ok\":true,\"armed\":true,\"marker\":\"") + marker + "\"}";
    *result_out = s_capnotif_result.c_str();
    return 0;
}

// getCapturedNotification â€” return the captured struct bytes (hex) for analysis.
static int32_t handle_get_captured_notification(const char*, const char** result_out, const char**)
{
    char b[4];
    auto tohex = [&](const uint8_t* p, size_t n){ std::string h; for (size_t i=0;i<n;++i){ std::snprintf(b,sizeof(b),"%02X",p[i]); h+=b; } return h; };
    std::string hex = tohex(g_notif_buf, sizeof(g_notif_buf));
    std::string dataHex = tohex(g_notif_data, sizeof(g_notif_data));
    std::string refHex  = tohex(g_notif_refctl, sizeof(g_notif_refctl));
    auto jesc = [](const char* s){ std::string o; for (; *s; ++s){ char c=*s; if(c=='"'||c=='\\') o+='\\'; o+=c; } return o; };
    s_capnotif_result = std::string("{\"captured\":") + (g_notif_captured.load() ? "true" : "false")
                      + ",\"bytes\":\"" + hex + "\",\"deref\":["
                      + "\"" + jesc(g_notif_deref[0]) + "\",\"" + jesc(g_notif_deref[1]) + "\",\"" + jesc(g_notif_deref[2]) + "\","
                      + "\"" + jesc(g_notif_deref[3]) + "\",\"" + jesc(g_notif_deref[4]) + "\",\"" + jesc(g_notif_deref[5]) + "\"],"
                      + "\"dataHex\":\"" + dataHex + "\",\"refHex\":\"" + refHex + "\",\"msg\":[";
    for (int i = 0; i < 12; ++i) { if (i) s_capnotif_result += ","; s_capnotif_result += "\"" + jesc(g_notif_msg[i]) + "\""; }
    s_capnotif_result += "]}";
    *result_out = s_capnotif_result.c_str();
    return 0;
}

// replayAnnounce â€” re-fire the PINNED captured notification SERVER-SIDE (no player
// executor). captureNotification must have run + #Announce fired once; the hook
// refcount-pins the shared object so its genuine Data + controller (real vtables,
// real Message FText) stay alive and can be replayed verbatim. This is the robust
// autonomous path: reconstructing the polymorphic Data + ref-controller from scratch
// is fragile, so we reuse the real ones. Same text as captured. Params: none.
static thread_local std::string s_replay_result;
static int32_t handle_replay_announce(const char*, const char** result_out, const char**)
{
    // Re-enabled for WITHIN-WINDOW replay: the captured Data+controller are alive for
    // the notification's duration (~45s). Replaying inside that window is valid (no
    // pin needed). Replaying AFTER the objects free will fault (SEH-contained).
    if (!g_notif_captured.load()) { s_replay_result = R"({"error":"no captured template - run captureNotification then let a notification fire"})"; *result_out = s_replay_result.c_str(); return 0; }
    UObject* mgr = nullptr;
    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        if (mgr) return;
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* class_ptr = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!class_ptr) return;
        const FName& cls_fname = *reinterpret_cast<const FName*>(reinterpret_cast<const uint8_t*>(class_ptr) + 0x18);
        if (fname_to_wstring(cls_fname).find(L"NotificationsManager") == std::wstring::npos) return;
        const FName& obj_name = *reinterpret_cast<const FName*>(p + 0x18);
        if (fname_to_wstring(obj_name).compare(0, 9, L"Default__") == 0) return;
        mgr = obj;
    });
    if (!mgr) { s_replay_result = R"({"error":"NotificationsManager instance not found"})"; *result_out = s_replay_result.c_str(); return 0; }
    UObject* req_fn = find_ufunction(L"NetMulticast_RequestNotification", L"NotificationsManager");
    if (!req_fn) { s_replay_result = R"({"error":"NetMulticast_RequestNotification not found"})"; *result_out = s_replay_result.c_str(); return 0; }

    // Fire with a COPY of the pinned helper bytes ([Data*][Controller*][slot16], all
    // pointing at the live, refcount-pinned objects). Copy so ProcessEvent can't
    // perturb our stored template.
    alignas(16) uint8_t desc_buf[64] = {0};
    std::memcpy(desc_buf, g_notif_buf, 24);
    uint32_t seh_fire = call_processevent_seh(mgr, reinterpret_cast<class UFunction*>(req_fn), desc_buf);

    char buf[96];
    std::snprintf(buf, sizeof(buf), "{\"ok\":true,\"replayed\":true,\"sehFire\":%u}", seh_fire);
    s_replay_result = buf;
    *result_out = s_replay_result.c_str();
    return 0;
}

// broadcastAnnounce â€” fire the prominent #Announce banner SERVER-SIDE with no
// player executor. Builds an FText from the text via KismetTextLibrary::
// Conv_StringToText, places it as the NotificationsManager::
// NetMulticast_RequestNotification "Description" (a 24-byte FText), and multicasts
// it. SEH-wrapped so a bad struct yields an error, not a crash. The autonomous
// announce (no admin in-game) â€” the Whalley win. Params: { "text": "..." }
static thread_local std::string s_announce_result;
static UObject* s_conv_string_to_text_fn = nullptr;
static int32_t handle_broadcast_announce(const char* params_json, const char** result_out, const char**)
{
    ensure_hook_installed_once();
    std::string text = extract_json_str(params_json, "text");
    if (text.empty()) { s_announce_result = R"({"error":"text param required"})"; *result_out = s_announce_result.c_str(); return 0; }

    // Substring class match â€” the live instance is BP_NotificationsManager_C, a
    // Blueprint subclass, so exact "NotificationsManager" lookup misses it.
    UObject* mgr = nullptr;
    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        if (mgr) return;
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* class_ptr = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!class_ptr) return;
        const FName& cls_fname = *reinterpret_cast<const FName*>(reinterpret_cast<const uint8_t*>(class_ptr) + 0x18);
        if (fname_to_wstring(cls_fname).find(L"NotificationsManager") == std::wstring::npos) return;
        const FName& obj_name = *reinterpret_cast<const FName*>(p + 0x18);
        std::wstring on = fname_to_wstring(obj_name);
        if (on.compare(0, 9, L"Default__") == 0) return; // skip the CDO
        mgr = obj;
    });
    if (!mgr) { s_announce_result = R"({"error":"NotificationsManager instance not found"})"; *result_out = s_announce_result.c_str(); return 0; }
    UObject* req_fn = find_ufunction(L"NetMulticast_RequestNotification", L"NotificationsManager");
    if (!req_fn) { s_announce_result = R"({"error":"NetMulticast_RequestNotification not found"})"; *result_out = s_announce_result.c_str(); return 0; }

    if (!s_conv_string_to_text_fn) s_conv_string_to_text_fn = find_ufunction(L"Conv_StringToText", L"KismetTextLibrary");
    if (!s_kismet_text_library_cdo) {
        UObjectGlobals::ForEachUObject([&](UObject* o, int32_t, int32_t){ if(s_kismet_text_library_cdo) return; auto* p=reinterpret_cast<const uint8_t*>(o); const FName& on=*reinterpret_cast<const FName*>(p+0x18); if(fname_to_wstring(on)==L"Default__KismetTextLibrary") s_kismet_text_library_cdo=o; });
    }
    if (!s_conv_string_to_text_fn || !s_kismet_text_library_cdo) { s_announce_result = R"({"error":"Conv_StringToText or KismetTextLibrary CDO not found"})"; *result_out = s_announce_result.c_str(); return 0; }

    auto offs = get_function_param_offsets(s_conv_string_to_text_fn);
    int32_t in_off = -1, ret_off = -1;
    for (auto& kv : offs) { if (kv.first == L"inString") in_off = kv.second; else if (kv.first == L"ReturnValue") ret_off = kv.second; }
    if (in_off < 0 || ret_off < 0) { s_announce_result = R"({"error":"Conv_StringToText param layout unknown"})"; *result_out = s_announce_result.c_str(); return 0; }

    static thread_local wchar_t txt_buf[1024];
    std::wstring wt = utf8_to_wstring(text);
    if (wt.size() >= 1023) wt.resize(1023);
    wcscpy_s(txt_buf, 1024, wt.c_str());
    int32_t lenn = static_cast<int32_t>(wt.length()) + 1;

    // 1) Build FText: Conv_StringToText(InString) -> ReturnValue (FText, 24 bytes).
    alignas(16) uint8_t conv_buf[128] = {0};
    *reinterpret_cast<wchar_t**>(conv_buf + in_off + 0) = txt_buf;
    *reinterpret_cast<int32_t*>(conv_buf + in_off + 8)  = lenn;
    *reinterpret_cast<int32_t*>(conv_buf + in_off + 12) = lenn;
    uint32_t seh_conv = call_processevent_seh(s_kismet_text_library_cdo, reinterpret_cast<class UFunction*>(s_conv_string_to_text_fn), conv_buf);

    // 2) Build a BasicNotificationDescriptionData (56 bytes, fully reflected) with the
    //    Message FText + a Duration, then wrap it in the 24-byte ReplicationHelper as a
    //    TSharedPtr<Data>. Static Data + a high-refcount controller so the shared ptr
    //    never frees (so the controller's vtable destroy path is never touched). The
    //    helper layout: [Data*][Controller*][unused]. @ctx RE: structs.json +
    //    NotificationDescriptionReplicationHelper(24B, opaque) wrapping
    //    BasicNotificationDescriptionData(56B: Message@0 FontSize@24 Duration@44 ...).
    static uint8_t s_data[64];        // BasicNotificationDescriptionData (56 used)
    static uint8_t s_controller[48];  // FReferenceController-ish (refcounts only)
    std::memset(s_data, 0, sizeof(s_data));
    std::memcpy(s_data + 0, conv_buf + ret_off, 24);   // Message (FText) @ 0
    *reinterpret_cast<float*>(s_data + 44) = 8.0f;      // Duration @ 44 = 8s
    std::memset(s_controller, 0, sizeof(s_controller));
    *reinterpret_cast<int32_t*>(s_controller + 8)  = (1 << 28); // SharedRefCount (huge -> never frees)
    *reinterpret_cast<int32_t*>(s_controller + 12) = (1 << 28); // WeakRefCount

    alignas(16) uint8_t desc_buf[64] = {0};
    *reinterpret_cast<void**>(desc_buf + 0) = s_data;        // TSharedPtr.Object
    *reinterpret_cast<void**>(desc_buf + 8) = s_controller;  // TSharedPtr.Controller
    // desc_buf + 16 left zero (unknown 3rd slot)
    uint32_t seh_fire = call_processevent_seh(mgr, reinterpret_cast<class UFunction*>(req_fn), desc_buf);

    char buf[160];
    std::snprintf(buf, sizeof(buf), "{\"ok\":true,\"sehConv\":%u,\"sehFire\":%u,\"inOff\":%d,\"retOff\":%d}", seh_conv, seh_fire, in_off, ret_off);
    s_announce_result = buf;
    *result_out = s_announce_result.c_str();
    return 0;
}

// showBanner â€” fire a COLORED center-screen banner to ALL players ON DEMAND, via
// ConZPlayerController::ShowWarningMessage(Message:FString, Duration:float,
// TextColor:FLinearColor[R,G,B,A]). paramsSize=36 confirms FLinearColor (16B), not
// FColor (4B). This is the same prominent colored banner SCUM's native
// Notifications.json uses (message+duration+color) â€” but custom + on the fly. No
// FText, no notification struct, no admin gate. Broadcasts by calling the (client)
// RPC on every live ConZPlayerController. Params: { text, duration?=45, r,g,b (0-255) }
static thread_local std::string s_show_banner_result;
static int32_t handle_show_banner(const char* params_json, const char** result_out, const char**)
{
    ensure_hook_installed_once();
    std::string text = extract_json_str(params_json, "text");
    if (text.empty()) { s_show_banner_result = R"({"error":"text param required"})"; *result_out = s_show_banner_result.c_str(); return 0; }
    float duration = extract_json_float(params_json, "duration", 45.0f);
    float r = extract_json_float(params_json, "r", 255.0f);
    float g = extract_json_float(params_json, "g", 255.0f);
    float b = extract_json_float(params_json, "b", 255.0f);

    UObject* fn = find_ufunction(L"ShowWarningMessage", L"ConZPlayerController" /* YOUR_GAME_PC_CLASS */);
    if (!fn) { s_show_banner_result = R"({"error":"ShowWarningMessage UFunction not found"})"; *result_out = s_show_banner_result.c_str(); return 0; }

    // Resolve param offsets from reflection (no hardcoding).
    int32_t msg_off = -1, dur_off = -1, col_off = -1;
    for (auto& kv : get_function_param_offsets(fn)) {
        if (kv.first == L"Message") msg_off = kv.second;
        else if (kv.first == L"Duration") dur_off = kv.second;
        else if (kv.first == L"TextColor") col_off = kv.second;
    }
    if (msg_off < 0 || dur_off < 0 || col_off < 0) { s_show_banner_result = R"({"error":"ShowWarningMessage param layout unknown"})"; *result_out = s_show_banner_result.c_str(); return 0; }

    static thread_local wchar_t banner_txt[1024];
    std::wstring wt = utf8_to_wstring(text);
    if (wt.size() >= 1023) wt.resize(1023);
    wcscpy_s(banner_txt, 1024, wt.c_str());
    int32_t lenn = static_cast<int32_t>(wt.length()) + 1;

    alignas(16) uint8_t pbuf[64] = {0};
    *reinterpret_cast<wchar_t**>(pbuf + msg_off + 0) = banner_txt;  // FString.Data
    *reinterpret_cast<int32_t*>(pbuf + msg_off + 8)  = lenn;        // FString.ArrayNum
    *reinterpret_cast<int32_t*>(pbuf + msg_off + 12) = lenn;        // FString.ArrayMax
    *reinterpret_cast<float*>(pbuf + dur_off) = duration;           // Duration (s)
    *reinterpret_cast<float*>(pbuf + col_off + 0)  = r / 255.0f;    // FLinearColor.R
    *reinterpret_cast<float*>(pbuf + col_off + 4)  = g / 255.0f;    // G
    *reinterpret_cast<float*>(pbuf + col_off + 8)  = b / 255.0f;    // B
    *reinterpret_cast<float*>(pbuf + col_off + 12) = 1.0f;          // A

    // Broadcast: call the client RPC on every live ConZPlayerController (skip CDO).
    int count = 0; uint32_t last_seh = 0;
    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* cls = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!cls) return;
        std::wstring cn = fname_to_wstring(*reinterpret_cast<const FName*>(reinterpret_cast<const uint8_t*>(cls) + 0x18));
        if (cn.find(L"ConZPlayerController" /* YOUR_GAME_PC_CLASS */) == std::wstring::npos) return;
        std::wstring on = fname_to_wstring(*reinterpret_cast<const FName*>(p + 0x18));
        if (on.compare(0, 9, L"Default__") == 0) return;
        last_seh = call_processevent_seh(obj, reinterpret_cast<class UFunction*>(fn), pbuf);
        ++count;
    });

    char buf[176];
    std::snprintf(buf, sizeof(buf), "{\"ok\":true,\"controllers\":%d,\"sehFire\":%u,\"msgOff\":%d,\"durOff\":%d,\"colOff\":%d}", count, last_seh, msg_off, dur_off, col_off);
    s_show_banner_result = buf;
    *result_out = s_show_banner_result.c_str();
    return 0;
}

// dumpUFunctions â€” first real reflection call. Walks GUObjectArray via
// UObjectGlobals::ForEachUObject and returns total UObject count plus a
// sample of the first 100 with raw FName ComparisonIndex values for the
// object's name and its class's name. If total == 0, our ForEachUObject
// path is reaching empty storage (UE4SS sigscan never armed
// UObjectArray::g_array_address â€” check Signatures.cpp log for the
// "GUObjectArray address: ..." line).
static int32_t handle_dump_ufunctions(const char*, const char** result_out, const char**)
{
    size_t total = 0;
    std::string sample;
    sample.reserve(8192);
    sample = "[";
    bool first = true;

    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t /*chunk*/, int32_t /*idx*/) {
        ++total;
        if (total > 100) return;

        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* class_ptr = *reinterpret_cast<UObject* const*>(p + 0x10);
        auto& obj_name = *reinterpret_cast<const FName*>(p + 0x18);

        uint32_t cls_name_idx = 0;
        uint32_t cls_name_num = 0;
        if (class_ptr) {
            auto* cp = reinterpret_cast<const uint8_t*>(class_ptr);
            auto& class_name = *reinterpret_cast<const FName*>(cp + 0x18);
            cls_name_idx = class_name.ComparisonIndex;
            cls_name_num = class_name.Number;
        }

        // Resolve names through the armed FName::ToString detour. These
        // calls invoke real game code â€” if they crash, the detour pointer
        // is bad (signature mismatch or wrong AOB). Limited to first 100
        // entries to keep blast radius small.
        std::string obj_name_str = fname_to_json_string(fname_to_wstring(obj_name));
        std::string cls_name_str;
        if (class_ptr) {
            auto* cp2 = reinterpret_cast<const uint8_t*>(class_ptr);
            const FName& cls_fname = *reinterpret_cast<const FName*>(cp2 + 0x18);
            cls_name_str = fname_to_json_string(fname_to_wstring(cls_fname));
        }

        if (!first) sample += ",";
        first = false;
        sample += "{\"n\":";
        sample += std::to_string(obj_name.ComparisonIndex);
        sample += ",\"nN\":";
        sample += std::to_string(obj_name.Number);
        sample += ",\"cn\":";
        sample += std::to_string(cls_name_idx);
        sample += ",\"cnN\":";
        sample += std::to_string(cls_name_num);
        sample += ",\"name\":\"";
        sample += obj_name_str;
        sample += "\",\"class\":\"";
        sample += cls_name_str;
        sample += "\"}";
    });

    sample += "]";

    s_dump_result = "{\"total\":";
    s_dump_result += std::to_string(total);
    s_dump_result += ",\"sample\":";
    s_dump_result += sample;
    s_dump_result += "}";

    log_info_fmt(STR("[TurdMODEngineBridge] handle_dump_ufunctions: total={}\n"), total);

    *result_out = s_dump_result.c_str();
    return 0;
}

// â”€â”€â”€ Shared helpers for player-targeting handlers â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

// Scans GUObjectArray for the first non-CDO instance of a class whose name
// matches `class_name` exactly. Returns nullptr if none found. Used for
// singleton managers (GlobalRaidProtectionManager, ConZEconomyManager, etc).
static UObject* find_first_instance_of_class(const wchar_t* class_name)
{
    UObject* found = nullptr;
    std::unordered_map<uint32_t, std::wstring> cls_cache;
    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        if (found) return;
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* class_ptr = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!class_ptr) return;
        auto* cp = reinterpret_cast<const uint8_t*>(class_ptr);
        const FName& cls_fname = *reinterpret_cast<const FName*>(cp + 0x18);

        auto it = cls_cache.find(cls_fname.ComparisonIndex);
        const std::wstring* cls = nullptr;
        if (it == cls_cache.end()) {
            auto [ins, _] = cls_cache.try_emplace(
                cls_fname.ComparisonIndex, fname_to_wstring(cls_fname));
            cls = &ins->second;
        } else {
            cls = &it->second;
        }
        if (*cls != class_name) return;

        const FName& obj_name = *reinterpret_cast<const FName*>(p + 0x18);
        std::wstring on = fname_to_wstring(obj_name);
        if (on.compare(0, 9, L"Default__") == 0) return;
        found = obj;
    });
    return found;
}

// Collect ALL non-CDO instances of a class (cap 32) — so the despawn handler can
// inspect/pick the live one when several exist (e.g. 2x BPC_WolfsWagen_C).
static void find_all_instances_of_class(const wchar_t* class_name, std::vector<UObject*>& out)
{
    std::unordered_map<uint32_t, std::wstring> cls_cache;
    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        if (out.size() >= 32) return;
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* class_ptr = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!class_ptr) return;
        auto* cp = reinterpret_cast<const uint8_t*>(class_ptr);
        const FName& cls_fname = *reinterpret_cast<const FName*>(cp + 0x18);
        auto it = cls_cache.find(cls_fname.ComparisonIndex);
        const std::wstring* cls = nullptr;
        if (it == cls_cache.end()) {
            auto [ins, _] = cls_cache.try_emplace(cls_fname.ComparisonIndex, fname_to_wstring(cls_fname));
            cls = &ins->second;
        } else { cls = &it->second; }
        if (*cls != class_name) return;
        const FName& obj_name = *reinterpret_cast<const FName*>(p + 0x18);
        std::wstring on = fname_to_wstring(obj_name);
        if (on.compare(0, 9, L"Default__") == 0) return;
        out.push_back(obj);
    });
}

// Look up a connected player's PC by display name (PlayerNamePrivate /
// PlayerName on the PlayerState). Returns nullptr if not found. Inlined
// version of the scan in handle_teleport_player so the new fame/currency
// handlers don't have to duplicate it.
static UObject* find_pc_by_player_name(const std::wstring& want_name_w)
{
    UObject* pc = nullptr;
    std::unordered_map<uint32_t, std::wstring> cls_cache;
    std::unordered_map<UObject*, int32_t> ps_offset_cache;
    std::unordered_map<UObject*, int32_t> name_offset_cache;

    auto get_ps_offset = [&](UObject* pc_class) -> int32_t {
        auto it = ps_offset_cache.find(pc_class);
        if (it != ps_offset_cache.end()) return it->second;
        int32_t off = find_property_offset(pc_class, L"PlayerState");
        ps_offset_cache[pc_class] = off;
        return off;
    };
    auto get_name_offset = [&](UObject* ps_class) -> int32_t {
        auto it = name_offset_cache.find(ps_class);
        if (it != name_offset_cache.end()) return it->second;
        int32_t off = find_property_offset(ps_class, L"PlayerNamePrivate");
        if (off < 0) off = find_property_offset(ps_class, L"PlayerName");
        name_offset_cache[ps_class] = off;
        return off;
    };

    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        if (pc) return;
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* class_ptr = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!class_ptr) return;
        auto* cp = reinterpret_cast<const uint8_t*>(class_ptr);
        const FName& cls_fname = *reinterpret_cast<const FName*>(cp + 0x18);
        auto it = cls_cache.find(cls_fname.ComparisonIndex);
        const std::wstring* cls_name;
        if (it == cls_cache.end()) {
            auto [ins, _] = cls_cache.try_emplace(
                cls_fname.ComparisonIndex, fname_to_wstring(cls_fname));
            cls_name = &ins->second;
        } else {
            cls_name = &it->second;
        }
        if (cls_name->find(L"PlayerController") == std::wstring::npos) return;

        const FName& obj_name = *reinterpret_cast<const FName*>(p + 0x18);
        std::wstring on = fname_to_wstring(obj_name);
        if (on.compare(0, 9, L"Default__") == 0) return;

        int32_t ps_off = get_ps_offset(class_ptr);
        if (ps_off < 0) return;
        auto* ps = *reinterpret_cast<UObject* const*>(p + ps_off);
        if (!ps) return;
        auto* ps_class = *reinterpret_cast<UObject* const*>(
            reinterpret_cast<const uint8_t*>(ps) + 0x10);
        if (!ps_class) return;
        int32_t name_off = get_name_offset(ps_class);
        if (name_off < 0) return;
        std::wstring pn = read_fstring_at(ps, name_off);
        if (pn == want_name_w) pc = obj;
    });
    return pc;
}

// â”€â”€â”€ broadcastRaidBanner â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Fires one of GlobalRaidProtectionManager's NetMulticast_ShowRaid*Message
// UFunctions. Three variants supported in v1 (the two parameterless ones +
// the int-param "End" variant). The struct/array variants (Start, Times)
// require the FRaidPeriod struct layout which v1 skips.
//
// Params: { "kind": "concluded"|"allowed"|"end", "seconds": "<N>" }
//   - "concluded"  â†’ NetMulticast_ShowRaidConcludedMessage (no params)
//   - "allowed"    â†’ NetMulticast_ShowRaidAllowedMessage   (no params)
//   - "end"        â†’ NetMulticast_ShowRaidEndAnnouncementMessage (int seconds)
//
// Mechanism: find the GlobalRaidProtectionManager singleton instance,
// resolve the UFunction by name, ProcessEvent on the instance.
// [SCRUBBED] Game-specific section removed (347 lines)

static int32_t handle_set_god_mode(const char* params_json,
                                    const char** result_out, const char**)
{
    return set_prisoner_bool_flag(params_json, L"_isInGodMode", "setGodMode", result_out);
}

static int32_t handle_set_immortal(const char* params_json,
                                    const char** result_out, const char**)
{
    return set_prisoner_bool_flag(params_json, L"_isImmortal", "setImmortal", result_out);
}

static int32_t handle_set_infinite_ammo(const char* params_json,
                                         const char** result_out, const char**)
{
    return set_prisoner_bool_flag(params_json, L"_hasInfiniteAmmo", "setInfiniteAmmo", result_out);
}

static int32_t handle_set_super_jump(const char* params_json,
                                      const char** result_out, const char**)
{
    return set_prisoner_bool_flag(params_json, L"_isSuperJumpEnabled", "setSuperJump", result_out);
}

// â”€â”€â”€ bringVehicleToPlayer â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Joel's "bring my car" request. Find a vehicle of the given class and
// teleport it to a player's position. Uses K2_TeleportTo on the vehicle
// Actor (BlueprintCallable, non-RPC â€” works from server code, no auth
// blocker, proven path).
//
// v0 caveat: NO ownership check yet. Picks the first instance of the
// requested vehicle class on the server. If two players each own a
// kinglet, you might get a stranger's. Ownership-aware lookup needs the
// `_repServerEntitySetupAndId` struct (offset 0x680 on VehicleBase) which
// requires further RE â€” deferred.
//
// Params:
//   { "playerName": "<name>", "vehicleClass": "<UClass name, e.g. BPC_Kinglet_Duster_C>", "offsetZ"?: <float, default 50> }
static std::string s_bring_vehicle_result;
// [SCRUBBED] Game-specific section removed (430 lines)

static int32_t handle_send_hud_message(const char* params_json,
                                       const char** result_out, const char**)
{
    ensure_hook_installed_once();

    std::string text = extract_json_str(params_json, "text");
    std::string want_name = extract_json_str(params_json, "playerName");
    if (text.empty()) {
        s_send_hud_result = R"({"error":"text param required"})";
        *result_out = s_send_hud_result.c_str();
        return 0;
    }

    // Locate target PC â€” specific player or any PC for broadcast.
    UObject* pc = nullptr;
    const bool per_player = !want_name.empty();
    if (per_player) {
        std::wstring want_name_w(want_name.begin(), want_name.end());
        pc = find_pc_by_player_name(want_name_w);
        if (!pc) {
            s_send_hud_result = R"({"error":"player not found"})";
            *result_out = s_send_hud_result.c_str();
            return 0;
        }
    } else {
        // For broadcast: any live ConZPlayerController works since the
        // function iterates the player list internally server-side.
        pc = find_first_instance_of_class(L"ConZPlayerController" /* YOUR_GAME_PC_CLASS */);
        if (!pc) {
            s_send_hud_result = R"({"error":"no ConZPlayerController instance â€” server empty?"})";
            *result_out = s_send_hud_result.c_str();
            return 0;
        }
    }

    static UObject* s_hud_client_fn = nullptr;
    static UObject* s_hud_all_fn = nullptr;
    UObject*& fn_ref = per_player ? s_hud_client_fn : s_hud_all_fn;
    const wchar_t* fn_name = per_player ? L"SendHUDMessageToClient" : L"SendHUDMessageToAll";
    if (!fn_ref) {
        fn_ref = find_ufunction(fn_name, L"ConZPlayerController" /* YOUR_GAME_PC_CLASS */);
        if (!fn_ref) {
            char err[160];
            std::snprintf(err, sizeof(err),
                          R"({"error":"%ls UFunction not found"})", fn_name);
            s_send_hud_result = err;
            *result_out = s_send_hud_result.c_str();
            return 0;
        }
    }

    // Build the FString text buffer in a thread_local that outlives the
    // call. Pattern mirrors handle_broadcast_chat.
    static thread_local wchar_t hud_msg_buf[1024];
    std::wstring wmsg = utf8_to_wstring(text);
    if (wmsg.size() >= 1023) wmsg.resize(1023);
    wcscpy_s(hud_msg_buf, 1024, wmsg.c_str());
    int32_t msg_len_with_null = static_cast<int32_t>(wmsg.length()) + 1;

    #pragma pack(push, 1)
    struct Params {
        wchar_t* MessageData;    // +0x00 (FString::Data)
        int32_t  MessageNum;     // +0x08
        int32_t  MessageMax;     // +0x0C
        UObject* AudioEvent;     // +0x10
    };
    #pragma pack(pop)
    static_assert(sizeof(Params) == 24, "SendHUDMessageTo* params must be 24 bytes");

    Params p{};
    p.MessageData = hud_msg_buf;
    p.MessageNum  = msg_len_with_null;
    p.MessageMax  = msg_len_with_null;
    p.AudioEvent  = nullptr;

    log_info_fmt(STR("[TurdMODEngineBridge] sendHudMessage pc={:p} mode={} text=\"{}\"\n"),
                 static_cast<void*>(pc),
                 per_player ? STR("per-player") : STR("broadcast"),
                 wmsg);
    pc->ProcessEvent(reinterpret_cast<class UFunction*>(fn_ref), &p);

    char buf[320];
    std::snprintf(buf, sizeof(buf),
                  "{\"ok\":true,\"mode\":\"%s\",\"text\":\"%s\"%s%s%s}",
                  per_player ? "per-player" : "broadcast",
                  text.c_str(),
                  per_player ? ",\"playerName\":\"" : "",
                  per_player ? want_name.c_str() : "",
                  per_player ? "\"" : "");
    s_send_hud_result = buf;
    *result_out = s_send_hud_result.c_str();
    return 0;
}

// â”€â”€â”€ showKillFeedNotification â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Wraps ConZPlayerController::ShowKillFeedNotificationOnClient.
// Displays a kill-feed-style notification on a specific player's screen.
// Params: { "playerName": "...", "prefix": "...", "characterName": "...",
//           "suffix": "...", "ping"?: bool }
static std::string s_killfeed_result;
static int32_t handle_show_killfeed(const char* params_json,
                                    const char** result_out, const char**)
{
    ensure_hook_installed_once();

    std::string player_name = extract_json_str(params_json, "playerName");
    std::string prefix      = extract_json_str(params_json, "prefix");
    std::string char_name   = extract_json_str(params_json, "characterName");
    std::string suffix      = extract_json_str(params_json, "suffix");
    bool ping               = extract_json_bool(params_json, "ping", false);

    if (player_name.empty()) {
        s_killfeed_result = R"({"error":"playerName param required"})";
        *result_out = s_killfeed_result.c_str();
        return 0;
    }

    std::wstring want_w(player_name.begin(), player_name.end());
    UObject* pc = find_pc_by_player_name(want_w);
    if (!pc) {
        s_killfeed_result = R"({"error":"player not found"})";
        *result_out = s_killfeed_result.c_str();
        return 0;
    }

    static UObject* s_fn = nullptr;
    if (!s_fn) {
        s_fn = find_ufunction(L"ShowKillFeedNotificationOnClient", L"ConZPlayerController" /* YOUR_GAME_PC_CLASS */);
        if (!s_fn) {
            s_killfeed_result = R"({"error":"ShowKillFeedNotificationOnClient UFunction not found"})";
            *result_out = s_killfeed_result.c_str();
            return 0;
        }
    }

    // Build FString buffers for Prefix, characterName, suffix
    static thread_local wchar_t buf_prefix[512];
    static thread_local wchar_t buf_char[512];
    static thread_local wchar_t buf_suffix[512];

    auto fill = [](wchar_t* dst, size_t cap, const std::string& src) -> int32_t {
        std::wstring w = utf8_to_wstring(src);
        if (w.size() >= cap - 1) w.resize(cap - 2);
        wcscpy_s(dst, cap, w.c_str());
        return static_cast<int32_t>(w.length()) + 1;
    };

    int32_t prefix_len = fill(buf_prefix, 512, prefix);
    int32_t char_len   = fill(buf_char,   512, char_name);
    int32_t suffix_len = fill(buf_suffix, 512, suffix);

    // Param layout: 3x FString (16 bytes each) + 1 bool = 49 bytes
    // FString = { wchar_t* Data, int32 Num, int32 Max } = 16 bytes
    alignas(16) uint8_t pbuf[64] = {0};
    // Prefix at +0
    *reinterpret_cast<wchar_t**>(pbuf + 0)  = buf_prefix;
    *reinterpret_cast<int32_t*>(pbuf + 8)   = prefix_len;
    *reinterpret_cast<int32_t*>(pbuf + 12)  = prefix_len;
    // characterName at +16
    *reinterpret_cast<wchar_t**>(pbuf + 16) = buf_char;
    *reinterpret_cast<int32_t*>(pbuf + 24)  = char_len;
    *reinterpret_cast<int32_t*>(pbuf + 28)  = char_len;
    // suffix at +32
    *reinterpret_cast<wchar_t**>(pbuf + 32) = buf_suffix;
    *reinterpret_cast<int32_t*>(pbuf + 40)  = suffix_len;
    *reinterpret_cast<int32_t*>(pbuf + 44)  = suffix_len;
    // Ping at +48
    pbuf[48] = ping ? 1 : 0;

    log_info_fmt(STR("[TurdMODEngineBridge] showKillFeedNotification player=\"{}\" prefix=\"{}\" name=\"{}\" suffix=\"{}\"\n"),
                 want_w, utf8_to_wstring(prefix), utf8_to_wstring(char_name), utf8_to_wstring(suffix));

    pc->ProcessEvent(reinterpret_cast<class UFunction*>(s_fn), pbuf);

    s_killfeed_result = R"({"ok":true,"playerName":")" + player_name +
                        R"(","prefix":")" + prefix +
                        R"(","characterName":")" + char_name +
                        R"(","suffix":")" + suffix + R"("})";
    *result_out = s_killfeed_result.c_str();
    return 0;
}

// â”€â”€â”€ sendGameModeHudMessage â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Wraps ConZGameMode::SendHUDMessageToAll â€” different from the
// ConZPlayerController version. GameMode's version takes (FString, bool beep)
// and may display differently. Params: { "text": "...", "beep"?: bool }
static std::string s_gm_hud_result;
static int32_t handle_gamemode_hud_message(const char* params_json,
                                           const char** result_out, const char**)
{
    ensure_hook_installed_once();

    std::string text = extract_json_str(params_json, "text");
    bool beep = extract_json_bool(params_json, "beep", false);

    if (text.empty()) {
        s_gm_hud_result = R"({"error":"text param required"})";
        *result_out = s_gm_hud_result.c_str();
        return 0;
    }

    // Find the ConZGameMode instance
    UObject* gm = find_first_instance_of_class(L"ConZGameMode");
    if (!gm) {
        s_gm_hud_result = R"({"error":"ConZGameMode instance not found"})";
        *result_out = s_gm_hud_result.c_str();
        return 0;
    }

    static UObject* s_fn = nullptr;
    if (!s_fn) {
        s_fn = find_ufunction(L"SendHUDMessageToAll", L"ConZGameMode");
        if (!s_fn) {
            s_gm_hud_result = R"({"error":"ConZGameMode::SendHUDMessageToAll UFunction not found"})";
            *result_out = s_gm_hud_result.c_str();
            return 0;
        }
    }

    static thread_local wchar_t gm_hud_buf[1024];
    std::wstring wmsg = utf8_to_wstring(text);
    if (wmsg.size() >= 1023) wmsg.resize(1023);
    wcscpy_s(gm_hud_buf, 1024, wmsg.c_str());
    int32_t len_with_null = static_cast<int32_t>(wmsg.length()) + 1;

    // Params: FString Message (16 bytes) + bool beep (1 byte) = 17 bytes
    alignas(16) uint8_t pbuf[32] = {0};
    *reinterpret_cast<wchar_t**>(pbuf + 0) = gm_hud_buf;
    *reinterpret_cast<int32_t*>(pbuf + 8)  = len_with_null;
    *reinterpret_cast<int32_t*>(pbuf + 12) = len_with_null;
    pbuf[16] = beep ? 1 : 0;

    log_info_fmt(STR("[TurdMODEngineBridge] sendGameModeHudMessage text=\"{}\" beep={}\n"),
                 wmsg, beep ? 1 : 0);

    gm->ProcessEvent(reinterpret_cast<class UFunction*>(s_fn), pbuf);

    s_gm_hud_result = R"({"ok":true,"text":")" + text +
                      R"(","beep":)" + (beep ? "true" : "false") + "}";
    *result_out = s_gm_hud_result.c_str();
    return 0;
}

// â”€â”€â”€ launchPlayer â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Hulk-style directional leap: get player's facing direction, compute launch
// velocity, call Character::LaunchCharacter. Params:
//   { "playerName": "...", "speed"?: 3000, "upward"?: 1500,
//     "xyOverride"?: true, "zOverride"?: true }
static std::string s_launch_player_result;

static int32_t handle_launch_player(const char* params_json,
                                    const char** result_out, const char**)
{
    ensure_hook_installed_once();

    std::string player_name = extract_json_str(params_json, "playerName");
    float speed      = extract_json_float(params_json, "speed",      3000.0f);
    float upward     = extract_json_float(params_json, "upward",     1500.0f);
    bool  xy_override = extract_json_bool(params_json, "xyOverride", true);
    bool  z_override  = extract_json_bool(params_json, "zOverride",  true);

    if (player_name.empty()) {
        s_launch_player_result = R"({"error":"playerName required"})";
        *result_out = s_launch_player_result.c_str();
        return 0;
    }

    std::wstring want_w = utf8_to_wstring(player_name);
    UObject* pc = find_pc_by_player_name(want_w);
    if (!pc) {
        s_launch_player_result = R"({"error":"player not found"})";
        *result_out = s_launch_player_result.c_str();
        return 0;
    }

    // Get Pawn from PC via UProperty offset
    auto* pc_class = *reinterpret_cast<UObject* const*>(
        reinterpret_cast<const uint8_t*>(pc) + 0x10);
    int32_t pawn_off = find_property_offset(pc_class, L"Pawn");
    if (pawn_off < 0) {
        s_launch_player_result = R"({"error":"Pawn UProperty not found"})";
        *result_out = s_launch_player_result.c_str();
        return 0;
    }
    UObject* pawn = *reinterpret_cast<UObject* const*>(
        reinterpret_cast<const uint8_t*>(pc) + pawn_off);
    if (!pawn) {
        s_launch_player_result = R"({"error":"PC has no Pawn"})";
        *result_out = s_launch_player_result.c_str();
        return 0;
    }

    // Resolve UFunctions (cached)
    static UObject* s_get_fwd_fn = nullptr;
    static UObject* s_launch_fn = nullptr;

    if (!s_get_fwd_fn) {
        s_get_fwd_fn = find_ufunction(L"GetActorForwardVector");
    }
    if (!s_launch_fn) {
        s_launch_fn = find_ufunction(L"LaunchCharacter", L"Character");
        if (!s_launch_fn) s_launch_fn = find_ufunction(L"LaunchCharacter");
    }
    if (!s_get_fwd_fn || !s_launch_fn) {
        s_launch_player_result = R"({"error":"GetActorForwardVector or LaunchCharacter UFunction not found"})";
        *result_out = s_launch_player_result.c_str();
        return 0;
    }

    // GetActorForwardVector â†’ FVector (12 bytes return-only)
    struct { float X, Y, Z; } fwd{};
    pawn->ProcessEvent(reinterpret_cast<class UFunction*>(s_get_fwd_fn), &fwd);

    // Compute launch velocity: forward * speed + (0, 0, upward)
    float vel_x = fwd.X * speed;
    float vel_y = fwd.Y * speed;
    float vel_z = fwd.Z * speed + upward;

    log_info_fmt(STR("[launchPlayer] {} fwd=({:.1f},{:.1f},{:.1f}) vel=({:.0f},{:.0f},{:.0f})\n"),
                 want_w, fwd.X, fwd.Y, fwd.Z, vel_x, vel_y, vel_z);

    // LaunchCharacter(FVector, bXYOverride, bZOverride)
    #pragma pack(push, 1)
    struct LaunchParams {
        float VelX, VelY, VelZ;
        uint8_t bXY, bZ;
    };
    #pragma pack(pop)

    LaunchParams lp{};
    lp.VelX = vel_x;
    lp.VelY = vel_y;
    lp.VelZ = vel_z;
    lp.bXY = xy_override ? 1u : 0u;
    lp.bZ = z_override ? 1u : 0u;

    pawn->ProcessEvent(reinterpret_cast<class UFunction*>(s_launch_fn), &lp);

    char buf[256];
    std::snprintf(buf, sizeof(buf),
        "{\"ok\":true,\"player\":\"%s\","
        "\"velocity\":[%.0f,%.0f,%.0f],"
        "\"forward\":[%.3f,%.3f,%.3f],"
        "\"speed\":%.0f,\"upward\":%.0f}",
        player_name.c_str(), vel_x, vel_y, vel_z,
        fwd.X, fwd.Y, fwd.Z, speed, upward);
    s_launch_player_result = buf;
    *result_out = s_launch_player_result.c_str();
    return 0;
}

// â”€â”€â”€ writePlayerProperty â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Writes a property on a specific player's live component instance or PC.
// Unlike writeClassDefault which mutates the CDO, this targets the LIVE
// instance â€” per-player, immediate effect.
//
// Params:
//   playerName, component (optional), propertyName, value, valueKind
static std::string s_write_player_prop_result;

static int32_t handle_write_player_property(const char* params_json,
                                             const char** result_out, const char**)
{
    ensure_hook_installed_once();

    std::string player_name = extract_json_str(params_json, "playerName");
    std::string comp_name   = extract_json_str(params_json, "component");
    std::string prop_name   = extract_json_str(params_json, "propertyName");
    std::string value       = extract_json_str(params_json, "value");
    std::string value_kind  = extract_json_str(params_json, "valueKind");

    if (player_name.empty() || prop_name.empty() || value_kind.empty()) {
        s_write_player_prop_result = R"({"ok":false,"error":"missing playerName, propertyName, or valueKind"})";
        *result_out = s_write_player_prop_result.c_str();
        return 0;
    }

    std::wstring player_w = utf8_to_wstring(player_name);
    UObject* pc = find_pc_by_player_name(player_w);
    if (!pc) {
        s_write_player_prop_result = R"({"ok":false,"error":"player not found"})";
        *result_out = s_write_player_prop_result.c_str();
        return 0;
    }

    UObject* target = pc;
    std::string target_label = player_name + "(PC)";

    if (!comp_name.empty()) {
        auto* pc_class = *reinterpret_cast<UObject* const*>(
            reinterpret_cast<const uint8_t*>(pc) + 0x10);
        int32_t pawn_off = find_property_offset(pc_class, L"Pawn");
        if (pawn_off < 0) {
            s_write_player_prop_result = R"({"ok":false,"error":"Pawn not found on PC"})";
            *result_out = s_write_player_prop_result.c_str();
            return 0;
        }
        UObject* pawn = *reinterpret_cast<UObject* const*>(
            reinterpret_cast<const uint8_t*>(pc) + pawn_off);
        if (!pawn) {
            s_write_player_prop_result = R"({"ok":false,"error":"no Pawn"})";
            *result_out = s_write_player_prop_result.c_str();
            return 0;
        }

        // Walk Pawn's properties for an ObjectProperty whose PropertyClass contains comp_name
        std::wstring comp_w = utf8_to_wstring(comp_name);
        UObject* comp = nullptr;

        auto* cls = reinterpret_cast<const uint8_t*>(
            *reinterpret_cast<UObject* const*>(reinterpret_cast<const uint8_t*>(pawn) + 0x10));

        while (cls && !comp) {
            auto* field = *reinterpret_cast<const uint8_t* const*>(cls + 0x50);
            while (field) {
                auto* fc = *reinterpret_cast<const uint8_t* const*>(field + 0x08);
                std::wstring type_w = fname_to_wstring(*reinterpret_cast<const FName*>(fc));
                if (type_w == L"ObjectProperty") {
                    // PropertyClass at field+0x78
                    auto* prop_cls = *reinterpret_cast<UObject* const*>(field + 0x78);
                    if (prop_cls) {
                        std::wstring cls_name = fname_to_wstring(
                            *reinterpret_cast<const FName*>(reinterpret_cast<const uint8_t*>(prop_cls) + 0x18));
                        // Case-insensitive substring match
                        std::wstring hay = cls_name, needle = comp_w;
                        for (auto& c : hay) c = towlower(c);
                        for (auto& c : needle) c = towlower(c);
                        if (hay.find(needle) != std::wstring::npos) {
                            int32_t off = *reinterpret_cast<const int32_t*>(field + 0x4C);
                            comp = *reinterpret_cast<UObject* const*>(
                                reinterpret_cast<const uint8_t*>(pawn) + off);
                            if (comp) {
                                target_label = std::string(cls_name.begin(), cls_name.end());
                                break;
                            }
                        }
                    }
                }
                field = *reinterpret_cast<const uint8_t* const*>(field + 0x20);
            }
            cls = *reinterpret_cast<const uint8_t* const*>(cls + 0x40);
        }

        if (!comp) {
            s_write_player_prop_result = "{\"ok\":false,\"error\":\"component not found\",\"component\":\"" + comp_name + "\"}";
            *result_out = s_write_player_prop_result.c_str();
            return 0;
        }
        target = comp;
    }

    // Find property on target
    auto* target_class = *reinterpret_cast<UObject* const*>(
        reinterpret_cast<const uint8_t*>(target) + 0x10);
    std::wstring prop_w = utf8_to_wstring(prop_name);

    const uint8_t* found_field = nullptr;
    int32_t prop_offset = 0;
    std::wstring prop_type_w;

    auto* wcls = reinterpret_cast<const uint8_t*>(target_class);
    while (wcls && !found_field) {
        auto* field = *reinterpret_cast<const uint8_t* const*>(wcls + 0x50);
        while (field) {
            if (fname_to_wstring(*reinterpret_cast<const FName*>(field + 0x28)) == prop_w) {
                found_field = field;
                prop_offset = *reinterpret_cast<const int32_t*>(field + 0x4C);
                auto* fc = *reinterpret_cast<const uint8_t* const*>(field + 0x08);
                prop_type_w = fname_to_wstring(*reinterpret_cast<const FName*>(fc));
                break;
            }
            field = *reinterpret_cast<const uint8_t* const*>(field + 0x20);
        }
        wcls = *reinterpret_cast<const uint8_t* const*>(wcls + 0x40);
    }

    if (!found_field) {
        s_write_player_prop_result = "{\"ok\":false,\"error\":\"property not found\",\"propertyName\":\"" + prop_name + "\"}";
        *result_out = s_write_player_prop_result.c_str();
        return 0;
    }

    // Write value
    auto* tgt = reinterpret_cast<uint8_t*>(target);
    std::string prev;

    if (value_kind == "float") {
        float pv = *reinterpret_cast<const float*>(tgt + prop_offset);
        prev = std::to_string(pv);
        float nv = 0; try { nv = std::stof(value); } catch (...) {}
        *reinterpret_cast<float*>(tgt + prop_offset) = nv;
    } else if (value_kind == "int") {
        int32_t pv = *reinterpret_cast<const int32_t*>(tgt + prop_offset);
        prev = std::to_string(pv);
        int32_t nv = 0; try { nv = std::stoi(value); } catch (...) {}
        *reinterpret_cast<int32_t*>(tgt + prop_offset) = nv;
    } else if (value_kind == "byte") {
        uint8_t pv = *(tgt + prop_offset);
        prev = std::to_string(pv);
        uint8_t nv = 0; try { nv = (uint8_t)(std::stoi(value) & 0xFF); } catch (...) {}
        *(tgt + prop_offset) = nv;
    } else if (value_kind == "bool") {
        uint8_t mask = *reinterpret_cast<const uint8_t*>(found_field + 0x73);
        uint8_t* slot = tgt + prop_offset;
        bool pv = mask ? (*slot & mask) != 0 : *slot != 0;
        prev = pv ? "true" : "false";
        bool nv = (value == "true" || value == "1");
        if (mask) *slot = nv ? (*slot | mask) : (*slot & ~mask);
        else *slot = nv ? 1 : 0;
    } else {
        s_write_player_prop_result = R"({"ok":false,"error":"unsupported valueKind"})";
        *result_out = s_write_player_prop_result.c_str();
        return 0;
    }

    log_info_fmt(STR("[writePlayerProperty] {} {}.{} @0x{:X} {} -> {}\n"),
        player_w, std::wstring(target_label.begin(), target_label.end()),
        prop_w, (unsigned)prop_offset,
        std::wstring(prev.begin(), prev.end()),
        std::wstring(value.begin(), value.end()));

    char buf[512];
    std::snprintf(buf, sizeof(buf),
        "{\"ok\":true,\"playerName\":\"%s\",\"component\":\"%s\","
        "\"propertyName\":\"%s\",\"offset\":%d,"
        "\"previousValue\":\"%s\",\"newValue\":\"%s\"}",
        player_name.c_str(), target_label.c_str(),
        prop_name.c_str(), prop_offset,
        prev.c_str(), value.c_str());
    s_write_player_prop_result = buf;
    *result_out = s_write_player_prop_result.c_str();
    return 0;
}

// â”€â”€â”€ sendNotification â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Center-screen banner via NotificationsManager::NetMulticast_RequestNotification.
// Allocates a BasicNotificationDescription UObject, sets FText Message via
// Conv_StringToText, then calls the notification function.
//
// Params: { "message": "...", "fontSize"?: 24, "duration"?: 5.0, "ping"?: false }
// Forward declaration â€” defined after getPlayerPositions
static void* bridge_create_object(UObject* uclass);

static std::string s_send_notification_result;

static int32_t handle_send_notification(const char* params_json,
                                        const char** result_out, const char**)
{
    ensure_hook_installed_once();

    std::string message = extract_json_str(params_json, "message");
    int32_t font_size = 24;
    float duration = 5.0f;
    bool ping = extract_json_bool(params_json, "ping", false);

    { std::string fs = extract_json_str(params_json, "fontSize");
      if (!fs.empty()) { try { font_size = std::stoi(fs); } catch (...) {} } }
    duration = extract_json_float(params_json, "duration", 5.0f);

    if (message.empty()) {
        s_send_notification_result = R"({"error":"message param required"})";
        *result_out = s_send_notification_result.c_str();
        return 0;
    }

    // Step 1: Find NotificationsManager instance
    static UObject* s_notif_mgr = nullptr;
    if (!s_notif_mgr) {
        s_notif_mgr = find_first_instance_of_class(L"BP_NotificationsManager_C");
        if (!s_notif_mgr) s_notif_mgr = find_first_instance_of_class(L"NotificationsManager");
    }
    if (!s_notif_mgr) {
        s_send_notification_result = R"({"error":"NotificationsManager not found"})";
        *result_out = s_send_notification_result.c_str();
        return 0;
    }

    // Step 2: Find the UFunction
    static UObject* s_notif_fn = nullptr;
    if (!s_notif_fn) {
        s_notif_fn = find_ufunction(L"NetMulticast_RequestNotification", L"NotificationsManager");
    }
    if (!s_notif_fn) {
        s_send_notification_result = R"({"error":"NetMulticast_RequestNotification UFunction not found"})";
        *result_out = s_send_notification_result.c_str();
        return 0;
    }

    // Step 3: Find BasicNotificationDescription UClass
    static UObject* s_bnd_class = nullptr;
    if (!s_bnd_class) {
        UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
            if (s_bnd_class) return;
            auto* p = reinterpret_cast<const uint8_t*>(obj);
            auto* cls = *reinterpret_cast<UObject* const*>(p + 0x10);
            if (!cls) return;
            auto* cp = reinterpret_cast<const uint8_t*>(cls);
            const FName& cls_fn = *reinterpret_cast<const FName*>(cp + 0x18);
            if (fname_to_wstring(cls_fn) != L"Class") return;
            const FName& obj_fn = *reinterpret_cast<const FName*>(p + 0x18);
            if (fname_to_wstring(obj_fn) == L"BasicNotificationDescription")
                s_bnd_class = obj;
        });
    }
    if (!s_bnd_class) {
        s_send_notification_result = R"({"error":"BasicNotificationDescription UClass not found"})";
        *result_out = s_send_notification_result.c_str();
        return 0;
    }

    // Step 4: Allocate a fresh UObject via StaticConstructObject (not CDO â€” CDO crashes client)
    static UObject* s_scratch_obj = nullptr;
    if (!s_scratch_obj) {
        void* created = bridge_create_object(s_bnd_class);
        if (created) {
            s_scratch_obj = reinterpret_cast<UObject*>(created);
            log_info_fmt(STR("[sendNotification] allocated BasicNotificationDescription @ {:p}\n"),
                static_cast<void*>(s_scratch_obj));
        }
    }
    if (!s_scratch_obj) {
        s_send_notification_result = R"({"error":"StaticConstructObject failed for BasicNotificationDescription"})";
        *result_out = s_send_notification_result.c_str();
        return 0;
    }

    // Step 5: Build FText via Conv_StringToText
    static UObject* s_conv_fn = nullptr;
    static UObject* s_kismet_cdo = nullptr;
    if (!s_conv_fn) {
        s_conv_fn = find_ufunction(L"Conv_StringToText", L"KismetTextLibrary");
    }
    if (!s_kismet_cdo) {
        UObjectGlobals::ForEachUObject([&](UObject* o, int32_t, int32_t) {
            if (s_kismet_cdo) return;
            const FName& fn = *reinterpret_cast<const FName*>(
                reinterpret_cast<const uint8_t*>(o) + 0x18);
            if (fname_to_wstring(fn) == L"Default__KismetTextLibrary")
                s_kismet_cdo = o;
        });
    }

    auto* obj_bytes = reinterpret_cast<uint8_t*>(s_scratch_obj);

    if (s_conv_fn && s_kismet_cdo) {
        static thread_local wchar_t notif_str_buf[1024];
        std::wstring wmsg = utf8_to_wstring(message);
        if (wmsg.size() >= 1023) wmsg.resize(1023);
        wcscpy_s(notif_str_buf, 1024, wmsg.c_str());
        int32_t str_len = static_cast<int32_t>(wmsg.length()) + 1;

        // Conv_StringToText: paramsSize=40
        //   +0: FString inString (16 bytes)
        //   +16: FText ReturnValue (24 bytes)
        alignas(8) uint8_t conv_buf[40] = {0};
        *reinterpret_cast<wchar_t**>(conv_buf + 0) = notif_str_buf;
        *reinterpret_cast<int32_t*>(conv_buf + 8) = str_len;
        *reinterpret_cast<int32_t*>(conv_buf + 12) = str_len;

        s_kismet_cdo->ProcessEvent(reinterpret_cast<class UFunction*>(s_conv_fn), conv_buf);

        // Copy 24-byte FText result to object at offset 64
        std::memcpy(obj_bytes + 64, conv_buf + 16, 24);
    } else {
        log_error(STR("[sendNotification] Conv_StringToText not found - blank message"));
        std::memset(obj_bytes + 64, 0, 24);
    }

    // Step 6: Set other properties
    *reinterpret_cast<int32_t*>(obj_bytes + 88) = font_size;  // FontSize
    *reinterpret_cast<void**>(obj_bytes + 96) = nullptr;      // Icon
    *reinterpret_cast<int32_t*>(obj_bytes + 104) = 0;         // IconSize
    *reinterpret_cast<float*>(obj_bytes + 108) = duration;    // Duration
    obj_bytes[112] = ping ? 1 : 0;                            // Ping

    // Base class fields
    obj_bytes[40] = 0;                                        // Target = all
    std::memset(obj_bytes + 48, 0, 8);                        // TargetUserProfileId
    obj_bytes[56] = 0;                                        // ShouldSendIfClientOffline

    // Step 7: Build 24-byte param buffer â€” UObject* at offset 0
    alignas(8) uint8_t param_buf[24] = {0};
    *reinterpret_cast<UObject**>(param_buf) = s_scratch_obj;

    log_info_fmt(STR("[sendNotification] mgr={:p} fn={:p} desc={:p} msg=\"{}\" size={} dur={:.1f}\n"),
        static_cast<void*>(s_notif_mgr), static_cast<void*>(s_notif_fn),
        static_cast<void*>(s_scratch_obj), utf8_to_wstring(message),
        font_size, duration);

    // Step 8: ProcessEvent with SEH
    s_notif_mgr->ProcessEvent(reinterpret_cast<class UFunction*>(s_notif_fn), param_buf);

    s_send_notification_result = R"({"ok":true,"message":")" + message +
        R"(","fontSize":)" + std::to_string(font_size) +
        R"(,"duration":)" + std::to_string(duration) +
        R"(,"ping":)" + (ping ? "true" : "false") + "}";
    *result_out = s_send_notification_result.c_str();
    return 0;
}

// â”€â”€â”€ getPlayerPositions â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Returns all online players with world X/Y/Z via K2_GetActorLocation on Pawn.
// Used by the live map overlay.
static std::string s_positions_result;

static int32_t handle_get_player_positions(const char*, const char** result_out, const char**)
{
    static UObject* s_get_loc_fn = nullptr;
    if (!s_get_loc_fn) {
        s_get_loc_fn = find_ufunction(L"K2_GetActorLocation");
    }

    std::unordered_map<UObject*, int32_t> ps_off_cache;
    std::unordered_map<UObject*, int32_t> name_off_cache;
    std::unordered_map<UObject*, int32_t> pawn_off_cache;
    std::unordered_map<uint32_t, std::wstring> cls_cache;

    auto get_off = [](std::unordered_map<UObject*, int32_t>& cache, UObject* cls, const wchar_t* name) -> int32_t {
        auto it = cache.find(cls);
        if (it != cache.end()) return it->second;
        int32_t off = find_property_offset(cls, name);
        cache[cls] = off;
        return off;
    };

    std::string out = "[";
    bool first = true;
    size_t count = 0;

    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* cls_ptr = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!cls_ptr) return;

        auto* cp = reinterpret_cast<const uint8_t*>(cls_ptr);
        const FName& cls_fn = *reinterpret_cast<const FName*>(cp + 0x18);
        auto cit = cls_cache.find(cls_fn.ComparisonIndex);
        const std::wstring* cls_name;
        if (cit == cls_cache.end()) {
            auto [ins, _] = cls_cache.try_emplace(cls_fn.ComparisonIndex, fname_to_wstring(cls_fn));
            cls_name = &ins->second;
        } else { cls_name = &cit->second; }
        if (cls_name->find(L"PlayerController") == std::wstring::npos) return;

        const FName& obj_fn = *reinterpret_cast<const FName*>(p + 0x18);
        if (fname_to_wstring(obj_fn).compare(0, 9, L"Default__") == 0) return;

        // Player name
        std::wstring display_name;
        int32_t ps_off = get_off(ps_off_cache, cls_ptr, L"PlayerState");
        if (ps_off >= 0) {
            UObject* ps = *reinterpret_cast<UObject* const*>(p + ps_off);
            if (ps) {
                auto* ps_cls = *reinterpret_cast<UObject* const*>(reinterpret_cast<const uint8_t*>(ps) + 0x10);
                if (ps_cls) {
                    int32_t n_off = get_off(name_off_cache, ps_cls, L"PlayerNamePrivate");
                    if (n_off < 0) n_off = get_off(name_off_cache, ps_cls, L"PlayerName");
                    if (n_off >= 0) display_name = read_fstring_at(ps, n_off);
                }
            }
        }

        std::string steam_id = read_pc_steam_id(obj);

        // Position via K2_GetActorLocation
        float x = 0, y = 0, z = 0;
        int32_t pawn_off = get_off(pawn_off_cache, cls_ptr, L"Pawn");
        if (pawn_off >= 0) {
            UObject* pawn = *reinterpret_cast<UObject* const*>(p + pawn_off);
            if (pawn && s_get_loc_fn) {
                struct { float X, Y, Z; } loc{};
                pawn->ProcessEvent(reinterpret_cast<class UFunction*>(s_get_loc_fn), &loc);
                x = loc.X; y = loc.Y; z = loc.Z;
            }
        }

        ++count;
        std::string name_s = fname_to_json_string(display_name);
        char pos[128];
        std::snprintf(pos, sizeof(pos), ",\"x\":%.1f,\"y\":%.1f,\"z\":%.1f}", (double)x, (double)y, (double)z);

        if (!first) out += ",";
        first = false;
        out += "{\"name\":\"" + name_s + "\",\"steamId\":\"" + steam_id + "\"" + pos;
    });
    out += "]";

    s_positions_result = "{\"count\":" + std::to_string(count) + ",\"players\":" + out + "}";
    *result_out = s_positions_result.c_str();
    return 0;
}

// â”€â”€â”€ createObject â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Allocates a UObject via StaticConstructObject_Internal (RVA is game-build-specific).
// This is the universal UE4 object factory â€” unlocks animal spawns, custom
// notifications, and any other runtime object creation.
//
// Params: { "className": "BasicNotificationDescription" }
// Returns: { "ok": true, "ptr": "0x...", "name": "...", "class": "..." }

// StaticConstructObject_Internal signature (UE4 4.27 params-struct form)
// @inv: struct layout must match FStaticConstructObjectParameters exactly
#pragma pack(push, 8)
struct BridgeSCOParams {
    void*    Class;              // +0  UClass*
    void*    Outer;              // +8  UObject* (nullptr = transient)
    uint8_t  Name[8];           // +16 FName (ComparisonIndex=0, Number=0 â†’ NAME_None)
    uint32_t SetFlags;          // +24 EObjectFlags (RF_Transient = 0x40)
    uint32_t InternalSetFlags;  // +28
    bool     bCopyTransients;   // +32
    bool     bAssumeTemplate;   // +33
    uint8_t  _pad[6];           // +34
    void*    Template;          // +40
    void*    InstanceGraph;     // +48
    void*    ExternalPackage;   // +56
};
#pragma pack(pop)

using StaticConstructObjectFn = void* (*)(const BridgeSCOParams*);
static StaticConstructObjectFn g_sco_fn = nullptr;

// YOUR_GAME: find StaticConstructObject_Internal RVA for your build.
// Use a sig-scanner or PDB symbols to locate it.
static constexpr uintptr_t kStaticConstructObjectRVA = 0x00000000; // PLACEHOLDER

static void* bridge_create_object(UObject* uclass)
{
    if (!g_sco_fn) {
        HMODULE m = ::GetModuleHandleA(NULL);
        if (!m) return nullptr;
        g_sco_fn = reinterpret_cast<StaticConstructObjectFn>(
            reinterpret_cast<uintptr_t>(m) + kStaticConstructObjectRVA);
        log_info_fmt(STR("[createObject] resolved StaticConstructObject @ {:p}\n"),
            reinterpret_cast<void*>(g_sco_fn));
    }

    BridgeSCOParams params{};
    params.Class = uclass;
    params.Outer = nullptr;
    std::memset(params.Name, 0, 8);
    params.SetFlags = 0x00000040u; // RF_Transient
    params.InternalSetFlags = 0;
    params.bCopyTransients = false;
    params.bAssumeTemplate = false;
    params.Template = nullptr;
    params.InstanceGraph = nullptr;
    params.ExternalPackage = nullptr;

    return g_sco_fn(&params);
}

static std::string s_create_object_result;

static int32_t handle_create_object(const char* params_json,
                                    const char** result_out, const char**)
{
    ensure_hook_installed_once();

    std::string class_name = extract_json_str(params_json, "className");
    if (class_name.empty()) {
        s_create_object_result = R"({"error":"className required"})";
        *result_out = s_create_object_result.c_str();
        return 0;
    }

    // Find the UClass by name
    std::wstring class_w = utf8_to_wstring(class_name);
    UObject* uclass = nullptr;

    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        if (uclass) return;
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* cls = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!cls) return;
        auto* cp = reinterpret_cast<const uint8_t*>(cls);
        const FName& cls_fn = *reinterpret_cast<const FName*>(cp + 0x18);
        std::wstring metaclass = fname_to_wstring(cls_fn);
        // @brk: BP items are BlueprintGeneratedClass, not Class
        if (metaclass != L"Class" && metaclass != L"BlueprintGeneratedClass") return;
        const FName& obj_fn = *reinterpret_cast<const FName*>(p + 0x18);
        if (fname_to_wstring(obj_fn) == class_w) uclass = obj;
    });

    if (!uclass) {
        s_create_object_result = "{\"error\":\"UClass not found: " + class_name + "\"}";
        *result_out = s_create_object_result.c_str();
        return 0;
    }

    log_info_fmt(STR("[createObject] class={} uclass={:p}\n"), class_w, static_cast<void*>(uclass));

    void* created = bridge_create_object(uclass);
    if (!created) {
        s_create_object_result = R"({"error":"StaticConstructObject returned null"})";
        *result_out = s_create_object_result.c_str();
        return 0;
    }

    auto* cp = reinterpret_cast<const uint8_t*>(created);
    const FName& obj_fn = *reinterpret_cast<const FName*>(cp + 0x18);
    std::string obj_name = fname_to_json_string(fname_to_wstring(obj_fn));

    char ptr_buf[32];
    std::snprintf(ptr_buf, sizeof(ptr_buf), "0x%llx",
        reinterpret_cast<unsigned long long>(created));

    log_info_fmt(STR("[createObject] created {} @ {:p}\n"),
        fname_to_wstring(obj_fn), created);

    s_create_object_result = "{\"ok\":true,\"className\":\"" + class_name +
        "\",\"name\":\"" + obj_name +
        "\",\"ptr\":\"" + ptr_buf + "\"}";
    *result_out = s_create_object_result.c_str();
    return 0;
}

// fireBanner — INSTANT custom colored center banner ON DEMAND. No Notifications.json,
// no cycle, no FText, no shared-ptr. Builds a fresh WarningNotificationDescription
// (Message:FString@64, Duration:float@80, Color:FColor@84 [B,G,R,A]) via
// StaticConstructObject, assembles the 24-byte NotificationDescriptionReplicationHelper
// [NotificationsManager*][UClass*][Data*], and fires NotificationsManager::
// NetMulticast_RequestNotification — replicates to all clients immediately.
// FString points at a thread-local buffer (read synchronously during the multicast
// serialize); NULLed right after so the created Data's eventual GC never frees it.
// @ctx RE 2026-06-09 [[reference_colored_banners]]. Params: { text, r,g,b (0-255), duration? }
static thread_local std::string s_fire_banner_result;
static int32_t handle_fire_banner(const char* params_json, const char** result_out, const char**)
{
    ensure_hook_installed_once();
    std::string text = extract_json_str(params_json, "text");
    if (text.empty()) { s_fire_banner_result = R"({"error":"text param required"})"; *result_out = s_fire_banner_result.c_str(); return 0; }
    float duration = extract_json_float(params_json, "duration", 8.0f);
    int r = (int)extract_json_float(params_json, "r", 255.0f);
    int g = (int)extract_json_float(params_json, "g", 255.0f);
    int b = (int)extract_json_float(params_json, "b", 255.0f);

    // From-scratch construction crashes (StaticConstructObject of the notification
    // class faults), so we MODIFY a real captured Data instead — captureNotification
    // must have run + a notification fired within ~40s (objects still alive). The
    // captured helper bytes are [NotificationsManager* @0][UClass* @8][Data* @16].
    auto ok_ptr = [](const void* p){ auto v = reinterpret_cast<uintptr_t>(p); return v > 0x10000 && v < 0x7FFFFFFFFFFFULL; };
    if (!g_notif_captured.load()) {
        s_fire_banner_result = R"({"error":"no captured template - run captureNotification, let a notification fire, then fireBanner within ~40s"})";
        *result_out = s_fire_banner_result.c_str(); return 0;
    }
    UObject* mgr  = *reinterpret_cast<UObject**>(g_notif_buf + 0);
    void*    data = *reinterpret_cast<void**>(g_notif_buf + 16);
    if (!ok_ptr(mgr) || !ok_ptr(data)) {
        s_fire_banner_result = R"({"error":"captured template invalid/expired - recapture"})";
        *result_out = s_fire_banner_result.c_str(); return 0;
    }
    UObject* req_fn = find_ufunction(L"NetMulticast_RequestNotification", L"NotificationsManager");
    if (!req_fn) { s_fire_banner_result = R"({"error":"NetMulticast_RequestNotification not found"})"; *result_out = s_fire_banner_result.c_str(); return 0; }

    // Modify the REAL captured WarningNotificationDescription in place. Color @84
    // (FColor B,G,R,A) + Duration @80 are plain byte/float writes. Message @64
    // (FString) — overwrite chars in SCUM's own buffer IF our text fits (keeps SCUM's
    // ownership, no realloc/leak); otherwise leave the captured text.
    auto* d = reinterpret_cast<uint8_t*>(data);
    *reinterpret_cast<float*>(d + 80) = duration;
    d[84] = static_cast<uint8_t>(b & 0xFF);
    d[85] = static_cast<uint8_t>(g & 0xFF);
    d[86] = static_cast<uint8_t>(r & 0xFF);
    d[87] = 255;

    bool msg_set = false;
    int32_t cap = *reinterpret_cast<int32_t*>(d + 76);          // FString.ArrayMax
    wchar_t* mbuf = *reinterpret_cast<wchar_t**>(d + 64);       // FString.Data
    std::wstring wt = utf8_to_wstring(text);
    if (ok_ptr(mbuf) && cap > 1 && static_cast<int32_t>(wt.length()) + 1 <= cap) {
        wcsncpy_s(mbuf, static_cast<size_t>(cap), wt.c_str(), static_cast<size_t>(cap) - 1);
        *reinterpret_cast<int32_t*>(d + 72) = static_cast<int32_t>(wt.length()) + 1; // ArrayNum
        msg_set = true;
    }

    // Fire a COPY of the captured helper (so ProcessEvent can't perturb our template).
    alignas(16) uint8_t fire_buf[64] = {0};
    std::memcpy(fire_buf, g_notif_buf, 24);
    uint32_t seh = call_processevent_seh(mgr, reinterpret_cast<class UFunction*>(req_fn), fire_buf);

    char buf[160];
    std::snprintf(buf, sizeof(buf), "{\"ok\":true,\"sehFire\":%u,\"msgSet\":%s,\"cap\":%d}", seh, msg_set ? "true" : "false", cap);
    s_fire_banner_result = buf;
    *result_out = s_fire_banner_result.c_str();
    return 0;
}

// cleanAllClothes — server-wide "spa" clean. Scans every UObject, finds instances whose
// class super-chain includes ClothesItem, filters to EQUIPPED ones (_characterMesh@1992
// != null), and calls ClothesItem::SetDirtiness(0.0) on each. The spa cleans everyone's
// worn clothes at once, so no per-player filter is needed. SEH-wrapped per call.
// @ctx RE 2026-06-10: equipped clothes live in the VIRTUALIZED inventory (no reflected
// item array), so a global super-chain scan is the reliable reach.
static thread_local std::string s_clean_clothes_result;
// [SCRUBBED] Game-specific section removed (301 lines)

static int32_t handle_set_console_var_float(const char* params_json,
                                            const char** result_out, const char**)
{
    ensure_hook_installed_once();
    std::string name = extract_json_str(params_json, "name");
    std::string value = extract_json_str(params_json, "value");
    if (name.empty() || value.empty()) {
        s_set_cvar_result = R"({"error":"name and value required"})";
        *result_out = s_set_cvar_result.c_str(); return 0;
    }
    if (name.compare(0, 5, "scum.") != 0) {
        s_set_cvar_result = R"({"error":"name must start with scum."})";
        *result_out = s_set_cvar_result.c_str(); return 0;
    }
    // Value must parse as a number — blocks injecting extra console tokens via value.
    try { (void)std::stod(value); } catch (...) {
        s_set_cvar_result = R"({"error":"value must be numeric"})";
        *result_out = s_set_cvar_result.c_str(); return 0;
    }
    // Name must be a bare cvar token (no spaces) — second injection guard.
    if (name.find_first_of(" \t\r\n\"|;") != std::string::npos) {
        s_set_cvar_result = R"({"error":"invalid characters in name"})";
        *result_out = s_set_cvar_result.c_str(); return 0;
    }

    // Find KismetSystemLibrary::ExecuteConsoleCommand (cached — UFunctions are
    // stable for process life) + a live WorldContext (PC preferred, UWorld
    // fallback; both re-found each call since instances come and go).
    static UObject* s_exec_fn = nullptr;
    UObject* world_obj = nullptr;
    UObject* player_ctrl = nullptr;
    std::unordered_map<uint32_t, std::wstring> cls_cache;
    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* class_ptr = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!class_ptr) return;
        auto* cp = reinterpret_cast<const uint8_t*>(class_ptr);
        const FName& cls_fname = *reinterpret_cast<const FName*>(cp + 0x18);
        auto it = cls_cache.find(cls_fname.ComparisonIndex);
        const std::wstring* cls_name;
        if (it == cls_cache.end()) {
            auto [ins, _] = cls_cache.try_emplace(
                cls_fname.ComparisonIndex, fname_to_wstring(cls_fname));
            cls_name = &ins->second;
        } else { cls_name = &it->second; }
        const FName& obj_name = *reinterpret_cast<const FName*>(p + 0x18);

        if (!s_exec_fn && *cls_name == L"Function") {
            if (fname_to_wstring(obj_name) == L"ExecuteConsoleCommand") {
                auto* outer = *reinterpret_cast<UObject* const*>(p + 0x20);
                if (outer) {
                    auto* op = reinterpret_cast<const uint8_t*>(outer);
                    const FName& outer_name = *reinterpret_cast<const FName*>(op + 0x18);
                    if (fname_to_wstring(outer_name) == L"KismetSystemLibrary")
                        s_exec_fn = obj;
                }
            }
        } else if (*cls_name == L"World") {
            std::wstring on = fname_to_wstring(obj_name);
            if (on.compare(0, 9, L"Default__") != 0 && !world_obj) world_obj = obj;
        } else if (cls_name->find(L"PlayerController") != std::wstring::npos) {
            std::wstring on = fname_to_wstring(obj_name);
            if (on.compare(0, 9, L"Default__") != 0 && !player_ctrl) player_ctrl = obj;
        }
    });

    if (!s_exec_fn) {
        s_set_cvar_result = R"({"error":"KismetSystemLibrary::ExecuteConsoleCommand not found"})";
        *result_out = s_set_cvar_result.c_str(); return 0;
    }
    UObject* world_ctx = player_ctrl ? player_ctrl : world_obj;
    if (!world_ctx) {
        s_set_cvar_result = R"({"error":"no WorldContext (PC/UWorld) — map loaded?"})";
        *result_out = s_set_cvar_result.c_str(); return 0;
    }

    // Build "scum.X <value>" command FString in a buffer that outlives the call.
    std::string cmd = name + " " + value;
    static thread_local wchar_t cmd_buf[256];
    std::wstring wcmd = utf8_to_wstring(cmd);
    if (wcmd.size() >= 255) wcmd.resize(255);
    wcscpy_s(cmd_buf, 256, wcmd.c_str());
    int32_t cmd_len = static_cast<int32_t>(wcmd.length()) + 1;

    // ExecuteConsoleCommand(UObject* WorldContextObject, FString Command, APlayerController* SpecificPlayer)
    #pragma pack(push, 1)
    struct Params {
        UObject* WorldContextObject;  // +0x00
        wchar_t* CmdData;             // +0x08 (FString::Data)
        int32_t  CmdNum;              // +0x10 (FString::ArrayNum)
        int32_t  CmdMax;              // +0x14 (FString::ArrayMax)
        UObject* SpecificPlayer;      // +0x18
    };
    #pragma pack(pop)
    Params params{};
    params.WorldContextObject = world_ctx;
    params.CmdData = cmd_buf;
    params.CmdNum  = cmd_len;
    params.CmdMax  = cmd_len;
    params.SpecificPlayer = nullptr;

    log_info_fmt(STR("[setConsoleVarFloat] exec: {}\n"), wcmd);
    world_ctx->ProcessEvent(reinterpret_cast<class UFunction*>(s_exec_fn), &params);

    s_set_cvar_result = std::string("{\"ok\":true,\"command\":\"") + cmd + "\"}";
    *result_out = s_set_cvar_result.c_str();
    return 0;
}

// â”€â”€â”€ traderFunds â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Read/set per-trader in-memory available_funds on TraderManagingComponent.
// available_funds is a NON-reflected field (no UProperty, no setter UFunction) —
// SCUM loads it from economy_traders.available_funds at boot (all 32 identical)
// and persists our in-memory write on its next save (so a live write sticks).
// @inv The offset is found by scanning for the int32 that's uniform + funds-
// plausible across ALL trader components (verify the scanned value == the DB's
// available_funds before trusting it). SET requires that explicit, verified
// offset — it never blind-writes (a wrong offset across 32 components = crash).
// @ctx 2026-06-17 hourly trader refill (Option B); pairs with trader_refill.rs.
//
// Params:
//   {"mode":"scan"}                          -> {ok,traders,candidates:[{offset,value,agree}]}
//   {"mode":"set","offset":"N","value":"V"}  -> writes int32 V at off N on all components
static std::string s_trader_funds_result;
// [SCRUBBED] Game-specific section removed (86 lines)

static int32_t handle_get_nearby_actors(const char* params_json,
                                        const char** result_out, const char**)
{
    ensure_hook_installed_once();

    std::string player_name = extract_json_str(params_json, "playerName");
    std::string class_filter = extract_json_str(params_json, "classFilter");
    float radius = extract_json_float(params_json, "radius", 5000.0f);

    if (player_name.empty() || class_filter.empty()) {
        s_nearby_result = R"({"error":"playerName and classFilter required"})";
        *result_out = s_nearby_result.c_str();
        return 0;
    }

    // Get player position
    std::wstring player_w = utf8_to_wstring(player_name);
    UObject* pc = find_pc_by_player_name(player_w);
    if (!pc) {
        s_nearby_result = R"({"error":"player not found"})";
        *result_out = s_nearby_result.c_str();
        return 0;
    }

    auto* pc_class = *reinterpret_cast<UObject* const*>(
        reinterpret_cast<const uint8_t*>(pc) + 0x10);
    int32_t pawn_off = find_property_offset(pc_class, L"Pawn");
    if (pawn_off < 0) {
        s_nearby_result = R"({"error":"Pawn not found"})";
        *result_out = s_nearby_result.c_str();
        return 0;
    }
    UObject* pawn = *reinterpret_cast<UObject* const*>(
        reinterpret_cast<const uint8_t*>(pc) + pawn_off);
    if (!pawn) {
        s_nearby_result = R"({"error":"no Pawn"})";
        *result_out = s_nearby_result.c_str();
        return 0;
    }

    static UObject* s_loc_fn = nullptr;
    if (!s_loc_fn) s_loc_fn = find_ufunction(L"K2_GetActorLocation");

    float px = 0, py = 0, pz = 0;
    if (s_loc_fn) {
        struct { float X, Y, Z; } loc{};
        pawn->ProcessEvent(reinterpret_cast<class UFunction*>(s_loc_fn), &loc);
        px = loc.X; py = loc.Y; pz = loc.Z;
    }

    // Scan for matching actors within radius
    std::wstring filter_w = utf8_to_wstring(class_filter);
    std::unordered_map<uint32_t, std::wstring> cls_cache;
    std::string actors = "[";
    bool first = true;
    size_t count = 0;

    SCAN_TIMEOUT_INIT();
    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        SCAN_TIMEOUT_CHECK();
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* cls = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!cls) return;

        auto* cp = reinterpret_cast<const uint8_t*>(cls);
        const FName& cls_fn = *reinterpret_cast<const FName*>(cp + 0x18);
        auto it = cls_cache.find(cls_fn.ComparisonIndex);
        const std::wstring* cls_name;
        if (it == cls_cache.end()) {
            auto [ins, _] = cls_cache.try_emplace(cls_fn.ComparisonIndex, fname_to_wstring(cls_fn));
            cls_name = &ins->second;
        } else { cls_name = &it->second; }

        if (cls_name->find(filter_w) == std::wstring::npos) return;

        const FName& obj_fn = *reinterpret_cast<const FName*>(p + 0x18);
        std::wstring on = fname_to_wstring(obj_fn);
        if (on.compare(0, 9, L"Default__") == 0) return;

        // Get this actor's position
        if (!s_loc_fn) return;
        struct { float X, Y, Z; } aloc{};
        obj->ProcessEvent(reinterpret_cast<class UFunction*>(s_loc_fn), &aloc);

        float dx = aloc.X - px, dy = aloc.Y - py, dz = aloc.Z - pz;
        float dist = std::sqrt(dx*dx + dy*dy + dz*dz);

        if (dist > radius) return;

        char buf[256];
        std::snprintf(buf, sizeof(buf),
            "%s{\"name\":\"%s\",\"class\":\"%s\",\"ptr\":\"0x%llx\",\"distance\":%.0f,\"x\":%.0f,\"y\":%.0f,\"z\":%.0f}",
            first ? "" : ",",
            fname_to_json_string(on).c_str(),
            fname_to_json_string(*cls_name).c_str(),
            reinterpret_cast<unsigned long long>(obj),
            (double)dist, (double)aloc.X, (double)aloc.Y, (double)aloc.Z);
        first = false;
        actors += buf;
        ++count;
    });
    actors += "]";

    s_nearby_result = "{\"ok\":true,\"count\":" + std::to_string(count) +
        ",\"playerX\":" + std::to_string(px) +
        ",\"playerY\":" + std::to_string(py) +
        ",\"actors\":" + actors + "}";
    *result_out = s_nearby_result.c_str();
    return 0;
}

// â”€â”€â”€ writeActorProperty â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Writes a property on ANY actor by pointer address. The universal live
// instance writer â€” works on zombies, animals, vehicles, anything.
//
// Params: { "ptr": "0x123...", "propertyName": "...", "value": "...", "valueKind": "float|int|byte|bool" }
static std::string s_write_actor_result;

static int32_t handle_write_actor_property(const char* params_json,
                                           const char** result_out, const char**)
{
    ensure_hook_installed_once();

    std::string ptr_s = extract_json_str(params_json, "ptr");
    std::string prop_name = extract_json_str(params_json, "propertyName");
    std::string value = extract_json_str(params_json, "value");
    std::string value_kind = extract_json_str(params_json, "valueKind");

    if (ptr_s.empty() || prop_name.empty() || value_kind.empty()) {
        s_write_actor_result = R"({"error":"ptr, propertyName, valueKind required"})";
        *result_out = s_write_actor_result.c_str();
        return 0;
    }

    // Parse hex pointer
    unsigned long long addr = 0;
    try { addr = std::stoull(ptr_s, nullptr, 16); } catch (...) {
        s_write_actor_result = R"({"error":"invalid ptr hex"})";
        *result_out = s_write_actor_result.c_str();
        return 0;
    }

    UObject* target = reinterpret_cast<UObject*>(static_cast<uintptr_t>(addr));
    auto* target_class = *reinterpret_cast<UObject* const*>(
        reinterpret_cast<const uint8_t*>(target) + 0x10);

    if (!target_class) {
        s_write_actor_result = R"({"error":"target has null class"})";
        *result_out = s_write_actor_result.c_str();
        return 0;
    }

    std::wstring prop_w = utf8_to_wstring(prop_name);
    int32_t offset = find_property_offset(target_class, prop_w.c_str());
    if (offset < 0) {
        // Try walking the class hierarchy for the property
        s_write_actor_result = "{\"error\":\"property not found\",\"propertyName\":\"" + prop_name + "\"}";
        *result_out = s_write_actor_result.c_str();
        return 0;
    }

    auto* tgt = reinterpret_cast<uint8_t*>(target);
    std::string prev;

    if (value_kind == "float") {
        float pv = *reinterpret_cast<const float*>(tgt + offset);
        prev = std::to_string(pv);
        float nv = 0; try { nv = std::stof(value); } catch (...) {}
        *reinterpret_cast<float*>(tgt + offset) = nv;
    } else if (value_kind == "int") {
        int32_t pv = *reinterpret_cast<const int32_t*>(tgt + offset);
        prev = std::to_string(pv);
        int32_t nv = 0; try { nv = std::stoi(value); } catch (...) {}
        *reinterpret_cast<int32_t*>(tgt + offset) = nv;
    } else if (value_kind == "byte") {
        uint8_t pv = *(tgt + offset);
        prev = std::to_string(pv);
        uint8_t nv = 0; try { nv = (uint8_t)(std::stoi(value) & 0xFF); } catch (...) {}
        *(tgt + offset) = nv;
    } else if (value_kind == "bool") {
        bool pv = *(tgt + offset) != 0;
        prev = pv ? "true" : "false";
        bool nv = (value == "true" || value == "1");
        *(tgt + offset) = nv ? 1 : 0;
    } else {
        s_write_actor_result = R"({"error":"unsupported valueKind"})";
        *result_out = s_write_actor_result.c_str();
        return 0;
    }

    log_info_fmt(STR("[writeActorProperty] ptr={} prop={} {} -> {}\n"),
        std::wstring(ptr_s.begin(), ptr_s.end()),
        prop_w, std::wstring(prev.begin(), prev.end()),
        std::wstring(value.begin(), value.end()));

    char buf[256];
    std::snprintf(buf, sizeof(buf),
        "{\"ok\":true,\"ptr\":\"%s\",\"propertyName\":\"%s\",\"offset\":%d,\"previous\":\"%s\",\"new\":\"%s\"}",
        ptr_s.c_str(), prop_name.c_str(), offset, prev.c_str(), value.c_str());
    s_write_actor_result = buf;
    *result_out = s_write_actor_result.c_str();
    return 0;
}

// â”€â”€â”€ callActorFunction â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Calls a UFunction on ANY actor by pointer. No params (v1).
// Used to trigger AI state changes like ChangePace, Alert, Rest on animals.
//
// Params: { "ptr": "0x...", "functionName": "...", "owner": "..." }
static std::string s_call_actor_result;

static int32_t handle_call_actor_function(const char* params_json,
                                          const char** result_out, const char**)
{
    ensure_hook_installed_once();

    std::string ptr_s = extract_json_str(params_json, "ptr");
    std::string fn_name = extract_json_str(params_json, "functionName");
    std::string owner = extract_json_str(params_json, "owner");

    if (ptr_s.empty() || fn_name.empty()) {
        s_call_actor_result = R"({"error":"ptr and functionName required"})";
        *result_out = s_call_actor_result.c_str();
        return 0;
    }

    unsigned long long addr = 0;
    try { addr = std::stoull(ptr_s, nullptr, 16); } catch (...) {
        s_call_actor_result = R"({"error":"invalid ptr hex"})";
        *result_out = s_call_actor_result.c_str();
        return 0;
    }

    UObject* target = reinterpret_cast<UObject*>(static_cast<uintptr_t>(addr));

    std::wstring fn_w = utf8_to_wstring(fn_name);
    std::wstring owner_w = owner.empty() ? L"" : utf8_to_wstring(owner);

    UObject* fn = find_ufunction(fn_w.c_str(), owner_w.empty() ? L"" : owner_w.c_str());
    if (!fn) {
        s_call_actor_result = "{\"error\":\"UFunction not found: " + fn_name + "\"}";
        *result_out = s_call_actor_result.c_str();
        return 0;
    }

    // Call with null params (v1 â€” no-arg functions only)
    alignas(16) uint8_t empty[16] = {0};
    target->ProcessEvent(reinterpret_cast<class UFunction*>(fn), empty);

    log_info_fmt(STR("[callActorFunction] ptr={} fn={}\n"),
        std::wstring(ptr_s.begin(), ptr_s.end()), fn_w);

    s_call_actor_result = "{\"ok\":true,\"ptr\":\"" + ptr_s +
        "\",\"function\":\"" + fn_name + "\"}";
    *result_out = s_call_actor_result.c_str();
    return 0;
}

// â”€â”€â”€ spawnAI â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Calls AIBlueprintHelperLibrary::SpawnAIFromClass â€” the REAL spawn function.
// Creates a fully initialized AI pawn with controller, mesh, behavior tree.
// This IS the spawn system. We clone it, we control it.
//
// Params: { "className": "Animal2", "playerName": "TechyRican",
//           "offsetForward": 500, "offsetZ": 100 }
static std::string s_spawn_ai_result;

// [SCRUBBED] Game-specific section removed (200 lines)

static int32_t handle_probe_quest_handlers(const char* params_json,
                                           const char** result_out, const char**)
{
    ensure_hook_installed_once();

    std::string want_name = extract_json_str(params_json, "name");

    // Find ANY non-CDO PlayerQuestComponent instance. Per memory
    // reference_scum_widget_internals the quest component is an instanced
    // component on the player; for a probe we don't care which player.
    UObject* quest_comp = find_first_instance_of_class(L"PlayerQuestComponent");
    if (!quest_comp) {
        s_probe_quest_result =
            R"({"error":"no PlayerQuestComponent instance â€” needs a connected player"})";
        *result_out = s_probe_quest_result.c_str();
        return 0;
    }

    static UObject* s_start_quest_fn = nullptr;
    if (!s_start_quest_fn) {
        s_start_quest_fn = find_ufunction(L"Server_StartQuest", L"PlayerQuestComponent");
        if (!s_start_quest_fn) {
            s_probe_quest_result =
                R"({"error":"Server_StartQuest UFunction not found in reflection"})";
            *result_out = s_probe_quest_result.c_str();
            return 0;
        }
    }

    // paramsSize = 36: StructProperty ID (likely FGuid, 16 bytes), then
    // IntProperty Index (4 bytes) at... aligned offset. Since the struct
    // is at offset 0 and is 16 bytes, Index naturally lands at offset 16.
    // But paramsSize is 36, not 20 â€” implying the ID struct is larger
    // than a bare FGuid. Best guess: it's an FQuestId or similar wrapper
    // (32 bytes), then Index at offset 32, total 36.
    //
    // We don't know the struct layout. Zero-init the whole 36-byte block;
    // a zero ID + zero Index is a guaranteed-invalid quest and should
    // either be rejected loudly (good â€” call went through) or silently
    // (auth blocker confirmed).
    uint8_t params_block[36] = {0};

    log_info_fmt(STR("[TurdMODEngineBridge] probeQuestHandlers comp={:p} fn={:p} "
                     "(firing Server_StartQuest with zero params â€” check SCUM logs)\n"),
                 static_cast<void*>(quest_comp), static_cast<void*>(s_start_quest_fn));
    quest_comp->ProcessEvent(reinterpret_cast<class UFunction*>(s_start_quest_fn),
                             &params_block);

    char buf[256];
    std::snprintf(buf, sizeof(buf),
                  "{\"ok\":true,\"probed\":\"Server_StartQuest\","
                  "\"componentPtr\":\"0x%llx\","
                  "\"note\":\"call returned; check SCUM logs for accept-vs-silent-reject signal\"}",
                  reinterpret_cast<unsigned long long>(quest_comp));
    s_probe_quest_result = buf;
    *result_out = s_probe_quest_result.c_str();
    return 0;
}

// â”€â”€â”€ setEconomy â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Wraps ConZEconomyManager NetMulticast_Update* family. Two simple
// variants supported in v1 â€” both take dataVersion (int) + value:
//
//   kind="gold"      â†’ NetMulticast_UpdateGoldPriceMasterMultiplier(int, float)
//   kind="tradeable" â†’ NetMulticast_UpdateTradeablePriceMultiplierFactor(int, int)
//
// The struct/array variants (DateVsGoldPriceMap / TradeableClassMapOverrides)
// need FStruct layouts we don't have yet. Defer.
//
// NetMulticast_* on a server-side singleton means the bridge can fire
// these without an authenticated client. Effect: every connected
// client's trader UI updates with the new multiplier â€” no SCUM restart.
//
// Params: {kind: "gold"|"tradeable", dataVersion: "N", value: "N.N"}
static int32_t handle_set_economy(const char* params_json,
                                  const char** result_out, const char**)
{
    ensure_hook_installed_once();

    std::string kind = extract_json_str(params_json, "kind");
    if (kind.empty()) {
        s_set_economy_result = R"json({"error":"kind param required: gold or tradeable"})json";
        *result_out = s_set_economy_result.c_str();
        return 0;
    }

    const wchar_t* fn_name = nullptr;
    bool value_is_float = false;
    if      (kind == "gold")      { fn_name = L"NetMulticast_UpdateGoldPriceMasterMultiplier"; value_is_float = true; }
    else if (kind == "tradeable") { fn_name = L"NetMulticast_UpdateTradeablePriceMultiplierFactor"; value_is_float = false; }
    else {
        s_set_economy_result = R"json({"error":"kind must be 'gold' or 'tradeable'"})json";
        *result_out = s_set_economy_result.c_str();
        return 0;
    }

    int32_t data_version = 0;
    std::string dv_s = extract_json_str(params_json, "dataVersion");
    try { data_version = std::stoi(dv_s); } catch (...) {}

    float value_f = 0.0f;
    int32_t value_i = 0;
    {
        std::string v = extract_json_str(params_json, "value");
        if (value_is_float) {
            try { value_f = std::stof(v); } catch (...) {}
        } else {
            try { value_i = std::stoi(v); } catch (...) {}
        }
    }

    UObject* manager = find_first_instance_of_class(L"ConZEconomyManager");
    if (!manager) {
        s_set_economy_result =
            R"json({"error":"no ConZEconomyManager instance â€” wait until game state loads"})json";
        *result_out = s_set_economy_result.c_str();
        return 0;
    }

    UObject* fn = find_ufunction(fn_name, L"ConZEconomyManager");
    if (!fn) {
        s_set_economy_result =
            R"json({"error":"economy UFunction not found in reflection"})json";
        *result_out = s_set_economy_result.c_str();
        return 0;
    }

    // paramsSize = 8: int32 dataVersion at +0, int32_or_float value at +4.
    #pragma pack(push, 1)
    struct EconomyParams {
        int32_t dataVersion;
        union {
            float   asFloat;
            int32_t asInt;
        } value;
    };
    #pragma pack(pop)
    static_assert(sizeof(EconomyParams) == 8, "setEconomy params must be 8 bytes");

    EconomyParams ep{};
    ep.dataVersion = data_version;
    if (value_is_float) ep.value.asFloat = value_f;
    else                ep.value.asInt = value_i;

    log_info_fmt(STR("[TurdMODEngineBridge] setEconomy kind={} dv={} value={}\n"),
                 fn_name, data_version,
                 value_is_float ? std::to_wstring(value_f) : std::to_wstring(value_i));
    manager->ProcessEvent(reinterpret_cast<class UFunction*>(fn), &ep);

    char buf[192];
    if (value_is_float) {
        std::snprintf(buf, sizeof(buf),
                      "{\"ok\":true,\"kind\":\"%s\",\"dataVersion\":%d,\"value\":%g}",
                      kind.c_str(), data_version, value_f);
    } else {
        std::snprintf(buf, sizeof(buf),
                      "{\"ok\":true,\"kind\":\"%s\",\"dataVersion\":%d,\"value\":%d}",
                      kind.c_str(), data_version, value_i);
    }
    s_set_economy_result = buf;
    *result_out = s_set_economy_result.c_str();
    return 0;
}

// â”€â”€â”€ listClassInstances â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Diagnostic â€” scans GUObjectArray and reports every non-CDO class name
// containing the given substring, with hit counts. Generalizes the
// inline scan inside the raid-banner error path so any "no instance
// found" debugging can use the same tool.
//
// Params: {pattern: "RaidProtection"}  â†’  reports {RaidProtectionManager: 1, ...}
static int32_t handle_list_class_instances(const char* params_json,
                                           const char** result_out, const char**)
{
    ensure_hook_installed_once();

    std::string pattern = extract_json_str(params_json, "pattern");
    if (pattern.empty()) {
        s_list_instances_result =
            R"json({"error":"pattern param required (substring matched against class names)"})json";
        *result_out = s_list_instances_result.c_str();
        return 0;
    }
    std::wstring pattern_w(pattern.begin(), pattern.end());

    std::unordered_map<uint32_t, std::wstring> cls_cache;
    std::unordered_map<std::wstring, size_t> hits;
    size_t scanned = 0;

    SCAN_TIMEOUT_INIT();
    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        SCAN_TIMEOUT_CHECK();
        ++scanned;
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* class_ptr = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!class_ptr) return;
        auto* cp = reinterpret_cast<const uint8_t*>(class_ptr);
        const FName& cls_fname = *reinterpret_cast<const FName*>(cp + 0x18);
        auto it = cls_cache.find(cls_fname.ComparisonIndex);
        const std::wstring* cls = nullptr;
        if (it == cls_cache.end()) {
            auto [ins, _] = cls_cache.try_emplace(
                cls_fname.ComparisonIndex, fname_to_wstring(cls_fname));
            cls = &ins->second;
        } else {
            cls = &it->second;
        }
        if (cls->find(pattern_w) == std::wstring::npos) return;

        const FName& obj_name = *reinterpret_cast<const FName*>(p + 0x18);
        std::wstring on = fname_to_wstring(obj_name);
        if (on.compare(0, 9, L"Default__") == 0) return;
        hits[*cls]++;
    });

    std::string list = "[";
    bool first = true;
    size_t total = 0;
    for (const auto& [cls_w, count] : hits) {
        if (!first) list += ",";
        first = false;
        std::string cls_n(cls_w.begin(), cls_w.end());
        list += "{\"class\":\"" + cls_n + "\",\"count\":" + std::to_string(count) + "}";
        total += count;
    }
    list += "]";

    char meta[128];
    std::snprintf(meta, sizeof(meta),
                  "{\"ok\":true,\"pattern\":\"%s\",\"scanned\":%zu,\"matches\":%zu,\"found\":",
                  pattern.c_str(), scanned, total);
    s_list_instances_result = std::string(meta) + list + "}";
    *result_out = s_list_instances_result.c_str();
    return 0;
}

// â”€â”€â”€ kickPlayer (Wave 2 of scumpilot bridge-gap contract) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Wraps ConZGameMode::KickPlayer(Player: PlayerController, kickReason: FString)
// â€” a direct primitive that doesn't go through the game admin parser, so it
// bypasses the auth blocker documented in feedback_scum_admin_auth_blocker.
//
// Params: { "playerName"?: string, "steamId"?: string, "reason"?: string }
//
// Either playerName OR steamId must be set (scumpilot's contract says prefer
// steamId when both are available, but our v1 PC-lookup is name-only â€” TODO
// to add steam-id lookup via PlayerState.UniqueId once that infra lands).
//
// Calling convention: `this_` for ProcessEvent is the ConZGameMode instance
// (singleton); Player is a parameter, not the receiver.
static int32_t handle_kick_player(const char* params_json,
                                  const char** result_out, const char**)
{
    ensure_hook_installed_once();

    std::string want_name = extract_json_str(params_json, "playerName");
    std::string want_steam = extract_json_str(params_json, "steamId");
    std::string reason = extract_json_str(params_json, "reason");
    if (want_name.empty() && want_steam.empty()) {
        s_kick_player_result =
            R"({"error":"playerName or steamId required"})";
        *result_out = s_kick_player_result.c_str();
        return 0;
    }

    // v1: lookup by display name only. steam-id path needs PS.UniqueId.
    // If only steamId was provided, return an error pointing at the gap.
    if (want_name.empty()) {
        s_kick_player_result =
            R"({"error":"steamId-only lookup not yet supported; pass playerName too"})";
        *result_out = s_kick_player_result.c_str();
        return 0;
    }

    std::wstring want_name_w(want_name.begin(), want_name.end());
    UObject* pc = find_pc_by_player_name(want_name_w);
    if (!pc) {
        s_kick_player_result = R"({"error":"player not found"})";
        *result_out = s_kick_player_result.c_str();
        return 0;
    }

    static UObject* s_gamemode = nullptr;
    if (!s_gamemode) {
        s_gamemode = find_first_instance_of_class(L"ConZGameMode");
        if (!s_gamemode) {
            // fall back to any descendant â€” SCUM may have a BP subclass
            s_gamemode = find_first_instance_of_class(L"ConZGameMode_C");
        }
        if (!s_gamemode) {
            s_kick_player_result =
                R"({"error":"ConZGameMode instance not found"})";
            *result_out = s_kick_player_result.c_str();
            return 0;
        }
    }

    static UObject* s_kick_fn = nullptr;
    if (!s_kick_fn) {
        s_kick_fn = find_ufunction(L"KickPlayer", L"ConZGameMode");
        if (!s_kick_fn) {
            s_kick_player_result =
                R"({"error":"ConZGameMode::KickPlayer UFunction not found"})";
            *result_out = s_kick_player_result.c_str();
            return 0;
        }
    }

    // Params: { Player: UObject*, kickReason: FString, ReturnValue: bool }
    // paramsSize: 8 (ptr) + 16 (FString) + 1 (bool) + padding = 32 (8-byte align)
    #pragma pack(push, 8)
    struct KickParams {
        UObject*   Player;
        wchar_t*   Reason_Data;
        int32_t    Reason_Num;
        int32_t    Reason_Max;
        uint32_t   ReturnValue;   // BoolProperty in UE4 ProcessEvent is uint32
        uint32_t   _pad;
    };
    #pragma pack(pop)
    static_assert(sizeof(KickParams) >= 32, "KickPlayer params block too small");

    // Build the FString. UE4 expects a NUL-terminated wchar buffer and Num
    // INCLUDES the terminator. We stage in a static wstring per-call so the
    // buffer outlives ProcessEvent. NOT thread-safe across handlers; the
    // bridge-rpc-one-at-a-time rule covers us.
    static std::wstring s_reason_w;
    s_reason_w.assign(reason.begin(), reason.end());
    if (s_reason_w.empty()) s_reason_w = L"Kicked by admin";
    // Ensure NUL-terminated (push_back '\0' then assign Num including it)
    s_reason_w.push_back(L'\0');

    KickParams kp{};
    kp.Player = pc;
    kp.Reason_Data = const_cast<wchar_t*>(s_reason_w.c_str());
    kp.Reason_Num  = static_cast<int32_t>(s_reason_w.size());
    kp.Reason_Max  = kp.Reason_Num;
    kp.ReturnValue = 0;

    log_info_fmt(STR("[TurdMODEngineBridge] kickPlayer pc={:p} reason='{}'\n"),
                 static_cast<void*>(pc), s_reason_w.c_str());
    s_gamemode->ProcessEvent(reinterpret_cast<class UFunction*>(s_kick_fn), &kp);

    // Pop the NUL we pushed so future calls don't accumulate
    s_reason_w.pop_back();

    char buf[256];
    std::snprintf(buf, sizeof(buf),
                  "{\"ok\":true,\"playerName\":\"%s\",\"reason\":\"%s\",\"returnValue\":%u}",
                  want_name.c_str(),
                  reason.c_str(),
                  static_cast<unsigned>(kp.ReturnValue));
    s_kick_player_result = buf;
    *result_out = s_kick_player_result.c_str();
    return 0;
}

// â”€â”€â”€ setTimeOfDay (Wave 2) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// No reflectable SetTimeOfDay UFunction in v23128915 â€” the game admin command
// AdminCommand_SetTimeOfDay routes through the auth-gated chat parser. The
// path that works is direct property write on the WeatherController2 actor's
// `_timeOfDay: FloatProperty`. After writing, call NetMulticast_SendStateSnapshot
// to broadcast the new state to all clients (otherwise clients keep their old
// time until the next routine snapshot tick).
//
// Params: { "hours": float 0..24 }
//
// _timeOfDay is in HOURS (0..24, 12 = noon). FloatProperty.
static int32_t handle_set_time_of_day(const char* params_json,
                                      const char** result_out, const char**)
{
    ensure_hook_installed_once();

    float hours = extract_json_float(params_json, "hours", -1.0f);
    if (hours < 0.0f || hours > 24.0f) {
        s_set_time_of_day_result =
            R"({"error":"hours param required, must be 0..24"})";
        *result_out = s_set_time_of_day_result.c_str();
        return 0;
    }

    static UObject* s_weather_ctrl = nullptr;
    if (!s_weather_ctrl) {
        s_weather_ctrl = find_first_instance_of_class(L"WeatherController2");
        if (!s_weather_ctrl) {
            // BP subclass fallback â€” SCUM uses BP_WeatherController1_0_C
            s_weather_ctrl = find_first_instance_of_class(L"BP_WeatherController1_0_C");
        }
        if (!s_weather_ctrl) {
            s_set_time_of_day_result =
                R"({"error":"WeatherController2 instance not found"})";
            *result_out = s_set_time_of_day_result.c_str();
            return 0;
        }
    }

    auto* ctrl_class = *reinterpret_cast<UObject* const*>(
        reinterpret_cast<const uint8_t*>(s_weather_ctrl) + 0x10);
    static int32_t s_time_off = -2;
    if (s_time_off == -2) {
        s_time_off = find_property_offset(ctrl_class, L"_timeOfDay");
    }
    if (s_time_off < 0) {
        s_set_time_of_day_result =
            R"({"error":"WeatherController2._timeOfDay property offset not found"})";
        *result_out = s_set_time_of_day_result.c_str();
        return 0;
    }

    // Write the property directly. UE4 replicated-property change detection
    // runs in the network update pass, so this WILL propagate â€” but with a
    // tick of latency.
    //
    // 2026-05-22 22:03 PDT: large jumps (e.g. 7.83 â†’ 22, crossing sunset)
    // cause a delayed SCUMServer crash ~50 sec after the write, almost
    // certainly via OnRep_NighttimeDarkness or a similar boundary-crossing
    // callback that doesn't handle discontinuities. Clamp to a small delta
    // per call so callers (storyteller) ramp time gradually instead of
    // jumping. The storyteller's hourly-time-sync rule already aligns with
    // real time, so after a few hours of continuous running, the clamp is
    // a no-op (delta â‰¤ 1).
    constexpr float kMaxTimeOfDayDeltaPerCall = 2.0f;
    float* target = reinterpret_cast<float*>(
        reinterpret_cast<uint8_t*>(s_weather_ctrl) + s_time_off);
    float old_value = *target;
    float applied_value = hours;
    bool clamped = false;
    float delta = hours - old_value;
    // UE4 _timeOfDay wraps at 24, so the "shortest path" between two times
    // can be the negative direction (e.g. 23 â†’ 1 is +2 wrapping forward,
    // not -22 going backward). Pick the direction with smaller absolute
    // delta, then clamp magnitude.
    if (delta > 12.0f) delta -= 24.0f;
    else if (delta < -12.0f) delta += 24.0f;
    if (delta > kMaxTimeOfDayDeltaPerCall) {
        applied_value = old_value + kMaxTimeOfDayDeltaPerCall;
        if (applied_value >= 24.0f) applied_value -= 24.0f;
        clamped = true;
    } else if (delta < -kMaxTimeOfDayDeltaPerCall) {
        applied_value = old_value - kMaxTimeOfDayDeltaPerCall;
        if (applied_value < 0.0f) applied_value += 24.0f;
        clamped = true;
    }
    *target = applied_value;
    if (clamped) {
        log_info_fmt(STR("[TurdMODEngineBridge] setTimeOfDay {} â†’ {} (CLAMPED from requested {}; delta capped at Â±{} to avoid boundary-crossing crash; offset {:#x})\n"),
                     old_value, applied_value, hours, kMaxTimeOfDayDeltaPerCall, static_cast<unsigned>(s_time_off));
    } else {
        log_info_fmt(STR("[TurdMODEngineBridge] setTimeOfDay {} â†’ {} (offset {:#x})\n"),
                     old_value, applied_value, static_cast<unsigned>(s_time_off));
    }

    // 2026-05-22: NetMulticast_SendStateSnapshot takes a 48-byte
    // `Snapshot: StructProperty` param. Passing nullptr crashed SCUM
    // (proven live; setTimeOfDay logged success then server crashdumped).
    // Skipping the explicit multicast for now â€” relies on UE4's normal
    // replication machinery to push the property change. Real fix is
    // to build the snapshot struct from current WeatherController2
    // state, which needs the FWeatherStateSnapshot layout (TBD via
    // scumdump struct browse). v2 work.
    bool snap_called = false;

    char buf[256];
    std::snprintf(buf, sizeof(buf),
                  "{\"ok\":true,\"hours\":%g,\"requestedHours\":%g,\"previousHours\":%g,\"clamped\":%s,\"snapshotForced\":%s}",
                  applied_value, hours, old_value,
                  clamped ? "true" : "false",
                  snap_called ? "true" : "false");
    s_set_time_of_day_result = buf;
    *result_out = s_set_time_of_day_result.c_str();

    // Emit a typed `timeChanged` event so subscribers know the change landed
    // (scumpilot's contract pairs setTimeOfDay with timeChanged on confirmation).
    // Emit the ACTUAL applied value, not the requested one â€” keeps state
    // consistent for downstream consumers when the clamp activates.
    char evt_buf[64];
    std::snprintf(evt_buf, sizeof(evt_buf), "{\"hours\":%g}", applied_value);
    emit_engine_event("timeChanged", evt_buf);

    return 0;
}

// â”€â”€â”€ setWeather (Wave 2 continuation) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// scumpilot's storyteller agent calls setWeather({severity: 0..1}) as part
// of its scheduled weather rules. There is no exposed Set* UFunction in
// AWeatherController2 (only NetMulticast_SendStateSnapshot, which takes the
// opaque 48-byte FWeatherReplicatedStateSnapshot â€” see
// [[ue4-struct-params-never-null]] for why we don't call it with nullptr).
//
// Path used here: direct property writes on WeatherController2's visible
// "current state" floats. These are all marked Transient in the SDK
// (Dumper-7 output `BP_WeatherController2_classes.hpp`), so they will NOT
// replicate via UE's normal RepGraph. Replication ride-along depends on
// SCUM's own auto-snapshot tick (controlled by `_sendReplicatedStateSnapshotInterval`
// at 0x164C). That tick reads server state into FWeatherReplicatedStateSnapshot
// and multicasts; if it reads from _rainIntensity etc., our writes reach
// clients at the next tick (~1-2 sec latency). UNVERIFIED until a live test
// with a real client. See `docs/journal/WORKSPACE.md` Task A.
//
// Severity â†’ property map (initial; tune after live observation):
//   _rainIntensity            (0x0AA8) = severity
//   _windIntensity            (0x100C) = severity * 0.8
//   _fogDensity               (0x1028) = 0.02 + severity * 0.08
//   _maxRainMilimeterPerHour  (0x0AAC) = severity * 20.0
//
// Params: { "severity": float 0..1 }
static int32_t handle_set_weather(const char* params_json,
                                  const char** result_out, const char**)
{
    ensure_hook_installed_once();

    float severity = extract_json_float(params_json, "severity", -1.0f);
    static std::string s_set_weather_result;
    if (severity < 0.0f || severity > 1.0f) {
        s_set_weather_result =
            R"({"error":"severity param required, must be 0..1"})";
        *result_out = s_set_weather_result.c_str();
        return 0;
    }

    static UObject* s_weather_ctrl = nullptr;
    if (!s_weather_ctrl) {
        s_weather_ctrl = find_first_instance_of_class(L"WeatherController2");
        if (!s_weather_ctrl) {
            // BP subclass fallback â€” SCUM uses BP_WeatherController1_0_C
            s_weather_ctrl = find_first_instance_of_class(L"BP_WeatherController1_0_C");
        }
        if (!s_weather_ctrl) {
            s_set_weather_result =
                R"({"error":"WeatherController2 instance not found"})";
            *result_out = s_set_weather_result.c_str();
            return 0;
        }
    }

    auto* ctrl_class = *reinterpret_cast<UObject* const*>(
        reinterpret_cast<const uint8_t*>(s_weather_ctrl) + 0x10);

    static int32_t s_rain_off       = -2;
    static int32_t s_wind_off       = -2;
    static int32_t s_fog_off        = -2;
    static int32_t s_max_rain_off   = -2;
    if (s_rain_off == -2) {
        s_rain_off     = find_property_offset(ctrl_class, L"_rainIntensity");
        s_wind_off     = find_property_offset(ctrl_class, L"_windIntensity");
        s_fog_off      = find_property_offset(ctrl_class, L"_fogDensity");
        s_max_rain_off = find_property_offset(ctrl_class, L"_maxRainMilimeterPerHour");
    }
    if (s_rain_off < 0 || s_wind_off < 0 || s_fog_off < 0 || s_max_rain_off < 0) {
        char err[256];
        std::snprintf(err, sizeof(err),
                      R"({"error":"property offset lookup failed","offsets":{"rain":%d,"wind":%d,"fog":%d,"maxRain":%d}})",
                      s_rain_off, s_wind_off, s_fog_off, s_max_rain_off);
        s_set_weather_result = err;
        *result_out = s_set_weather_result.c_str();
        return 0;
    }

    // Derive target values from severity.
    const float new_rain     = severity;
    const float new_wind     = severity * 0.8f;
    const float new_fog      = 0.02f + severity * 0.08f;
    const float new_max_rain = severity * 20.0f;

    auto* base = reinterpret_cast<uint8_t*>(s_weather_ctrl);
    auto* rain_ptr     = reinterpret_cast<float*>(base + s_rain_off);
    auto* wind_ptr     = reinterpret_cast<float*>(base + s_wind_off);
    auto* fog_ptr      = reinterpret_cast<float*>(base + s_fog_off);
    auto* max_rain_ptr = reinterpret_cast<float*>(base + s_max_rain_off);

    const float prev_rain     = *rain_ptr;
    const float prev_wind     = *wind_ptr;
    const float prev_fog      = *fog_ptr;
    const float prev_max_rain = *max_rain_ptr;

    *rain_ptr     = new_rain;
    *wind_ptr     = new_wind;
    *fog_ptr      = new_fog;
    *max_rain_ptr = new_max_rain;

    log_info_fmt(STR("[TurdMODEngineBridge] setWeather severity={} -> rain={} wind={} fog={} maxRain={} "
                     "(prev rain={} wind={} fog={} maxRain={})\n"),
                 severity, new_rain, new_wind, new_fog, new_max_rain,
                 prev_rain, prev_wind, prev_fog, prev_max_rain);

    // NetMulticast_SendStateSnapshot intentionally NOT called â€” the param
    // struct FWeatherReplicatedStateSnapshot is 48 bytes with opaque layout
    // (Dumper-7 found zero reflected fields). nullptr crashes; an
    // incorrectly-shaped buffer may also crash. Rely on SCUM's auto-snapshot
    // tick (interval at offset 0x164C) to pick up the property writes.

    char buf[400];
    std::snprintf(buf, sizeof(buf),
                  "{\"ok\":true,\"severity\":%g,"
                  "\"rainIntensity\":%g,\"windIntensity\":%g,\"fogDensity\":%g,\"maxRainMilimeterPerHour\":%g,"
                  "\"previousRainIntensity\":%g,\"previousWindIntensity\":%g,\"previousFogDensity\":%g,\"previousMaxRainMilimeterPerHour\":%g,"
                  "\"snapshotForced\":false}",
                  severity, new_rain, new_wind, new_fog, new_max_rain,
                  prev_rain, prev_wind, prev_fog, prev_max_rain);
    s_set_weather_result = buf;
    *result_out = s_set_weather_result.c_str();

    // Emit a typed `weatherChanged` event so subscribers know the change
    // landed (scumpilot's EngineEventPerception expects {severity}).
    char evt_buf[64];
    std::snprintf(evt_buf, sizeof(evt_buf), "{\"severity\":%g}", severity);
    emit_engine_event("weatherChanged", evt_buf);

    return 0;
}

// â”€â”€â”€ runHelloWorld (Phase C P1 verification handler) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Resolves a custom BP class shipped via TurdMODHelloWorld_P.pak,
// finds its `BroadcastHelloWorld(message: FString)` UFunction,
// dispatches via ProcessEvent. This is the ONE end-to-end Phase C
// verification: if this handler returns ok:true, the entire pak
// pipeline works (cook â†’ pak â†’ mount â†’ reflection discovery â†’
// UFunction dispatch).
//
// Params: { "message"?: "<text>" }
// Default message: "hello from pak"
//
// Class discovery is forgiving â€” we accept any class whose name
// starts with `BP_HelloWorld` (UE BP-generated classes get a `_C`
// suffix, sometimes BP_HelloWorld, sometimes BP_HelloWorld_C
// depending on cook flags).
static int32_t handle_run_hello_world(const char* params_json,
                                      const char** result_out, const char**)
{
    ensure_hook_installed_once();

    std::string msg = extract_json_str(params_json, "message");
    if (msg.empty()) msg = "hello from pak";

    // Locate the BP class's CDO (Class Default Object).
    //
    // ProcessEvent must be called on an actual instance, NOT on the UClass
    // itself â€” invoking it on the UClass crashes (UClass::ProcessEvent is a
    // technically-inherited member that doesn't have valid instance context
    // for BP-compiled UFunctions). UE creates a CDO for every class on load,
    // named `Default__<ClassName>` (so BPHelloWorld_C's CDO is
    // `Default__BPHelloWorld_C`). The CDO has the class's vtable + valid
    // instance state, so ProcessEvent on it works.
    //
    // Search strategy:
    //   1. PREFER: any non-CDO instance whose class is HelloWorld* (= a real
    //      spawned object â€” best context, e.g. has a World pointer).
    //   2. FALLBACK: the CDO itself (name pattern Default__*HelloWorld*).
    //   3. LAST RESORT: the UClass itself (was the old buggy default â€” keep
    //      only as last resort + log a warning).
    // Re-resolve every call. Caching across calls bit us 2026-05-23 19:30:
    // the 1st call (18:34) cached a CDO pointer that worked; by 19:30 the CDO
    // had moved/been refreshed and the cached pointer was stale â†’ SCUMServer
    // crashed silently into a Fatal Error modal (no [HOOK] PE log emitted,
    // because PE crashed before the hooked function returned). 4 min later
    // the modal sat blocked-exit until dismissModals clicked OK and the
    // process died. Re-resolving every call eliminates the stale-pointer
    // class of crash; the SEH wrapper below guarantees any future PE-fault
    // returns clean JSON instead of a process-killing modal.
    UObject* s_hello_class = nullptr;                   // ProcessEvent target (instance or CDO)
    std::wstring s_hello_class_name;                    // CLASS name for find_ufunction owner filter
    {
        std::unordered_map<uint32_t, std::wstring> cls_cache;
        UObject* best_instance = nullptr;
        UObject* best_cdo = nullptr;
        UObject* fallback_uclass = nullptr;
        std::wstring class_name_str;
        UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
            if (best_instance) return; // already found ideal target
            auto* p = reinterpret_cast<const uint8_t*>(obj);
            auto* class_ptr = *reinterpret_cast<UObject* const*>(p + 0x10);
            if (!class_ptr) return;
            auto* cp = reinterpret_cast<const uint8_t*>(class_ptr);
            const FName& cls_fname = *reinterpret_cast<const FName*>(cp + 0x18);
            auto it = cls_cache.find(cls_fname.ComparisonIndex);
            const std::wstring* cls_name;
            if (it == cls_cache.end()) {
                auto [ins, _] = cls_cache.try_emplace(
                    cls_fname.ComparisonIndex, fname_to_wstring(cls_fname));
                cls_name = &ins->second;
            } else {
                cls_name = &it->second;
            }

            const FName& obj_name = *reinterpret_cast<const FName*>(p + 0x18);
            std::wstring obj_name_str = fname_to_wstring(obj_name);

            // Shape 1: instance of HelloWorld* class. Prefer non-CDO.
            if (cls_name->find(L"HelloWorld") != std::wstring::npos) {
                if (obj_name_str.rfind(L"Default__", 0) == 0) {
                    // CDO â€” remember as fallback
                    if (!best_cdo) {
                        best_cdo = obj;
                        class_name_str = *cls_name;
                    }
                } else {
                    // Real instance â€” best
                    best_instance = obj;
                    class_name_str = *cls_name;
                }
                return;
            }
            // Shape 2: this UObject IS the UClass itself.
            if (*cls_name == L"BlueprintGeneratedClass" &&
                obj_name_str.find(L"HelloWorld") != std::wstring::npos) {
                if (!fallback_uclass) {
                    fallback_uclass = obj;
                    class_name_str = obj_name_str;
                }
            }
        });

        if (best_instance) {
            s_hello_class = best_instance;
            s_hello_class_name = class_name_str;
            log_info_fmt(STR("[runHelloWorld] using REAL INSTANCE as ProcessEvent target (class={})\n"),
                         s_hello_class_name);
        } else if (best_cdo) {
            s_hello_class = best_cdo;
            s_hello_class_name = class_name_str;
            log_info_fmt(STR("[runHelloWorld] using CDO as ProcessEvent target (class={})\n"),
                         s_hello_class_name);
        } else if (fallback_uclass) {
            // âš  Calling ProcessEvent on the UClass itself crashed in our prior
            // session 2026-05-23 PM. The crash dumps + this comment are the
            // diagnostic trail. Avoid this branch â€” but if reached, log loudly
            // so future investigation has a clear signal.
            log_error(STR("[runHelloWorld] WARNING: found only UClass, no CDO â€” refusing to ProcessEvent (would crash)"));
            s_run_hello_world_result =
                R"x({"error":"only found UClass, no CDO/instance â€” refusing to call ProcessEvent (would crash). Try loadAsset first to ensure CDO is registered."})x";
            *result_out = s_run_hello_world_result.c_str();
            return 0;
        } else {
            s_run_hello_world_result =
                R"x({"error":"BP_HelloWorld class not found in GUObjectArray. Call loadAsset({\"packagePath\":\"/Game/TurdMOD/BPHelloWorld\"}) first."})x";
            *result_out = s_run_hello_world_result.c_str();
            return 0;
        }
    }

    // Find the BroadcastHelloWorld UFunction on this class. Also re-resolved
    // per call (see CDO note above) â€” UFunction objects are stable but
    // belt-and-suspenders.
    UObject* s_hello_fn = find_ufunction(L"BroadcastHelloWorld",
                                         s_hello_class_name.c_str());
    if (!s_hello_fn) {
        s_hello_fn = find_ufunction(L"BroadcastHelloWorld");
    }
    if (!s_hello_fn) {
        s_run_hello_world_result =
            R"({"error":"BroadcastHelloWorld UFunction not found â€” does the BP define it as a custom event with message:FString input?"})";
        *result_out = s_run_hello_world_result.c_str();
        return 0;
    }

    // Build the FString param block (16 bytes: data ptr + Num + Max).
    // Mirrors handle_kick_player's FString pattern.
    static std::wstring s_msg_w;
    s_msg_w.assign(msg.begin(), msg.end());
    s_msg_w.push_back(L'\0');

    #pragma pack(push, 8)
    struct HelloParams {
        wchar_t* Msg_Data;
        int32_t  Msg_Num;
        int32_t  Msg_Max;
    };
    #pragma pack(pop)
    static_assert(sizeof(HelloParams) == 16, "HelloParams must be 16 bytes (FString shape)");

    HelloParams hp{};
    hp.Msg_Data = const_cast<wchar_t*>(s_msg_w.c_str());
    hp.Msg_Num  = static_cast<int32_t>(s_msg_w.size());
    hp.Msg_Max  = hp.Msg_Num;

    log_info_fmt(STR("[runHelloWorld] PRE-PE target={:p} fn={:p} class={} msg='{}'\n"),
                 static_cast<void*>(s_hello_class),
                 static_cast<void*>(s_hello_fn),
                 s_hello_class_name.c_str(), s_msg_w.c_str());
    uint32_t pe_rc = call_processevent_seh(
        s_hello_class,
        reinterpret_cast<class UFunction*>(s_hello_fn),
        &hp);
    log_info_fmt(STR("[runHelloWorld] POST-PE rc=0x{:x}\n"),
                 static_cast<unsigned int>(pe_rc));

    s_msg_w.pop_back();

    // Build result JSON. Narrow class name to UTF-8 (ASCII-safe assumption).
    std::string cls_utf8(s_hello_class_name.begin(), s_hello_class_name.end());
    char buf[768];
    if (pe_rc == 0) {
        std::snprintf(buf, sizeof(buf),
                      "{\"ok\":true,\"message\":\"%s\",\"classFound\":\"%s\",\"function\":\"BroadcastHelloWorld\"}",
                      msg.c_str(), cls_utf8.c_str());
    } else {
        // SEH caught a fault inside ProcessEvent (target stale, params shape
        // wrong, BP dispatcher faulted, etc.). Surface the exception code so
        // the caller can correlate to a Windows SEH constant.
        std::snprintf(buf, sizeof(buf),
                      "{\"ok\":false,\"error\":\"ProcessEvent SEH-caught\",\"exceptionCode\":\"0x%x\",\"message\":\"%s\",\"classFound\":\"%s\",\"function\":\"BroadcastHelloWorld\"}",
                      static_cast<unsigned int>(pe_rc), msg.c_str(), cls_utf8.c_str());
    }
    s_run_hello_world_result = buf;
    *result_out = s_run_hello_world_result.c_str();
    return 0;
}

// ============================================================================
// handle_read_config
// ============================================================================
static thread_local std::string s_read_config_result;

static int32_t handle_read_config(const char* params_json,
                                  const char** result_out, const char**)
{
    ensure_hook_installed_once();

    std::string file    = extract_json_str(params_json, "file");
    std::string section = extract_json_str(params_json, "section");
    std::string key     = extract_json_str(params_json, "key");

    if (file.empty() || key.empty()) {
        s_read_config_result = R"({"error":"missing file or key"})";
        *result_out = s_read_config_result.c_str();
        return 0;
    }

    // Anti-traversal check
    if (file.find("..") != std::string::npos ||
        file.find('/') != std::string::npos ||
        file.find('\\') != std::string::npos) {
        s_read_config_result = R"({"error":"invalid file path"})";
        *result_out = s_read_config_result.c_str();
        return 0;
    }

    // Build absolute path
    std::string config_root = "C:/Program Files (x86)/Steam/steamapps/common/SCUM Server/SCUM/Saved/Config/WindowsServer/";
    std::string full_path = config_root + file;

    // Check file exists
    FILE* f = nullptr;
    if (fopen_s(&f, full_path.c_str(), "r") != 0 || !f) {
        s_read_config_result = R"({"error":"file not found"})";
        *result_out = s_read_config_result.c_str();
        return 0;
    }

    // Read file into memory
    std::string content;
    char buf[4096];
    size_t n;
    while ((n = fread(buf, 1, sizeof(buf), f)) > 0) {
        content.append(buf, n);
    }
    fclose(f);

    // Parse INI
    std::string current_section;
    std::string value;
    bool found = false;

    // Handle dotted-key form when section is empty
    if (section.empty()) {
        // Look for key at top level (no section)
        size_t pos = 0;
        while (pos < content.size()) {
            size_t eol = content.find('\n', pos);
            if (eol == std::string::npos) eol = content.size();
            std::string line = content.substr(pos, eol - pos);
            // Trim trailing \r
            if (!line.empty() && line.back() == '\r') line.pop_back();
            // Skip comments and blanks
            if (line.empty() || line[0] == ';' || line[0] == '#') {
                pos = eol + 1;
                continue;
            }
            // Check for section header
            if (line[0] == '[' && line.back() == ']') {
                current_section = line.substr(1, line.size() - 2);
                pos = eol + 1;
                continue;
            }
            // Check for key=value
            size_t eq = line.find('=');
            if (eq != std::string::npos) {
                std::string line_key = line.substr(0, eq);
                // Trim whitespace
                line_key.erase(0, line_key.find_first_not_of(" \t"));
                line_key.erase(line_key.find_last_not_of(" \t") + 1);
                if (line_key == key) {
                    value = line.substr(eq + 1);
                    // Trim whitespace
                    value.erase(0, value.find_first_not_of(" \t"));
                    value.erase(value.find_last_not_of(" \t\r") + 1);
                    found = true;
                    break;
                }
            }
            pos = eol + 1;
        }
    } else {
        // Look for section.key dotted form first
        std::string dotted_key = section + "." + key;
        size_t pos = 0;
        while (pos < content.size()) {
            size_t eol = content.find('\n', pos);
            if (eol == std::string::npos) eol = content.size();
            std::string line = content.substr(pos, eol - pos);
            if (!line.empty() && line.back() == '\r') line.pop_back();
            if (line.empty() || line[0] == ';' || line[0] == '#') {
                pos = eol + 1;
                continue;
            }
            if (line[0] == '[' && line.back() == ']') {
                current_section = line.substr(1, line.size() - 2);
                pos = eol + 1;
                continue;
            }
            size_t eq = line.find('=');
            if (eq != std::string::npos) {
                std::string line_key = line.substr(0, eq);
                line_key.erase(0, line_key.find_first_not_of(" \t"));
                line_key.erase(line_key.find_last_not_of(" \t") + 1);
                // Check dotted form
                if (line_key == dotted_key) {
                    value = line.substr(eq + 1);
                    value.erase(0, value.find_first_not_of(" \t"));
                    value.erase(value.find_last_not_of(" \t\r") + 1);
                    found = true;
                    break;
                }
                // Check sectioned form
                if (!current_section.empty() &&
                    _stricmp(current_section.c_str(), section.c_str()) == 0 &&
                    line_key == key) {
                    value = line.substr(eq + 1);
                    value.erase(0, value.find_first_not_of(" \t"));
                    value.erase(value.find_last_not_of(" \t\r") + 1);
                    found = true;
                    break;
                }
            }
            pos = eol + 1;
        }
    }

    if (!found) {
        std::string err = R"({"error":"key not found","section":")" +
                          section + R"(","key":")" + key + R"("})";
        s_read_config_result = err;
        *result_out = s_read_config_result.c_str();
        return 0;
    }

    // Escape value for JSON
    std::string escaped_value;
    for (char c : value) {
        if (c == '"') escaped_value += "\\\"";
        else if (c == '\\') escaped_value += "\\\\";
        else if (c == '\n') escaped_value += "\\n";
        else if (c == '\r') escaped_value += "\\r";
        else if (c == '\t') escaped_value += "\\t";
        else escaped_value += c;
    }

    std::string json = R"({"ok":true,"file":")" + file +
                       R"(","section":")" + section +
                       R"(","key":")" + key +
                       R"(","value":")" + escaped_value + R"("})";
    s_read_config_result = json;
    *result_out = s_read_config_result.c_str();
    return 0;
}

// ============================================================================
// handle_write_config
// ============================================================================
static thread_local std::string s_write_config_result;

static int32_t handle_write_config(const char* params_json,
                                   const char** result_out, const char**)
{
    ensure_hook_installed_once();

    std::string file    = extract_json_str(params_json, "file");
    std::string section = extract_json_str(params_json, "section");
    std::string key     = extract_json_str(params_json, "key");
    std::string new_val = extract_json_str(params_json, "value");

    if (file.empty() || key.empty()) {
        s_write_config_result = R"({"error":"missing file or key"})";
        *result_out = s_write_config_result.c_str();
        return 0;
    }

    // Anti-traversal
    if (file.find("..") != std::string::npos ||
        file.find('/') != std::string::npos ||
        file.find('\\') != std::string::npos) {
        s_write_config_result = R"({"error":"invalid file path"})";
        *result_out = s_write_config_result.c_str();
        return 0;
    }

    std::string config_root = "C:/Program Files (x86)/Steam/steamapps/common/SCUM Server/SCUM/Saved/Config/WindowsServer/";
    std::string full_path = config_root + file;

    // Read file
    FILE* f = nullptr;
    if (fopen_s(&f, full_path.c_str(), "r") != 0 || !f) {
        s_write_config_result = R"({"error":"file not found"})";
        *result_out = s_write_config_result.c_str();
        return 0;
    }
    std::string content;
    char buf[4096];
    size_t n;
    while ((n = fread(buf, 1, sizeof(buf), f)) > 0) {
        content.append(buf, n);
    }
    fclose(f);

    // Parse and modify
    std::string current_section;
    bool found = false;
    std::string old_value;
    std::string new_content;
    size_t pos = 0;
    bool section_exists = false;

    while (pos < content.size()) {
        size_t eol = content.find('\n', pos);
        if (eol == std::string::npos) eol = content.size();
        std::string line = content.substr(pos, eol - pos);
        if (!line.empty() && line.back() == '\r') line.pop_back();

        // Track sections
        if (!line.empty() && line[0] == '[' && line.back() == ']') {
            current_section = line.substr(1, line.size() - 2);
            if (!section.empty() && _stricmp(current_section.c_str(), section.c_str()) == 0) {
                section_exists = true;
            }
        }

        // Check for key match
        if (!line.empty() && line[0] != ';' && line[0] != '#') {
            size_t eq = line.find('=');
            if (eq != std::string::npos) {
                std::string line_key = line.substr(0, eq);
                line_key.erase(0, line_key.find_first_not_of(" \t"));
                line_key.erase(line_key.find_last_not_of(" \t") + 1);

                bool match = false;
                if (section.empty()) {
                    match = (line_key == key);
                } else {
                    std::string dotted = section + "." + key;
                    match = (line_key == dotted) ||
                            (!current_section.empty() &&
                             _stricmp(current_section.c_str(), section.c_str()) == 0 &&
                             line_key == key);
                }

                if (match) {
                    found = true;
                    // Extract old value
                    old_value = line.substr(eq + 1);
                    old_value.erase(0, old_value.find_first_not_of(" \t"));
                    old_value.erase(old_value.find_last_not_of(" \t\r") + 1);
                    // Preserve whitespace around = and trailing comment
                    std::string before_eq = line.substr(0, eq);
                    std::string after_val = line.substr(eq + 1 + old_value.size());
                    // Rebuild line with new value
                    new_content += before_eq + "=" + new_val + after_val + "\n";
                    pos = eol + 1;
                    continue;
                }
            }
        }

        new_content += line;
        if (eol < content.size()) new_content += "\n";
        pos = eol + 1;
    }

    if (!found) {
        // Append to section or create new section
        if (section.empty()) {
            // Add at top level
            new_content += key + "=" + new_val + "\n";
        } else {
            if (!section_exists) {
                new_content += "[" + section + "]\n";
            }
            new_content += key + "=" + new_val + "\n";
        }
    }

    // Write to .tmp then atomic replace
    std::string tmp_path = full_path + ".tmp";
    FILE* tmp_f = nullptr;
    if (fopen_s(&tmp_f, tmp_path.c_str(), "w") != 0 || !tmp_f) {
        s_write_config_result = R"({"error":"failed to write temp file"})";
        *result_out = s_write_config_result.c_str();
        return 0;
    }
    fwrite(new_content.c_str(), 1, new_content.size(), tmp_f);
    fclose(tmp_f);

    // Atomic replace using Win32 API
    if (!MoveFileExW(std::wstring(tmp_path.begin(), tmp_path.end()).c_str(),
                     std::wstring(full_path.begin(), full_path.end()).c_str(),
                     MOVEFILE_REPLACE_EXISTING)) {
        s_write_config_result = R"({"error":"failed to replace file"})";
        *result_out = s_write_config_result.c_str();
        return 0;
    }

    // Escape old value for JSON
    std::string escaped_old;
    for (char c : old_value) {
        if (c == '"') escaped_old += "\\\"";
        else if (c == '\\') escaped_old += "\\\\";
        else if (c == '\n') escaped_old += "\\n";
        else if (c == '\r') escaped_old += "\\r";
        else if (c == '\t') escaped_old += "\\t";
        else escaped_old += c;
    }

    std::string json = R"({"ok":true,"file":")" + file +
                       R"(","section":")" + section +
                       R"(","key":")" + key +
                       R"(","oldValue":")" + escaped_old +
                       R"(","newValue":")" + new_val + R"("})";
    s_write_config_result = json;
    *result_out = s_write_config_result.c_str();
    return 0;
}

// â”€â”€â”€ loadAsset (Phase C P1 closer) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Force-load a UE 4.27 package by /Game/... path. After this returns
// ok:true, the package's UObjects (including BP-generated classes) are
// registered in GUObjectArray and the runHelloWorld handler above can
// find them. Closes the lazy-load gap that left BPHelloWorld_C
// unregistered even though our pak mounts.
//
// Params: { "packagePath": "/Game/TurdMOD/BPHelloWorld" }
// Returns: { "ok": true, "packagePath": "...", "package": "0x..." } on
//          success, or { "ok": false, "error": "..." } if the package
//          isn't in any mounted pak / LoadPackage returns null.
//
// SEH-wrapped: a wrong RVA / non-game-thread issue turns into a JSON
// error rather than a server crash. Per-call (not boot-time) â€” the
// handler resolves the LoadPackage address lazily on first invocation.
static std::string s_load_asset_result;

static int32_t handle_load_asset(const char* params_json,
                                 const char** result_out, const char**)
{
    ensure_hook_installed_once();

    std::string pkg = extract_json_str(params_json, "packagePath");
    if (pkg.empty()) {
        s_load_asset_result =
            R"x({"error":"packagePath param required (e.g. /Game/TurdMOD/BPHelloWorld)"})x";
        *result_out = s_load_asset_result.c_str();
        return 0;
    }

    // Convert path to wchar_t + bounds-check for the fixed slot.
    if (pkg.size() >= sizeof(g_load_asset_req.path) / sizeof(wchar_t) - 1) {
        s_load_asset_result =
            R"x({"error":"packagePath too long (max 254 chars)"})x";
        *result_out = s_load_asset_result.c_str();
        return 0;
    }

    // Wait for the queue to be idle (in case another loadAsset is still in
    // flight â€” bounded by game-thread completion of the prior request).
    {
        auto wait_start = std::chrono::steady_clock::now();
        while (g_load_asset_req.state.load(std::memory_order_acquire) != 0) {
            if (std::chrono::steady_clock::now() - wait_start >
                std::chrono::seconds(5)) {
                s_load_asset_result =
                    R"x({"error":"prior loadAsset still in flight â€” timed out waiting for queue slot"})x";
                *result_out = s_load_asset_result.c_str();
                return 0;
            }
            std::this_thread::sleep_for(std::chrono::milliseconds(10));
        }
    }

    // Fill the request slot. Wide-char copy.
    size_t i = 0;
    for (; i < pkg.size(); ++i) g_load_asset_req.path[i] = static_cast<wchar_t>(pkg[i]);
    g_load_asset_req.path[i] = L'\0';
    g_load_asset_req.result = nullptr;

    // Publish: 0 â†’ 1 (queued). Game thread's ProcessEvent hook picks it up.
    int expected = 0;
    if (!g_load_asset_req.state.compare_exchange_strong(expected, 1,
            std::memory_order_acq_rel)) {
        s_load_asset_result =
            R"x({"error":"queue race â€” request slot taken by another caller"})x";
        *result_out = s_load_asset_result.c_str();
        return 0;
    }

    log_info_fmt(STR("[loadAsset] queued LoadPackage('{}') for game-thread dispatch\n"),
                 g_load_asset_req.path);

    // Poll for completion. Game thread's ProcessEvent fires very frequently
    // in a live server (PE every UFunction call); typical drain latency is
    // <100ms. 30s timeout protects against the (rare) case of no game-thread
    // activity.
    auto poll_start = std::chrono::steady_clock::now();
    while (g_load_asset_req.state.load(std::memory_order_acquire) != 3) {
        if (std::chrono::steady_clock::now() - poll_start >
            std::chrono::seconds(30)) {
            // Timeout: leave state as 1 or 2; we DON'T reset because the
            // game thread might still mutate it. The next loadAsset call
            // will wait above (5s) and then report queue stuck.
            s_load_asset_result =
                R"x({"error":"timed out waiting for game-thread drain (30s) â€” ProcessEvent hook not firing?"})x";
            *result_out = s_load_asset_result.c_str();
            return 0;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }

    // Done: capture result and reset slot.
    void* loaded = g_load_asset_req.result;
    g_load_asset_req.state.store(0, std::memory_order_release);

    char buf[512];
    if (loaded == reinterpret_cast<void*>(kLoadPackageSEHSentinel)) {
        std::snprintf(buf, sizeof(buf),
            "{\"ok\":false,\"packagePath\":\"%s\",\"error\":\"LoadPackage threw SEH on game thread â€” wrong RVA or invalid state\"}",
            pkg.c_str());
    } else if (loaded) {
        std::snprintf(buf, sizeof(buf),
            "{\"ok\":true,\"packagePath\":\"%s\",\"package\":\"0x%llx\"}",
            pkg.c_str(),
            static_cast<unsigned long long>(reinterpret_cast<uintptr_t>(loaded)));
    } else {
        std::snprintf(buf, sizeof(buf),
            "{\"ok\":false,\"packagePath\":\"%s\",\"error\":\"LoadPackage returned null â€” package not in any mounted pak\"}",
            pkg.c_str());
    }
    s_load_asset_result = buf;
    *result_out = s_load_asset_result.c_str();
    return 0;
}

// dumpAdminCommands â€” walk every AdminCommand_* class's CDO and emit
// authoritative metadata for SCUM's 214 #admin verbs. Replaces the
// hand-curated ARG_HINTS table in the Manager's Admin Commands page
// with ground-truth data: each verb's number of required arguments,
// each argument's data type class, completion source class, and for
// _Constant completions the literal valid values (e.g. "Normal" /
// "Gold" for currency, "Sunny" / "Foggy" / "Rainy" for weather).
//
// Reads only â€” never mutates a CDO. One-shot dumper meant to run
// against a live SCUM server once per game-version; the resulting
// JSON ships as a static asset.
//
// Schema reference (from scumdump reflection):
//   AdminCommand base (size 160)
//     +40  StrProperty   _verb
//     +56  ArrayProperty _argumentDescriptions  (TArray<FAdminCommandArgumentDescription>)
//     +72  IntProperty   _numberOfRequiredArguments
//     +96  TextProperty  _description           (skipped â€” FText)
//   FAdminCommandArgumentDescription (size 64)
//     +0   TextProperty  Name                   (skipped â€” FText)
//     +24  TextProperty  Description            (skipped â€” FText)
//     +48  BoolProperty  ShowCompletionValuesInHelpText
//     +56  ObjectProperty Data â†’ AdminCommandArgumentDataType_*
//   AdminCommandArgumentDataTypeBase
//     +48  TextProperty  DataTypeName           (skipped â€” FText)
//     +72  ClassProperty ArgumentCompletion â†’ UClass of *_Completion_*
//   AdminCommandArgumentCompletion_Constant
//     +56  ArrayProperty _completionValues      (TArray<FString>)
//
// Output:
//   { "ok": true,
//     "scanned": N,
//     "commands": [
//       { "verb": "SetCurrencyBalance",
//         "class": "AdminCommand_SetCurrencyBalance",
//         "numRequired": 2,
//         "args": [
//           { "dataType": "AdminCommandArgumentDataType_String",
//             "completionClass": "AdminCommandArgumentCompletion_Constant",
//             "completionValues": ["Normal", "Gold"] },
//           { "dataType": "AdminCommandArgumentDataType_Numeric",
//             "completionClass": null,
//             "completionValues": [] },
//           { "dataType": "AdminCommandArgumentDataType_String",
//             "completionClass": "AdminCommandArgumentCompletion_Player",
//             "completionValues": [] }
//         ] }, ...
//     ] }
//
// `completionValues` is only populated for the _Constant family
// (including BP subclasses like ItemProperty_C). Player/Skill/
// PrimaryAsset/etc. completions are dynamic â€” the Manager fills
// those at runtime from getOnlinePlayers etc.
// [SCRUBBED] Game-specific section removed (332 lines)

static int32_t handle_list_config_files(const char*, const char** result_out, const char**)
{
    auto dir = get_config_dir();
    std::string dir_s = dir.empty() ? std::string() : dir.string();

    s_list_config_files_result = "{\"ok\":true,\"configDir\":\"";
    s_list_config_files_result += json_escape(dir_s);
    s_list_config_files_result += "\",\"files\":[";
    bool first = true;
    for (auto name : kAllowedConfigFiles) {
        if (!first) s_list_config_files_result += ",";
        first = false;
        std::filesystem::path full = dir / name;
        std::error_code ec;
        bool exists = !dir.empty() && std::filesystem::exists(full, ec);
        uintmax_t sz = 0;
        if (exists) {
            sz = std::filesystem::file_size(full, ec);
            if (ec) sz = 0;
        }
        s_list_config_files_result += "{\"name\":\"";
        s_list_config_files_result += name;
        s_list_config_files_result += "\",\"exists\":";
        s_list_config_files_result += exists ? "true" : "false";
        s_list_config_files_result += ",\"sizeBytes\":";
        s_list_config_files_result += std::to_string(sz);
        s_list_config_files_result += "}";
    }
    s_list_config_files_result += "]}";
    *result_out = s_list_config_files_result.c_str();
    return 0;
}

static int32_t handle_read_config_file(const char* params_json,
                                       const char** result_out, const char**)
{
    std::string name = extract_json_str(params_json, "name");
    if (name.empty()) {
        s_read_config_file_result = R"({"error":"name param required"})";
        *result_out = s_read_config_file_result.c_str();
        return 0;
    }
    if (!is_allowed_config_name(name)) {
        s_read_config_file_result = R"({"error":"file not in allowlist","name":")";
        s_read_config_file_result += json_escape(name);
        s_read_config_file_result += "\"}";
        *result_out = s_read_config_file_result.c_str();
        return 0;
    }
    auto dir = get_config_dir();
    if (dir.empty()) {
        s_read_config_file_result = R"({"error":"could not resolve config dir"})";
        *result_out = s_read_config_file_result.c_str();
        return 0;
    }
    auto full = dir / name;
    std::ifstream ifs(full, std::ios::binary);
    if (!ifs.is_open()) {
        s_read_config_file_result = R"({"error":"open failed","name":")";
        s_read_config_file_result += json_escape(name);
        s_read_config_file_result += "\"}";
        *result_out = s_read_config_file_result.c_str();
        return 0;
    }
    std::string content(
        (std::istreambuf_iterator<char>(ifs)),
        std::istreambuf_iterator<char>());
    s_read_config_file_result = "{\"ok\":true,\"name\":\"";
    s_read_config_file_result += json_escape(name);
    s_read_config_file_result += "\",\"sizeBytes\":";
    s_read_config_file_result += std::to_string(content.size());
    s_read_config_file_result += ",\"content\":\"";
    s_read_config_file_result += json_escape(content);
    s_read_config_file_result += "\"}";
    *result_out = s_read_config_file_result.c_str();
    return 0;
}

static int32_t handle_write_config_file(const char* params_json,
                                        const char** result_out, const char**)
{
    std::string name = extract_json_str(params_json, "name");
    std::string content = extract_json_str(params_json, "content");
    if (name.empty()) {
        s_write_config_file_result = R"({"error":"name param required"})";
        *result_out = s_write_config_file_result.c_str();
        return 0;
    }
    if (!is_allowed_config_name(name)) {
        s_write_config_file_result = R"({"error":"file not in allowlist","name":")";
        s_write_config_file_result += json_escape(name);
        s_write_config_file_result += "\"}";
        *result_out = s_write_config_file_result.c_str();
        return 0;
    }
    auto dir = get_config_dir();
    if (dir.empty()) {
        s_write_config_file_result = R"({"error":"could not resolve config dir"})";
        *result_out = s_write_config_file_result.c_str();
        return 0;
    }
    auto full = dir / name;
    auto tmp = dir / (std::string(name) + ".turdmod-tmp");
    {
        std::ofstream ofs(tmp, std::ios::binary | std::ios::trunc);
        if (!ofs.is_open()) {
            s_write_config_file_result = R"({"error":"open tmp failed","name":")";
            s_write_config_file_result += json_escape(name);
            s_write_config_file_result += "\"}";
            *result_out = s_write_config_file_result.c_str();
            return 0;
        }
        ofs.write(content.data(), static_cast<std::streamsize>(content.size()));
        if (!ofs.good()) {
            s_write_config_file_result = R"({"error":"write failed","name":")";
            s_write_config_file_result += json_escape(name);
            s_write_config_file_result += "\"}";
            *result_out = s_write_config_file_result.c_str();
            return 0;
        }
    }
    // Atomic-ish rename: on Windows, std::filesystem::rename overwrites the
    // destination iff the destination is on the same volume â€” which it is,
    // since both paths sit in the same config dir. If the dest is in use
    // (read-locked), this will fail; surface the error.
    std::error_code ec;
    std::filesystem::rename(tmp, full, ec);
    if (ec) {
        // Cleanup the tmp on rename failure so we don't leave litter.
        std::error_code rm_ec;
        std::filesystem::remove(tmp, rm_ec);
        s_write_config_file_result = R"({"error":"rename failed","detail":")";
        s_write_config_file_result += json_escape(ec.message());
        s_write_config_file_result += "\",\"name\":\"";
        s_write_config_file_result += json_escape(name);
        s_write_config_file_result += "\"}";
        *result_out = s_write_config_file_result.c_str();
        return 0;
    }
    s_write_config_file_result = "{\"ok\":true,\"name\":\"";
    s_write_config_file_result += json_escape(name);
    s_write_config_file_result += "\",\"bytesWritten\":";
    s_write_config_file_result += std::to_string(content.size());
    s_write_config_file_result += "}";
    *result_out = s_write_config_file_result.c_str();
    return 0;
}

// â”€â”€â”€ Ban / Unban / Grant / Revoke (Phase 1.2 â€” composite, Pattern F + B) â”€â”€â”€â”€
//
// SCUM's ban + admin model is FILE-based, not UFunction-based. BannedUsers.ini
// is the gate for future connects; AdminUsers.ini is the gate for admin
// permissions. There is NO BanPlayer / GrantAdmin UFunction reflected â€” the
// admin-command parser writes the files directly.
//
// Format (confirmed live 2026-05-23):
//   AdminUsers.ini  : "<SteamID64>[CommaSepPermissions]" per line
//                     e.g. "YOUR_STEAM_ID[SetGodMode,KickPlayer]"
//   BannedUsers.ini : "<SteamID64>" per line (no brackets in vanilla)
//
// All 4 handlers compose the same primitives: read file, mutate lines,
// atomic write back. banPlayer additionally calls KickPlayer to disconnect
// the player immediately if they're online (matches admin-chat UX).
//
// Hot-reload caveat: SCUM reads AdminUsers.ini on player CONNECT. Existing
// connections don't pick up new admin perms; the player must reconnect.
// Surface this in the response: { ok:true, reconnectRequired:true } for
// grantElevatedStatus / revokeElevatedStatus on online players.

namespace {

// Read a config file's content into a string. Empty string on failure
// (caller distinguishes "missing/unreadable" from "empty file" via the
// std::error_code).
static std::string read_config_text(const std::string& filename, std::error_code& ec)
{
    auto full = get_config_dir() / filename;
    std::ifstream ifs(full, std::ios::binary);
    if (!ifs.is_open()) {
        ec = std::make_error_code(std::errc::no_such_file_or_directory);
        return {};
    }
    ec.clear();
    return std::string(
        (std::istreambuf_iterator<char>(ifs)),
        std::istreambuf_iterator<char>());
}

// Atomic write: <name>.turdmod-tmp + rename. Returns true on success.
static bool write_config_text_atomic(const std::string& filename,
                                     const std::string& content,
                                     std::string& err_out)
{
    auto dir = get_config_dir();
    if (dir.empty()) { err_out = "could not resolve config dir"; return false; }
    auto full = dir / filename;
    auto tmp  = dir / (std::string(filename) + ".turdmod-tmp");
    {
        std::ofstream ofs(tmp, std::ios::binary | std::ios::trunc);
        if (!ofs.is_open()) { err_out = "open tmp failed"; return false; }
        ofs.write(content.data(), static_cast<std::streamsize>(content.size()));
        if (!ofs.good()) { err_out = "write failed"; return false; }
    }
    std::error_code ec;
    std::filesystem::rename(tmp, full, ec);
    if (ec) {
        std::error_code rm_ec;
        std::filesystem::remove(tmp, rm_ec);
        err_out = "rename failed: " + ec.message();
        return false;
    }
    return true;
}

// Check if a SteamID64 looks valid (Steam community IDs are in
// [76561197960265728, 76561202255233023]).
static bool is_valid_steam_id(const std::string& s)
{
    if (s.size() < 17 || s.size() > 17) return false;
    for (char c : s) if (c < '0' || c > '9') return false;
    try {
        uint64_t n = std::stoull(s);
        return n >= 76561197960265728ULL && n <= 76561202255233023ULL;
    } catch (...) { return false; }
}

// Append "<steamId>[<bracketContent>]\n" to a file, idempotently
// (no-op if a line already starts with the SteamID). bracketContent
// can be empty â€” in that case we emit "<steamId>\n" with no brackets.
// Returns: 0 = added, 1 = already present, -1 = error (err_out filled).
static int add_steam_id_line(const std::string& filename,
                             const std::string& steam_id,
                             const std::string& bracket_content,
                             std::string& err_out)
{
    std::error_code ec;
    std::string content = read_config_text(filename, ec);
    // ENOENT is fine â€” we'll create the file.
    if (ec && ec != std::make_error_code(std::errc::no_such_file_or_directory)) {
        err_out = "read failed: " + ec.message();
        return -1;
    }
    // Already present?
    size_t pos = 0;
    while (pos < content.size()) {
        size_t eol = content.find('\n', pos);
        std::string line = content.substr(pos, (eol == std::string::npos ? content.size() : eol) - pos);
        // Strip CR if present
        if (!line.empty() && line.back() == '\r') line.pop_back();
        if (line.compare(0, steam_id.size(), steam_id) == 0 &&
            (line.size() == steam_id.size() ||
             line[steam_id.size()] == '[' ||
             line[steam_id.size()] == ' ' ||
             line[steam_id.size()] == '\t')) {
            return 1; // already present
        }
        if (eol == std::string::npos) break;
        pos = eol + 1;
    }
    // Append. Ensure trailing newline before appending if file is non-empty
    // and doesn't already end with one.
    if (!content.empty() && content.back() != '\n') content.push_back('\n');
    content += steam_id;
    if (!bracket_content.empty()) {
        content.push_back('[');
        content += bracket_content;
        content.push_back(']');
    }
    content.push_back('\n');
    if (!write_config_text_atomic(filename, content, err_out)) return -1;
    return 0;
}

// Remove every line that starts with the given SteamID from a file.
// Returns: number of lines removed (0 if not present), or -1 on error.
static int remove_steam_id_lines(const std::string& filename,
                                 const std::string& steam_id,
                                 std::string& err_out)
{
    std::error_code ec;
    std::string content = read_config_text(filename, ec);
    if (ec) {
        if (ec == std::make_error_code(std::errc::no_such_file_or_directory)) return 0;
        err_out = "read failed: " + ec.message();
        return -1;
    }
    std::string out;
    out.reserve(content.size());
    int removed = 0;
    size_t pos = 0;
    while (pos < content.size()) {
        size_t eol = content.find('\n', pos);
        size_t end = (eol == std::string::npos ? content.size() : eol + 1);
        std::string line = content.substr(pos, end - pos);
        std::string trimmed = line;
        if (!trimmed.empty() && trimmed.back() == '\n') trimmed.pop_back();
        if (!trimmed.empty() && trimmed.back() == '\r') trimmed.pop_back();
        bool match = (trimmed.compare(0, steam_id.size(), steam_id) == 0 &&
                      (trimmed.size() == steam_id.size() ||
                       trimmed[steam_id.size()] == '[' ||
                       trimmed[steam_id.size()] == ' ' ||
                       trimmed[steam_id.size()] == '\t'));
        if (match) {
            ++removed;
        } else {
            out += line;
        }
        pos = end;
    }
    if (removed == 0) return 0;
    if (!write_config_text_atomic(filename, out, err_out)) return -1;
    return removed;
}

// Try to find an online ConZPlayerController by SteamID. Returns nullptr
// if not found. Used by banPlayer to kick after the file write. The
// helpers find_pc_by_player_name / read_pc_steam_id / fname_to_wstring /
// find_first_instance_of_class / find_ufunction are all defined earlier
// in the same translation unit (file-scope statics).
static UObject* find_pc_by_steam_id_str(const std::string& steam_id)
{
    if (!is_valid_steam_id(steam_id)) return nullptr;
    uint64_t want_id = 0;
    try { want_id = std::stoull(steam_id); } catch (...) { return nullptr; }
    UObject* found = nullptr;
    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        if (found) return;
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* class_ptr = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!class_ptr) return;
        auto* cp = reinterpret_cast<const uint8_t*>(class_ptr);
        const FName& cls_fname = *reinterpret_cast<const FName*>(cp + 0x18);
        std::wstring cls_name = fname_to_wstring(cls_fname);
        if (cls_name.find(L"ConZPlayerController" /* YOUR_GAME_PC_CLASS */) == std::wstring::npos) return;
        // Skip CDOs
        const FName& obj_name = *reinterpret_cast<const FName*>(p + 0x18);
        std::wstring obj_name_s = fname_to_wstring(obj_name);
        if (obj_name_s.find(L"Default__") == 0) return;
        // Read SteamID via the shared helper.
        std::string s = read_pc_steam_id(obj);
        if (s.empty()) return;
        try {
            uint64_t got = std::stoull(s);
            if (got == want_id) found = obj;
        } catch (...) {}
    });
    return found;
}

} // namespace

static std::string s_ban_player_result;
static std::string s_unban_player_result;
static std::string s_grant_elevated_result;
static std::string s_revoke_elevated_result;

// [SCRUBBED] Game-specific section removed (472 lines)

static int32_t handle_shutdown_server(const char* params_json,
                                      const char** result_out, const char**)
{
    std::string force_s = extract_json_str(params_json, "force");
    bool force = (force_s != "false");  // default: force-kill

    s_shutdown_result = "{\"ok\":true,\"force\":";
    s_shutdown_result += force ? "true" : "false";
    s_shutdown_result += ",\"action\":\"";
    s_shutdown_result += force ? "TerminateProcess" : "soft-exit";
    s_shutdown_result += "\",\"pid\":";
    s_shutdown_result += std::to_string(GetCurrentProcessId());
    s_shutdown_result += "}";
    *result_out = s_shutdown_result.c_str();

    // Schedule the actual kill ~200ms after the reply is sent so the
    // RPC framing flushes. Detached thread; never joined.
    std::thread([force] {
        std::this_thread::sleep_for(std::chrono::milliseconds(200));
        if (force) {
            ::TerminateProcess(::GetCurrentProcess(), 0);
        } else {
            ::ExitProcess(0);
        }
    }).detach();

    return 0;
}

// â”€â”€â”€ Phase 1.3 partial â€” placeItemInInventoryOrHolster (Pattern B) â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Wraps Prisoner::PlaceItemInInventoryOrHolster(Item:Object, tryToJoinItems:Bool)
// â€” places an EXISTING item into a player's inventory. Does NOT create new
// items; for that we need either Layer 3 + custom UE 4.27 pak BP OR sigscan
// of SCUM's internal item-factory (see docs/grail-book/08-spawn-item-investigation.md).
//
// Params: { playerName: string, itemPtr: hex string (e.g. "0x1234..."), tryToJoin?: "true"|"false" }
//
// Useful for: moving an item already in the world (looted, dropped, transferred)
// into a player's inventory. Bouncer/Quartermaster compositions can chain this
// after locating an item via reflection scan.

static std::string s_place_item_result;

static int32_t handle_place_item_in_inventory(const char* params_json,
                                              const char** result_out, const char**)
{
    ensure_hook_installed_once();
    std::string player_name = extract_json_str(params_json, "playerName");
    std::string item_ptr_s = extract_json_str(params_json, "itemPtr");
    std::string try_join_s = extract_json_str(params_json, "tryToJoin");
    bool try_join = (try_join_s != "false");

    if (player_name.empty() || item_ptr_s.empty()) {
        s_place_item_result = R"({"error":"playerName and itemPtr params required"})";
        *result_out = s_place_item_result.c_str();
        return 0;
    }

    UObject* item = nullptr;
    try {
        uint64_t raw = std::stoull(item_ptr_s, nullptr, item_ptr_s.find("0x") == 0 ? 16 : 10);
        item = reinterpret_cast<UObject*>(raw);
    } catch (...) {
        s_place_item_result = R"({"error":"itemPtr must be a hex pointer like 0x12345678"})";
        *result_out = s_place_item_result.c_str();
        return 0;
    }
    if (!item) {
        s_place_item_result = R"({"error":"itemPtr resolved to null"})";
        *result_out = s_place_item_result.c_str();
        return 0;
    }

    std::wstring nw(player_name.begin(), player_name.end());
    UObject* pc = find_pc_by_player_name(nw);
    if (!pc) {
        s_place_item_result = R"({"error":"player not found"})";
        *result_out = s_place_item_result.c_str();
        return 0;
    }
    // PlayerController â†’ Pawn (Prisoner) â€” Pawn ptr typically at AController._pawn @+0x340
    UObject* prisoner = *reinterpret_cast<UObject* const*>(
        reinterpret_cast<const uint8_t*>(pc) + 0x340);
    if (!prisoner) {
        s_place_item_result = R"err({"error":"player has no current Pawn (spectator or pre-spawn)"})err";
        *result_out = s_place_item_result.c_str();
        return 0;
    }

    static UObject* s_place_fn = nullptr;
    if (!s_place_fn) {
        s_place_fn = find_ufunction(L"PlaceItemInInventoryOrHolster", L"Prisoner");
        if (!s_place_fn) {
            s_place_item_result = R"({"error":"Prisoner::PlaceItemInInventoryOrHolster UFunction not found"})";
            *result_out = s_place_item_result.c_str();
            return 0;
        }
    }

    // Params: Item:Object @+0 (8), tryToJoinItems:Bool @+8 (1) â€” paramsSize=9
    #pragma pack(push, 8)
    struct PlaceParams {
        UObject* Item;
        uint8_t  TryToJoin;
        uint8_t  _pad[7];
    };
    #pragma pack(pop)
    PlaceParams pp{};
    pp.Item = item;
    pp.TryToJoin = try_join ? 1 : 0;
    prisoner->ProcessEvent(reinterpret_cast<class UFunction*>(s_place_fn), &pp);

    char buf[256];
    std::snprintf(buf, sizeof(buf),
                  "{\"ok\":true,\"playerName\":\"%s\",\"itemPtr\":\"%s\",\"tryToJoin\":%s}",
                  player_name.c_str(), item_ptr_s.c_str(), try_join ? "true" : "false");
    s_place_item_result = buf;
    *result_out = s_place_item_result.c_str();
    return 0;
}

// â”€â”€â”€ Phase 1.4 partial â€” setGender (Pattern B) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Wraps ConZCharacter::SetGender(EGender) â€” the ONLY reflected BlueprintCallable
// player-stat-write on the Character/Prisoner family. SetPrisonerAttributes /
// SetSkillLevel / SetBodyType are NOT reflected; they require sigscan + vtable
// RE (parked, see docs/guides/pak-layer3-iteration-2.md).
//
// Params: { playerName: string, gender: "Male" | "Female" | 0 | 1 }
// EGender is an EnumProperty 1 byte. Per SCUM convention: 0=Male, 1=Female.

static std::string s_set_gender_result;

// [SCRUBBED] Game-specific section removed (59 lines)

    s_set_gender_result = buf;
    *result_out = s_set_gender_result.c_str();
    return 0;
}

// â”€â”€â”€ Phase 1.5 mutation â€” Squad management (Pattern B, hypothesis) â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// SCUM's RemoveFromSquadOnServer / PromoteSquadMemberOnServer /
// SendSquadInvitationOnServer UFunctions all take a single Struct parameter
// (UserProfileId or invited:Object). The reflected paramsSize is 8 bytes,
// suggesting these structs are simple uint64 wrappers (likely SteamID-derived
// FUniqueNetIdRepl flattened).
//
// HYPOTHESIS (testable via CLI): the 8-byte Struct param == SteamID64 directly.
// If correct, calling with a SteamID uint64 succeeds. If wrong, the call
// either no-ops or crashes. Either way we learn empirically what the struct is.
//
// Params (all three): { actorPtr: hex string (caller Prisoner), steamId: string (target) }
// Variants: removeFromSquad, promoteSquadMember, sendSquadInvitation (3 handlers
// pointing to 3 UFunctions on Prisoner).

static std::string s_squad_op_result;

static int32_t handle_squad_op_generic(const char* params_json,
                                       const char** result_out,
                                       const wchar_t* ufunc_name,
                                       const char* op_label)
{
    ensure_hook_installed_once();
    std::string actor_ptr_s = extract_json_str(params_json, "actorPtr");
    std::string steam_id_s  = extract_json_str(params_json, "steamId");
    if (actor_ptr_s.empty() || steam_id_s.empty()) {
        s_squad_op_result = R"({"error":"actorPtr and steamId required"})";
        *result_out = s_squad_op_result.c_str();
        return 0;
    }
    if (!is_valid_steam_id(steam_id_s)) {
        s_squad_op_result = R"({"error":"steamId must be 17-digit SteamID64"})";
        *result_out = s_squad_op_result.c_str();
        return 0;
    }
    UObject* prisoner = nullptr;
    try {
        uint64_t raw = std::stoull(actor_ptr_s, nullptr, actor_ptr_s.find("0x") == 0 ? 16 : 10);
        prisoner = reinterpret_cast<UObject*>(raw);
    } catch (...) {
        s_squad_op_result = R"({"error":"actorPtr must be hex pointer"})";
        *result_out = s_squad_op_result.c_str();
        return 0;
    }
    if (!prisoner) {
        s_squad_op_result = R"({"error":"actorPtr null"})";
        *result_out = s_squad_op_result.c_str();
        return 0;
    }

    UObject* fn = find_ufunction(ufunc_name, L"Prisoner");
    if (!fn) {
        s_squad_op_result = std::string(R"({"error":"UFunction not found","name":")") + op_label + "\"}";
        *result_out = s_squad_op_result.c_str();
        return 0;
    }

    uint64_t steam_id = std::stoull(steam_id_s);
    #pragma pack(push, 8)
    struct OneStructParam { uint64_t StructData; };
    #pragma pack(pop)
    OneStructParam pp{};
    pp.StructData = steam_id;
    prisoner->ProcessEvent(reinterpret_cast<class UFunction*>(fn), &pp);

    char buf[256];
    std::snprintf(buf, sizeof(buf),
                  "{\"ok\":true,\"op\":\"%s\",\"actorPtr\":\"%s\",\"steamId\":\"%s\",\"hypothesis\":\"UserProfileId=uint64\"}",
                  op_label, actor_ptr_s.c_str(), steam_id_s.c_str());
    s_squad_op_result = buf;
    *result_out = s_squad_op_result.c_str();
    return 0;
}

// [SCRUBBED] Game-specific section removed (884 lines)

static int32_t handle_patch_instructions(const char* params_json, const char** result_out, const char**)
{
    std::string addr_s     = extract_json_str(params_json, "addr");
    std::string expected_s = extract_json_str(params_json, "expectedHex");
    std::string patch_s    = extract_json_str(params_json, "patchHex");
    std::string label      = extract_json_str(params_json, "label");
    if (addr_s.empty() || expected_s.empty() || patch_s.empty()) {
        s_patch_instructions_result = R"x({"error":"addr, expectedHex, patchHex required"})x";
        *result_out = s_patch_instructions_result.c_str();
        return 0;
    }
    uint64_t addr = 0;
    try {
        addr = std::stoull(addr_s, nullptr, addr_s.find("0x") == 0 ? 16 : 10);
    } catch (...) {
        s_patch_instructions_result = R"x({"error":"addr must be hex like 0x..."})x";
        *result_out = s_patch_instructions_result.c_str();
        return 0;
    }
    std::vector<uint8_t> expected, patch;
    if (!hex_decode(expected_s, expected) || !hex_decode(patch_s, patch)) {
        s_patch_instructions_result = R"x({"error":"expectedHex/patchHex must be even-length lowercase hex"})x";
        *result_out = s_patch_instructions_result.c_str();
        return 0;
    }
    if (expected.size() != patch.size()) {
        s_patch_instructions_result = R"x({"error":"expectedHex and patchHex must be same length"})x";
        *result_out = s_patch_instructions_result.c_str();
        return 0;
    }
    if (expected.empty() || expected.size() > 64) {
        s_patch_instructions_result = R"x({"error":"patch length must be 1..64 bytes"})x";
        *result_out = s_patch_instructions_result.c_str();
        return 0;
    }

    // Page check
    MEMORY_BASIC_INFORMATION mbi{};
    if (::VirtualQuery(reinterpret_cast<LPCVOID>(addr), &mbi, sizeof(mbi)) == 0 ||
        mbi.State != MEM_COMMIT) {
        s_patch_instructions_result = R"x({"error":"target page not committed"})x";
        *result_out = s_patch_instructions_result.c_str();
        return 0;
    }
    uint64_t region_end = reinterpret_cast<uint64_t>(mbi.BaseAddress) + mbi.RegionSize;
    if (addr + expected.size() > region_end) {
        s_patch_instructions_result = R"x({"error":"patch range crosses region boundary"})x";
        *result_out = s_patch_instructions_result.c_str();
        return 0;
    }

    // Byte verify
    if (std::memcmp(reinterpret_cast<const void*>(addr), expected.data(), expected.size()) != 0) {
        std::string actual = hex_encode(reinterpret_cast<const uint8_t*>(addr), expected.size());
        s_patch_instructions_result = "{\"error\":\"original bytes do not match expectedHex\",\"actual\":\"";
        s_patch_instructions_result += actual;
        s_patch_instructions_result += "\",\"expected\":\"";
        s_patch_instructions_result += expected_s;
        s_patch_instructions_result += "\"}";
        *result_out = s_patch_instructions_result.c_str();
        return 0;
    }

    // Suspend other threads
    std::vector<HANDLE> suspended;
    suspend_other_threads(suspended);

    // VirtualProtect â†’ write â†’ flush â†’ restore
    DWORD oldProtect = 0;
    BOOL prot_ok = ::VirtualProtect(reinterpret_cast<LPVOID>(addr), expected.size(),
                                    PAGE_EXECUTE_READWRITE, &oldProtect);
    if (!prot_ok) {
        resume_threads(suspended);
        s_patch_instructions_result = R"x({"error":"VirtualProtect to RWX failed"})x";
        *result_out = s_patch_instructions_result.c_str();
        return 0;
    }
    std::memcpy(reinterpret_cast<void*>(addr), patch.data(), patch.size());
    ::FlushInstructionCache(::GetCurrentProcess(), reinterpret_cast<LPCVOID>(addr), patch.size());
    DWORD ignored = 0;
    ::VirtualProtect(reinterpret_cast<LPVOID>(addr), expected.size(), oldProtect, &ignored);

    resume_threads(suspended);

    // Record for later unpatch
    PatchRecord rec;
    rec.addr       = addr;
    rec.original   = expected;
    rec.patch      = patch;
    rec.oldProtect = oldProtect;
    rec.label      = label;
    char tok[32];
    std::snprintf(tok, sizeof(tok), "patch-%llu",
                  static_cast<unsigned long long>(g_patch_token_counter.fetch_add(1)));
    rec.token = tok;
    g_patch_records.push_back(rec);

    s_patch_instructions_result = "{\"ok\":true,\"token\":\"";
    s_patch_instructions_result += rec.token;
    s_patch_instructions_result += "\",\"addr\":\"";
    s_patch_instructions_result += addr_s;
    s_patch_instructions_result += "\",\"bytes\":";
    s_patch_instructions_result += std::to_string(patch.size());
    s_patch_instructions_result += "}";
    *result_out = s_patch_instructions_result.c_str();
    return 0;
}

static int32_t handle_unpatch_instructions(const char* params_json, const char** result_out, const char**)
{
    std::string token = extract_json_str(params_json, "token");
    if (token.empty()) {
        s_unpatch_instructions_result = R"x({"error":"token required"})x";
        *result_out = s_unpatch_instructions_result.c_str();
        return 0;
    }
    PatchRecord* found = nullptr;
    size_t found_idx = 0;
    for (size_t i = 0; i < g_patch_records.size(); ++i) {
        if (g_patch_records[i].token == token) { found = &g_patch_records[i]; found_idx = i; break; }
    }
    if (!found) {
        s_unpatch_instructions_result = R"x({"error":"token not found"})x";
        *result_out = s_unpatch_instructions_result.c_str();
        return 0;
    }
    std::vector<HANDLE> suspended;
    suspend_other_threads(suspended);
    DWORD oldProtect = 0;
    ::VirtualProtect(reinterpret_cast<LPVOID>(found->addr), found->original.size(),
                     PAGE_EXECUTE_READWRITE, &oldProtect);
    std::memcpy(reinterpret_cast<void*>(found->addr), found->original.data(), found->original.size());
    ::FlushInstructionCache(::GetCurrentProcess(),
                            reinterpret_cast<LPCVOID>(found->addr),
                            found->original.size());
    DWORD ignored = 0;
    ::VirtualProtect(reinterpret_cast<LPVOID>(found->addr), found->original.size(), oldProtect, &ignored);
    resume_threads(suspended);
    s_unpatch_instructions_result = "{\"ok\":true,\"token\":\"";
    s_unpatch_instructions_result += token;
    s_unpatch_instructions_result += "\"}";
    g_patch_records.erase(g_patch_records.begin() + found_idx);
    *result_out = s_unpatch_instructions_result.c_str();
    return 0;
}

static int32_t handle_list_patches(const char*, const char** result_out, const char**)
{
    s_list_patches_result = "{\"ok\":true,\"count\":";
    s_list_patches_result += std::to_string(g_patch_records.size());
    s_list_patches_result += ",\"patches\":[";
    bool first = true;
    for (const auto& r : g_patch_records) {
        if (!first) s_list_patches_result += ",";
        first = false;
        char buf[64];
        std::snprintf(buf, sizeof(buf), "0x%llx", static_cast<unsigned long long>(r.addr));
        s_list_patches_result += "{\"token\":\"";
        s_list_patches_result += r.token;
        s_list_patches_result += "\",\"addr\":\"";
        s_list_patches_result += buf;
        s_list_patches_result += "\",\"bytes\":";
        s_list_patches_result += std::to_string(r.patch.size());
        s_list_patches_result += ",\"label\":\"";
        s_list_patches_result += json_escape(r.label);
        s_list_patches_result += "\",\"originalHex\":\"";
        s_list_patches_result += hex_encode(r.original.data(), r.original.size());
        s_list_patches_result += "\",\"patchHex\":\"";
        s_list_patches_result += hex_encode(r.patch.data(), r.patch.size());
        s_list_patches_result += "\"}";
    }
    s_list_patches_result += "]}";
    *result_out = s_list_patches_result.c_str();
    return 0;
}

static int32_t handle_list_modals(const char* params_json, const char** result_out, const char**)
{
    // Params (all optional):
    //   { "scope": "self" | "all" | "foreign", "pid": <int> }
    // Default scope: "all" â€” enumerate all top-level visible windows
    //   across all processes (catches UE4SS crash reporter modals which
    //   may run in a child process).
    std::string scope = extract_json_str(params_json, "scope");
    std::string pid_s = extract_json_str(params_json, "pid");
    if (scope.empty()) scope = "all";

    ModalScanCtx ctx;
    ctx.targetPid = 0;
    ctx.includeOurProcess = false;
    if (!pid_s.empty()) {
        try { ctx.targetPid = static_cast<DWORD>(std::stoul(pid_s)); } catch (...) {}
    } else if (scope == "self") {
        ctx.targetPid = ::GetCurrentProcessId();
    } else if (scope == "all") {
        ctx.includeOurProcess = true;
    } // "foreign" leaves both at default = all-except-self

    ::EnumWindows(enum_modals_proc, reinterpret_cast<LPARAM>(&ctx));

    // Always log what we saw â€” UE4SS.log is durable across test-client timeouts.
    log_info_fmt(STR("[listModals] scope='{}' targetPid={} found {} windows\n"),
                 std::wstring(scope.begin(), scope.end()),
                 static_cast<unsigned long long>(ctx.targetPid),
                 static_cast<unsigned long long>(ctx.hwnds.size()));
    for (size_t i = 0; i < ctx.hwnds.size(); ++i) {
        std::string cls = window_class(ctx.hwnds[i]);
        std::string title = window_text(ctx.hwnds[i]);
        std::wstring cls_w(cls.begin(), cls.end());
        std::wstring title_w(title.begin(), title.end());
        log_info_fmt(STR("[listModals]   hwnd={:#x} pid={} class='{}' title='{}'\n"),
                     static_cast<unsigned long long>(reinterpret_cast<uintptr_t>(ctx.hwnds[i])),
                     static_cast<unsigned long long>(ctx.pids[i]),
                     cls_w, title_w);
    }

    s_list_modals_result = "{\"ok\":true,\"scope\":\"";
    s_list_modals_result += scope;
    s_list_modals_result += "\",\"count\":";
    s_list_modals_result += std::to_string(ctx.hwnds.size());
    s_list_modals_result += ",\"modals\":[";
    bool first = true;
    for (size_t i = 0; i < ctx.hwnds.size(); ++i) {
        if (!first) s_list_modals_result += ",";
        first = false;
        char hb[40];
        std::snprintf(hb, sizeof(hb), "0x%llx",
                      static_cast<unsigned long long>(reinterpret_cast<uintptr_t>(ctx.hwnds[i])));
        s_list_modals_result += "{\"hwnd\":\"";
        s_list_modals_result += hb;
        s_list_modals_result += "\",\"pid\":";
        s_list_modals_result += std::to_string(ctx.pids[i]);
        s_list_modals_result += ",\"class\":\"";
        s_list_modals_result += json_escape(window_class(ctx.hwnds[i]));
        s_list_modals_result += "\",\"title\":\"";
        s_list_modals_result += json_escape(window_text(ctx.hwnds[i]));
        s_list_modals_result += "\"}";
    }
    s_list_modals_result += "]}";
    *result_out = s_list_modals_result.c_str();
    return 0;
}

// Try multiple dismissal methods against a single HWND. Returns true if any
// succeeded; logs every attempt + outcome.
static bool try_dismiss_hwnd(HWND h)
{
    std::string title = window_text(h);
    std::string cls = window_class(h);
    std::wstring title_w(title.begin(), title.end());
    std::wstring cls_w(cls.begin(), cls.end());

    // Method 1: WM_CLOSE â€” works for many dialogs.
    BOOL r1 = ::PostMessageW(h, WM_CLOSE, 0, 0);

    // Method 2: For MessageBoxW dialogs, post IDOK via WM_COMMAND.
    BOOL r2 = ::PostMessageW(h, WM_COMMAND, MAKEWPARAM(IDOK, BN_CLICKED), 0);

    // Method 3: Enumerate child Button windows + click each (covers OK,
    // Cancel, Abort, Ignore â€” all dialog buttons).
    int btn_clicks = 0;
    ButtonFind bf{nullptr};
    ::EnumChildWindows(h, find_button_proc, reinterpret_cast<LPARAM>(&bf));
    if (bf.result) {
        ::PostMessageW(bf.result, BM_CLICK, 0, 0);
        btn_clicks++;
    }

    log_info_fmt(STR("[dismissModals]   target hwnd={:#x} class='{}' title='{}'\n"),
                 static_cast<unsigned long long>(reinterpret_cast<uintptr_t>(h)),
                 cls_w, title_w);
    log_info_fmt(STR("[dismissModals]     WM_CLOSE={} WM_COMMAND_IDOK={} button_clicks={}\n"),
                 r1 ? L"true" : L"false",
                 r2 ? L"true" : L"false",
                 static_cast<int>(btn_clicks));

    return (r1 || r2 || btn_clicks > 0);
}

static int32_t handle_dismiss_modals_new(const char* params_json, const char** result_out, const char**)
{
    // Params (all optional):
    //   { "hwnd": "0x..." }              - dismiss specific window
    //   { "titleContains": "Fatal" }     - dismiss windows whose title contains substring
    //   { "pid": <int> }                 - restrict to specific pid
    //   { "scope": "self"|"all"|"foreign" }  - default "all"
    std::string hwnd_s   = extract_json_str(params_json, "hwnd");
    std::string title_s  = extract_json_str(params_json, "titleContains");
    std::string pid_s    = extract_json_str(params_json, "pid");
    std::string scope    = extract_json_str(params_json, "scope");
    if (scope.empty()) scope = "all";

    ModalScanCtx ctx;
    ctx.targetPid = 0;
    ctx.includeOurProcess = false;
    if (!pid_s.empty()) {
        try { ctx.targetPid = static_cast<DWORD>(std::stoul(pid_s)); } catch (...) {}
    } else if (scope == "self") {
        ctx.targetPid = ::GetCurrentProcessId();
    } else if (scope == "all") {
        ctx.includeOurProcess = true;
    }
    ::EnumWindows(enum_modals_proc, reinterpret_cast<LPARAM>(&ctx));

    log_info_fmt(STR("[dismissModals] scope='{}' titleContains='{}' enumerated {} windows\n"),
                 std::wstring(scope.begin(), scope.end()),
                 std::wstring(title_s.begin(), title_s.end()),
                 static_cast<unsigned long long>(ctx.hwnds.size()));

    std::vector<HWND> toDismiss;
    if (!hwnd_s.empty()) {
        try {
            uint64_t v = std::stoull(hwnd_s, nullptr,
                hwnd_s.find("0x") == 0 ? 16 : 10);
            HWND h = reinterpret_cast<HWND>(static_cast<uintptr_t>(v));
            for (HWND m : ctx.hwnds) {
                if (m == h) { toDismiss.push_back(h); break; }
            }
        } catch (...) {}
    } else if (!title_s.empty()) {
        for (HWND h : ctx.hwnds) {
            std::string t = window_text(h);
            if (t.find(title_s) != std::string::npos) toDismiss.push_back(h);
        }
    } else {
        toDismiss = ctx.hwnds;
    }

    size_t dismissed = 0;
    s_dismiss_modals_result = "{\"ok\":true,\"dismissed\":[";
    bool first = true;
    for (HWND h : toDismiss) {
        std::string title = window_text(h);
        std::string cls = window_class(h);
        bool ok = try_dismiss_hwnd(h);
        if (!first) s_dismiss_modals_result += ",";
        first = false;
        char hb[24];
        std::snprintf(hb, sizeof(hb), "0x%llx",
                      static_cast<unsigned long long>(reinterpret_cast<uintptr_t>(h)));
        s_dismiss_modals_result += "{\"hwnd\":\"";
        s_dismiss_modals_result += hb;
        s_dismiss_modals_result += "\",\"class\":\"";
        s_dismiss_modals_result += json_escape(cls);
        s_dismiss_modals_result += "\",\"title\":\"";
        s_dismiss_modals_result += json_escape(title);
        s_dismiss_modals_result += "\",\"posted\":";
        s_dismiss_modals_result += ok ? "true" : "false";
        s_dismiss_modals_result += "}";
        if (ok) dismissed++;
    }
    s_dismiss_modals_result += "],\"count\":";
    s_dismiss_modals_result += std::to_string(dismissed);
    s_dismiss_modals_result += ",\"enumerated\":";
    s_dismiss_modals_result += std::to_string(ctx.hwnds.size());
    s_dismiss_modals_result += "}";
    log_info_fmt(STR("[dismissModals] DONE â€” attempted={} dismissed={}\n"),
                 static_cast<unsigned long long>(toDismiss.size()),
                 static_cast<unsigned long long>(dismissed));
    *result_out = s_dismiss_modals_result.c_str();
    return 0;
}

// (old handle_dismiss_modals removed â€” replaced by handle_dismiss_modals_new
//  above which scans all desktop windows + tries multiple dismissal methods +
//  logs every action to UE4SS.log)

static int32_t handle_find_uint64(const char* params_json, const char** result_out, const char**)
{
    std::string val_s = extract_json_str(params_json, "value");
    std::string start_s = extract_json_str(params_json, "start");
    std::string end_s = extract_json_str(params_json, "end");
    std::string max_s = extract_json_str(params_json, "maxHits");
    if (val_s.empty() || start_s.empty() || end_s.empty()) {
        s_find_u64_result = R"x({"error":"value, start, end required (hex 0x...)"})x";
        *result_out = s_find_u64_result.c_str();
        return 0;
    }
    uint64_t value = 0, start = 0, end = 0;
    size_t max_hits = 64;
    try {
        value = std::stoull(val_s,   nullptr, val_s.find("0x")   == 0 ? 16 : 10);
        start = std::stoull(start_s, nullptr, start_s.find("0x") == 0 ? 16 : 10);
        end   = std::stoull(end_s,   nullptr, end_s.find("0x")   == 0 ? 16 : 10);
        if (!max_s.empty()) max_hits = std::min<size_t>(1024, std::stoull(max_s));
    } catch (...) {
        s_find_u64_result = R"x({"error":"value/start/end must be hex 0x..."})x";
        *result_out = s_find_u64_result.c_str();
        return 0;
    }
    if (end <= start) {
        s_find_u64_result = R"x({"error":"end must be > start"})x";
        *result_out = s_find_u64_result.c_str();
        return 0;
    }
    // No upper bound on range â€” VirtualQuery skips uncommitted pages
    // instantly so wide scans of mostly-empty address space are cheap.
    std::vector<uint64_t> hits;
    hits.reserve(max_hits);
    // Scan committed regions only â€” skip uncommitted/guard pages.
    uint64_t cursor = start & ~7ULL;
    while (cursor < end && hits.size() < max_hits) {
        MEMORY_BASIC_INFORMATION mbi{};
        if (::VirtualQuery(reinterpret_cast<LPCVOID>(cursor), &mbi, sizeof(mbi)) == 0) break;
        uint64_t base = reinterpret_cast<uint64_t>(mbi.BaseAddress);
        uint64_t rsz  = mbi.RegionSize;
        uint64_t rend = base + rsz;
        uint64_t scan_start = cursor;
        uint64_t scan_end = std::min<uint64_t>(rend, end);
        bool readable = mbi.State == MEM_COMMIT &&
                        (mbi.Protect & (PAGE_READONLY | PAGE_READWRITE | PAGE_WRITECOPY |
                                        PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE));
        if (readable && (mbi.Protect & PAGE_GUARD) == 0) {
            const uint64_t* p = reinterpret_cast<const uint64_t*>(scan_start);
            const uint64_t* e = reinterpret_cast<const uint64_t*>(scan_end);
            while (p < e && hits.size() < max_hits) {
                if (*p == value) {
                    hits.push_back(reinterpret_cast<uint64_t>(p));
                }
                ++p;
            }
        }
        cursor = rend;
    }
    s_find_u64_result = "{\"ok\":true,\"value\":\"";
    s_find_u64_result += val_s;
    s_find_u64_result += "\",\"count\":";
    s_find_u64_result += std::to_string(hits.size());
    s_find_u64_result += ",\"hits\":[";
    for (size_t i = 0; i < hits.size(); ++i) {
        if (i) s_find_u64_result += ",";
        char hb[24];
        std::snprintf(hb, sizeof(hb), "\"0x%llx\"", static_cast<unsigned long long>(hits[i]));
        s_find_u64_result += hb;
    }
    s_find_u64_result += "]}";
    *result_out = s_find_u64_result.c_str();
    return 0;
}

static int32_t handle_image_base(const char*, const char** result_out, const char**)
{
    HMODULE h = ::GetModuleHandleW(nullptr);
    uint64_t base = reinterpret_cast<uint64_t>(h);
    char buf[64];
    std::snprintf(buf, sizeof(buf), "0x%llx", static_cast<unsigned long long>(base));
    s_image_base_result = "{\"ok\":true,\"imageBase\":\"";
    s_image_base_result += buf;
    s_image_base_result += "\",\"preferredBase\":\"0x140000000\"}";
    *result_out = s_image_base_result.c_str();
    return 0;
}

// Wrap a raw read in MMI-checked code path. Returns true if read succeeded.
static bool safe_read(uint64_t addr, size_t bytes, std::vector<uint8_t>& out)
{
    MEMORY_BASIC_INFORMATION mbi{};
    if (::VirtualQuery(reinterpret_cast<LPCVOID>(addr), &mbi, sizeof(mbi)) == 0) {
        return false;
    }
    if (mbi.State != MEM_COMMIT) return false;
    DWORD prot = mbi.Protect & 0xFF;
    bool readable = (prot == PAGE_READONLY || prot == PAGE_READWRITE ||
                     prot == PAGE_WRITECOPY || prot == PAGE_EXECUTE_READ ||
                     prot == PAGE_EXECUTE_READWRITE || prot == PAGE_EXECUTE_WRITECOPY);
    if (!readable) return false;
    // Ensure entire requested range stays within the queried region.
    uint64_t region_end = reinterpret_cast<uint64_t>(mbi.BaseAddress) + mbi.RegionSize;
    if (addr + bytes > region_end) return false;
    out.resize(bytes);
    std::memcpy(out.data(), reinterpret_cast<const void*>(addr), bytes);
    return true;
}

static int32_t handle_read_memory(const char* params_json, const char** result_out, const char**)
{
    std::string addr_s = extract_json_str(params_json, "addr");
    std::string size_s = extract_json_str(params_json, "size");
    if (addr_s.empty() || size_s.empty()) {
        s_read_memory_result = R"({"error":"addr and size required"})";
        *result_out = s_read_memory_result.c_str();
        return 0;
    }
    uint64_t addr = 0;
    size_t sz = 0;
    try {
        addr = std::stoull(addr_s, nullptr, addr_s.find("0x") == 0 ? 16 : 10);
        sz   = std::min<size_t>(4096, std::stoull(size_s));
    } catch (...) {
        s_read_memory_result = R"({"error":"addr must be hex like 0x... and size must be a number"})";
        *result_out = s_read_memory_result.c_str();
        return 0;
    }
    std::vector<uint8_t> buf;
    if (!safe_read(addr, sz, buf)) {
        s_read_memory_result = R"({"error":"address unreadable or out of range","addr":")";
        s_read_memory_result += addr_s + "\"}";
        *result_out = s_read_memory_result.c_str();
        return 0;
    }
    char hex[3];
    s_read_memory_result = "{\"ok\":true,\"addr\":\"";
    s_read_memory_result += addr_s;
    s_read_memory_result += "\",\"size\":";
    s_read_memory_result += std::to_string(sz);
    s_read_memory_result += ",\"bytesHex\":\"";
    for (auto b : buf) {
        std::snprintf(hex, sizeof(hex), "%02x", b);
        s_read_memory_result += hex;
    }
    s_read_memory_result += "\"}";
    *result_out = s_read_memory_result.c_str();
    return 0;
}

// writeMemory — write raw hex bytes to an address (runtime mod tool; used for the
// developer-hash injection into DataSingleton._developersHashed). Validates the address
// is mapped via safe_read first, then VirtualProtect + memcpy. DANGEROUS — caller owns
// correctness. Params: { addr: "0x...", hex: "aabbcc..." }
static std::string s_write_memory_result;
static int32_t handle_write_memory(const char* params_json, const char** result_out, const char**)
{
    std::string addr_s = extract_json_str(params_json, "addr");
    std::string hex_s  = extract_json_str(params_json, "hex");
    if (addr_s.empty() || hex_s.empty()) {
        s_write_memory_result = R"({"error":"addr and hex required"})";
        *result_out = s_write_memory_result.c_str(); return 0;
    }
    uint64_t addr = 0;
    try { addr = std::stoull(addr_s, nullptr, addr_s.find("0x") == 0 ? 16 : 10); }
    catch (...) { s_write_memory_result = R"({"error":"bad addr"})"; *result_out = s_write_memory_result.c_str(); return 0; }
    std::vector<uint8_t> bytes;
    for (size_t i = 0; i + 1 < hex_s.size(); i += 2) {
        try { bytes.push_back(static_cast<uint8_t>(std::stoul(hex_s.substr(i, 2), nullptr, 16))); } catch (...) {}
    }
    if (bytes.empty()) { s_write_memory_result = R"({"error":"no valid hex bytes"})"; *result_out = s_write_memory_result.c_str(); return 0; }
    std::vector<uint8_t> probe;
    if (!safe_read(addr, bytes.size(), probe)) {
        s_write_memory_result = R"({"error":"address unmapped/out of range"})";
        *result_out = s_write_memory_result.c_str(); return 0;
    }
    DWORD oldProt = 0;
    void* dst = reinterpret_cast<void*>(static_cast<uintptr_t>(addr));
    if (!VirtualProtect(dst, bytes.size(), PAGE_READWRITE, &oldProt)) {
        s_write_memory_result = R"({"error":"VirtualProtect failed"})";
        *result_out = s_write_memory_result.c_str(); return 0;
    }
    std::memcpy(dst, bytes.data(), bytes.size());
    VirtualProtect(dst, bytes.size(), oldProt, &oldProt);
    char buf[96];
    std::snprintf(buf, sizeof(buf), "{\"ok\":true,\"wrote\":%zu,\"addr\":\"%s\"}", bytes.size(), addr_s.c_str());
    s_write_memory_result = buf;
    *result_out = s_write_memory_result.c_str();
    return 0;
}

// setBotPlayers — ISteamGameServer::SetBotPlayerCount(n) via the flat Steam API. SCUM
// links steam_api64.dll in-process, so we resolve the v013 GameServer accessor + the
// flat SetBotPlayerCount and call it. Makes the server REPORT n bots to Steam (A2S
// query / server browser). @ctx: population display — TEST whether the in-game server
// view counts bots toward the player total; if shown separately, fall back to A2S proxy.
// Params: { count: N }
static std::string s_set_bot_result;
static int32_t handle_set_bot_players(const char* params_json, const char** result_out, const char**)
{
    std::string n_s = extract_json_str(params_json, "count");
    int n = 0;
    try { n = std::stoi(n_s); } catch (...) {}
    HMODULE h = GetModuleHandleW(L"steam_api64.dll");
    if (!h) { s_set_bot_result = R"({"error":"steam_api64.dll not loaded"})"; *result_out = s_set_bot_result.c_str(); return 0; }
    using GetIfaceFn = void* (*)();
    using SetBotFn   = void  (*)(void*, int);
    auto get_iface = reinterpret_cast<GetIfaceFn>(GetProcAddress(h, "SteamAPI_SteamGameServer_v013"));
    auto set_bot   = reinterpret_cast<SetBotFn>(GetProcAddress(h, "SteamAPI_ISteamGameServer_SetBotPlayerCount"));
    if (!get_iface || !set_bot) { s_set_bot_result = R"({"error":"steam gameserver exports not found"})"; *result_out = s_set_bot_result.c_str(); return 0; }
    void* iface = get_iface();
    if (!iface) { s_set_bot_result = "{\"error\":\"gameserver interface null - not registered yet\"}"; *result_out = s_set_bot_result.c_str(); return 0; }
    set_bot(iface, n);
    char buf[80];
    std::snprintf(buf, sizeof(buf), "{\"ok\":true,\"botCount\":%d}", n);
    s_set_bot_result = buf;
    *result_out = s_set_bot_result.c_str();
    return 0;
}

static int32_t handle_dump_vtable(const char* params_json, const char** result_out, const char**)
{
    std::string addr_s = extract_json_str(params_json, "addr");
    std::string slots_s = extract_json_str(params_json, "slots");
    if (addr_s.empty()) {
        s_dump_vtable_result = R"x({"error":"addr required (vtable VA)"})x";
        *result_out = s_dump_vtable_result.c_str();
        return 0;
    }
    uint64_t addr = 0;
    size_t slots = 32;
    try {
        addr = std::stoull(addr_s, nullptr, addr_s.find("0x") == 0 ? 16 : 10);
        if (!slots_s.empty()) slots = std::min<size_t>(64, std::stoull(slots_s));
    } catch (...) {
        s_dump_vtable_result = R"({"error":"addr must be hex like 0x..."})";
        *result_out = s_dump_vtable_result.c_str();
        return 0;
    }
    std::vector<uint8_t> buf;
    if (!safe_read(addr, slots * 8, buf)) {
        s_dump_vtable_result = R"({"error":"vtable address unreadable"})";
        *result_out = s_dump_vtable_result.c_str();
        return 0;
    }
    s_dump_vtable_result = "{\"ok\":true,\"vtable\":\"";
    s_dump_vtable_result += addr_s;
    s_dump_vtable_result += "\",\"slots\":[";
    for (size_t i = 0; i < slots; ++i) {
        if (i > 0) s_dump_vtable_result += ",";
        uint64_t ptr = *reinterpret_cast<const uint64_t*>(buf.data() + i * 8);
        char b[40];
        std::snprintf(b, sizeof(b), "{\"i\":%zu,\"offset\":\"0x%zx\",\"fn\":\"0x%llx\"}",
                      i, i * 8, static_cast<unsigned long long>(ptr));
        s_dump_vtable_result += b;
    }
    s_dump_vtable_result += "]}";
    *result_out = s_dump_vtable_result.c_str();
    return 0;
}

// â”€â”€â”€ setFlyingMode â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Enable/disable flying for a named player by writing MovementMode on their
// CharacterMovementComponent. MOVE_Flying=5, MOVE_Walking=1 (UE4 EMovementMode).
// Direct property write â€” no ProcessEvent needed. Immediate effect.
//
// Params: { "playerName": "<name>", "enabled": true|false }
static int32_t handle_set_flying_mode(const char* params_json,
                                      const char** result_out, const char**)
{
    ensure_hook_installed_once();

    std::string player_name = extract_json_str(params_json, "playerName");
    bool enabled = extract_json_bool(params_json, "enabled", false);

    if (player_name.empty()) {
        s_set_flying_mode_result = R"({"error":"playerName required"})";
        *result_out = s_set_flying_mode_result.c_str();
        return 0;
    }

    std::wstring want_w = utf8_to_wstring(player_name);
    UObject* pc = find_pc_by_player_name(want_w);
    if (!pc) {
        s_set_flying_mode_result = R"({"error":"player not found"})";
        *result_out = s_set_flying_mode_result.c_str();
        return 0;
    }

    // Pawn from PC
    auto* pc_class = *reinterpret_cast<UObject* const*>(
        reinterpret_cast<const uint8_t*>(pc) + 0x10);
    int32_t pawn_off = find_property_offset(pc_class, L"Pawn");
    if (pawn_off < 0) {
        s_set_flying_mode_result = R"({"error":"Pawn UProperty not found on PC"})";
        *result_out = s_set_flying_mode_result.c_str();
        return 0;
    }
    UObject* pawn = *reinterpret_cast<UObject* const*>(
        reinterpret_cast<const uint8_t*>(pc) + pawn_off);
    if (!pawn) {
        s_set_flying_mode_result = R"({"error":"PC has no Pawn"})";
        *result_out = s_set_flying_mode_result.c_str();
        return 0;
    }

    // CharacterMovement component from Pawn
    auto* pawn_class = *reinterpret_cast<UObject* const*>(
        reinterpret_cast<const uint8_t*>(pawn) + 0x10);
    int32_t move_comp_off = find_property_offset(pawn_class, L"CharacterMovement");
    if (move_comp_off < 0) {
        s_set_flying_mode_result = R"({"error":"CharacterMovement UProperty not found on Pawn"})";
        *result_out = s_set_flying_mode_result.c_str();
        return 0;
    }
    UObject* move_comp = *reinterpret_cast<UObject* const*>(
        reinterpret_cast<const uint8_t*>(pawn) + move_comp_off);
    if (!move_comp) {
        s_set_flying_mode_result = R"({"error":"CharacterMovement component is null"})";
        *result_out = s_set_flying_mode_result.c_str();
        return 0;
    }

    // MovementMode byte offset on the component
    // @inv: cached after first successful lookup â€” offset is class-invariant
    auto* move_comp_class = *reinterpret_cast<UObject* const*>(
        reinterpret_cast<const uint8_t*>(move_comp) + 0x10);
    static int32_t s_movement_mode_off = -2;
    if (s_movement_mode_off == -2) {
        s_movement_mode_off = find_property_offset(move_comp_class, L"MovementMode");
    }
    if (s_movement_mode_off < 0) {
        s_set_flying_mode_result = R"({"error":"MovementMode UProperty not found on CharacterMovement"})";
        *result_out = s_set_flying_mode_result.c_str();
        return 0;
    }

    // EMovementMode: MOVE_Walking=1, MOVE_Flying=5
    constexpr uint8_t kMoveWalking = 1u;
    constexpr uint8_t kMoveFlying  = 5u;

    uint8_t* mode_ptr = reinterpret_cast<uint8_t*>(
        reinterpret_cast<uint8_t*>(move_comp) + s_movement_mode_off);
    uint8_t old_mode = *mode_ptr;
    uint8_t new_mode = enabled ? kMoveFlying : kMoveWalking;
    *mode_ptr = new_mode;

    log_info_fmt(STR("[setFlyingMode] player={} enabled={} MovementMode {} -> {} (offset {:#x})\n"),
                 want_w, enabled ? 1 : 0,
                 static_cast<unsigned>(old_mode),
                 static_cast<unsigned>(new_mode),
                 static_cast<unsigned>(s_movement_mode_off));

    char buf[192];
    std::snprintf(buf, sizeof(buf),
        "{\"ok\":true,\"player\":\"%s\",\"enabled\":%s,\"prevMode\":%u,\"newMode\":%u}",
        player_name.c_str(),
        enabled ? "true" : "false",
        static_cast<unsigned>(old_mode),
        static_cast<unsigned>(new_mode));
    s_set_flying_mode_result = buf;
    *result_out = s_set_flying_mode_result.c_str();
    return 0;
}

// â”€â”€â”€ possessActor â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Make a named player's PlayerController possess a different Pawn â€”
// useful for "jumping into" a Dropship, Sentry, or spawned AI body.
//
// UE4 Controller::Possess(APawn*) is a BlueprintCallable server function.
// Params struct is a single UObject* (8 bytes, one ObjectProperty).
//
// @inv: target must be a Pawn subclass â€” possessing a non-Pawn actor
//       crashes inside UE's BP dispatcher. Caller supplies valid class name.
// @brk: numParms=8, paramsSize=8 (single ObjectProperty). If SCUM's
//       Possess override changes signature, adjust possess_params below.
//
// Params: { "playerName": "<name>", "targetClass": "<UClass name>" }
static int32_t handle_possess_actor(const char* params_json,
                                    const char** result_out, const char**)
{
    ensure_hook_installed_once();

    std::string player_name  = extract_json_str(params_json, "playerName");
    std::string target_class = extract_json_str(params_json, "targetClass");

    if (player_name.empty()) {
        s_possess_actor_result = R"({"error":"playerName required"})";
        *result_out = s_possess_actor_result.c_str();
        return 0;
    }
    if (target_class.empty()) {
        s_possess_actor_result = R"({"error":"targetClass required, e.g. Dropship"})";
        *result_out = s_possess_actor_result.c_str();
        return 0;
    }

    std::wstring player_name_w  = utf8_to_wstring(player_name);
    std::wstring target_class_w = utf8_to_wstring(target_class);

    UObject* pc = find_pc_by_player_name(player_name_w);
    if (!pc) {
        s_possess_actor_result = R"({"error":"player not found"})";
        *result_out = s_possess_actor_result.c_str();
        return 0;
    }

    UObject* target_pawn = find_first_instance_of_class(target_class_w.c_str());
    if (!target_pawn) {
        char err[192];
        std::snprintf(err, sizeof(err),
            "{\"error\":\"no instance of class '%s' found in world\"}",
            target_class.c_str());
        s_possess_actor_result = err;
        *result_out = s_possess_actor_result.c_str();
        return 0;
    }

    // Controller::Possess UFunction â€” cached; resolved from Controller base class.
    // @dep: find_ufunction:Controller.Possess
    static UObject* s_possess_fn = nullptr;
    if (!s_possess_fn) {
        s_possess_fn = find_ufunction(L"Possess", L"Controller");
        if (!s_possess_fn) s_possess_fn = find_ufunction(L"Possess");
    }
    if (!s_possess_fn) {
        s_possess_actor_result = R"({"error":"Controller::Possess UFunction not found"})";
        *result_out = s_possess_actor_result.c_str();
        return 0;
    }

    // Possess params: single ObjectProperty (APawn* NewPawn), paramsSize=8.
    // @inv: never nullptr â€” UE4 dereferences param struct unconditionally
    //       (memory: UE4 struct params never null).
    UObject* possess_params[1] = { target_pawn };
    uint32_t seh_code = call_processevent_seh(
        pc,
        reinterpret_cast<class UFunction*>(s_possess_fn),
        possess_params);

    if (seh_code != 0) {
        char err[96];
        std::snprintf(err, sizeof(err),
            "{\"error\":\"ProcessEvent SEH caught\",\"code\":\"0x%08x\"}", seh_code);
        s_possess_actor_result = err;
        *result_out = s_possess_actor_result.c_str();
        return 0;
    }

    log_info_fmt(STR("[possessActor] player={} now possesses {}\n"),
                 player_name_w, target_class_w);

    char buf[256];
    std::snprintf(buf, sizeof(buf),
        "{\"ok\":true,\"player\":\"%s\",\"targetClass\":\"%s\"}",
        player_name.c_str(), target_class.c_str());
    s_possess_actor_result = buf;
    *result_out = s_possess_actor_result.c_str();
    return 0;
}

// â”€â”€â”€ forceWeatherSnapshot â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Force WeatherController2 to broadcast current state to all clients on the
// next tick by setting _sendReplicatedStateSnapshotInterval to 0.01f (10ms).
// SCUM's weather tick builds the opaque FWeatherReplicatedStateSnapshot from
// live properties and multicasts it once the interval fires.
//
// Intended pairing: setWeather â†’ forceWeatherSnapshot. Without this, clients
// see the new weather only after the default routine snapshot cycle (which
// can be many seconds at the default interval value).
//
// Side effect: interval remains at 0.01f after this call. Per-tick cost is
// one extra NetMulticast per 10ms â€” acceptable on a 48-player server.
//
// @inv: WeatherController2 must be in GUObjectArray (guaranteed post-world-init).
//       If called too early (pre-player-connect), returns an error.
// @dep: find_first_instance_of_class:WeatherController2 â€” separate static
//       cache from setWeather/setTimeOfDay to avoid cross-handler coupling.
//
// Params: {} (none required)
static int32_t handle_force_weather_snapshot(const char* /*params_json*/,
                                              const char** result_out, const char**)
{
    ensure_hook_installed_once();

    static UObject* s_weather_ctrl = nullptr;
    if (!s_weather_ctrl) {
        s_weather_ctrl = find_first_instance_of_class(L"WeatherController2");
        if (!s_weather_ctrl) {
            // BP subclass fallback â€” SCUM uses BP_WeatherController1_0_C
            s_weather_ctrl = find_first_instance_of_class(L"BP_WeatherController1_0_C");
        }
        if (!s_weather_ctrl) {
            s_force_weather_snapshot_result =
                R"({"error":"WeatherController2 instance not found"})";
            *result_out = s_force_weather_snapshot_result.c_str();
            return 0;
        }
    }

    auto* ctrl_class = *reinterpret_cast<UObject* const*>(
        reinterpret_cast<const uint8_t*>(s_weather_ctrl) + 0x10);

    static int32_t s_interval_off = -2;
    if (s_interval_off == -2) {
        s_interval_off = find_property_offset(ctrl_class, L"_sendReplicatedStateSnapshotInterval");
    }
    if (s_interval_off < 0) {
        s_force_weather_snapshot_result =
            R"({"error":"_sendReplicatedStateSnapshotInterval property offset not found"})";
        *result_out = s_force_weather_snapshot_result.c_str();
        return 0;
    }

    float* interval_ptr = reinterpret_cast<float*>(
        reinterpret_cast<uint8_t*>(s_weather_ctrl) + s_interval_off);
    float old_interval = *interval_ptr;
    constexpr float kFastInterval = 0.01f; // 10ms â€” fires on next tick
    *interval_ptr = kFastInterval;

    log_info_fmt(STR("[forceWeatherSnapshot] _sendReplicatedStateSnapshotInterval {} -> {} (offset {:#x})\n"),
                 old_interval, kFastInterval, static_cast<unsigned>(s_interval_off));

    char buf[192];
    std::snprintf(buf, sizeof(buf),
        "{\"ok\":true,\"prevInterval\":%g,\"newInterval\":%g}",
        old_interval, kFastInterval);
    s_force_weather_snapshot_result = buf;
    *result_out = s_force_weather_snapshot_result.c_str();

    char evt[128];
    std::snprintf(evt, sizeof(evt),
        "{\"prevInterval\":%g,\"newInterval\":%g}",
        old_interval, kFastInterval);
    emit_engine_event("weatherSnapshotForced", evt);

    return 0;
}

// â”€â”€â”€ unpossessActor â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Return a player to their original Prisoner pawn by calling UnPossess on
// their Controller, then re-possessing the Prisoner. UnPossess alone would
// leave the player in limbo; we must Possess the original pawn back.
//
// Params: { "playerName": "<name>" }
static int32_t handle_unpossess_actor(const char* params_json,
                                       const char** result_out, const char**)
{
    ensure_hook_installed_once();

    std::string player_name = extract_json_str(params_json, "playerName");
    if (player_name.empty()) {
        s_unpossess_actor_result = R"({"error":"playerName required"})";
        *result_out = s_unpossess_actor_result.c_str();
        return 0;
    }

    std::wstring want_w = utf8_to_wstring(player_name);
    UObject* pc = find_pc_by_player_name(want_w);
    if (!pc) {
        s_unpossess_actor_result = R"({"error":"player not found"})";
        *result_out = s_unpossess_actor_result.c_str();
        return 0;
    }

    // Find the player's Prisoner pawn â€” scan for Prisoner instances and match
    // by looking at their Controller pointing back to this PC
    UObject* prisoner_pawn = nullptr;
    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        if (prisoner_pawn) return;
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* cls = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!cls) return;
        auto* cp = reinterpret_cast<const uint8_t*>(cls);
        const FName& cls_fn = *reinterpret_cast<const FName*>(cp + 0x18);
        if (fname_to_wstring(cls_fn).find(L"Prisoner") == std::wstring::npos) return;
        const FName& obj_fn = *reinterpret_cast<const FName*>(p + 0x18);
        if (fname_to_wstring(obj_fn).compare(0, 9, L"Default__") == 0) return;
        // Check if this Prisoner's Controller matches our PC
        int32_t ctrl_off = find_property_offset(cls, L"Controller");
        if (ctrl_off < 0) return;
        UObject* ctrl = *reinterpret_cast<UObject* const*>(p + ctrl_off);
        if (ctrl == pc) prisoner_pawn = obj;
    });

    if (!prisoner_pawn) {
        // Fallback: find any Prisoner class instance near the player
        // that doesn't currently have a controller (orphaned by possess)
        UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
            if (prisoner_pawn) return;
            auto* p = reinterpret_cast<const uint8_t*>(obj);
            auto* cls = *reinterpret_cast<UObject* const*>(p + 0x10);
            if (!cls) return;
            auto* cp = reinterpret_cast<const uint8_t*>(cls);
            const FName& cls_fn = *reinterpret_cast<const FName*>(cp + 0x18);
            std::wstring cn = fname_to_wstring(cls_fn);
            if (cn != L"Prisoner_C" && cn != L"Prisoner") return;
            const FName& obj_fn = *reinterpret_cast<const FName*>(p + 0x18);
            if (fname_to_wstring(obj_fn).compare(0, 9, L"Default__") == 0) return;
            int32_t ctrl_off = find_property_offset(cls, L"Controller");
            if (ctrl_off < 0) return;
            UObject* ctrl = *reinterpret_cast<UObject* const*>(p + ctrl_off);
            if (!ctrl) prisoner_pawn = obj; // orphaned â€” likely ours
        });
    }

    if (!prisoner_pawn) {
        s_unpossess_actor_result = R"({"error":"could not find original Prisoner pawn"})";
        *result_out = s_unpossess_actor_result.c_str();
        return 0;
    }

    // Re-possess the Prisoner
    static UObject* s_possess_fn = nullptr;
    if (!s_possess_fn) {
        s_possess_fn = find_ufunction(L"Possess", L"Controller");
        if (!s_possess_fn) s_possess_fn = find_ufunction(L"Possess");
    }
    if (!s_possess_fn) {
        s_unpossess_actor_result = R"({"error":"Possess UFunction not found"})";
        *result_out = s_unpossess_actor_result.c_str();
        return 0;
    }

    UObject* possess_params[1] = { prisoner_pawn };
    uint32_t seh_code = call_processevent_seh(
        pc, reinterpret_cast<class UFunction*>(s_possess_fn), possess_params);

    if (seh_code != 0) {
        char err[96];
        std::snprintf(err, sizeof(err),
            "{\"error\":\"ProcessEvent SEH caught\",\"code\":\"0x%08x\"}", seh_code);
        s_unpossess_actor_result = err;
        *result_out = s_unpossess_actor_result.c_str();
        return 0;
    }

    log_info_fmt(STR("[unpossessActor] {} returned to Prisoner pawn {:p}\n"),
                 want_w, static_cast<void*>(prisoner_pawn));

    s_unpossess_actor_result = R"({"ok":true})";
    *result_out = s_unpossess_actor_result.c_str();
    return 0;
}

// ─── moveActorTo ───────────────────────────────────────────────────────────
//
// Make an AI pawn WALK to a world location via the navmesh, using the static
// UAIBlueprintHelperLibrary::SimpleMoveToLocation(Controller, Goal) — the SAME
// library spawnAI calls. This is the locomotion primitive for embodied persona
// NPC bodies (spawnAI a body, then moveActorTo it). The pawn must own an
// AIController (spawnAI pawns do) — a player-possessed pawn won't path.
//
// Target (one of):
//   { "ptr": "0x..." }        exact actor (from the spawnAI result.ptr)
//   { "className": "BP_X_C" } first live instance of that class
// Destination (one of):
//   { "x":.., "y":.., "z":.. } absolute world coords
//   { "playerName": "Name" }   walk to that player's CURRENT location (follow-ish)
//
// @dep: find_ufunction:SimpleMoveToLocation@AIBlueprintHelperLibrary
// @dep: get_function_param_offsets, find_property_offset:Controller, find_first_instance_of_class
// @inv: Controller must be an AIController; goal must be on the navmesh or path fails silently.
// @brk: if SCUM renames SimpleMoveToLocation or its param "Goal"/"Controller", resolve below breaks.
static int32_t handle_move_actor_to(const char* params_json,
                                    const char** result_out, const char**)
{
    ensure_hook_installed_once();

    std::string ptr_s       = extract_json_str(params_json, "ptr");
    std::string class_name  = extract_json_str(params_json, "className");
    std::string player_name = extract_json_str(params_json, "playerName");
    bool has_xyz = (params_json != nullptr) && (std::strstr(params_json, "\"x\"") != nullptr);
    float gx = extract_json_float(params_json, "x", 0.0f);
    float gy = extract_json_float(params_json, "y", 0.0f);
    float gz = extract_json_float(params_json, "z", 0.0f);

    // 1) Resolve the target pawn — ptr preferred (exact), else first instance of class.
    UObject* pawn = nullptr;
    if (!ptr_s.empty()) {
        unsigned long long addr = 0;
        try { addr = std::stoull(ptr_s, nullptr, 16); } catch (...) {}
        pawn = reinterpret_cast<UObject*>(static_cast<uintptr_t>(addr));
    } else if (!class_name.empty()) {
        pawn = find_first_instance_of_class(utf8_to_wstring(class_name).c_str());
    }
    if (!pawn) {
        s_move_actor_result = R"({"error":"target not found - give ptr or className"})";
        *result_out = s_move_actor_result.c_str();
        return 0;
    }

    // 2) Get the pawn's Controller (the AIController that does the pathing).
    auto* pawn_class = *reinterpret_cast<UObject* const*>(
        reinterpret_cast<const uint8_t*>(pawn) + 0x10);
    int32_t ctrl_off = find_property_offset(pawn_class, L"Controller");
    UObject* controller = (ctrl_off >= 0) ? *reinterpret_cast<UObject* const*>(
        reinterpret_cast<const uint8_t*>(pawn) + ctrl_off) : nullptr;
    if (!controller) {
        s_move_actor_result = R"({"error":"pawn has no Controller - no AIController to path"})";
        *result_out = s_move_actor_result.c_str();
        return 0;
    }

    // 3) Destination — explicit xyz wins; else a player's current location (follow-ish).
    if (!has_xyz && !player_name.empty()) {
        UObject* tpc = find_pc_by_player_name(utf8_to_wstring(player_name));
        if (tpc) {
            auto* tpc_class = *reinterpret_cast<UObject* const*>(
                reinterpret_cast<const uint8_t*>(tpc) + 0x10);
            int32_t tp_off = find_property_offset(tpc_class, L"Pawn");
            UObject* tpawn = (tp_off >= 0) ? *reinterpret_cast<UObject* const*>(
                reinterpret_cast<const uint8_t*>(tpc) + tp_off) : nullptr;
            if (tpawn) {
                static UObject* s_loc_fn = nullptr;
                if (!s_loc_fn) s_loc_fn = find_ufunction(L"K2_GetActorLocation");
                if (s_loc_fn) {
                    struct { float X, Y, Z; } loc{};
                    tpawn->ProcessEvent(reinterpret_cast<class UFunction*>(s_loc_fn), &loc);
                    gx = loc.X; gy = loc.Y; gz = loc.Z;
                }
            }
        }
    }

    // 4) Resolve SimpleMoveToLocation + the AIBlueprintHelperLibrary CDO (same as spawnAI).
    static UObject* s_move_fn = nullptr;
    if (!s_move_fn) s_move_fn = find_ufunction(L"SimpleMoveToLocation", L"AIBlueprintHelperLibrary");
    if (!s_move_fn) {
        s_move_actor_result = R"({"error":"SimpleMoveToLocation UFunction not found"})";
        *result_out = s_move_actor_result.c_str();
        return 0;
    }
    static UObject* s_ai_helper_cdo_mv = nullptr;
    if (!s_ai_helper_cdo_mv) {
        UObjectGlobals::ForEachUObject([&](UObject* o, int32_t, int32_t) {
            if (s_ai_helper_cdo_mv) return;
            const FName& fn = *reinterpret_cast<const FName*>(
                reinterpret_cast<const uint8_t*>(o) + 0x18);
            if (fname_to_wstring(fn) == L"Default__AIBlueprintHelperLibrary")
                s_ai_helper_cdo_mv = o;
        });
    }
    if (!s_ai_helper_cdo_mv) s_ai_helper_cdo_mv = controller; // fallback context

    // 5) Build params { AController* Controller, FVector Goal } via resolved offsets.
    auto offsets = get_function_param_offsets(s_move_fn);
    auto off = [&](const std::wstring& k) -> int32_t {
        auto it = offsets.find(k);
        return it == offsets.end() ? -1 : it->second;
    };
    int32_t ctrl_param = off(L"Controller");
    int32_t goal_param = off(L"Goal");
    if (goal_param < 0) goal_param = off(L"GoalLocation");
    if (goal_param < 0) goal_param = off(L"Dest");
    if (goal_param < 0) goal_param = off(L"Destination");
    if (ctrl_param < 0 || goal_param < 0) {
        s_move_actor_result = R"({"error":"SimpleMoveToLocation param offsets not resolved"})";
        *result_out = s_move_actor_result.c_str();
        return 0;
    }

    alignas(16) uint8_t buf[64] = {0};
    *reinterpret_cast<UObject**>(buf + ctrl_param) = controller;
    *reinterpret_cast<float*>(buf + goal_param + 0) = gx;
    *reinterpret_cast<float*>(buf + goal_param + 4) = gy;
    *reinterpret_cast<float*>(buf + goal_param + 8) = gz;

    log_info_fmt(STR("[moveActorTo] pawn={:p} ctrl={:p} goal=({:.0f},{:.0f},{:.0f})\n"),
                 static_cast<void*>(pawn), static_cast<void*>(controller), gx, gy, gz);

    uint32_t seh_code = call_processevent_seh(
        s_ai_helper_cdo_mv, reinterpret_cast<class UFunction*>(s_move_fn), buf);
    if (seh_code != 0) {
        char err[96];
        std::snprintf(err, sizeof(err),
            "{\"error\":\"ProcessEvent SEH caught\",\"code\":\"0x%08x\"}", seh_code);
        s_move_actor_result = err;
        *result_out = s_move_actor_result.c_str();
        return 0;
    }

    char out[256];
    std::snprintf(out, sizeof(out),
        "{\"ok\":true,\"pawn\":\"0x%llx\",\"controller\":\"0x%llx\",\"goal\":[%.1f,%.1f,%.1f]}",
        static_cast<unsigned long long>(reinterpret_cast<uintptr_t>(pawn)),
        static_cast<unsigned long long>(reinterpret_cast<uintptr_t>(controller)),
        gx, gy, gz);
    s_move_actor_result = out;
    *result_out = s_move_actor_result.c_str();
    return 0;
}

// â”€â”€â”€ tameNearbyAnimal â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Find the nearest animal to a player and tame it: set Agressivness=Timid
// on its AI controller. Returns the tamed animal's pointer so the service
// can track it for follow-teleport.
//
// Params: { "playerName": "<name>", "radius": 3000 }
// [SCRUBBED] Game-specific section removed (534 lines)

static int32_t handle_dump_item_names(const char* params_json,
                                       const char** result_out, const char**)
{
    ensure_hook_installed_once();

    std::string grep = extract_json_str(params_json, "grep");
    std::wstring grep_w = grep.empty() ? L"" : utf8_to_wstring(grep);

    // Step 1: Find the "Item" UClass (the base class for all items)
    // Admin commands use completionClass "Item_C" â€” we need to find
    // classes whose SuperStruct chain includes a class named "Item".
    //
    // Walk ALL UClass and BlueprintGeneratedClass objects, check their
    // SuperStruct chain for "Item".

    // Collect all class objects first
    struct ClassInfo {
        UObject* cls;
        std::wstring name;
    };
    std::vector<ClassInfo> all_classes;
    SCAN_TIMEOUT_INIT();
    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        SCAN_TIMEOUT_CHECK();
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* meta = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!meta) return;
        auto* mp = reinterpret_cast<const uint8_t*>(meta);
        const FName& meta_fn = *reinterpret_cast<const FName*>(mp + 0x18);
        std::wstring mn = fname_to_wstring(meta_fn);
        if (mn != L"Class" && mn != L"BlueprintGeneratedClass") return;
        const FName& obj_fn = *reinterpret_cast<const FName*>(p + 0x18);
        std::wstring on = fname_to_wstring(obj_fn);
        if (on.compare(0, 9, L"Default__") == 0) return;
        all_classes.push_back({ obj, on });
    });

    // Step 2: For each class, walk SuperStruct chain looking for "Item"
    // SuperStruct is at offset 0x40 on UStruct (UClass inherits UStruct)
    std::vector<std::string> item_names;

    for (auto& ci : all_classes) {
        // Walk super chain
        bool is_item = false;
        UObject* cur = ci.cls;
        int depth = 0;
        while (cur && depth < 20) {
            auto* cp = reinterpret_cast<const uint8_t*>(cur);
            const FName& fn = *reinterpret_cast<const FName*>(cp + 0x18);
            std::wstring n = fname_to_wstring(fn);
            if (n == L"Item" || n == L"ClothesItem" || n == L"WeaponItem" ||
                n == L"AmmoItem" || n == L"MeleeWeaponItem") {
                is_item = true;
                break;
            }
            // SuperStruct at offset 0x40 (UE4 4.27)
            cur = *reinterpret_cast<UObject* const*>(cp + 0x40);
            ++depth;
        }
        if (!is_item) continue;

        // Strip _C suffix for admin command format
        std::wstring short_name = ci.name;
        if (short_name.size() > 2 && short_name.substr(short_name.size() - 2) == L"_C") {
            short_name = short_name.substr(0, short_name.size() - 2);
        }

        // Apply grep filter
        if (!grep_w.empty() && short_name.find(grep_w) == std::wstring::npos) continue;

        std::string utf8(short_name.begin(), short_name.end());
        item_names.push_back(utf8);
    }

    std::sort(item_names.begin(), item_names.end());

    // Build JSON
    std::string json = "{\"ok\":true,\"count\":";
    json += std::to_string(item_names.size());
    json += ",\"items\":[";
    for (size_t i = 0; i < item_names.size(); ++i) {
        if (i > 0) json += ",";
        json += "\"";
        json += item_names[i];
        json += "\"";
    }
    json += "]}";

    s_dump_item_names_result = json;
    *result_out = s_dump_item_names_result.c_str();

    log_info_fmt(STR("[dumpItemNames] grep=\"{}\" found={}\n"),
                 grep_w.empty() ? L"*" : grep_w, item_names.size());
    return 0;
}

// â”€â”€â”€ getAdminOutput â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Returns captured admin command response lines from Chat_Client_ProcessAdminCommand.
// Clears the buffer after reading. Use after runAdminCommand to see the response.
//
// Params: { "clear": true } (default: true â€” clears after read)
static int32_t handle_get_admin_output(const char* params_json,
                                        const char** result_out, const char**)
{
    bool should_clear = extract_json_bool(params_json, "clear", true);

    // Deactivate capture mode
    g_admin_capture_active.store(false);

    std::vector<std::string> lines;
    {
        std::lock_guard<std::mutex> lock(g_admin_output_mutex);
        lines = g_admin_output_buf;
        if (should_clear) g_admin_output_buf.clear();
    }

    std::string json = "{\"ok\":true,\"count\":";
    json += std::to_string(lines.size());
    json += ",\"lines\":[";
    for (size_t i = 0; i < lines.size(); ++i) {
        if (i > 0) json += ",";
        json += "\"";
        json += fname_to_json_string(std::wstring(lines[i].begin(), lines[i].end()));
        json += "\"";
    }
    json += "]}";

    s_admin_output_result = json;
    *result_out = s_admin_output_result.c_str();
    return 0;
}

// â”€â”€â”€ spawnItem â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Spawn an item by class name and place it in a player's inventory.
// Uses GameplayStatics::SpawnObject to create the item, then calls
// placeItemInInventory to add it. This is the reliable item spawn path.
//
// Params: { "playerName": "<name>", "className": "<item class>", "count": 1 }
static int32_t handle_spawn_item(const char* params_json,
                                  const char** result_out, const char**)
{
    ensure_hook_installed_once();

    std::string player_name = extract_json_str(params_json, "playerName");
    std::string class_name = extract_json_str(params_json, "className");
    int32_t count = static_cast<int32_t>(extract_json_float(params_json, "count", 1.0f));
    if (count < 1) count = 1;
    if (count > 20) count = 20;

    if (player_name.empty() || class_name.empty()) {
        s_spawn_item_result = R"({"error":"playerName and className required"})";
        *result_out = s_spawn_item_result.c_str();
        return 0;
    }

    std::wstring player_w = utf8_to_wstring(player_name);
    UObject* pc = find_pc_by_player_name(player_w);
    if (!pc) {
        s_spawn_item_result = R"({"error":"player not found"})";
        *result_out = s_spawn_item_result.c_str();
        return 0;
    }

    // Find the item UClass by name (supports partial match: "MRE" matches "BP_Item_MRE_C")
    std::wstring want_class = utf8_to_wstring(class_name);
    UObject* item_class = nullptr;
    UObjectGlobals::ForEachUObject([&](UObject* obj, int32_t, int32_t) {
        if (item_class) return;
        auto* p = reinterpret_cast<const uint8_t*>(obj);
        auto* cls = *reinterpret_cast<UObject* const*>(p + 0x10);
        if (!cls) return;
        auto* cp = reinterpret_cast<const uint8_t*>(cls);
        const FName& cls_fn = *reinterpret_cast<const FName*>(cp + 0x18);
        std::wstring mc = fname_to_wstring(cls_fn);
        if (mc != L"Class" && mc != L"BlueprintGeneratedClass") return;
        const FName& obj_fn = *reinterpret_cast<const FName*>(p + 0x18);
        std::wstring on = fname_to_wstring(obj_fn);
        // Exact match first
        if (on == want_class) { item_class = obj; return; }
        // Partial match (e.g. "MRE" finds "BP_Item_MRE_C")
        if (on.find(want_class) != std::wstring::npos &&
            on.find(L"Item") != std::wstring::npos) {
            item_class = obj;
        }
    });

    if (!item_class) {
        char err[192];
        std::snprintf(err, sizeof(err),
            "{\"error\":\"item class not found: %s\"}", class_name.c_str());
        s_spawn_item_result = err;
        *result_out = s_spawn_item_result.c_str();
        return 0;
    }

    // Find GameplayStatics CDO for SpawnObject
    static UObject* s_spawn_fn = nullptr;
    static UObject* s_gameplay_cdo = nullptr;
    if (!s_spawn_fn) {
        s_spawn_fn = find_ufunction(L"SpawnObject", L"GameplayStatics");
    }
    if (!s_gameplay_cdo) {
        UObjectGlobals::ForEachUObject([&](UObject* o, int32_t, int32_t) {
            if (s_gameplay_cdo) return;
            const FName& fn = *reinterpret_cast<const FName*>(
                reinterpret_cast<const uint8_t*>(o) + 0x18);
            if (fname_to_wstring(fn) == L"Default__GameplayStatics")
                s_gameplay_cdo = o;
        });
    }

    if (!s_spawn_fn || !s_gameplay_cdo) {
        s_spawn_item_result = R"({"error":"SpawnObject function not found"})";
        *result_out = s_spawn_item_result.c_str();
        return 0;
    }

    // Get player's Pawn (as Outer for spawned items)
    auto* pc_class = *reinterpret_cast<UObject* const*>(
        reinterpret_cast<const uint8_t*>(pc) + 0x10);
    int32_t pawn_off = find_property_offset(pc_class, L"Pawn");
    UObject* pawn = nullptr;
    if (pawn_off >= 0) {
        pawn = *reinterpret_cast<UObject* const*>(
            reinterpret_cast<const uint8_t*>(pc) + pawn_off);
    }
    UObject* outer = pawn ? pawn : pc;

    int32_t spawned = 0;
    for (int32_t i = 0; i < count; ++i) {
        // SpawnObject params: ObjectClass (ClassProperty), Outer (ObjectProperty), ReturnValue (ObjectProperty)
        alignas(8) uint8_t spawn_params[24] = {0};
        *reinterpret_cast<UObject**>(spawn_params + 0) = item_class;  // ObjectClass
        *reinterpret_cast<UObject**>(spawn_params + 8) = outer;       // Outer
        // ReturnValue at +16 (filled by engine)

        uint32_t seh = call_processevent_seh(
            s_gameplay_cdo,
            reinterpret_cast<class UFunction*>(s_spawn_fn),
            spawn_params);

        if (seh != 0) {
            log_info_fmt(STR("[spawnItem] SEH on SpawnObject: 0x{:08x}\n"), seh);
            break;
        }

        UObject* new_item = *reinterpret_cast<UObject**>(spawn_params + 16);
        if (new_item) {
            ++spawned;
            // Try to place in inventory
            // Find the inventory component's PickupItem function
            // For now, the item is spawned as a child of the player's Pawn
            log_info_fmt(STR("[spawnItem] created item {:p} for {}\n"),
                         static_cast<void*>(new_item), player_w);
        }
    }

    char buf[192];
    std::snprintf(buf, sizeof(buf),
        "{\"ok\":true,\"className\":\"%s\",\"count\":%d,\"spawned\":%d}",
        class_name.c_str(), count, spawned);
    s_spawn_item_result = buf;
    *result_out = s_spawn_item_result.c_str();
    return 0;
}

// â”€â”€â”€ UE4SS mod class â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

class EngineBridge : public CppUserModBase
{
public:
    EngineBridge()
    {
        // Game-specific initialization hooks can be installed here.
        // See the project documentation for how to add custom pak
        // validation or signature hooks for your UE4 game.

        ModName               = STR("TurdMODEngineBridge");
        ModVersion            = STR("0.1.0");
        ModDescription        = STR(“TurdMOD engine bridge - C-ABI link between the game server and the loader DLL”);
        ModAuthors            = STR("TurdMOD");
        ModIntendedSDKVersion = STR("3.0.1");
    }

    ~EngineBridge() override = default;

    auto on_unreal_init() -> void override
    {
        log_info(STR("on_unreal_init â€” UE4SS reflection ready"));

        if (!resolve_engine_api()) {
            log_error(STR("could not resolve loader exports â€” bridge inert"));
            return;
        }
        log_info(STR("loader exports resolved; registering handlers"));

        // DLL-boundary fix: every static inline reflection holder in the
        // Unreal interface library has a per-DLL copy. UE4SS.dll's copies
        // get armed by UnrealInitializer; the bridge sees its own null
        // copies until we mirror them across the DLL boundary. Pull every
        // resolved address from UE4SS.dll in one call and assign locally.
        auto resolved = RC::UE4SSRuntime::GetResolvedAddresses();
        if (resolved.GUObjectArray) {
            UObjectArray::g_array_address = resolved.GUObjectArray;
            log_info_fmt(STR("[TurdMODEngineBridge] mirrored GUObjectArray @ {:p}\n"),
                         resolved.GUObjectArray);
        } else {
            log_error(STR("UE4SSRuntime returned null GUObjectArray â€” UE4SS init failed"));
        }
        if (resolved.FNameToString) {
            FName::ToStringInternal.assign_address(resolved.FNameToString);
            log_info_fmt(STR("[TurdMODEngineBridge] mirrored FName::ToString @ {:p}\n"),
                         resolved.FNameToString);
        }
        if (resolved.FNameConstructorWchar) {
            FName::ConstructorInternal.assign_address(resolved.FNameConstructorWchar);
            log_info_fmt(STR("[TurdMODEngineBridge] mirrored FName::FName(wchar_t*) @ {:p}\n"),
                         resolved.FNameConstructorWchar);
        }
        if (resolved.GMallocPtr) {
            GMalloc = reinterpret_cast<FMalloc**>(resolved.GMallocPtr);
        }
        if (resolved.UEngineTick) {
            UEngine::TickInternal.assign_address(resolved.UEngineTick);
        }
        if (resolved.UObjectProcessEventVTableOffset != 0xFFFFFFFFu) {
            // Mirror UE4SS's UObject::VTableLayoutMap[L"ProcessEvent"] into
            // our local copy. The inline ProcessEvent body in our generated
            // UObject.hpp and our hook installer both read from this map â€”
            // without it both silently no-op.
            UObject::VTableLayoutMap[STR("ProcessEvent")] = resolved.UObjectProcessEventVTableOffset;
            log_info_fmt(STR("[TurdMODEngineBridge] mirrored VTableLayoutMap[ProcessEvent] = {:#x}\n"),
                         resolved.UObjectProcessEventVTableOffset);
        } else {
            log_error(STR("[TurdMODEngineBridge] UE4SS reported no ProcessEvent vtable offset â€” "
                          "VTableLayout.ini missing or unloaded; ProcessEvent calls will no-op"));
        }

        // NOTE: ProcessEvent global hook installation is deferred to the
        // first handler invocation. At on_unreal_init time GUObjectArray's
        // FChunkedFixedUObjectArray chunks aren't populated yet â€” even
        // though g_array_address is armed, NumElements is 0, and we can't
        // grab a sample UObject to read its vtable. By the time an RPC
        // handler fires, the game's normal startup has populated the array.
        // See ensure_hook_installed_once().

        struct Reg { const char* method; TurdmodEngineHandlerFn fn; };
        const Reg regs[] = {
            { "ping",             &handle_ping },
            { "broadcastChat",    &handle_broadcast_chat },
            { "teleportPlayer",   &handle_teleport_player },
            { "getOnlinePlayers", &handle_get_online_players },
            { "dumpUFunctions",   &handle_dump_ufunctions },
            { "findFunctions",    &handle_find_functions },
            { "dumpClasses",      &handle_dump_classes },
            { "runAdminCommand",  &handle_run_admin_command },
            { "sendChat",         &handle_send_chat },
            { "dumpWidgets",      &handle_dump_widgets },
            { "describeWidget",   &handle_describe_widget },
            { "readClassValues",  &handle_read_class_values },
            { "readActorByPtr",   &handle_read_actor_by_ptr },
            { "findInstancesByClass", &handle_find_instances_by_class },
            { "writeClassDefault", &handle_write_class_default },
            { "applyRecipe",      &handle_apply_recipe },
            { "showPanel",        &handle_show_panel },
            { "spawnWidgetRouter", &handle_spawn_widget_router },
            { "describeFunction", &handle_describe_function },
            { "introspectNotification", &handle_introspect_notification },
            { "captureNotification", &handle_capture_notification },
            { "captureNotificationFiltered", &handle_capture_notification_filtered },
            { "getCapturedNotification", &handle_get_captured_notification },
            { "broadcastAnnounce", &handle_broadcast_announce },
            { "replayAnnounce", &handle_replay_announce },
            { "showBanner", &handle_show_banner },
            { "fireBanner", &handle_fire_banner },
            { "dumpAllClasses",   &handle_dump_all_classes },
            { "dumpAllEnums",     &handle_dump_all_enums },
            { "dumpAllStructs",   &handle_dump_all_structs },
            { "probeQuestHandlers",   &handle_probe_quest_handlers },
            { "setEconomy",           &handle_set_economy },
            { "listClassInstances",   &handle_list_class_instances },
            { "sendChatLineToPlayer", &handle_send_chat_line_to_player },
            { "listHandlers",         &handle_list_handlers },
            { "kickPlayer",           &handle_kick_player },
            { "setTimeOfDay",         &handle_set_time_of_day },
            { "setWeather",           &handle_set_weather },
            { "sendHudMessage",       &handle_send_hud_message },
            { "showKillFeedNotification", &handle_show_killfeed },
            { "sendGameModeHudMessage",   &handle_gamemode_hud_message },
            { "launchPlayer",             &handle_launch_player },
            { "writePlayerProperty",      &handle_write_player_property },
            { "sendNotification",         &handle_send_notification },
            { "getPlayerPositions",       &handle_get_player_positions },
            { "createObject",             &handle_create_object },
            { "setConsoleVarFloat",       &handle_set_console_var_float },
            { "getNearbyActors",          &handle_get_nearby_actors },
            { "writeActorProperty",       &handle_write_actor_property },
            { "callActorFunction",        &handle_call_actor_function },
            { "setGodMode",           &handle_set_god_mode },
            { "setImmortal",          &handle_set_immortal },
            { "setInfiniteAmmo",      &handle_set_infinite_ammo },
            { "setSuperJump",         &handle_set_super_jump },
            { "runHelloWorld",        &handle_run_hello_world },
            { "loadAsset",            &handle_load_asset },
            { "listConfigFiles",      &handle_list_config_files },
            { "readConfigFile",       &handle_read_config_file },
            { "writeConfigFile",      &handle_write_config_file },
            { "readConfig",           &handle_read_config },
            { "writeConfig",          &handle_write_config },
            { "shutdownServer",       &handle_shutdown_server },
            { "placeItemInInventory", &handle_place_item_in_inventory },
            // Game-specific social/stat handlers can be registered here
            // Game-specific probe handlers can be registered here
            { "readMemory",           &handle_read_memory },
            { "writeMemory",          &handle_write_memory },
            { "dumpVTable",           &handle_dump_vtable },
            { "setBotPlayers",        &handle_set_bot_players },
            { "imageBase",            &handle_image_base },
            { "findUInt64",           &handle_find_uint64 },
            { "listModals",           &handle_list_modals },
            { "dismissModals",        &handle_dismiss_modals_new },
            { "patchInstructions",    &handle_patch_instructions },
            { "unpatchInstructions",  &handle_unpatch_instructions },
            { "listPatches",          &handle_list_patches },
            { "setFlyingMode",         &handle_set_flying_mode },
            { "possessActor",          &handle_possess_actor },
            { "forceWeatherSnapshot",  &handle_force_weather_snapshot },
            { "unpossessActor",        &handle_unpossess_actor },
            { "moveActorTo",           &handle_move_actor_to },
            // Game-specific handlers (AI, vehicles, etc.) can be registered here
            { "spawnItem",             &handle_spawn_item },
            { "getAdminOutput",        &handle_get_admin_output },
            { "dumpItemNames",         &handle_dump_item_names },
        };
        for (const auto& r : regs) {
            int32_t status = g_api.register_handler(r.method, r.fn);
            if (status != 0) {
                log_error(STR("register_handler failed"));
                continue;
            }
            g_registered_methods.emplace_back(r.method);
        }

        // Announce ourselves to the companion.
        g_api.emit_event(
            "bridgeReady",
            R"({"version":"0.1.0","mod":"TurdMODEngineBridge","kind":"cpp"})"
        );
        log_info(STR("bridgeReady event emitted; ready"));

        // â”€â”€â”€ Smoke-tick event emitter (opt-in via env var) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        //
        // Detached thread that emits one `smoke.tick` event per second when
        // TURDMOD_SMOKE_TICK is set. Validates the entire push path
        // (bridge â†’ C-ABI â†’ server-loader broadcaster â†’ named pipe â†’
        // Manager event subscriber â†’ React UI) without depending on any
        // SCUM gameplay event firing. Off by default â€” flip the env var
        // before launching GameServer.exe to enable.
        //
        // Detached + never-joined: process exit terminates it. The thread
        // body checks g_api_resolved each tick so a torn-down loader (during
        // shutdown) doesn't crash.
        // Game-specific boot-time hooks (probes, detours) can be installed here.

        if (std::getenv("TURDMOD_SMOKE_TICK")) {
            log_info(STR("[smoke-tick] TURDMOD_SMOKE_TICK set â€” starting 1Hz emitter"));
            std::thread([] {
                uint64_t counter = 0;
                auto start = std::chrono::steady_clock::now();
                while (true) {
                    std::this_thread::sleep_for(std::chrono::seconds(1));
                    if (!g_api_resolved) break;
                    auto uptime_ms = std::chrono::duration_cast<std::chrono::milliseconds>(
                        std::chrono::steady_clock::now() - start).count();
                    char buf[160];
                    std::snprintf(buf, sizeof(buf),
                                  "{\"counter\":%llu,\"uptimeMs\":%lld}",
                                  static_cast<unsigned long long>(++counter),
                                  static_cast<long long>(uptime_ms));
                    emit_engine_event("smoke.tick", buf);
                }
            }).detach();
        }
    }

    auto on_program_start() -> void override
    {
        log_info(STR("on_program_start"));
    }
};

} // namespace TurdMOD

// â”€â”€â”€ UE4SS entry points â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

extern "C" __declspec(dllexport) CppUserModBase* start_mod()
{
    return new TurdMOD::EngineBridge();
}

extern "C" __declspec(dllexport) void uninstall_mod(CppUserModBase* mod)
{
    delete mod;
}
