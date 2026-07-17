# Bridge Events — wire protocol + consumer recipe

**Status (2026-05-22):** Plumbing live end-to-end. `chat` event + a
`smoke.tick` validator are wired. The other 7 promised events
(`login`, `logout`, `kill`, `vehicleSpawned`, `vehicleDestroyed`,
`weatherChanged`, `timeChanged`) are scaffolded; each one extends
the existing dispatcher when its SCUM UFunction is identified.

---

## The push path in one diagram

```
┌── GameServer.exe (one process) ──────────────────────────────┐
│                                                              │
│   UE4 game thread                                            │
│        ↓                                                     │
│   UObject::ProcessEvent  (every UFunction call, ~10k/sec)    │
│        ↓                                                     │
│   PolyHook2 detour → hooked_process_event                    │
│        ↓                                                     │
│   g_fn_event_dispatch cache lookup (FName::ComparisonIndex)  │
│        ↓ if recognized                                       │
│   dispatch_engine_event(kind, this, fn, params)              │
│        ↓                                                     │
│   emit_engine_event("chat", "{...json...}")                  │
│        ↓                                                     │
│   g_api.emit_event → turdmod_engine_emit_event (C ABI)       │
│        ↓                                                     │
│   ── crosses into turdmod_server_loader.dll (same proc) ──   │
│        ↓                                                     │
│   admin_api::EventBroadcaster::emit                          │
│        ↓                                                     │
│   tokio::sync::broadcast::Sender (cap 256, drop-on-overflow) │
│        ↓                                                     │
│   serve_connection: every connected client's receiver        │
│        ↓                                                     │
│   write [4-byte LE length][JSON body] to named pipe          │
│                                                              │
└──────────────────────────────────────────────────────────────┘
        ↓
  \\.\pipe\turdmod-engine-<pid>  (discovery at %LOCALAPPDATA%\TurdMOD\engine\pipe.txt)
        ↓
  ┌─ Manager (Tauri/Rust)                                      ┐
  │   engine_events.rs long-lived listener task                │
  │   ↓ decodes frames, discriminates event vs response        │
  │   ↓ tauri::Emitter::emit("engine://event", payload)        │
  │   ↓                                                        │
  │   React useEngineEvents() hook → BridgeSmokePage firehose  │
  └────────────────────────────────────────────────────────────┘
        ↓ (parallel)
  ┌─ scumpilot / tools/engine-events-tail.mjs                  ┐
  │   open pipe, read frames, parse JSON, handle event         │
  └────────────────────────────────────────────────────────────┘
```

Key consequences of the shape:

- **Multi-consumer.** The broadcaster fans out to *every* connected
  client. Manager + scumpilot + CLI tail can all subscribe at once
  with no extra plumbing.
- **Push, not poll.** Subscribers don't request anything. They open
  the pipe and read forever. Events arrive as the bridge emits them.
- **Drop-on-overflow.** If a subscriber lags more than 256 events
  behind, tokio's broadcast channel drops the lagged ones and logs.
  Burst-tolerant but no replay — late subscribers don't see history.
- **One pipe, mixed frames.** The same pipe carries both RPC
  responses (`{id, result?, error?}`) and event pushes
  (`{event, data}`). Subscribers discriminate by presence of `event`.

---

## Wire format

Length-prefixed JSON frames on the named pipe:

```
[4 bytes uint32 LE length][N bytes UTF-8 JSON body]
```

Event frame body:

```json
{
  "event": "<kind>",
  "data": { ... event-specific payload ... }
}
```

Response frame body (for the OTHER direction — RPC replies):

```json
{
  "id": "<uuid>",
  "result": { ... } | null,
  "error":  { ... } | null
}
```

Anything else on the pipe is unknown — silently ignore in clients.

---

## Event catalog

Authoritative shape spec: `turdmod-companion/src/parsers.ts ServerEvent`
(canonical) + `scumpilot/docs/bridge-gap-contract.md` (consumer side).

| event | payload | status | source UFunction |
|---|---|---|---|
| `smoke.tick` | `{ counter, uptimeMs }` | **LIVE** (env-gated) | self-driven 1Hz thread |
| `bridgeReady` | `{ version, mod, kind }` | **LIVE** (one-shot) | bridge `on_unreal_init` |
| `chat`       | `{ ts, channel: "Local"\|"Squad"\|"Global"\|"Admin", player, steam, text }` | **LIVE** | `PlayerRpcChannel::Chat_Server_BroadcastChatMessage` |
| `login`      | `{ ts, steam, player, pos: {x,y,z} }` | **LIVE** | `GameModeBase::HandleStartingNewPlayer` |
| `logout`     | `{ ts, steam, player }` | **DEFERRED** — no reflectable UFunction (LogoutCallbackProxy is a BP helper, not the disconnect-completion event). Needs patternsleuth scan of EndPlay / NotifyClientDisconnect. | TBD |
| `kill`             | scumpilot Wave 4 | scaffolded | needs grep — `OnKilled` / `PlayerKilled` |
| `vehicleSpawned`   | scumpilot Wave 4 | scaffolded | needs grep — vehicle manager |
| `vehicleDestroyed` | scumpilot Wave 4 | scaffolded | needs grep |
| `weatherChanged`   | optional, paired with `setWeather` handler | scaffolded | needs grep |
| `timeChanged`      | optional, paired with `setTimeOfDay` handler | scaffolded | needs grep |

**Live event v1 caveats** to fix in v2:
- `chat.steam` and `login.steam` are emitted as empty strings. Reading
  SteamID64 requires walking PlayerState.UniqueId (FUniqueNetIdRepl)
  through KismetOnlineHelpers — TODO. scumpilot's contract falls back
  to all-zeros so events still fire correctly.
- `login.pos` defaults to `(0,0,0)` because `HandleStartingNewPlayer`
  is too early to know the spawn location.
- `login` fires very early; `PlayerName` may be momentarily empty if
  replication hasn't filled the PlayerState yet. scumpilot's brain
  re-folds chat-event senders so the name self-corrects.

To add a new event:

1. Grep `scumdump/data/extracted/v<build>/classes.json` for the
   canonical SCUM UFunction name + outer class + param layout.
2. Add a new `EV_*` value to `EventDispatchKind` in
   `TurdMODEngineBridge.cpp` — keep order stable so cached entries
   survive hot-reloads.
3. Add the name match in `hooked_process_event`'s first-sighting
   branch.
4. Add a `case EV_*:` in `dispatch_engine_event` that reads the
   params and emits a stable JSON payload.
5. Sync the canonical file to
   `C:/Development/RE-UE4SS/cppmods/TurdMODEngineBridge/src/dllmain.cpp`.
6. Build via `tmp\build-bridge.cmd`, deploy via Manager → Engine →
   Install, restart GameServer.exe.

---

## Consumer recipes

### From the Manager (already wired)

`apps/turdmod-manager/src/hooks/useEngineEvents.ts`:

```ts
const { events, countsByKind, totalSeen, clear } = useEngineEvents({
  bufferSize: 500,
  filter: ['chat', 'login'],   // null = all kinds
});
```

The `engine_events.rs` listener in `src-tauri` keeps a long-lived
pipe connection open, auto-reconnects when the engine isn't running,
and re-emits every event through the Tauri event channel
`engine://event`. React only sees the Tauri channel.

### From the command line

```powershell
node tools/engine-events-tail.mjs
node tools/engine-events-tail.mjs --filter chat,login
node tools/engine-events-tail.mjs --raw
```

Self-contained Node script. Prints one line per event, color-coded.

### From scumpilot (or any other consumer)

Open the pipe (read its name from `%LOCALAPPDATA%/TurdMOD/engine/pipe.txt`),
read length-prefixed frames, discriminate by presence of `event`.
JavaScript reference:

```js
import { createConnection } from 'node:net';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const name = readFileSync(
  join(process.env.LOCALAPPDATA, 'TurdMOD', 'engine', 'pipe.txt'),
  'utf-8',
).trim();
const sock = createConnection(name);
let buf = Buffer.alloc(0);
sock.on('data', (chunk) => {
  buf = Buffer.concat([buf, chunk]);
  while (buf.length >= 4) {
    const len = buf.readUInt32LE(0);
    if (buf.length < 4 + len) break;
    const body = JSON.parse(buf.subarray(4, 4 + len).toString('utf-8'));
    buf = buf.subarray(4 + len);
    if (typeof body.event === 'string') {
      // handle body.event + body.data
    }
  }
});
```

Rust reference: copy `apps/turdmod-manager/src-tauri/src/engine_events.rs`
into scumpilot. Same protocol — just replace the `tauri::Emitter` call
with your own dispatch.

---

## Validating the path end-to-end

The simplest "is the firehose alive" test:

```powershell
# 1. Stop SCUMServer if running.
# 2. Deploy fresh DLL (Manager → Engine → Install).
# 3. Enable smoke tick.
$env:TURDMOD_SMOKE_TICK = '1'
# 4. Start SCUMServer (Manager → Engine → Start).
# 5. Tail events.
node tools/engine-events-tail.mjs --filter smoke.tick
# Should see one event per second printed.
# 6. Open Manager → Bridge Smoke. The Engine Event Firehose card
#    should show the same ticks landing live.
```

If the tail sees `bridgeReady` once but no `smoke.tick`, the env var
didn't reach GameServer.exe (Manager may launch via a different
shell). Set TURDMOD_SMOKE_TICK in System Properties → Environment
Variables, or pass it through the Manager's process spawn.

To validate `chat`: have a player type in chat, or fire
`broadcastChat` from the Manager's BridgeSmokePage. A `chat` event
should appear with the message text and channel byte.

---

## Game-thread safety contract

- `emit_engine_event` is called from the UE4 game thread (via the
  ProcessEvent hook). It MUST NOT block, allocate large amounts of
  memory, or take long locks.
- `g_api.emit_event` ultimately calls tokio's
  `broadcast::Sender::send`, which is non-blocking. Safe.
- The smoke-tick thread is detached and runs on its own. It does
  not touch UE4 state and is safe to leave running.
- Dispatch cache (`g_fn_event_dispatch`) is read-only after first
  sighting per FName. Insert is std::unordered_map which is not
  thread-safe but the hook runs on a single game thread; safe.

If a future event requires expensive work (e.g. walking
GUObjectArray to enrich a vehicle payload), do that work on a
background thread and have the hook only schedule it — don't block
the game.
