# Dump Pipeline Runbook

**Purpose.** Operate the 3-phase SCUM reflection + SDK + pak extraction
pipeline from clean state to fully populated archive. Encodes the
non-obvious facts that cost us full sessions to discover. Read this
before launching anything related to `scumdump`, Dumper-7 injection,
or the Manager's Dump Management tab.

**Last verified end-to-end:** 2026-05-22.
**SCUM build at last verification:** `23128915`.

---

## 1. Architecture in 30 seconds

```
GameServer.exe ─┐
                ├── Phase A (live)   UE4SS bridge RPC → reflection JSON
SCUM.exe       ─┘                    14507 classes, 1717 enums, 3372 structs

GameServer.exe ─┐
                ├── Phase B (SDK)    Dumper-7 inject → C++ headers
SCUM.exe       ─┘                    server = 9006 files / 569 MB
                                     client = 4475 files / 85.6 MB

SCUM.pak       ──── Phase C (paks)   CUE4Parse + AESDumpster
                                     widgets/datatables/strings JSON
```

All three phases land under
`C:/Development/Claude/scumdump/data/extracted/v<build>/` with
sibling `sdk/`, `sdk-client/`, `widgets/`, `datatables/`, `strings/`,
plus a top-level `_meta.json` aggregating phase status.

The **forensic archive** at `scumdump/data/archive/keys.jsonl` is an
append-only JSONL log of binary hashes, AES keys, GObjects RVAs, and
file-count summaries — one line per discovery, dedup'd on
`(keyType, scumBuild, value)`. Treat it as the official history.

---

## 2. Repos and where things actually live

| concern | path |
|---|---|
| scumdump CLI + node tooling | `C:/Development/Claude/scumdump/` |
| scumdump entry point | `scumdump/src/cli.ts` (tsx-run) |
| scumdump output (gitignored) | `scumdump/data/extracted/v<build>/` |
| scumdump archive (committed) | `scumdump/data/archive/keys.jsonl` |
| Dumper-7 server fork | `scumdump/tools/Dumper-7/` (gitignored) |
| Dumper-7 client fork | `scumdump/tools/Dumper-7-Client/` (gitignored) |
| AES extractor | `scumdump/tools/aes_finder/` (gitignored) |
| CUE4Parse runner | `scumdump/tools/CUE4Parse/` + `scumdump-extractor/` (.NET) |
| Manager GUI integration | `turdmod/apps/turdmod-manager/src-tauri/src/dump_commands.rs` + `dump.rs` |
| Manager page | `turdmod/apps/turdmod-manager/src/pages/DumpManagementPage.tsx` |
| Inject helper (elevated) | `turdmod/apps/turdmod-loader/launcher/src/bin/turdmod-injector.rs` |
| UE4SS source root (for bridge builds) | `C:/Development/RE-UE4SS/` |
| UE4SS bridge cppmod wired-in copy | `C:/Development/RE-UE4SS/cppmods/TurdMODEngineBridge/src/dllmain.cpp` |
| Bridge build output | `C:/Development/RE-UE4SS/build/Game__Shipping__Win64/bin/TurdMODEngineBridge.dll` |
| Bridge deploy target | `C:/Program Files (x86)/Steam/steamapps/common/SCUM Server/SCUM/Binaries/Win64/UE4SS/Mods/TurdMODEngineBridge/dlls/main.dll` |
| SCUM client install | `C:/Program Files (x86)/Steam/steamapps/common/SCUM/SCUM/Binaries/Win64/SCUM.exe` |
| SCUM server install | `C:/Program Files (x86)/Steam/steamapps/common/SCUM Server/GameServer.exe` |

The Manager doesn't have its own copy of any tool — it shells out
to scumdump's CLI for every phase. Single source of truth.

---

## 3. Phase A — live reflection (UE4SS bridge)

**What it produces.** JSON files at `extracted/v<build>/`:
- `classes.json` (14507 entries for build 23128915)
- `enums.json` (1717)
- `structs.json` (3372)

**Prerequisites.**
- GameServer.exe running with UE4SS loaded
- `TurdMODEngineBridge.dll` injected and the named pipe alive
- Pipe path is at `%LOCALAPPDATA%/TurdMOD/engine/pipe.txt` —
  **never hardcode** the pipe name; it's `\\.\pipe\turdmod-engine-<pid>`.

**Run.**
```powershell
cd C:/Development/Claude/scumdump
pnpm phase-a
```
Or from the Manager GUI: Dump Management → Phase A row → Run.

**Gotcha — serial RPCs only.** Every bridge handler runs on the
game thread through a single named pipe. **Never** issue parallel
`dumpClasses` / `findFunctions` / `readClassValues` calls — they
will crash GameServer.exe. We verified this empirically on
2026-05-17 (10 concurrent → crash). See memory file
`feedback_bridge_rpc_one_at_a_time.md`.

---

## 4. Phase B — SDK headers (Dumper-7)

This is the largest source of pitfalls. Read the gotcha box even
if you think you remember.

### Phase B: server target (`GameServer.exe`)

```powershell
cd C:/Development/Claude/scumdump
pnpm phase-b
```

- Dispatches to `tools/Dumper-7/` (the **server** variant).
- That DLL hardcodes `ObjectArray::Init(0x712ED20, 0x10000)` —
  the server's FChunkedFixedUObjectArray RVA for build 23128915.
- Produces ~9006 files / 569 MB in `extracted/v<build>/sdk/`.

### Phase B: client target (`SCUM.exe`)

```powershell
cd C:/Development/Claude/scumdump
pnpm phase-b-client
```

- Dispatches to `tools/Dumper-7-Client/` (the **client** variant).
- That DLL reverts to `ObjectArray::Init()` (auto-detect sigscan).
  Empirically WORKS on SCUM.exe — no patternsleuth fallback needed.
- Produces ~4475 files / 85.6 MB in `extracted/v<build>/sdk-client/`.

### Phase B: gotchas

**1. Per-target DLL is mandatory.** The server DLL has a hardcoded
GObjects RVA. Injecting it into SCUM.exe dereferences a different
address space and faults immediately, producing 0 files. Never share
DLLs across targets. This is hard-baked into `phase-b-sdk.ts` and
`dump_commands.rs::dumper7_dll_path(target)`.

**2. Client target — wait 60-90s after launching SCUM.exe before
injecting.** This is the single biggest time-sink we've hit. With a
10-second warmup, Dumper-7's per-package walk faults silently mid-
execution because SCUM is still streaming UE packages and the
PackageManager hits half-initialized class structures. Symptom:
4 files / 11.7 MB partial dump (master `SDK.hpp` + GObjects-Dump
landed; `CppSDK/SDK/` empty; `PropertyFixup.hpp` is 0 bytes
because its stream destructor never ran). Diagnose by RAM:
SCUM plateaus at ~3.9 GB once main-menu packages are stable. The
Manager UI now shows an amber 60-90s warmup hint when target=client
on the inject button (added 2026-05-22, commit `1aa3ab9`).

**3. Stop BEService and use `-NoBattleye` for client inject.**
BattlEye blocks `VirtualAllocEx` against SCUM.exe (injector returns
exit code 4). Both steps required:
```powershell
Stop-Service BEService
Start-Process 'C:/.../SCUM.exe' -ArgumentList '-NoBattleye'
```
The server doesn't have BattlEye, so GameServer.exe injects with no
prep.

**4. The Manager's "Inject Dumper-7" button always elevates.**
Implemented via `turdmod-injector.exe` standalone bin under
`apps/turdmod-loader/launcher/src/bin/`, invoked via
`ShellExecuteExW(verb="runas")` from
`engine::elevate_launch`. UAC prompts on every click. This is
intentional — works for both elevated GameServer.exe and
non-elevated SCUM.exe uniformly. The in-process `inject.rs` is dead
code kept for reference.

**5. Dumper-7 writes to a fixed path: `C:/Dumper-7/`.** Not
configurable. scumdump polls there, detects stability (3 consecutive
unchanged byte-counts at 2s intervals = stable), then copies into
`extracted/v<build>/sdk(-client)/` and updates `_meta.json`. Hard
30-minute timeout.

**6. `hasSdkFiles` filter is bug-prone.** Dumper-7 emits `.hpp`
headers + `.txt` GObjects dumps — NOT `.h` or `.cpp`. The original
filter missed everything, hanging forever on a successful dump.
Fixed 2026-05-22 (commit `f739442` in scumdump). If you re-fork
Dumper-7 in future, verify the file extensions match.

**7. Dumper-7's AllocConsole window dies when the host process
dies.** If SCUM crashes mid-dump, you lose the diagnostic output.
For deep diagnostics, patch `main.cpp` to also tee `stderr` to a
file at `C:/Dumper-7/dumper7.log`.

**8. Building the Dumper-7 fork.** Open `.sln` in VS 2022, set
`x64 | Release`, Build. Output → `tools/<fork-name>/x64/Release/Dumper-7.dll`.
The server's GObjects RVA hardcode lives at
`Dumper/Generator/Private/Generators/Generator.cpp` line ~55. To
update for a new SCUM build, see
`feedback_dumper7_setup_gotchas.md` in memory for the +0x10 trick.

---

## 5. Phase C — pak content (CUE4Parse)

```powershell
cd C:/Development/Claude/scumdump
pnpm phase-c
```

- Requires AES-256 key. Extract via `pnpm extract-aes` (runs
  `AESDumpster` against `GameServer.exe`); key persists in
  `scumdump.config.json` (gitignored).
- Output: `extracted/v<build>/widgets/`, `datatables/`, `strings/`
  as JSON. ~496 widget classes for SCUM build 23128915.
- Re-run `extract-aes` after every SCUM update — the key
  rotates per build.

---

## 6. Forensic archive (`data/archive/keys.jsonl`)

Append-only JSONL log. **One line per discovery.** Schema:

```json
{
  "ts": "ISO8601",
  "scumBuild": "23128915",
  "scumServerSha256": "...",   // optional
  "scumExeSha256": "...",      // optional
  "keyType": "aes256_pak | sha256_scum_server | sha256_scum_exe | usmap_hash | dumper7_sdk_filecount | gobjects_rva | ...",
  "value": "...",
  "source": "AESDumpster | Dumper-7 | fileSha256 | ...",
  "target": "server | client | null",
  "notes": "..."
}
```

- **Dedup:** identical `(keyType, scumBuild, value)` is skipped by
  `appendKey()` — re-running phases is idempotent.
- **Commit it.** This is the only piece of `data/` that's tracked
  (`data/extracted/` is gitignored). The archive IS the project's
  long-term forensic record across SCUM updates.

---

## 7. Diff system

```powershell
cd C:/Development/Claude/scumdump
pnpm diff                 # diffs current build vs previous (by mtime)
```

Writes `extracted/v<currentBuild>/_diff.json` with `added` /
`removed` / `changedCount` arrays per phase (A classes/enums/structs,
B file lists, C widget/datatable/strings). The Manager's Dump
Management page reads `_diff.json` and renders a card with an
"Explain this diff" button (Ollama route — see below).

---

## 8. AI Assistant (Ollama route)

- Opt-in toggle. Off by default.
- Detects GPU via `nvidia-smi`. Supports 4+ GiB VRAM (model picker
  filters by your card). Tested on Joel's RTX 5060 Ti (Blackwell;
  needs `cu128` nightly wheel for PyTorch — see
  `reference_pytorch_gpu_install.md`).
- Ollama API: `/api/tags` (list installed), `/api/pull` (streams
  download via Tauri event `assist://progress`), `/api/generate`.
- Two predefined helpers: `assist_summarize_diff` and
  `assist_explain_phase_log`. Both stream tokens back through the
  Manager UI.
- Tauri command names are snake_case in `invoke()` — see
  `reference_tauri_command_naming` (this burned us once).

---

## 9. Full update workflow (after a SCUM patch)

```powershell
# 1. Update Steam (Manager has a boot-time banner that flags this)

# 2. Pull a fresh AES key — it rotates per build
cd C:/Development/Claude/scumdump
pnpm extract-aes

# 3. Phase A — live reflection (server-side; cheap, ~30s)
#    Requires GameServer.exe + bridge running.
pnpm phase-a

# 4. Phase B server — inject Dumper-7 into GameServer.exe
#    Either: Manager → Dump Management → Phase B server row → Run
#    Or:     pnpm phase-b   (then inject manually with the injector)
pnpm phase-b

# 5. Phase B client — needs 60-90s SCUM warmup, BE off, -NoBattleye
Stop-Service BEService
Start-Process 'C:/.../SCUM.exe' -ArgumentList '-NoBattleye'
# wait until RAM plateaus at ~3.9 GB (or watch 90 seconds on a timer)
pnpm phase-b-client
# inject via Manager button (elevated) OR:
& 'turdmod/apps/turdmod-loader/launcher/target/release/turdmod-injector.exe' `
    --target SCUM-Win64-Shipping.exe `
    --fallback SCUM.exe `
    --dll 'C:/Development/Claude/scumdump/tools/Dumper-7-Client/x64/Release/Dumper-7.dll'

# 6. Phase C — pak content
pnpm phase-c

# 7. Diff against previous build (auto if previous exists)
pnpm diff

# 8. Commit the archive update
cd C:/Development/Claude/scumdump
git add data/archive/keys.jsonl
git commit -m "data(archive): build <newBuild> dump baseline"
```

Or, from the Manager: Dump Management → Run All. Manager streams
each phase's stdout/stderr through `dump://log` Tauri event into the
log pane with timestamps, color-coded lines, and live status
heartbeats from `[STATUS|<groupId>]` lines.

---

## 10. Reading the SDK once it's dumped

**Server SDK (8990 per-package headers).** Contains all server-side
gameplay code — zombies, NPCs, AI behaviors, vehicle physics, world
simulation, server admin commands. Locate by class:
```powershell
ls C:/Development/Claude/scumdump/data/extracted/v23128915/sdk/4.27.2-115523+++scum+release-1.2.3-SCUM/CppSDK/SDK/*<ClassNameFragment>*
```

**Client SDK (4459 per-package headers).** Contains the UI / HUD /
render surface — `BP_MainMenuHUD`, `BP_WeaponScopeWidget`,
`CompassWidget`, `HUD_classes.hpp`, `InventorySlotWidget`, all 126
UMG/Slate/Niagara widgets, plus client-side gameplay (camera,
animation blueprints, sound). Same path with `sdk-client` in place
of `sdk`.

**Shared classes have different SHAs on each side.** `Engine_classes.hpp`
has the same byte count on both but different vtable layouts (server
+ client compiled from same source against different platform code).
Sanity check: `SlateCore_classes.hpp` SHA is IDENTICAL on both
(Slate is pure engine, no platform divergence).

**For class/enum/struct lookup, grep Phase A JSON FIRST** — it's
faster, smaller, and the reflection truth source. SDK headers are
for code generation and offset-level work. See
`reference_scum_reflection_dump.md` in memory.

---

## 11. This session's discoveries (2026-05-22)

Captured here so future sessions don't re-derive these.

**Modal "Failed to open descriptor file" was a red herring.** Spent
hours chasing a `lpCurrentDirectory` fix in the launcher. The modal
is actually triggered by the bridge's pak validator hook returning 0
unconditionally. Reverted the cwd fix (commit `4616bbd`); kept the
"always return 0" hook + Path B `ExitProcess` suppression in
`turdmod-server-loader/src/server_hooks.rs`. Modal is now cosmetic —
one click and the server boots.

**Per-target Dumper-7 fork.** Started 2026-05-22. Server fork's
hardcoded RVA is fatal when injected into client. Client fork at
`tools/Dumper-7-Client/` reverts to auto-detect, which works on
SCUM.exe out of the box. Dispatch wired into both `scumdump`
(commit `f739442`) and `turdmod-manager` (commit `c228d83`).

**The 90-second warmup rule.** Discovered 2026-05-22. Client target
Dumper-7 inject must come 60-90s after SCUM.exe launch, after RAM
plateaus at ~3.9 GB. Otherwise PackageManager faults on
half-initialized class structures. Verified empirically: 10s → 4
files; 90s → 4475 files. Manager UI hint added (commit `1aa3ab9`).

**`hasSdkFiles` filter bug.** scumdump's polling gate filtered for
`.h`/`.cpp` only; Dumper-7 emits `.hpp` + `GObjects-Dump*.txt`.
Every prior client run polled forever despite Dumper-7 having
written output minutes earlier. Fixed in commit `f739442`.

**Elevated injection via standalone exe.** Manager's Inject button
shells to `turdmod-injector.exe` via `ShellExecuteExW(verb=runas)`.
Works for both elevated GameServer.exe and non-elevated SCUM.exe.
In-process injector (`inject.rs`) is dead code kept for reference.

**Four-feature arsenal landed.** Phase B-Client, forensic archive,
diff system, AI Assistant. All committed and verified end-to-end
this session.

---

## 12. Commands cheat sheet

```powershell
# Dump pipeline (scumdump)
pnpm phase-a              # live reflection (server bridge required)
pnpm phase-b              # Dumper-7 inject → GameServer.exe
pnpm phase-b-client       # Dumper-7 inject → SCUM.exe (90s warmup!)
pnpm phase-c              # CUE4Parse pak extraction
pnpm extract-aes          # rerun on every SCUM update
pnpm diff                 # compare current vs previous build
pnpm all                  # phase-a + phase-b + phase-c + diff

# Manager (turdmod)
pnpm tauri dev            # local dev — HMR for TS/React, restart for Rust
                          # NEVER `pnpm tauri build` for testing (distribution installers)
pnpm typecheck            # tsc + cargo check
```

```powershell
# Inject (standalone, elevated — UAC prompts)
& 'C:/Development/Claude/turdmod/apps/turdmod-loader/launcher/target/release/turdmod-injector.exe' `
    --target GameServer.exe `
    --dll  'C:/Development/Claude/scumdump/tools/Dumper-7/x64/Release/Dumper-7.dll'

# Stop BattlEye + relaunch SCUM client without it
Stop-Service BEService
Start-Process 'C:/Program Files (x86)/Steam/steamapps/common/SCUM/SCUM/Binaries/Win64/SCUM.exe' `
    -ArgumentList '-NoBattleye'
```

---

## 13. When to update this runbook

- New phase added to scumdump
- New gotcha worth more than 10 minutes of someone's time
- Path moves or build commands change
- A new SCUM update changes the GObjects RVA / AES key approach
- Dumper-7 upstream is upgraded
