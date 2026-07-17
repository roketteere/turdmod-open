# Mod Hot-Reload Guide

## Can I load/unload mods while the server is running?

| Mod Type | Hot-load | Hot-unload | Restart needed? | Example |
|---|---|---|---|---|
| **Companion mods** (TypeScript, chat commands) | YES | YES | No | economy, teleport, vehicles, permissions |
| **Engine handlers** (C++ bridge RPCs) | No | No | Full server restart | broadcastChat, setTimeOfDay, spawnVehicle |
| **Pak mods** (Blueprint UE4 assets) | Partial* | No | Pak must exist at boot | TurdMODLoader widgets, GUI Builder panels |

*Pak mods: the `.pak` file must be in `Content/Paks/` at boot (bypass handles validation). Individual Blueprint classes can be force-loaded at runtime via `loadAsset` RPC, but removing a pak requires restart.

## How It Works

The **companion process** (`turdmod-companion`) hosts all TypeScript mods. It runs separately from GameServer.exe and communicates via log-tailing + engine pipe. Mods can be loaded/unloaded at any time without touching the game server.

## API Endpoints (companion IPC server)

Once the companion is running, manage mods via HTTP:

```
GET  http://127.0.0.1:<port>/mods              → { loaded: [...], available: [...] }
POST http://127.0.0.1:<port>/mods/load         → { "modId": "economy" }
POST http://127.0.0.1:<port>/mods/unload       → { "modId": "economy" }
POST http://127.0.0.1:<port>/mods/reload       → { "modId": "economy" }
GET  http://127.0.0.1:<port>/health            → { ok: true, subscribers: N }
```

The port is written to `~/.scummy-map/turdmod-companion.json` on startup (auto-discovered).

## From the Manager GUI

The Manager's Library page calls these endpoints to show installed/running mods with Install/Uninstall/Enable/Disable buttons. No server restart needed for companion mods.

## Mod Lifecycle

```
discoverMods(modsDir)     → finds all turdmod.json in mods/
  ↓
loadMod(discovered)       → import() the scripts/main.ts
  ↓
bindMod(modBinding)       → create per-mod runtime, call on_load()
  ↓
[running — handles events, chat commands, ticks]
  ↓
unbindMod(modId)          → call on_unload(), dispose handlers
  ↓
[mod fully removed from memory — can reload at any time]
```

## Mod Directory Layout

```
mods/
  economy/
    turdmod.json          ← manifest (mode: "server-side")
    scripts/main.ts       ← entrypoint (exports on_load, on_unload)
  teleport/
    turdmod.json
    scripts/main.ts
  ...
```

## Environment

| Var | Purpose | Default |
|---|---|---|
| `TURDMOD_MODS_DIR` | Path to mods directory | `mods/` in repo root |
| `SCUM_SERVER_LOGS_DIR` | SCUM server logs (for log-tail) | required |
| `TURDMOD_COMPANION_PORT` | IPC server port | auto (free port) |
| `TURDMOD_DISCORD_WEBHOOK` | Discord webhook for mod output | optional |
