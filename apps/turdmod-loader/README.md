# turdmod-loader

In-game loader DLL for **TurdMOD** — the Strategy C path.

This is the foundation: a Rust DLL that gets injected into `SCUM.exe` plus a launcher that does the injection. The DLL gets a foothold inside the game process and (eventually) hosts the LuaJIT runtime + UE4 hooks that real mods plug into. Today it logs that it loaded, runs the BE / official-server detection, and refuses to do anything dangerous.

## Components

```
apps/turdmod-loader/
├── Cargo.toml          # the DLL crate (cdylib)
├── build.rs            # placeholder; reserved for the dxgi proxy path
├── src/
│   ├── lib.rs          # DllMain → init_thread → mode-gated startup
│   ├── detect.rs       # in-process BE / SCUM detection
│   ├── logging.rs      # append-only audit log at %LOCALAPPDATA%/TurdMOD/loader.log
│   └── proxy.rs        # placeholder
└── launcher/
    ├── Cargo.toml      # the launcher exe crate
    └── src/main.rs     # CreateProcessW + CreateRemoteThread injection
```

## Build

Both crates build independently — they're not in a workspace yet (the rest of the monorepo is JS/Python/.NET, the loader is the first Rust addition besides the overlay's Tauri shell).

```powershell
# DLL (release ~150 KB)
cd apps\turdmod-loader
cargo build --release

# Launcher
cd apps\turdmod-loader\launcher
cargo build --release
```

Outputs:

* `apps\turdmod-loader\target\release\turdmod_loader.dll`
* `apps\turdmod-loader\launcher\target\release\turdmod-launcher.exe`

## Run (smoke test against notepad)

You don't need SCUM to verify the injection chain — any 64-bit process works:

```powershell
.\apps\turdmod-loader\launcher\target\release\turdmod-launcher.exe `
  --scum "C:\Windows\System32\notepad.exe" `
  --dll  "C:\Development\Claude\turdmod\apps\turdmod-loader\target\release\turdmod_loader.dll" `
  --skip-safety-check
```

Then check the audit log:

```powershell
Get-Content "$env:LOCALAPPDATA\TurdMOD\loader.log" -Tail 5
```

You should see three lines: `DllMain attach`, `detected mode: Unknown`, `staying inert`. The `Unknown` is correct — notepad isn't SCUM, so the loader refuses to do any engine work.

## Run (against SCUM)

Don't pass `--skip-safety-check` for real targets. With BE binaries present in the SCUM install, the launcher will refuse:

```powershell
# Standard private-server (BE off) flow:
.\turdmod-launcher.exe --scum "<path-to-SCUM.exe>"
```

If your SCUM install has the `BattlEye/` directory present (every vanilla install does), you'll need to remove or rename it AS A HOST before launching with TurdMOD. **Never run the loader anywhere BattlEye is active.** This is the rule the detection layer enforces; `--skip-safety-check` is a developer escape hatch, not a "yolo" flag.

## What's not here yet (and where it's going)

Layer | Status | Filed as
---|---|---
**Layer 0**: Detection (BE present, SCUM running, mode inference) | done in `apps/turdmod-cli/src/detect.ts`; in-DLL mirror lives in `src/detect.rs` | KTask `#170`
**Layer 1**: Loader DLL + launcher (this) | **done — minimum viable injection** | KTask `#167`
**Layer 2**: LuaJIT runtime + sandbox | not started | KTask `#168`
**Layer 3**: UE4 hooks (UFunction → Lua, signature scans) | not started | KTask `#169`

Once Layer 2 lands, mods written against `@turdmod/turdmod-api` will run via Lua bindings in the game process and the welcome panel will actually appear inside SCUM (not just over Discord / the companion's stdout). Today, server-side mods already work via `apps/turdmod-companion`; this loader is what unblocks the in-game UI and gameplay-mutating mods.

## Audit log

`%LOCALAPPDATA%\TurdMOD\loader.log` is append-only NDJSON-ish. Both the CLI's pre-launch detection (`apps/turdmod-cli`) and the in-process loader write to it, so a single timeline captures `user launched → BE check passed → DLL injected → mode resolved → engine work began (or was refused)`.

The file rotates at 5 MB; the CLI doctor command (`turdmod doctor`) tails it.

## Refusing to run

These are the conditions under which the loader stays inert (proxy forwarders only, no Lua, no hooks, no UI):

1. Process isn't `SCUM.exe` (defense-in-depth — the proxy can be loaded into anything that imports the DLL we shadow).
2. A `BattlEye*` or `BEService*` module is loaded in the current process.
3. The launcher's pre-flight detected BE binaries in the SCUM install AND the user didn't pass `--skip-safety-check`.

Any of these → log a refusal line, set `INIT_DONE = true`, and exit `init_thread`. The DLL stays mapped (we can't unload from inside DllMain) but does nothing useful.
