# TurdMOD — Developer Handbook

The deep guide to how TurdMOD modifies a live SCUM dedicated server. Read this
to understand the system end-to-end before getting your hands dirty. (Short
public blurb lives in `README.md`; this is the real map.)

TurdMOD turns a stock **SCUM dedicated server** into a moddable one by injecting
an in-process C++ bridge that can read/write the engine's live objects, call its
functions, and patch its code — plus a parallel **RCON** channel for admin
commands. Players get features like `!myride in/out` (store/retrieve a vehicle)
without anyone needing an admin account.

---

## 1. The stack — bottom to top (the "1s and 0s up to the apps" map)

```
L7  KNOWLEDGE        scion / lobe — indexed RE facts, tool registry, the auth-gate map
                     └ C:\Development\Claude\scion  (MEMORY.md, IDEAS.md, TOOL-REGISTRY.md)
        ▲ retrieval across sessions
L6  FEATURES         !myride in/out, god mode, despawn — composed from L5 calls,
                     triggered by players (chat) or admins (Manager GUI)
        ▲
L5  APPS & TOOLS     Node/TS/Rust/Tauri programs that CALL the bridge / RCON
                     └ apps/  → turdmod-manager (Tauri GUI), turdmod-service (Rust API,
                       holds the RCON client), -bot, -cli, -web …
                       + tools/engine-rpc-test.mjs (the bridge CLI)
                       admin-commands.json (canonical verb list) lives in turdmod-manager
        ▲ JSON-RPC over a local transport   |   BattlEye RCON (BERcon, UDP)
L4  IPC / RPC        bridge handler registry (readMemory, runAdminCommand,
                     despawnVehicleNative …) — named methods, JSON in/out
        ▲ in-process function calls
L3  BRIDGE (our C++) TurdMODEngineBridge.dll — hooks ProcessEvent (PolyHook2),
                     reads/writes UE memory, calls reflected+native fns, patches code
                     └ source : apps/turdmod-engine-bridge/src/TurdMODEngineBridge.cpp (~13.7k lines)
                       mirror : C:\Development\RE-UE4SS\cppmods\TurdMODEngineBridge\src\dllmain.cpp
                       built  : C:\Development\RE-UE4SS\build\Game__Shipping__Win64\bin\TurdMODEngineBridge.dll
                       live   : <server>\SCUM\Binaries\Win64\UE4SS\Mods\TurdMODEngineBridge\dlls\main.dll
        ▲ injected by…
L2b UE4SS            UE script-extender DLL that injects L3 into the running server
                     └ C:\Development\RE-UE4SS\
        ▲
L2  UE OBJECTS       Unreal's live object graph in RAM — what the bridge manipulates
   (in memory)       └ GUObjectArray (~1.5M UObjects): UClass, UFunction (reflection),
                       instances (PlayerRpcChannel, ConZPlayerController, Prisoner, vehicles).
                       Data at fixed offsets: PC+0xe8 (admin role), Prisoner+0x1D58 (flags),
                       WeatherCtrl+0x2A8 (time-of-day).
        ▲ created at runtime from…
L1  THE BINARY       SCUMServer.exe — machine code + constants, the RE surface
   (RE layer)        └ <Steam>\common\SCUM Server\SCUM\Binaries\Win64\SCUMServer.exe (127 MB PE)
                       sections .text/.rdata/.data; read with tools/re/*.py + capstone
        ▲ loaded/relocated by Windows to…
L0  CPU + RAW BYTES  x86-64 instructions; preferred base 0x140000000, loaded at an
                     ASLR base (e.g. 0x7ff730c70000). The literal 1s & 0s.
```

**Request flow (e.g. `!myride in`):** player types it (L6) → bot/app (L5) sends
JSON `{despawnVehicleNative…}` over IPC (L4) → bridge handler (L3) finds the
vehicle UObject (L2), queues a game-thread call, runs SCUM's native despawn (L1
code) on real bytes (L0) → vehicle vanishes → result bubbles back up.

---

## 2. SCUM netcode & how we drive it (the juicy part)

SCUM is an Unreal Engine 4 game. Understanding UE's networking is the difference
between "the command fires but nothing happens" and "it works."

### 2.1 Authority & replication

- The **dedicated server has authority** over the world. Clients hold proxies.
- Every replicated `AActor` has a `Role` and `RemoteRole`. On the server, an
  actor it owns is `ROLE_Authority`; the client's copy is a proxy.
- State the server writes to a **replicated property** is auto-pushed to clients
  next net-update. This is why **direct field writes work**: write
  `WeatherController2._timeOfDay` (offset `0x2A8`) or a `Prisoner` flag byte
  (`0x1D58`+) on the server and UE replicates it out for free. No function call,
  no auth — we ARE the authority.

### 2.2 RPCs and `ProcessEvent` — the trap

UE functions flagged `Server` / `Client` / `NetMulticast` are **RPCs**. The name
tells you the direction:

| Flag           | Runs on | Called from |
|----------------|---------|-------------|
| `Server` (`Chat_Server_*`) | server | client |
| `Client` (`Chat_Client_*`) | client | server |
| `NetMulticast` | everyone | server |

When you call a function via `UObject::ProcessEvent`, UE first checks
`GetFunctionCallspace`. For a **net function**, the result can be `Local`
(execute here), `Remote` (serialize + send over the wire), or **`Absorbed`**
(do nothing). A `Server` RPC invoked on the server, on a **client-owned**
object, does **not** simply run — UE routes/absorbs it, because that function's
contract is "client asks server." **`ProcessEvent` returns cleanly and the
implementation never executes.** This is the wall that ate multiple sessions.

**What this means in practice:**

- ✅ **`damageVehicle`** works. It calls the reflected `Server_ApplyDamageToRegion`
  on the **vehicle** (a server-authoritative actor) — callspace `Local`, executes,
  SCUM's destruction pipeline despawns the vehicle. Self-contained, no auth.
- ✅ **`teleportPlayer`** (`K2_TeleportTo` on the pawn), **`spawnItem`**
  (`GameplayStatics::SpawnObject`) — stock UE reflected calls, no admin path.
- ✅ **Direct field writes** (Pattern-D): time, god/ammo/jump flags, etc.
- ❌ **Admin commands via the bridge** (`Chat_Server_ProcessAdminCommand` on the
  **PlayerRpcChannel**) — `ProcessEvent` does not execute it; SCUM's pipeline also
  validates game-context up front and aborts off the game thread. Dead end — and
  RCON can't run SCUM `#`-commands either (§2.5). We **bypass** admin commands
  entirely (§2.6).

### 2.3 The exec thunk → `_Implementation` chain

Reflected functions have a tiny generated **exec thunk** (`exec<Name>`) whose job
is: read parameters from the `FFrame`, then call the real C++ body
(`<Name>_Implementation`). For `Chat_Server_ProcessAdminCommand` the thunk does
`call qword [vtable+0x7d8]` — i.e. the impl is a **virtual** method at
`PlayerRpcChannel` vtable offset `0x7d8`. Static VA of that impl (build 23451409):
`0x141e54ff0`. When a real client command arrives over the wire, SCUM's netcode
calls this `_Implementation` **directly** — never `ProcessEvent`.

### 2.4 SCUM's admin-command auth gate (fully reversed, build 23451409)

Inside `Chat_Server_ProcessAdminCommand_Implementation` (`0x141e54ff0`):

1. `r13 = [channel+0xa0]` = owning `ConZPlayerController` (the sender PC).
2. Tokenize → recognize verb against a command table (`0x141e945d0`; miss =
   "unrecognized command").
3. Resolve the `AdminCommand_*` object, then the **auth gate**:
   `call [cmdObj_vtable+0x280] (PC)` at `0x141e553fb` → bool. `false` → "no
   permission" (error code 7). `true` → execute.
4. That `CanExecute` routes to `ConZPlayerController::CanExecuteCommand`
   (`0x1419ce040`): super-admin role → all commands; else the verb must be in the
   PC's **per-command permission map** at `PC+0x698` (the bracketed allowlist from
   `AdminUsers.ini`).

**Key PC offsets (levers):**

| Offset | Meaning |
|---|---|
| `PC+0xe8` (dword) | admin role/level. Compared by `roleCheck` (`0x142a105c0`) against `g_table[*]`. A regular bracketed admin = `0x2e8` (== `g_table[0x11a]`). |
| `PC+0xec` (dword) | guard; super branch requires `== 0`. |
| `PC+0x698` | per-command permission map (the `[Bracket]` allowlist). |
| `PC+0x6b8` (byte) | hard override inside `IsUserAdmin` (`0x1419cdf60`): non-zero → admin. (On the `IsUserAdmin` path, not the command path.) |

The CanExecute branch (`jne` at `0x141e55403`) can be flipped `75→eb` to force
"authorized" — proven with a scoped, reversible 1-byte patch. **But forcing auth
doesn't help when `ProcessEvent` won't run the impl in the first place (§2.2).**

### 2.5 RCON — BattlEye, not Source; server-management only

> ⚠️ **Correction (2026-05-30).** Earlier docs (and `turdmod-service/src/rcon.rs`)
> claimed SCUM uses **Source RCON** and that RCON runs admin commands. **Both are
> wrong.** SCUM uses **BattlEye RCON (BERcon, a UDP protocol)**, and it only runs
> **BattlEye's own** commands — *not* SCUM's `#` game-admin commands.

**What RCON is.** SCUM is BattlEye-protected; its admin console is **BattlEye RCON
(BERcon)** — UDP, CRC32-framed (`'B''E' | crc32 | 0xFF <type> <data>`). Authenticated
by the RCON password = server console = full authority. Configured in
**`<server>/BattlEye/BEServer_x64.cfg`** (`RConPassword`/`RConPort`/`RConIP`), NOT
`ServerSettings.ini`.

- ✅ **Working client: `scripts/rcon_be.py`** (BERcon). PROVEN live: `players`
  returns the live roster with full authority.
- ✅ **Rust port: `apps/turdmod-service/src/rcon_be.rs`** (Phase 3, 2026-05-30) — same
  BERcon wire format (CRC32 unit-tested against `zlib.crc32`). The router's `Rcon` bridge
  calls `rcon_be::exec_cfg`, reading host/port/pass from `C:\TurdMOD\data\rcon.json`.
  Routes `!kick` / `!ban` / `!say` (`!announce`). `!kick`/`!ban` resolve a player **name**
  to its BE slot `#` via a `players` lookup. `!players` stays on the engine pipe (richer
  structured data) rather than parsing BE text.
- ❌ `apps/turdmod-service/src/rcon.rs` and `scripts/rcon.py` are **Source RCON
  (TCP)** — they do **not** work against direct SCUM (nothing binds the port).
  They may only work through a managed-host RCON proxy.

**What RCON can and cannot do (proven 2026-05-30):**

| Command class | Example | Via BattlEye RCON? |
|---|---|---|
| BattlEye server-management | `players`, `kick`, `ban`, `say` | ✅ executes with authority |
| SCUM `#` game-admin commands | `#SpawnVehicle`, `#SetGodMode`, `#SetTime` | ❌ return empty / no effect |

So there is **NO working "run any SCUM admin command" channel.** `#`-commands
fail both via RCON (wrong layer; position-bound ones have no admin location) **and**
via the bridge `ProcessEvent` (§2.2). The auth-gate RE (§2.4) explains *why a
bracketed admin is limited*, but is **not** a lever for injecting commands.

**⚠ BattlEye on/off decides whether RCON even exists (VERIFIED 2026-05-30).**
SCUM's RCON *is* BattlEye RCON — so the admin channel's availability is gated by BE mode:

| Server BattlEye | RCON (BErcon UDP) | Admin path |
|---|---|---|
| **ON** | ✅ binds (local 30016 — `players` returned a live roster, proven) | BErcon (`rcon_be.py` / `rcon_be.rs`) |
| **OFF** (`-NoBattlEye`) | ❌ **no listener binds** — `-RCONEnabled=1 -RCONPort=N` is silently dead | **engine bridge only** |

Evidence: OVH runs `-NoBattlEye -RCONEnabled=1 -RCONPort=7048`; `netstat` on the SCUM
PID shows **only** UDP 7042 (game) + TCP/UDP 7044 (query) — **nothing on 7048**, TCP or
UDP. The RCON flags are ignored without BattlEye.

**Policy — BattlEye is ALWAYS OFF on Joel's own servers** (memory `battleye-always-off`;
private/modded only, never official). **Consequence:** BErcon — and therefore the Phase-3
`rcon_be.rs` — has **no live target on a deployment**. RCON is *not* the deployment admin
channel; the **engine bridge** is. `rcon_be` stays valid only in the BE-**on** RE/bypass
research context (the local box currently runs BE on for that work). Managed hosts
(G-Portal) run BE on and can't disable it, so the Lite/RCON tier still applies *there* —
but not on Joel's BE-off infrastructure.

**Client-side BE disable.** To connect a client to a `-NoBattlEye` server, the client must
also run BE-free: launch `SCUM\Binaries\Win64\SCUM.exe` **directly** (skip
`SCUM_Launcher.exe`, which bootstraps BE), or run `SCUM\BattlEye\Uninstall_BattlEye.bat` /
remove the `BattlEye\` dir. `turdmod-loader` (`detect.rs`) *requires* this — it refuses to
inject if a `BattlEye*`/`BEService*` module is loaded in the game process.

### 2.6 How we actually do things — bypass, don't inject

Because admin commands can't be injected, every feature either calls a **specific
engine function directly** or uses **RCON for server-management**:

| Capability | Path | Status |
|---|---|---|
| **Messages** (per-player, colored) | bridge `sendChatLineToPlayer` (`channel` = color: 3=green admin, 4=reply) | ✅ proven |
| **Despawn vehicle** (`!myride in`) | bridge `damageVehicle` (`Server_ApplyDamageToRegion`) | ✅ proven |
| Teleport / items / time / weather / fly / possess | bridge direct reflected/native calls + field writes | ✅ working |
| Server management (kick/ban/players) | BattlEye RCON (`rcon_be.py`) | ✅ channel proven — **BE-on only**; on BE-off deployments use the engine bridge (§2.5) |
| **Full-assembly vehicle spawn** (`!myride out`) | *(needs native `VehicleManager` spawn on the game thread)* | ❌ **OPEN — only real gap** |

**The mental model:** there are **two channels** — the **engine bridge** (messages
+ direct actions) and the **BattlEye RCON channel** (server management). Neither is
a general "admin command runner." For any new feature, find the direct engine call;
don't route it through SCUM's `#` admin pipeline. **On Joel's BE-off deployments only
the engine bridge exists** (RCON needs BattlEye on — §2.5), so server-management there
must also go through the bridge.

**Deployment tiers:**

| Tier | Channel | Needs | Can do |
|---|---|---|---|
| **soft** | BattlEye RCON (UDP) | BattlEye **ON** + `BEServer_x64.cfg` RCON config | server-management (kick/ban/players/say). Applies to **managed hosts (G-Portal) that run BE** — **NOT** Joel's own BE-off servers, where no RCON binds. |
| **engine** | bridge DLL (ProcessEvent / memory) | write access to `…/Binaries/Win64/` + UE4SS | messages, despawn, field writes, reflection, native calls, custom widgets. Local + OVH. **The only admin channel on BE-off servers.** |

### 2.7 Inbound event pipeline (how `!` commands reach the service)

A player typing `!time 20` travels:
```
player chat → SCUM ProcessEvent(Chat_Server_BroadcastChatMessage)
  → bridge hooked_process_event: POINTER-compare fn against g_fn_ptr_dispatch → EV_CHAT
  → emit_engine_event("chat", …) → named pipe (turdmod-engine-<pid>)
  → turdmod-service events.rs → tokio broadcast → chat_cmds::cmd_loop (parser)
  → router/handler → bridge handler / RCON / Ollama / file
```
**Build-safe routing (2026-05-30):** the hot path does **not** read the UFunction FName
(`fn+0x18`) per call — that crashed build 23451409 on the join spawn-flood. Instead
`resolve_event_dispatch_ptrs()` resolves the 6 dispatch UFunctions **once** at startup into
`g_fn_ptr_dispatch` (`UFunction* → event_kind`) and the hook pointer-compares. Routing is
**default-on** (kill-switch `TURDMOD_PE_HOOK=0`). If `!` commands ever die, check routing
first: UE4SS.log `[HOOK] event-dispatch resolved 6 …`, service log `events: chat` /
`chat_cmds: <cmd>`. The command hub is `chat_cmds.rs` (the **parser**): it builds a normalized `Command` and
hands it to `router::dispatch`, the **interpreter** that routes to the right bridge. Live
bridges: **EnginePipe** (Phase 2 — time/weather/tp/spawn/stats/…) and **BattlEye-RCON**
(Phase 3 — `rcon_be.rs`, `!kick`/`!ban`/`!say`). Ollama + FileState bridges are Phase 4.
Unhandled actions fall through to `chat_cmds`' legacy match.

### 2.8 Hosts, build parity & deploy (2026-05-30)

**Topology:** **OVH RISE-1** (`YOUR_SERVER_IP`) = **primary production** server, BattlEye **OFF**.
**Local** (Steam `SCUM Server`) = **RE/dev parallel** — currently BE-off too, flip to BE-**ON** only
for pak-bypass/signature RE. Both run `turdmod-service` as a Windows service (SYSTEM, so DLL
injection works) and expose the HTTP API on `:9090`; the manager drives both over HTTP. See memory
[[ovh-primary-local-re]] + [[battleye-always-off]].

**Build parity:** only the *build* must match across hosts (BattlEye mode may differ by role). The
service is git-SHA stamped (`build.rs` → `/health`+`/status` `build`). The 4 artifacts are bridge
`main.dll`, `turdmod_server_loader.dll`, `UE4SS.dll`, `turdmod-service.exe`.
- Verify: `scripts/verify-parity.ps1` (SHA256 all 4 + `/health` build + BE flag).
- Deploy (OVH has **no git/toolchain** — copy prebuilt): `scripts/deploy-service.ps1` (exe-only by
  default — never clobbers OVH's real bearer token), `scripts/deploy-engine.ps1` (DLLs, changed only).
- Local service install: `scripts/install-local-service.ps1` (elevated). SSH user is `admin`.

---

## 3. Deployment topology

| Target | What runs | Tier | Notes |
|---|---|---|---|
| **Joel's local box** | Steam SCUM dedicated + UE4SS + bridge + RCON | engine + soft | primary dev/test |
| **OVH RISE-1** (Windows Server 2022) | public SCUM + UE4SS + bridge + `turdmod-service` (HTTP API) + RCON | engine + soft | public test/prod. Engine tier possible because we control the box (full FS access). |
| **G-Portal** (managed) | SCUM + RCON only | soft only | `…/Binaries/Win64/` is host-protected → no UE4SS/bridge. RCON + `Saved/Config` edits only. |

Hosts, ports, keys, passwords: **`.secrets/credentials.md`** (gitignored,
local-only — read at session start before any deploy/ops work). Never commit it.

---

## 4. Build / run / test

### Bridge (C++ → DLL)
```
# Source of truth: apps/turdmod-engine-bridge/src/TurdMODEngineBridge.cpp
# 1. Sync the build mirror (the build compiles dllmain.cpp, NOT the source file):
cp apps/turdmod-engine-bridge/src/TurdMODEngineBridge.cpp \
   C:/Development/RE-UE4SS/cppmods/TurdMODEngineBridge/src/dllmain.cpp
# 2. Build (PowerShell → cmd.exe; Git Bash mangles cmd /c):
#    & cmd.exe /c "C:\Development\Claude\turdmod\tmp\build-bridge.cmd"
#    (vcvars64 + cmake --build … --target TurdMODEngineBridge --config Game__Shipping__Win64)
# 3. Deploy + restart the local server headlessly (kicks connected players):
#    & "scripts\engine-control\cycle-bypass-test.ps1" -DeployBridge -ClearExport
```
Built DLL: `C:\Development\RE-UE4SS\build\Game__Shipping__Win64\bin\TurdMODEngineBridge.dll`
→ deployed to `…\UE4SS\Mods\TurdMODEngineBridge\dlls\main.dll`.

### Calling the bridge (dev CLI)
```
node tools/engine-rpc-test.mjs <method> '<json-params>'
# e.g. node tools/engine-rpc-test.mjs readMemory '{"addr":"0x...","size":"8"}'
#      node tools/engine-rpc-test.mjs despawnVehicleNative '{"vehicleClass":"BP_...","dryRun":"1"}'
```

### turdmod-service (Rust API, holds RCON)
```
cargo run        # from apps/turdmod-service   (HTTP API; bearer-token auth)
cargo test       # from apps/turdmod-service
```

### Web / Manager (TS / Tauri)
```
pnpm dev         # turdmod-web (Next.js) / turdmod-manager (Tauri: pnpm tauri dev)
pnpm test ; pnpm typecheck
```

### RE tooling (binary analysis, no server needed)
```
python tools/re/xref-omni.py <va_hex>          # find xrefs to an address
python tools/re/disasm-range.py <start> <end>  # disassemble a VA range
# Reflection dump: C:\Development\Claude\scumdump\data\extracted\v23451409\classes.json
```

---

## 5. File map (where things live)

| Path | What |
|---|---|
| `apps/turdmod-engine-bridge/src/TurdMODEngineBridge.cpp` | the bridge (L3) — handlers, hooks, game-thread queues |
| `scripts/rcon_be.py` | **BattlEye RCON (BERcon) client — the working RCON path** (server-mgmt) |
| `apps/turdmod-service/src/rcon.rs` | Source RCON client — ⚠️ wrong protocol for direct SCUM (see §2.5) |
| `apps/turdmod-manager/` | Tauri admin GUI; `src/data/admin-commands.json` = verb list |
| `tools/engine-rpc-test.mjs` | dev CLI for the bridge IPC |
| `tools/re/` | binary-RE scripts (capstone disasm, xref, vtable dump) |
| `docs/journal/WORKSPACE.md` | running RE journal (despawn, auth gate, offsets) |
| `.secrets/credentials.md` | hosts/ports/keys (gitignored) — read at session start |
| `IDEAS.md` | forward-looking log; `CODEX.md` = project dictionary |

---

## 6. Gotchas

- **Build mirror**: edits go to `TurdMODEngineBridge.cpp`, but the build compiles
  the copy at `…/RE-UE4SS/.../dllmain.cpp`. Sync before every build or you ship a no-op.
- **Raw strings**: `R"(...)"` breaks if the body contains `)"` (e.g. `installed?)"`).
  Use a custom delimiter `R"x(...)x"` or escape.
- **Off the game thread**: actor destruction / admin-command pipelines must run on
  the UE game thread. The bridge queues them (single-slot atomic, drained by the
  ProcessEvent hook). Don't call them synchronously from the IPC thread.
- **Build-pinned RVAs**: every hardcoded address (gate `0x141e55403`, despawn fn,
  vtable `0x7d8`) is specific to SCUM build **23451409**. Re-derive after a patch.
- **pak-bypass detours** crash on join-time asset streaming; keep
  `C:\TurdMOD\pak_bypass.*` disabled during vehicle tests.
