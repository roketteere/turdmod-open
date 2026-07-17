# turdmod-server-loader

Server-side DLL for TurdMOD. Injects into `GameServer.exe`, hosts a
Lua 5.4 runtime, and (once Sprint 1 is complete) exposes:

- **Admin RPC** (`admin_api.rs` / Part B — shipped) — named-pipe JSON-RPC
  the companion calls into. Methods: broadcastChat, sendPrivateMessage,
  teleportPlayer, getOnlinePlayers, getPlayerPosition, spawnVehicle,
  executeAdminCommand, ping. Events: chat, playerLogin, playerLogout,
  playerDeath. Pipe name written to `%LOCALAPPDATA%\TurdMOD\engine\pipe.txt`
  for companion discovery.
- **Server hooks** (`server_hooks.rs` / Part C — stub) — UE4 function
  hooks that emit player-login / chat / kill events to the companion and
  implement the `EngineApi` trait that backs the RPC methods.

## Build

```powershell
cd apps/turdmod-server-loader
cargo build --release
# Output: target/release/turdmod_server_loader.dll
```

## Inject (development)

Use the existing `turdmod-launcher` binary — it is process-agnostic:

```powershell
turdmod-launcher.exe `
  --scum "C:\path\to\GameServer.exe" `
  --dll  "C:\path\to\turdmod_server_loader.dll" `
  --skip-safety-check
```

`--skip-safety-check` is needed because the launcher's BE-binary check
looks for a `BattlEye/` directory relative to the EXE; server installs
sometimes have this directory even when BE is not active.

## Smoke-test without GameServer.exe

```powershell
$env:TURDMOD_FORCE_TEST = "1"
# Then inject into any process (e.g. notepad.exe) via the launcher.
# The DLL will skip the GameServer.exe guard and bring up the runtime.
```

## Paths

| Artifact | Path |
|---|---|
| Server mods | `%PROGRAMDATA%\TurdMOD\server-mods\<id>\main.lua` |
| Log | `%PROGRAMDATA%\TurdMOD\server-loader.log` |
| Persistence | `%PROGRAMDATA%\TurdMOD\server-mods\<id>\data\<key>.json` |
| RPC pipe name | `%LOCALAPPDATA%\TurdMOD\engine\pipe.txt` |

## Architecture

```
GameServer.exe
  └─ turdmod_server_loader.dll (this crate)
       ├─ detect.rs     — is_scum_server_process() guard
       ├─ runtime.rs    — mlua Lua 5.4 VM
       ├─ api.rs        — turdmod.* Lua surface (notify_panel = log stub)
       ├─ ipc.rs        — outbound POST to companion /ingest
       ├─ admin_api.rs  — named-pipe JSON-RPC server (Part B, shipped)
       └─ server_hooks.rs — STUB: Part C (UE4 hook layer + EngineApi impl)
```

`sigscan.rs` is shared from `apps/turdmod-loader/src/sigscan.rs` via
`#[path]` — no copy.

## Test

```powershell
cd apps/turdmod-server-loader
cargo test
```

Includes RPC frame-codec roundtrip and dispatch unit tests (ping,
unknown method, missing required param).
