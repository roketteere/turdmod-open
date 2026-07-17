# `@turdmod/turdmod-manager`

Tauri 2 desktop app — the primary TurdMOD GUI. One app adapts to both
tiers (Lite for managed hosts via FTP+RCON; Engine/Pro for own-host
VPS via UE4SS+bridge). Owns engine lifecycle (install / start / stop),
the Schema Browser, server connector (SFTP/FTP/RCON), mod
browse/install, and the Ollama bridge dispatcher.

See [`CLAUDE.md`](./CLAUDE.md) for deeper architecture notes and the
non-derivable Tauri-command-naming gotcha.

## Run locally

From this directory:

```powershell
pnpm tauri dev
```

That's enough for everyday work. HMR covers TS/React; Rust changes
need a restart. The bridge DLL is built separately (see
[`apps/turdmod-engine-bridge/README.md`](../turdmod-engine-bridge/README.md))
and Manager just invokes it via the in-process named-pipe RPC.

**Don't** run `pnpm tauri build` for testing — that produces
distribution installers and takes much longer.

## Other scripts

```powershell
pnpm dev          # Vite dev server only (no Tauri window — rarely useful by itself)
pnpm build        # production front-end build (Vite output to dist/)
pnpm preview      # serve the prod build
pnpm tauri:build  # full installer build (NSIS + MSI) — distribution only
pnpm typecheck    # tsc --noEmit
```

## Preconditions

- **pnpm** installed (workspace uses pnpm).
- **Rust toolchain** for the Tauri backend. Stable, MSVC target on Windows.
- **Steam SCUM Server** install on the local box for Engine-tier work.
  Manager auto-detects via the Engine page; you can also override the
  install path manually.
- **WebView2 runtime** (ships with current Windows; rarely needs install).
- **Engine-tier extras** (Pro / Engine page only): UE4SS + bridge DLLs
  built per `apps/turdmod-engine-bridge/README.md`, and a built
  `turdmod-launcher.exe` at
  `apps/turdmod-loader/launcher/target/release/turdmod-launcher.exe`
  (Manager's Engine Start invokes it in place from that path).

## Dump Management page workflow

The Dump Management page (`/dump-management`, Builder nav group)
drives the sibling
[`scumdump`](https://github.com/roketteere/scumdump) extraction
pipeline from the GUI. It surfaces SCUM's current Steam build, the
latest on-disk extracted dump, per-phase counts, and provides Run
buttons for each of the three phases plus a "Run All" composite.

**Typical flow:**

1. Open the page. The status pane shows current Steam build vs latest
   extracted dump (`v<buildid>`). If they differ, the top-of-app
   banner already prompted you on launch.
2. Click **Run Phase A** to refresh live UE4SS reflection (classes /
   enums / structs). Requires the engine running — phase A calls
   bridge RPCs `dumpAllClasses` / `dumpAllEnums` / `dumpAllStructs`.
3. Click **Run Phase B** to regenerate Dumper-7 SDK headers. Requires
   the engine running.
4. Click **Run Phase C** to re-extract pak content (widgets /
   datatables / strings) via CUE4Parse. Uses the AES key from
   `scumdump/scumdump.config.json`; does **not** require the engine.
5. Click **Run All Phases** to do the full sweep in sequence; stops
   on first failure.
6. Click **Re-extract AES key** when SCUM updates and the pak key
   rotates (runs `pnpm detect` in the sibling repo).
7. Click **Open Dump Folder** to browse the extracted JSON / SDK /
   pak content under `scumdump/data/extracted/v<buildid>/`.

**Outputs** land under
`C:/Development/Claude/scumdump/data/extracted/v<buildid>/`:

- `classes.json` / `enums.json` / `structs.json` — Phase A
- `sdk/` — Phase B
- `widgets/`, `datatables/`, `strings/` — Phase C
- `_meta.json` — per-phase metadata + counts (the file the page reads)

**Boot-time check:** on Manager startup the page's helper command
fires once. If Steam updated SCUM since the last extraction, a yellow
banner appears across the top of the window linking here.

## Engine page workflow

1. Detect / pick the SCUM Server install (Manager auto-detects; override if needed).
2. **Install Engine** — copies `UE4SS.dll` + `TurdMODEngineBridge.dll`
   into the canonical `<install>\SCUM\Binaries\Win64\UE4SS\…` layout,
   writes `mods.txt` + `UE4SS-settings.ini` if missing. Idempotent.
3. **Start Engine** — UAC prompts, then the elevated launcher injects
   UE4SS + the loader DLL into a suspended SCUMServer.exe and resumes
   it. Manager tails `UE4SS.log`, `server-loader.log`, and `SCUM.log`
   into the in-app log pane.
4. **Stop Engine** — terminates the server cleanly.

## Where things live

| Module | Purpose |
|---|---|
| `src-tauri/src/engine.rs` + `engine_commands.rs` | Engine install/start/stop + log tailers |
| `src-tauri/src/engine_rpc.rs` | Generic JSON-RPC pass-through to the bridge's named pipe; **all bridge handlers reach React through this single `engine_rpc` command** |
| `src-tauri/src/commands.rs` | Mod install/list/detect + log-tail + open-in-default-app helper |
| `src-tauri/src/server.rs` + `server_commands.rs` | SCUM-server connector (SFTP/FTP/RCON) for Lite tier |
| `src-tauri/src/scum_paths.rs` | Steam install discovery |
| `src-tauri/src/companion.rs` | Companion-process orchestration |
| `src-tauri/src/dump.rs` + `dump_commands.rs` | Sibling scumdump pipeline — Steam manifest parsing, `_meta.json` reads, phase orchestration via `pnpm phase-a/b/c`; streams output over `dump://log` |
| `src/pages/DumpManagementPage.tsx` + `components/DumpUpdateBanner.tsx` | GUI for the dump pipeline + boot-time SCUM-updated banner |
| `src/lib/adapter.ts` | Tier adapter (Lite vs Engine vs Pro route selection) |

## Tauri command naming gotcha

`#[tauri::command(rename_all = "camelCase")]` renames **parameters**,
NOT the function name. From JS, `invoke()` uses the literal Rust
function name verbatim — which means **snake_case**:

```ts
invoke('ollama_pool_health')   // ✅
invoke('ollamaPoolHealth')     // ❌ silently fails
```

## License

MIT.
