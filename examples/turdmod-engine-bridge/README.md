# TurdMOD Engine Bridge — UE4SS Lua Mod

The UE4SS-side half of the TurdMOD Engine. Runs inside `GameServer.exe`
alongside our `turdmod-server-loader.dll`; bridges between UE4SS's
reflection API and the named-pipe RPC server the companion talks to.

## What it does

- **Inbound** — polls `%LOCALAPPDATA%\TurdMOD\engine\inbox.jsonl` every
  100ms for commands written by `turdmod-server-loader.dll`. Dispatches
  to UE4SS Lua APIs (with all UE4 actor mutations marshalled to the game
  thread via `ExecuteInGameThread`).
- **Outbound** — registers `RegisterHook` callbacks on UE4 functions
  (PostLogin, Logout — chat / death pending Sprint 2.5 discovery).
  Appends events to `%LOCALAPPDATA%\TurdMOD\engine\outbox.jsonl`. The
  DLL drains and forwards over the named pipe to the companion.

## Status

**v0.2.0 — production-grade. Real handlers using documented UE4SS Lua APIs.**

Functioning today (works on any UE4 4.27 game; SCUM-tested in Sprint 2.5):
- `ping` — round-trip test
- `bridgeReady` event on boot (includes method list)
- `broadcastChat({text})` — iterates `FindAllOf("PlayerController")`,
  calls `ClientMessage(text, "Default", 5.0)` on each via
  `ExecuteInGameThread`. Returns `{ok, queued: N}` immediately +
  emits `broadcastChatExecuted` event when done.
- `sendPrivateMessage({steamId, text})` — finds PC by steam, sends
  ClientMessage to that one
- `teleportPlayer({steamId, x, y, z})` — finds PC, calls
  `pawn:K2_SetActorLocation(...)` on the game thread. Standard UE4 API.
- `getOnlinePlayers()` — returns `[{steamId, name, x, y, z, hasPawn,
  controllerName}]` for every connected PC
- `getPlayerPosition({steamId})` — `{x, y, z}` for one player
- `playerLogin` / `playerLogout` events via `RegisterHook` on
  `/Script/Engine.GameMode:PostLogin` and `:Logout` (engine base class —
  SCUM subclass paths discoverable via `discover.classes("Scum")`)

**Discovery API** — probes SCUM internals from the companion side
without needing in-game Ctrl+J dumps:
- `discover.classes({pattern, limit})` — list UClass FullNames matching
  Lua pattern. Use `pattern="Scum"` to find SCUM-specific classes.
- `discover.functions({classPath})` — list UFunctions on a class.
  E.g. `discover.functions({classPath: "/Script/Scum.SCUMPlayerController"})`
  returns every callable function on SCUM's PC subclass.
- `discover.world` — quick orientation: UWorld + GameMode + GameState +
  NetDriver class names + connected player count
- `discover.players` — full debug dump with PC/PS/Pawn class names so
  we can see what SCUM-specific UE4 subclasses are in play

Still TODO (Sprint 2.5):
- `spawnVehicle` — needs SCUM-specific BP function library; returns
  `E_HOOK_FAILED` for now with a discovery hint
- `chat` event — needs the SCUM-specific receive UFunction path
  (identifiable via `discover.functions` once we know the PC class name)
- `playerDeath` event — same; needs SCUM's death-notification UFunction

## Install

```
SCUMServer install root\SCUM\Binaries\Win64\
├── GameServer.exe
├── UE4SS.dll                     <- from UE4SS v3.0.1 release
├── dwmapi.dll                    <- UE4SS proxy DLL (only if using
│                                    proxy-DLL install; not needed when
│                                    injecting via turdmod-launcher)
├── UE4SS-settings.ini
└── Mods\
    ├── shared\                   <- UE4SS-bundled
    ├── Keybinds\
    └── TurdMODBridge\            <- THIS MOD
        ├── enabled.txt
        └── Scripts\
            ├── main.lua
            └── json.lua
```

Then launch:

```powershell
turdmod-launcher.exe `
  --scum  "C:\Path\To\GameServer.exe" `
  --dll   "C:\Path\To\turdmod_server_loader.dll" `
  --extra-dll "C:\Path\To\UE4SS.dll" `
  --skip-safety-check
```

**Elevation required** — GameServer.exe has `requireAdministrator` in
its manifest. Launch from an elevated PowerShell.

## Verify the bridge is up

After launch, check:

```powershell
type "$env:LOCALAPPDATA\TurdMOD\engine\lua-ready.marker"
# expect: "ready TurdMODBridge v0.1.0 pid=<unix-seconds>"

type "$env:LOCALAPPDATA\TurdMOD\engine\outbox.jsonl"
# expect first line: {"event":"bridgeReady","data":{...},"ts":...}

type "$env:LOCALAPPDATA\TurdMOD\engine\pipe.txt"
# expect: \\.\pipe\turdmod-engine-<scumserver-pid>
```

If `lua-ready.marker` is missing, the Lua mod failed to load — check
`C:\Tools\UE4SS\UE4SS.log` (or wherever UE4SS writes its log per
`UE4SS-settings.ini`) for the `Starting Lua mod 'TurdMODBridge'` line
and any error after it.

## Bridge protocol

### Inbox format (DLL → Lua, JSONL)

Each line is one JSON object:

```json
{ "id": "<uuid>", "method": "<name>", "params": <json> }
```

### Outbox format (Lua → DLL, JSONL)

Each line is either an event:

```json
{ "event": "<name>", "data": <json>, "ts": <unix> }
```

Or a response (correlated by `id` from the inbox):

```json
{ "id": "<uuid>", "result": <json> }
{ "id": "<uuid>", "error": { "code": <int>, "message": "<str>" } }
```

### Supported methods (v0.1)

| Method | Status | Notes |
|---|---|---|
| `ping` | working | returns `{ pong: true, version, mod }` |
| `broadcastChat` | stub | logs intent; needs SCUM broadcast UFunction path |
| `teleportPlayer` | stub | logs intent; needs SCUM PlayerState offset for steamId |
| `getOnlinePlayers` | stub | returns `[]`; needs SCUM-specific iteration |
| `spawnVehicle` | stub | logs intent; needs StaticFindObject + SpawnActor wiring |

### Emitted events (v0.1)

| Event | When | Status |
|---|---|---|
| `bridgeReady` | mod loads | working |
| `playerLogin` | engine `PostLogin` fires | working on base class; may need SCUM override |
| `playerLogout` | engine `Logout` fires | same |
| `chat` | player sends chat | TODO Sprint 2.5 |
| `playerDeath` | player dies | TODO Sprint 2.5 |

## Sprint 2.5 discovery process (next steps)

To fill in the SCUM-specific hooks:

1. Boot SCUMServer with this mod loaded.
2. Press **Ctrl+H** (UE4SS keybind) to dump CXX headers, OR **Ctrl+J**
   to dump all UObjects. Output lands next to `GameServer.exe`.
3. Grep the dump for `SCUM_PlayerController`, `SCUM_GameMode`,
   `SCUM_Character` (or whatever prefix SCUM uses).
4. Find UFunctions named like `ServerChat`, `Server_SendMessage`,
   `Die`, `OnPlayerDeath`, `Kick`.
5. Add `safe_register_hook("/Script/Scum.SCUM_PlayerController:Server_SendChatMessage", ...)`
   blocks to `main.lua`.
6. Find `APlayerState` member named `SteamID` or similar; read it in
   the `on_post_login` callback.

## License

`json.lua` is rxi/json.lua under MIT (header preserved in the file).
The rest is part of TurdMOD — MIT.
