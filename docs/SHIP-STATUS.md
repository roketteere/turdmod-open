<!-- Last corrected 2026-06-03 (was stale: Rust-DLL/named-pipe Sprint framing; engine has shipped as a UE4SS C++ cppmod with ~101 registered RPC handlers and a solved pak-bypass) -->
<!-- Sources: apps/turdmod-engine-bridge/CLAUDE.md, apps/turdmod-engine-bridge/README.md, src/TurdMODEngineBridge.cpp (regs[] table = 101 names), CAPABILITY-MAP.md, CLAUDE.md (root), IDEAS.md -->

# SHIP-STATUS — What works today, what needs the Engine

This matrix is the operator-facing answer to "can I run mod X on my server right now?"

Three buckets:

- **Ships today** — works on stock SCUM (vanilla client + dedicated server) using the companion log-tail + optional FTP path. No Engine required.
- **Partial** — half ships today (usually the Discord side); the in-game half waits for the Engine or the client loader.
- **Engine-required** — preview only on stock SCUM. Needs the TurdMOD Engine (UE4SS C++ bridge + server-side loader DLL) to move bits in the running game. If the effect is **in-game visual only** (HUD panels, client-rendered overlays), it also needs the **TurdMOD client loader** (`turdmod-loader` / `turdmod-launcher`, the DayZ-style launcher that turns BattlEye off for our servers while leaving the Steam install untouched).

The **TurdMOD Engine** is the **UE4SS C++ cppmod** (`TurdMODEngineBridge.dll`) loaded inside `SCUMServer.exe`, bridging UE4SS reflection ↔ `turdmod_server_loader.dll` over an **in-process C ABI** (direct function-pointer calls — no IPC between these two). The loader's named-pipe JSON-RPC server (`\\.\pipe\turdmod-engine-<pid>`, pipe path written to `%LOCALAPPDATA%\TurdMOD\engine\pipe.txt`) is the channel between **external clients** (Manager UI, Companion, Pro) and the loader; from the loader into the bridge is in-process only.

**Status: shipped and proven running.** The engine is functional on Joel's local SCUMServer. 967,265 UObjects walked via `listClassInstances`; `setTimeOfDay` live-verified (`3.81 → 6`, i.e. 3:48am → sunrise in-game). The UEPseudo build blocker was resolved 2026-05-16 (commit `71b6512`). Pak-signature bypass is solved — hooks live in the cppmod constructor (earlier than SCUM's pak enumeration; proven by UE4SS.log vs SCUM.log timestamps). Deploy is via Manager → Engine page → **Install** button.

---

## Mod-by-mod matrix

| Mod | Bucket | What works now | What needs the Engine / Client Loader |
|---|---|---|---|
| **Welcome Screen** | Partial | Discord embed on first-join (via companion `discord-webhook` sink) | In-game branded panel with banner + dismiss button (needs client loader for in-game UI render) |
| **Killfeed** | Ships today | Discord kill ticker from log-tail death events | Optional in-game HUD overlay (needs client loader) |
| **Squad-Mate** | Partial | Discord squad list + voice-status; companion-driven | In-game squad panel with keybind toggle (needs client loader) |
| **Teleport** | Engine-required | Chat command parsing + waypoint persistence; Discord notify | Actual position change — Engine `teleportPlayer` RPC (available today on Engine tier) |
| **Companion** | Engine-required | Adoption metadata stored per-player | In-game follower AI + persistence across logout |
| **Survivor Rescue** | Engine-required | Rescue ledger + traits storage | Actual zombie→NPC transformation in-game |
| **MapZoom** | Engine-required | Config schema only | UE4 map widget hook + render (needs client loader for in-game map overlay) |
| **DropPin** | Engine-required | Pin persistence + Discord notify | In-game HUD bearing strip (needs client loader) |
| **VehicleControls** | Engine-required | — | UE4 input/state hooks on vehicle interactions (Engine `writeClassDefault`/`applyRecipe`) |
| **Patrols & Bandits** | Engine-required | Schedule + config persistence | Actor spawn + AI behaviour scripting (Engine `spawnVehicle`/actor RPCs) |
| **EventsManager** | Partial | Schedule storage, calendar UI, Discord triggers, audit log, Lua hook | Live spawn-wave execution + weather/time-of-day scripting (Engine `setTimeOfDay` live; spawn-wave handlers in progress) |
| **PrebuiltBlueprints** | Engine-required | JSON preset library + export/import | In-game build placement |
| **BetterCooking** | Engine-required | Timer queue persistence | In-game persistent timer overlay (needs client loader for HUD) |
| **LootTableManager** | Engine-required | Per-zone/per-event table editor + JSON export | Live container population + bunker hook |

---

## How "Ships today" works (no Engine)

The TurdMOD **companion** (`apps/turdmod-companion`, Node.js) tails the SCUM dedicated server log files and parses gameplay events (chat, kill, login, vehicle, etc.). Each event is dispatched on a typed bus. Mods are TypeScript modules that subscribe to channels and emit outbound payloads — typically Discord webhooks, optional FTP writes to admin INI files (BannedUsers.ini etc.), or per-player persistence under the companion store dir.

This covers any mod whose effect is "do something *outside* the game" (Discord notify, persist state, schedule things). It cannot move bits *inside* the running game — that needs the Engine.

The Manager desktop app (`apps/turdmod-manager`) ships a **soft-RCON Admin tab** on the Server detail page that uses FTP to read/write admin files on g-portal / Nitrado / self-hosted boxes. This works today and covers most admin-file workflows (whitelist, bans, server settings).

---

## How Engine-required mods work (Engine tier — self-hosted/VPS)

The TurdMOD Engine is **already deployed and proven running** on self-hosted servers. The full stack:

1. `SCUMServer.exe` is launched with `turdmod_server_loader.dll` loaded (either via `turdmod-launcher.exe` injection or UE4SS proxy-DLL install).
2. `turdmod_server_loader.dll` (Rust) starts the named-pipe JSON-RPC server at `\\.\pipe\turdmod-engine-<pid>` and registers the C-ABI extern surface (`register_handler`, `emit_event`). The pipe path is written to `%LOCALAPPDATA%\TurdMOD\engine\pipe.txt`.
3. UE4SS loads `TurdMODEngineBridge.dll` (C++ cppmod) from `UE4SS\Mods\TurdMODEngineBridge\dlls\main.dll`. The cppmod constructor installs the **pak-signature bypass hooks** (before SCUM's pak enumeration races UE4SS init). In `on_unreal_init`, it mirrors UE4SS globals (`GUObjectArray`, `FName::ToString`, `GMalloc`, ProcessEvent vtable) then calls `GetProcAddress` on the loader's C-ABI exports and registers all RPC handlers.
4. **~101 RPC handlers** are registered (canonical source of truth: the `regs[]` table in `src/TurdMODEngineBridge.cpp`; cross-checked against `CAPABILITY-MAP.md`). A core set is **live-verified** — `ping`, `broadcastChat`, `sendChatLineToPlayer`, `teleportPlayer`, `setTimeOfDay`, `setFamePoints`, `setCurrencyBalance`, `broadcastRaidBanner`, `setEconomy`, `getOnlinePlayers`, `getServerStats`, `getActorPopulation`, `listClassInstances`, `dumpClasses`, `dumpWidgets`, `describeWidget`, `dumpUFunctions`, `findFunctions`, `dumpAdminCommands`, `applyRecipe`, `listHandlers`, … Others are registered but **pending deploy-verification** (e.g. `forceGC`, `runConsoleCommand` — see the memory-leak dossier).
5. The Manager UI (or Companion/Pro) connects via the named pipe. Mods call typed RPCs — `broadcastChat({ text })`, `teleportPlayer({ steamId, x, y, z })`, `setFamePoints({ name, value })`, `applyRecipe(…)` — the bridge marshals these to the UE4 game thread and executes them in-process.
6. Inbound events flow back to the companion as the engine's build-safe ProcessEvent dispatch (`g_fn_ptr_dispatch` pointer-compare) fires. **Shipped:** `playerLogin`, `playerLogout`, `chat` (ts/channel/player/SteamID/text via `this_`→Outer→PlayerController→PlayerState walk), client-chat, admin-response. **Remaining:** `playerDeath` — the one inbound event not yet wired (see roadmap).

**Important RPC caution:** `runAdminCommand` / `runTestAdminCommand` are registered but should **not** be used as a general admin path — SCUM's admin UFunctions silently reject server-side dispatch because their auth checks rely on network metadata our PE-bypass strips. Use the direct primitives (`broadcastChat`, `sendChatLineToPlayer`, `setFamePoints`, etc.) instead.

g-portal (and other locked-down hosts that don't allow custom binaries) get the soft-RCON tier only — Engine work needs self-hosted or Nitrado.

---

## Client-side loader (in-game visual mods)

For mods that need **client-side in-game rendering** (HUD overlays, squad panels, welcome panels, map zoom), a second component is required: the **TurdMOD client loader** (`apps/turdmod-loader` — a Rust DLL injected into `SCUM.exe`). This is distinct from the server-side Engine.

The DayZ-style **TurdMOD Launcher** (`apps/turdmod-launcher`, Tauri/Rust, branch `feat/client-modded-launcher`) orchestrates this: it lets players pick a BE-off server, toggle client mods, and launch. BattlEye is disabled for our engine/servers via choosing a separate launch path — the Steam install is **never modified**, so "Play" on Steam stays vanilla+BE for official servers.

**Current status of the client loader (`turdmod-loader`):**

| Layer | Status |
|---|---|
| Layer 0: Detection (BE present, SCUM running, mode inference) | Done (`src/detect.rs`) |
| Layer 1: Loader DLL + launcher (injection chain) | Done — minimum viable injection |
| Layer 2: Rich-text decorators (`<img>`/`<a>`/action-prompt via vtable detour) | In progress — `decorators/` staged, observe-only; blocked on runtime CDO + UTexture2D resolution (see `docs/engine/research/loader-decorator-resolution-dossier.md`) |
| Layer 3: General UE4 hook surface (UFunction → handler, broad signature scans) | Not started |

<!-- SOURCE: apps/turdmod-loader/README.md status table; apps/turdmod-loader/decorators/src/*; IDEAS.md 2026-05-30 "Client modded launcher" entry -->

Until Layers 2–3 land, client-side in-game visuals (HUD panels, map overlays, etc.) remain unavailable even on Engine-tier servers.

---

## Roadmap

The Sprint 1/2/3 Rust-DLL plan described in earlier versions of this document is **superseded**. The engine shipped as a UE4SS C++ cppmod, not as a standalone Rust injection DLL. Active work tracks as follows:

**Engine tier (server-side) — working today:**
- ✅ UE4SS C++ cppmod (`TurdMODEngineBridge.dll`) loaded inside `SCUMServer.exe`
- ✅ ~101 registered RPC handlers over in-process C ABI → named-pipe JSON-RPC (core set live-verified)
- ✅ Pak-signature bypass (constructor hooks, proven stable)
- ✅ UEPseudo build blocker resolved (commit `71b6512`, 2026-05-16)
- ✅ Manager → Engine page → Install deploy flow
- ✅ 967,265 UObjects proven walked; `setTimeOfDay`, `setFamePoints`, `broadcastChat`, `broadcastRaidBanner`, `setCurrencyBalance`, `teleportPlayer` all live-verified
- ✅ `getServerStats` capacity-poll leak fixed (commit `25415ac`, deployed to OVH 2026-05-31; was ~4.9 GB/h via uncached `fname_to_wstring`)
- ✅ `chat` inbound event — **shipped** (`EV_CHAT` hooks `Chat_Server_BroadcastChatMessage`; emits ts/channel/player/SteamID/text). Also shipped via the same pointer-compare dispatch: `playerLogin`, `playerLogout`, client-chat (`Chat_Client_SendMessageToChat` → `EV_CLIENT_CHAT`), admin-response.
- ⏳ `playerDeath` inbound event — the **one remaining** inbound event, and the hardest. No clean death UFunction (it's the per-hit damage path, e.g. `Prisoner::Server_ApplyPointDamage`), and no obvious `Health`/`IsDead` field on `Prisoner` (147 props; health/death-state likely on a component) to detect lethality. Needs grounded RE — use the new L5/L6 digests + a live probe to locate where death/health-state lives, then add `EV_DEATH` following the `EV_CHAT` pattern. Do NOT guess.
- ⏳ Slow-growth leak verdict (separate from the fixed poll leak) — pending a real `forceGC` on a grown OVH server (see `docs/engine/research/ovh-memory-leak-dossier.md`)
- ⏳ Pak-bypass v3 (caller-aware filter via `_ReturnAddress()`) — required before custom `.pak` mods can mount; v2 shipped but still triggers the SCUM.uproject modal for descriptor validators. Phase C (Hello World pak) is blocked on this. <!-- VERIFY: confirm v3 not yet shipped before next publish -->

**Client tier — in progress:**
- ✅ Client loader skeleton (`turdmod-loader` DLL + injection launcher, Layer 0–1)
- ✅ DayZ-style modded launcher (`turdmod-launcher`, branch `feat/client-modded-launcher`) — server picker, mod toggles, BE-off launch path, Steam install untouched <!-- VERIFY: still on branch, not yet merged to main -->
- ⏳ Rich-text decorator detours (Layer 2) — unblock via runtime resolution dossier
- ⏳ General UE4 hook surface (Layer 3) — unlocks in-game HUD panels, map zoom, welcome screen, squad UI

See `IDEAS.md` (search `[shipped]` near "TurdMOD Engine", "Real reflection", "Client modded launcher") for the full chronological history.
