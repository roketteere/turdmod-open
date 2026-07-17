# Custom HUD Panels — implementation plan

**Discovered state, V1 architecture, and phased roadmap.** Written 2026-05-18 as Joel was leaving for the store; verified loader internals first-hand before drafting.

---

## TL;DR

Two surprises that change the roadmap:

1. **The loader is much further along than the project's tier model implied.** `turdmod-loader/src/hooks.rs` (948 lines) ships a fully working DXGI Present hook via `hudhook 0.9`, ImGui-based rendering loop, font loading, TurdMOD theme, and a public `enqueue_panel(JsonValue)` API that already draws arbitrary panels on-screen. There's also a default loading splash and welcome panel that fire on injection.
2. **We don't need UE4 reflection or UMG widget construction.** The agent's earlier "Path B" research talked about hooking `AddToViewport` and instantiating UMG widgets — that's the wrong tree. ImGui draws on top of the swap chain regardless of UE4. Skipping UMG entirely simplifies the surface enormously.

What's missing is one thing: **a way for the Manager to call `enqueue_panel` from outside the SCUM client process.** That's the entire V1.

---

## Verified loader state (2026-05-18)

### Files (`apps/turdmod-loader/src/`)

| File | Lines | Status | Purpose |
|---|---|---|---|
| `lib.rs` | ~250 | live | DllMain → spawn `init_thread` → detect → runtime → hooks → ipc. |
| `detect.rs` | ~? | live | Mode enum: Solo / PrivateBeOff / PrivateBeOn / Unknown. Refuses to proceed on BattlEye-loaded targets (defense-in-depth; Joel's servers always have BE off per `project_battleye_always_off`). |
| `hooks.rs` | 948 | **live, shipping** | hudhook DX11 swap-chain hook + ImGui render loop + panel queue lifecycle (PENDING → ACTIVE, frame-clock + wall-clock gates, auto-close timing) + Consolas/Cascadia font loading + animated TurdMOD theme. Public API: `install()`, `enqueue_panel(JsonValue)`, `enqueue_loading_splash()`, `enqueue_default_welcome_panel()`. |
| `runtime.rs` | ~? | live | Lua 5.4 (vendored mlua) sandbox + mod discovery from `%LOCALAPPDATA%/TurdMOD/mods/<id>/main.lua`. |
| `api.rs` | ~? | live | Lua FFI surface: `turdmod` global table, event dispatch handler registry. |
| `ipc.rs` | ~? | live | **Outbound** SSE subscriber: connects to companion `/events`, dispatches to Lua. Discovery: `~/.scummy-map/turdmod-companion.json` → `url`, env override `TURDMOD_COMPANION_URL`, fallback `http://127.0.0.1:8911`. |
| `sigscan.rs` | ~? | scaffold | UE4 global pointer scans (GEngine / GWorld / GameViewport / FUObjectArray / FNamePool). **Patterns are stubs** — needs real byte sequences from SCUM.exe to validate. Not blocking for ImGui panels. |
| `logging.rs` | ~? | live | Append-only NDJSON audit log to `%LOCALAPPDATA%/TurdMOD/loader.log`. |
| `proxy.rs` | ~? | placeholder | `dxgi.dll` export forwarding stubs. |

### Dependencies (`Cargo.toml`)

All actively used:
- `windows-sys 0.59` — Win32 APIs. **Note:** `Win32_System_Pipes` is NOT currently in the feature list; need to add it for named-pipe server.
- `mlua 0.10` (lua54, vendored, send, serialize)
- `hudhook 0.9` (default = dx11 + dx12)
- `ureq 2` — outbound HTTP for SSE
- `serde`, `serde_json`, `once_cell`, `parking_lot`, `chrono`

### Existing IPC contract observed

The loader's `ipc.rs` is **outbound only** (consumer of companion events). There's no inbound RPC channel today — the companion is the only thing that can push data into the loader.

The companion has the discovery-file pattern that we'd mirror for an inbound named pipe:
- Discovery file at a well-known path
- Loader writes the pipe name on startup
- External callers read the discovery file and connect

The `turdmod-engine-bridge` (in `SCUMServer.exe`) has its own inbound named pipe (`\\.\pipe\turdmod-engine-{pid}`) that the Manager already uses via Tauri's `engine_rpc` command. That's the pattern to clone for the client-side loader.

---

## V1 architecture

**Goal:** Admin composes a panel in the Manager → it appears on the SCUM client's screen via ImGui.

### Hop diagram

```
Manager (Tauri/React)                        SCUM Client
─────────────────────────                    ─────────────────────────────────
[CustomPanelsPage]                           [SCUMClient.exe (loader injected)]
  ↓ button click                                   │
[useCustomPanels hook]                             │
  ↓ engineRpc-style call                           │
[Tauri command: loader_rpc]   ──named pipe──>  [loader::ipc_server]
                                                     ↓ deserialize JSON
                                                 [hooks::enqueue_panel(json)]
                                                     ↓ render-thread drain
                                                 [ImGui draws the panel]
```

**Same machine assumption for V1.** Manager + SCUM client are both on Joel's dev box. The Tauri command opens the loader's pipe directly. Cross-machine (Manager controls a remote client) is a future concern that would route through the bridge.

### Panel JSON shape (proposed)

```json
{
  "id": "welcome-banner-1",
  "title": "Welcome",
  "body": "Welcome to the server, {playerName}!",
  "anchor": "top-center",
  "x_offset": 0,
  "y_offset": 50,
  "background_color": [60, 40, 20, 200],
  "text_color": [240, 220, 180, 255],
  "duration_ms": 5000,
  "fade_in_ms": 300,
  "fade_out_ms": 300
}
```

The existing `enqueue_panel` accepts arbitrary `JsonValue` so the V1 spec is whatever `hooks.rs` already understands. Need to read the panel-deserialization code in `hooks.rs` to confirm exact field names before drafting the Manager form.

---

## Phased roadmap

### Phase 1 — Loader inbound IPC ✅ shipped 2026-05-18

**Scope:** Loader exposes a named pipe `\\.\pipe\turdmod-loader` (static for V1), writes its name to `%LOCALAPPDATA%/TurdMOD/loader/pipe.txt`, accepts JSON-RPC requests with two methods: `ping`, `enqueuePanel`.

**Files:**
- NEW `apps/turdmod-loader/src/ipc_server.rs` — named-pipe server + per-connection handler; raw Win32 (no tokio), length-prefixed JSON framing matching the bridge's protocol.
- EDIT `apps/turdmod-loader/src/lib.rs` — `mod ipc_server;` + `ipc_server::start()` in `init_thread`.
- EDIT `apps/turdmod-loader/Cargo.toml` — added `Win32_System_Pipes`, `Win32_System_IO`, `Win32_Security` features.

**Status:** Compiles clean (release build, 19s). Not yet runtime-tested with a connected Manager — that's Phase 2.

**Limitation:** static pipe name → one SCUM client per machine. Two concurrent injections would race on CreateNamedPipeW with PIPE_UNLIMITED_INSTANCES; the second client's pipe creation succeeds but its `enqueuePanel` calls land in the wrong process. Phase 2 should expose a per-PID variant if Joel ever wants to run multiple clients.

### Phase 2 — Manager Tauri command + page ✅ shipped 2026-05-18

**Scope:** Tauri command `loader_rpc(method, params)` reads the loader's discovery file and sends length-prefixed JSON-RPC over the pipe. `CustomPanelsPage.tsx` has a Quick form (title/body/duration) + Raw JSON editor side-by-side, three preset payloads, recent-sends history with replay, and a live loader status badge polling every 5s via `useLoaderStatus`.

**Files:**
- NEW `apps/turdmod-manager/src-tauri/src/loader_rpc.rs` (clone of engine_rpc with loader-side paths)
- EDIT `apps/turdmod-manager/src-tauri/src/lib.rs` — registered `loader_rpc::loader_rpc`
- NEW `apps/turdmod-manager/src/lib/tauri-loader.ts` — `loaderRpc<T>`, `loaderPing`, `enqueuePanel`
- NEW `apps/turdmod-manager/src/hooks/useLoaderStatus.ts` — TanStack Query polling + `useEnqueuePanel` mutation
- NEW `apps/turdmod-manager/src/pages/CustomPanelsPage.tsx`
- EDIT `apps/turdmod-manager/src/App.tsx` — added `/custom-panels` route under Builder group

**Status:** Compiles clean (tsc + cargo check). Not yet runtime-tested with a live loader injection — that's the next session: launch SCUM client with dxgi.dll proxy in place, open Manager → Builder → Custom Panels, confirm status flips green, send a Quick panel and watch it draw in-game.

### Phase 3 — Panel persistence + presets (~½ day)

**Scope:** Save composed panels to `apps/turdmod-manager/src-tauri/store` (tauri-plugin-store). Preset library so admins reuse common panels.

**Risk:** Low. UI feature, no engine touching.

### Phase 4 — Cross-machine support (~1 day, optional)

**Scope:** Route panel-send requests through the bridge instead of directly to the loader. Bridge connects to the loader on the same machine as the SCUM client (which is also where the bridge runs — both in the same Steam install). Manager can be remote.

**Files:**
- NEW C++ `handle_loader_send_panel` in TurdMODEngineBridge.cpp
- Connects to `\\.\pipe\turdmod-loader` (same machine as bridge, since both run on the server box when the loader is on a player client this gets weirder — see open question)

**Risk:** Medium. Bridge runs in `SCUMServer.exe`; the loader runs in `SCUMClient.exe`. For a server admin to push a panel to a connected player, the bridge needs to know the player's client machine, and the loader needs to be reachable from there. Probably involves an outbound channel from the loader (already exists — SSE subscriber to companion) being repurposed for inbound commands.

**Open Q:** Architecture for server-admin → connected-player flow. May need a companion-mediated push: Manager → companion → companion forwards to client loaders via existing SSE channel.

### Phase 5 — Designer page (~2-3 days)

**Scope:** Drag-and-drop panel composer in the Manager. Layout primitives (text, button, image, list). Style controls (font, color, padding). Save as panel JSON.

**Risk:** Low (UI only) but big surface.

### Phase 6 — Interactive panels (~research)

**Scope:** Player clicks a button on a custom panel → loader sends event back to companion/bridge → server reacts. Needs an outbound IPC channel from the loader to the companion (already exists — could reuse).

**Risk:** Low-Medium. Existing outbound IPC infrastructure already does most of this.

### What we are NOT doing

- ❌ UE4 UMG widget construction (ImGui replaces the need entirely)
- ❌ `AddToViewport` hooks (ImGui draws above the viewport)
- ❌ Server-side widget replication (we never touch SCUM's UMG)
- ❌ Lua sandboxing for panel logic (panels are declarative JSON; if we want behaviour later, add a button-callback model in Phase 6)

---

## Open questions to resolve when Joel is back

1. **Pipe naming:** static `\\.\pipe\turdmod-loader` (simple, single-instance) or per-PID (multi-instance-safe)? V1 leans static.
2. **Panel JSON schema:** confirm the exact fields `hooks.rs` currently understands by reading the panel-deserialization code. Today's plan assumed common ones (title, body, color, duration) — needs verification before the Manager form is built.
3. **Cross-machine architecture:** for a real production server pushing panels to remote players, what's the topology? Companion-mediated push (reusing the SSE channel) seems right but needs Joel's input on whether the companion is willing to grow inbound RPC duties.
4. **Sigscan patterns:** when (if ever) do we need to resolve real GEngine / GWorld / etc.? Phase 4+ if we ever want server-side data on the panels. Defer until needed.

---

## Concrete next-step session plan

When Joel is back, the proposed order:

1. **Together (~30 min):** read `hooks.rs` panel-deserialization to lock the JSON schema.
2. **I write (Sonnet/Haiku if budget allows, otherwise Opus):** Phase 1 IPC scaffold. ~150 lines of Rust + 5 lines of `lib.rs` integration + 1 Cargo.toml line. Build the loader. Don't deploy yet (loader rebuilds need careful injection testing).
3. **I write:** Phase 2 Manager Tauri command + page. ~200 lines TS + Rust. HMR-friendly.
4. **Joel tests:** boot SCUM client with fresh loader, open Manager, send a panel, confirm it draws.
5. **Iterate** on JSON schema + form ergonomics until the basic flow feels good.

After that V1 lands, Phases 3-6 are independent improvements that can ship as separate PRs.
