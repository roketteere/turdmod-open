# TurdMOD — architecture map

A plain-English tour of the entire TurdMOD modding stack: what each
piece does, where its source lives, and how it talks to the others. If
you ever lose the thread of "wait, which thing is this and why does it
exist?", come here first.

This is intentionally separate from every component-level README — those
explain *how to use* one piece. This file explains *how the whole thing
fits together*.

---

## The 30-second elevator pitch

TurdMOD is a **mod engine for SCUM** that runs in two parallel halves:

1. **Server-side mods** — JavaScript/TypeScript "scripts" that watch the
   dedicated server's log files, react to in-game events, and push
   messages back into the game (or out to Discord). No client install
   required, runs on private + modded servers, **does not touch the
   game client at all**.

2. **Client-side mods** — native Windows DLLs injected into the running
   `SCUM.exe` game process by our launcher. These can change how the
   game renders things — the headline use case is the **rich-text
   decorator DLL**, which upgrades SCUM's chat / message panels so they
   can render inline images, clickable hyperlinks, and dismiss-key
   prompts driven by markup the server emits.

Players who only want server features run nothing locally. Players who
want client features run our launcher instead of the SCUM shortcut.

```
                ┌─────────────────────────────────────┐
                │        SCUM dedicated server        │
                │  (gameplay + writes log files)      │
                └────────────────┬────────────────────┘
                                 │ tails .log files
                                 ▼
        ┌────────────────────────────────────────────────┐
        │  apps/turdmod-companion/  (Node.js runtime)    │
        │  • discovers + loads server-side mods          │
        │  • dispatches log events to mod hooks          │
        │  • each mod posts to Discord, persists state,  │
        │    reacts however its main.ts says            │
        └────────────────┬───────────────────────────────┘
                         │
                         ▼
        ┌────────────────────────────────────────────────┐
        │  examples/turdmod/<mod>/  (one folder per mod) │
        │  welcome-screen, kill-feed, vehicle-manager,   │
        │  my-squad, cosmetic-icon-pack                  │
        └────────────────────────────────────────────────┘


  ┌────────────────────────────────────────────────────────────┐
  │                Client-side path (per-player)               │
  │                                                            │
  │  apps/turdmod-loader/launcher/  (Rust .exe — turdmod-      │
  │      launcher.exe)                                         │
  │     • spawns SCUM.exe SUSPENDED                            │
  │     • injects 1..N DLLs (CreateRemoteThread + LoadLibrary) │
  │     • resumes the main thread                              │
  │                                                            │
  │            │                            │                  │
  │            ▼                            ▼                  │
  │  apps/turdmod-loader/src/      apps/turdmod-loader/        │
  │  (the kitchen-sink loader      decorators/src/             │
  │   DLL: turdmod_loader.dll)     (the rich-text decorator    │
  │                                 DLL: turdmod_rich_         │
  │                                 decorators.dll)            │
  └────────────────────────────────────────────────────────────┘
```

---

## Top-level repo map

Everything TurdMOD-related lives under a small number of paths inside
the `scummap` monorepo:

| Path | What it is |
|---|---|
| `apps/turdmod-loader/`            | Rust workspace for the **client-side stack** — launcher .exe, kitchen-sink loader DLL, and rich-text decorator DLL. |
| `apps/turdmod-companion/`         | Node.js runtime that **runs server-side mods** by tailing dedicated-server logs. |
| `examples/turdmod/`               | Each subfolder is **one server-side mod** that already runs end-to-end (the "verified" set). |
| `examples/turdmod-design-stage/`  | Design-stage mods that target the loader DLL's Lua runtime — kept here separate so people don't think they run today. |
| `docs/turdmod/`                   | TurdMOD-specific design docs (this file, manifest spec, manager UI spec, Gamepires CUI spec, etc.). |
| `docs/scum-internals/`            | The **SCUM reverse-engineering dossier** — game-side facts (engine version, log layouts, memory signatures, class hierarchies, decorator internals). The decorator DLL pulls layout offsets and sigscan patterns straight out of these files; every "candidate" comment in the Rust source cites a dossier section. |

---

## Server-side stack (Node.js)

This is the half that needs **zero client install**. It runs next to a
dedicated SCUM server, reads what the game writes to disk, and pushes
events forward.

### `apps/turdmod-companion/` — the runtime

The runtime that loads and dispatches mods. Folders/files:

| File | Role |
|---|---|
| `src/index.ts`   | Process entry. Discovers mods in `MODS_DIR`, validates each one's `turdmod.json`, calls its `main.ts`, then keeps the dispatcher alive. **Silently skips** mods whose `mode` isn't `"server-side"` so no log noise from design-stage mods. |
| `src/runtime.ts` | Per-mod runtime context — the `ctx` object every mod's `main.ts` receives. Wraps persistence (key/value store on disk), Discord webhook fan-out, and event-bus subscriptions. |
| `src/api.ts`     | The shape of the `ctx` argument: `ctx.on(event, handler)`, `ctx.send(...)`, `ctx.persist(...)`. Mods *only* see this surface. |
| `src/ipc.ts`     | Optional sidechannel — when enabled, lets external processes (a Discord bot worker, a web admin panel) inject synthetic events into the bus for testing. |
| `src/proxy.ts`   | Companion-side helper for forwarding events between processes (multi-server / multi-companion deployments). |
| `src/hooks.ts`   | Built-in event taxonomy — `playerLogin`, `playerLogout`, `kill`, `vehicleDestroyed`, `squadMemberOnline`, … Every event the log-tailer emits is normalized through this file before fanout. |
| `src/detect.ts`  | Log-format probes. SCUM has changed log shapes across patches; `detect.ts` sniffs the first lines of each file and picks the right parser. |
| `src/sigscan.ts` | Companion-internal pattern matching for log-line shapes (NOT the same as the Rust `sigscan.rs` — that one matches bytes in `SCUM.exe`). |
| `src/logging.ts` | Companion's own log file (`%LOCALAPPDATA%/TurdMOD/companion.log`). Also writes a structured stream so the future admin web UI can watch live. |
| `src/verify.ts`  | The `verify` CLI subcommand — drives synthetic events through any one mod against a fixture file, asserts on broadcasts/persistence-keys touched. Each mod ships its own `<mod>.verify.json` describing the expected shape. |

### `examples/turdmod/<mod>/` — the mods themselves

Each subfolder is a self-contained mod. The shared shape:

```
examples/turdmod/<modname>/
├── turdmod.json          ← manifest (name, version, mode, tags, …)
├── PAYLOAD-SCHEMA.md     ← Zod-validated config + payload shape (human readable)
├── README.md             ← what this mod does, how to configure it
├── EVIDENCE.md           ← live-test screenshots / payload captures
├── <mod>.verify.json     ← fixtures + expectations for `companion verify`
├── test-fixtures/        ← (optional) seed persistence state for verify
└── scripts/main.ts       ← the actual hook code; `export default (ctx) => { … }`
```

The "runs today" set:

| Mod | What it does |
|---|---|
| `welcome-screen`   | Posts a configurable welcome embed to Discord on first login per player; tracks "seen" keys per player. |
| `kill-feed`        | Per-class templates (PvP / PvE / fall / suicide) push every kill to a Discord webhook with weapon, distance, headshot. |
| `vehicle-manager`  | Tails `vehicle_destruction.log`; broadcasts destroyed / disappeared / failed-to-spawn events with optional anonymization. |
| `my-squad`         | Admin pre-configures squad mappings; on login/logout, broadcasts which squadmate came/went. |
| `cosmetic-icon-pack` | Pure-asset mod — emoji/icon palette consumed by the other mods' templates. |

Design-stage mods (under `examples/turdmod-design-stage/`) target the
loader DLL's Lua runtime — they're *intentionally* separated because
the loader's Lua side isn't yet end-to-end verified. Don't mix them in.

---

## Client-side stack (Rust)

Three Rust crates living in one workspace.
**`apps/turdmod-loader/Cargo.toml`** is the workspace root.

```
apps/turdmod-loader/
├── Cargo.toml          ← workspace manifest
├── src/                ← crate #1 — the kitchen-sink loader DLL
├── launcher/           ← crate #2 — the launcher .exe
└── decorators/         ← crate #3 — the rich-text decorator DLL
```

### Crate #2 — `launcher/` (the .exe the player runs)

Builds to `turdmod-launcher.exe`. Single source file at
`apps/turdmod-loader/launcher/src/main.rs`.

Job:
1. Find SCUM.exe (CLI flag, env var, or Steam common-folder probe).
2. `CreateProcessW` with `CREATE_SUSPENDED` so the game starts but
   doesn't run yet.
3. Inject the **primary loader DLL** (`--dll <path>`) using
   `CreateRemoteThread` + `LoadLibraryW`. If injection fails,
   `TerminateProcess` the suspended game — never resume a broken state.
4. Inject any **extra DLLs** passed via `--extra-dll <path>` (repeatable).
   Failures here are warnings, not fatal — the primary loader runs and
   each extra logs its own attach state to its own sink.
5. `ResumeThread`. The game runs from byte zero with our DLL(s) already
   loaded inside it.

The `--extra-dll` flag is what lets one launcher invocation pull in BOTH
the kitchen-sink loader AND the rich-text decorator DLL in a single
shot — they're separate cdylibs by design (different blast radius,
different update cadence) but ship together.

### Crate #1 — `src/` (the kitchen-sink loader DLL)

Builds to `turdmod_loader.dll`. This is the "do everything" DLL: hooks
DXGI for an ImGui overlay, hosts a Lua runtime for design-stage scripts,
sigscan-resolves engine globals (`GEngine`, `GWorld`, `FUObjectArray`,
`FNamePool`).

| File | Role |
|---|---|
| `lib.rs`     | DllMain. Spawns init thread (loader-lock-safe), banners, version exports for the launcher's smoke test. |
| `hooks.rs`   | The big one — `resolve_engine` runs the byte-pattern sigscans, stashes the resulting absolute pointers in `EnginePointers`. Future: DXGI present hook, ImGui draw, Lua marshalling. |
| `sigscan.rs` | Reusable byte-pattern engine: `Pattern` parser (`"48 8B 05 ?? ?? ?? ?? …"`), `find()` walker, `resolve_rip()` for RIP-relative-to-absolute conversion. **Shared with the decorator DLL** via a `#[path]` attribute (no rlib refactor needed). If the share grows, extract to a `packages/scum-sigscan` crate. |
| `detect.rs`  | Local equivalent of the companion's detect — game build / process state probes that gate which sigscan patterns to use. |
| `proxy.rs`   | Loader-side IPC for the companion / web tooling to talk to a running game's overlay. |
| `runtime.rs` | Lua runtime host — script lifecycle, sandbox boundary, marshalling helpers. (Design-stage mods like `world-snapshot` target this.) |
| `api.rs`     | Lua-visible API surface — what scripts can call. |
| `ipc.rs`     | The socket / pipe end of `proxy.rs`. |
| `logging.rs` | `%LOCALAPPDATA%/TurdMOD/loader.log`. |

Currently the kitchen-sink loader is **scaffolded but not the headline
deliverable** — its big-ticket items (DXGI hook, full Lua runtime) sit
behind the decorator DLL, which is a smaller, more focused piece that
ships first.

### Crate #3 — `decorators/` (the rich-text decorator DLL)

Builds to `turdmod_rich_decorators.dll`. **This is the active focus.**
Job: detour SCUM's engine-stock `URichTextBlock*Decorator` classes so
existing chat / panel widgets can render new markup tags emitted by
server-side mods. It does *not* mod gameplay or touch combat — purely a
rendering upgrade.

Every module is its own concern; the file list IS the architecture:

| File | What it does |
|---|---|
| `lib.rs`        | DllMain + init thread. Same boot dance as the kitchen-sink loader (`DisableThreadLibraryCalls`, detached worker, never `LoadLibrary`/heap-touch on a non-calling thread inside `DllMain`). Registers every other module via `mod` declarations. Exports `turdmod_decorators_is_ready` / `turdmod_decorators_version` for the launcher's smoke test. |
| `logging.rs`    | Append-only `%LOCALAPPDATA%/TurdMOD/decorators.log`. Crash-safe — every write is one syscall, no buffering. |
| `whitelist.rs`  | Image-host allowlist at `%USERPROFILE%/.scummy-map/turdmod-image-hosts.txt`. Auto-seeds with `cdn.scummap.com` / `placehold.co` / `i.imgur.com` on first absent-file load. No wildcard subdomains — explicit list only. |
| `image_fetch.rs`| `fetch_and_decode(url, whitelist, limits)`. URL parse → host whitelist check → `ureq` GET (3 s timeout) → 512 KiB body cap → `image` crate decode → RGBA→BGRA in place → 4 MiB decoded cap. Returns a `Bgra8Buffer` ready to hand to the engine-side texture path. |
| `dispatch.rs`   | Pure-logic markup-attribute dispatch. Tag name + `attrs` HashMap + content → `Action` enum (`FetchImage`, `LookupImage`, `HoistBanner`, `Hyperlink`, `Dismiss`, `HoistTitle`, `SectionStart`, `Unknown`). URL validation rejects non-`https://`. Label/alt capped at 200 chars (Unicode-safe). NO unsafe, NO new deps. |
| `sigscan.rs`    | **Shared with the kitchen-sink loader** via `#[path = "../../src/sigscan.rs"]`. Both crates have compatible `logging` modules so `crate::logging::log` resolves either way. |
| `sigscan_ext.rs`| Extends the shared sigscan: PE-section walker (`module_sections`), `scan_wide_string` (UTF-16 LE NUL-NUL needle), `scan_lea_to` (find `lea rcx/rdx/r8/r9, [rip+disp]` whose RIP-relative target equals a given address), `find_guarray` (canonical UE 4.27 `FUObjectArray` byte pattern + string-anchored fallback per dossier 16). |
| `fname.rs`      | UE 4.27 `FName` (8-byte struct, `comparison_index` + `number`) + `FNamePool` walker. `find_string("RichTextBlockImageDecorator", 0) → Option<FName>` is the lookup the decorator needs. Decodes `FNameEntryHandle = (Block:13, OffsetInStrides:16)`, walks chunked 64 KiB blocks, handles ASCII + UTF-16 entry bodies. Status: **candidate, requires runtime probe** — the `FNAMEPOOL_BLOCKS_OFFSET` is the patch-fragile constant. |
| `uobject.rs`    | UE 4.27 `UObject` / `UStruct` / `UClass` binary-layout helpers. Opaque pointer newtypes (`UObjectPtr`, `UClassPtr`, `UStructPtr`), field readers (`name_private`, `class_of`, `outer_of`, `super_struct`, `vtable`), and the `CdoProbe` for runtime-resolving `UClass::ClassDefaultObject` (Dumper-7 `OffsetFinder.cpp:1057` heuristic). Every offset cites a dossier section (`§Q2 UObjectBase:238`, `§Q2 Class.h:1356`, …). |
| `guarray.rs`    | `GUObjectArray` walker — wraps the resolved `FUObjectArray*` and exposes `walk()`, `find_by_fname(target)`, `find_class_by_fname(target, filter)`. Hard cap at 4 M objects so a corrupt sigscan match can't hang the walker. SCUM's actual count is ~700k–1.2M. |
| `vtable.rs`     | The vtable-diff probe. `diff_first_override(parent_cdo, child_cdo) → DiffOutcome`. Per dossier 16, the first slot where child and parent vtables differ is the index of the first virtual the child overrides — and for `RichTextBlockDecorator` → `*Image` / `*ActionPrompt`, that's `CreateDecorator`. Capped at 256 slots. |
| `engine_resolve.rs` | The Phase-B step-1 composition layer. Drives the foundation modules in dependency order — sigscan FNamePool + GUObjectArray, walk for each decorator class FName, probe the CDO offset using four well-known `(class, Default__class)` seed pairs, fetch each decorator class's CDO, run vtable-diff on each child vs the parent. Returns a `ProbeReport` whose `summary()` is the multi-line block `hooks::install` writes to `decorators.log`. The strongest pre-detour gate is `child_vtable_diffs_agree() → Some(idx)` — both children should override `CreateDecorator` and therefore diverge at the same slot. |
| `hooks.rs`      | The detour install/uninstall layer. Wraps `retour` 0.3 (statically-linked MinHook) in a `HookRegistry` so live detours are tracked for `DLL_PROCESS_DETACH` cleanup. `install()` currently runs `engine_resolve::probe()`, logs the report, and branches: install if all checks agree, refuse otherwise. **No actual detour is installed yet** — the body is log-only by design until one live SCUM run validates every layout assumption. The smoke-hook self-test that validates the `retour` install/trampoline/lift round-trip without touching any UE4 internals still ships. |

### Running the Phase-B step-1 live test

One-shot runner that builds all three crates, launches SCUM with both
DLLs injected, and tails the probe report:

```powershell
.\scripts\turdmod-decorator-probe.ps1
```

Useful flags:

```powershell
# Reuse existing build artifacts (skip cargo build)
.\scripts\turdmod-decorator-probe.ps1 -SkipBuild

# Watch the log without launching SCUM (use this when SCUM is already
# running, e.g. started from Steam):
.\scripts\turdmod-decorator-probe.ps1 -OnlyTail

# Override SCUM.exe location:
.\scripts\turdmod-decorator-probe.ps1 -ScumExe "D:\SteamLibrary\…\SCUM.exe"
```

Or the manual equivalent:

```powershell
cargo build --release --manifest-path apps/turdmod-loader/Cargo.toml
cargo build --release --manifest-path apps/turdmod-loader/launcher/Cargo.toml
cargo build --release --manifest-path apps/turdmod-loader/decorators/Cargo.toml

$loader = "apps/turdmod-loader/target/release/turdmod_loader.dll"
$rich   = "apps/turdmod-loader/decorators/target/release/turdmod_rich_decorators.dll"

apps/turdmod-loader/launcher/target/release/turdmod-launcher.exe `
    --dll       $loader `
    --extra-dll $rich

Get-Content -Wait "$env:LOCALAPPDATA\TurdMOD\decorators.log"
```

The probe writes one block per attach. A **fully successful** run looks
roughly like:

```
[engine-resolve] probe report:
  module          : base=0x7ff7… size=0x6c00000
  FNamePool       : 0x7ff7…
  GUObjectArray   : 0x7ff7… (NumElements=981234)
  CDO probe       : seeds_resolved=4/4 offset=0x108
  decorator classes:
    RichTextBlockDecorator                   uclass=0x… cdo=0x… diff=(parent)
    RichTextBlockImageDecorator              uclass=0x… cdo=0x… diff=override@27
    RichTextBlockActionPromptDecorator       uclass=0x… cdo=0x… diff=override@27
  consensus       : children agree on CreateDecorator@vtable[27]
[hooks] all checks pass — children agree on CreateDecorator@vtable[27]; …
```

A miss looks like one of these (each has a one-line note on the same
file pinpointing what failed):

- `FNamePool : NOT FOUND (sigscan miss)` — the canonical pattern in
  `docs/scum-internals/16-memory-signatures.md` §FNamePool needs
  re-derivation against the current SCUM build.
- `GUObjectArray : NOT FOUND (sigscan miss)` — same, §FUObjectArray.
- `CDO probe : seeds_resolved=0/4` — the FName lookup found classes but
  no `Default__*` matches; means the `Default__` naming convention
  drifted (very unlikely on stock UE 4.27) or the GUObjectArray walk is
  reading garbage.
- One class shows `MISS — FName not in FNamePool` — that exact class
  name isn't in the binary; double-check capitalization or whether
  SCUM's engine fork renamed it.

### How a single SCUM startup fans out across the client stack

```
player runs:
  turdmod-launcher.exe \
    --dll      target\release\turdmod_loader.dll \
    --extra-dll target\release\turdmod_rich_decorators.dll \
    -- -nosplash

   1. launcher CreateProcessW(SCUM.exe, CREATE_SUSPENDED)
      ↓
   2. inject_dll(turdmod_loader.dll) — kitchen-sink DllMain runs, spawns
      its init thread, sigscans engine globals, sets up logging
      ↓
   3. inject_dll(turdmod_rich_decorators.dll) — decorator DllMain runs,
      spawns its init thread, runs hooks::install() → sigscan_ext +
      FNamePool + GUObjectArray + vtable-diff → installs retour detour
      ↓
   4. ResumeThread — SCUM's main thread runs from byte zero with both
      DLLs already loaded; the engine's first decorator instantiation
      hits our detour
```

---

## How a server-pushed image becomes pixels in-game

End-to-end, top to bottom:

```
1. Admin authors examples/turdmod/welcome-screen/scripts/main.ts
     → registers ctx.on('playerLogin', …)
     → emits a markup string like "<img src='https://cdn.scummap.com/welcome.png'/>"
     → companion's broadcast surface ships that string into the SCUM server
       (RCON / chat-broadcast — the exact carrier depends on phase wiring)

2. Player joins; SCUM.exe receives the broadcast, instantiates a
   URichTextBlock to render it, calls its decorator's CreateDecorator()
   for the <img> tag.

3. Our decorator DLL has detoured CreateDecorator on
   URichTextBlockImageDecorator. Detour body:
     a. Read FTextRunInfo.MetaData["src"] — the URL
     b. Call dispatch::dispatch("img", attrs, content) → Action::FetchImage{ url, alt }
     c. whitelist::is_allowed(host) — reject if not
     d. Off the game thread: image_fetch::fetch_and_decode(url) → Bgra8Buffer
     e. Marshal back to the game thread: FImageUtils::ImportBufferAsTexture2D
        → UDataTable::AddRow → keep the UTexture2D alive via TStrongObjectPtr
     f. Return a Slate widget rendering that texture

4. Player sees the image inline in the panel.
```

The same chain handles `<a href>`, `<dismiss key>`, `<title>`,
`<banner>`, `<section>` — only the `Action` variant changes.

---

## "Where do I look when…?"

| If you want to… | Look at |
|---|---|
| Add a new server-side mod                  | Copy a folder under `examples/turdmod/`, edit `turdmod.json` + `scripts/main.ts`, document in `PAYLOAD-SCHEMA.md`. |
| Change which events the runtime exposes    | `apps/turdmod-companion/src/hooks.ts` (taxonomy) + `runtime.ts` (dispatch). |
| Change how a log line gets parsed          | `apps/turdmod-companion/src/detect.ts` + `sigscan.ts`. |
| Add a new markup tag the client renders    | `apps/turdmod-loader/decorators/src/dispatch.rs` (add `Action` variant + tag-name match) + the corresponding `hooks.rs` detour-body branch. |
| Update an image whitelist for a deployment | The user-side file at `%USERPROFILE%/.scummy-map/turdmod-image-hosts.txt`. The defaults live in `decorators/src/whitelist.rs`. |
| Rebase sigscan patterns after a SCUM patch | `docs/scum-internals/16-memory-signatures.md` (canonical patterns + maintenance notes) → mirror changes into `apps/turdmod-loader/src/sigscan.rs` callers and the `decorators/src/sigscan_ext.rs::find_guarray` fallback. |
| Add a new Phase-B foundation module        | New file under `apps/turdmod-loader/decorators/src/`, then `mod <name>;` in `lib.rs`. Tests inline (`#[cfg(test)]`) — no live SCUM needed; the live-binary tests are `#[ignore]`d. |
| Read the latest game-side investigation    | `docs/scum-internals/18-investigation-log.md`. |

---

## Convention reminders (so the diffs stay legible)

- **Status labels** — every module that depends on layout offsets we
  haven't runtime-verified yet says `Status: candidate, requires runtime
  probe against SCUM <build>` at the top, the same way
  `docs/scum-internals/16-memory-signatures.md` marks unverified
  patterns. When a probe lands and works, promote to "verified".

- **Dossier citations** — every magic offset in the Rust source cites
  the dossier section it came from. If you change an offset, update
  both sides — leaving the cite stale will burn a future debugging
  session.

- **Banner art** — every TurdMOD process logs the same shared "TurdMOD
  is running" banner (`BANNER` const, kept in sync between
  `apps/turdmod-loader/src/lib.rs` and `apps/turdmod-loader/decorators/src/lib.rs`).
  When you add a new process or DLL, log the same banner so its
  attach moment is greppable in any of the three log files.

- **Logging sinks** — three separate files under
  `%LOCALAPPDATA%/TurdMOD/`: `loader.log`, `decorators.log`,
  `companion.log`. Never share — failure modes are easier to diagnose
  per-component.

---

## What's not here yet

The TurdMOD layout is still growing. Expected additions in upcoming
sessions, with their planned home in this layout:

- `apps/turdmod-manager/` — DayZ-style mod browser / installer that
  lets players auto-install client + server-side mods for a server
  they're about to join (Tauri-based UI). See `IDEAS.md`.
- `apps/turdmod-loader/decorators/src/hooks.rs::install` body — the
  actual detour wiring on the resolved vtable index.
- A `packages/scum-sigscan` crate — only if the sigscan share between
  the loader and the decorators starts producing maintenance friction.

When those land, this file should be updated alongside them. If you
ever can't find a thing you expect to be here, this doc is the
authoritative place to add a pointer.
