# TurdMOD example mods

These are the production-shape reference mods that run end-to-end on the
TurdMOD companion runtime today. Each is a worked example of the API
surface a real server-side mod uses; copy any of them as the starting
point for a new mod.

## Runtime / mode mapping

TurdMOD has four mod delivery modes, each hosted by a different runtime:

| `mode` | Hosted by | Status |
|---|---|---|
| `server-side` | `apps/turdmod-companion` (Node.js) | **Live.** TS/JS modules, log-tail-driven event stream. The four examples in this directory use this mode. |
| `pak-content`  | UE4 native (drop-in pak) | **Live.** Asset-only; no scripts. `cosmetic-icon-pack/` demonstrates. |
| `offline-only` | `apps/turdmod-loader` (Rust DLL + Lua) | **Wired up; not yet end-to-end verified.** Lua scripts run in-process inside the SCUM **game** client; refuses to inject when BattlEye is active. Examples for this mode live in `../turdmod-design-stage/`. |
| `external-tool` | Separate companion app | Reserved for future use. |

The companion silently skips mods whose `mode` it does not host, so you
can mix modes freely in the same `mods-dir`.

## What's in this directory

| Folder | Mode | What it demonstrates |
|---|---|---|
| `welcome-screen/`   | `server-side` | First-join branded panel: server name, tagline, banner image, house rules, mod-brag chips, command list, dismiss action. Broadcasts both a Discord embed (`welcome.<steam>`) and a renderer payload (`panel.welcome.<steam>`). Persists per-player "seen" state. |
| `kill-feed/`        | `server-side` | Rust-style kill announcements with weapon, distance, headshot. Discord embeds with PvE/PvP classification. Recent-kills ring buffer in persistence (`recent`). |
| `vehicle-manager/`  | `server-side` | Per-vehicle ownership registry with Discord owner-mention DMs on `Destroyed` / `Disappeared` / `Failed to spawn`. Persists one record per vehicle id (`v/<id>`). |
| `my-squad/`         | `server-side` | Squad-private feed scaffold (squad data not exposed in current SCUM logs — partial impl, see `design-stage/` for the upgrade path). |
| `events-manager/`   | `server-side` | Scheduled announcements + player-count / time-of-day / login-burst triggers. Audit log + per-event run history. v0.1.0 covers the schedule + trigger slice; spawn waves + weather changes wait on the `world.*` engine surface. |
| `teleport/`         | `server-side` | RCON-backed waypoint teleport: `/tp save`, `/tp <name>`, `/tp list`, admin `/tp here`. Per-player cooldown ledger, per-player waypoints, audit log. **First mod that writes back to the game via RCON** — proves the platform can do more than react to logs. |
| `cosmetic-icon-pack/` | `pak-content` | Drop-in pak overrides for inventory icons. No scripts; cooked + installed via `turdmod pak install`. |

For mods targeting modes that aren't yet wired up end-to-end (Lua-on-loader,
hotkey UI), see [`../turdmod-design-stage/`](../turdmod-design-stage/).

## Writing a new server-side mod

### 1. Manifest (`turdmod.json`)

```json
{
  "schema": 1,
  "id": "my-mod",
  "name": "My Mod",
  "version": "0.1.0",
  "author": "you",
  "description": "What it does in one line.",
  "mode": "server-side",
  "minBuild": "23128448",
  "dependencies": {},
  "capabilities": [
    { "filesystem": "read" }
  ],
  "entrypoint": "scripts/main.ts",
  "tags": []
}
```

`id` is lowercase kebab-case. `version` is semver-shaped. See
[`packages/turdmod-manifest/src/index.ts`](../../packages/turdmod-manifest/src/index.ts)
for the full schema.

### 2. Entrypoint (`scripts/main.ts`)

```ts
import { player, network, persistence, log } from "@turdmod/turdmod-api";

export async function on_load() {
  log.info("my-mod loaded");

  player.onSpawn(async (p) => {
    const seen = (await persistence.get<{ first?: number }>(`p/${p.steamId}/seen`)) ?? {};
    if (!seen.first) {
      seen.first = Date.now();
      await persistence.set(`p/${p.steamId}/seen`, seen);
      await network.broadcast(`hello.${p.steamId}`, `Welcome, ${p.name}!`);
    }
  });
}

export async function on_unload() {
  log.info("my-mod unloaded");
}
```

The full API surface is in [`packages/turdmod-api/`](../../packages/turdmod-api/).
The companion runtime currently implements the synchronous + log-tail-driven
parts; APIs that require an in-process loader (`world.spawn`, `player.giveItem`,
`network.rpc`) throw `NOT_SUPPORTED` from this runtime. They run from the
loader DLL once a mod targets `offline-only` mode.

### 3. Verify it works

```bash
pnpm --filter @turdmod/turdmod-companion verify my-mod
```

The `verify` CLI loads your mod into a self-contained companion runtime,
dispatches one synthetic event of every kind (login / logout / chat /
kill / vehicle ×3 / bunker / admin), captures every `network.broadcast`
your mod made, lists every persistence key it touched, and reports
`PASS` / `FAIL` with details. Add `--json` for machine-readable output
suitable for the contract evidence pack.

### 4. Run live

```bash
# tail a real SCUM dedicated server
SCUM_SERVER_LOGS_DIR="C:\\Program Files (x86)\\Steam\\steamapps\\common\\SCUM Server\\SCUM\\Saved\\SaveFiles\\Logs" \
  pnpm --filter @turdmod/turdmod-companion dev

# or fire synthetic events for demos / smoke testing
pnpm --filter @turdmod/turdmod-companion demo
```

Mods loaded from `examples/turdmod/` by default. Override with
`--mods-dir <path>` or the `TURDMOD_MODS_DIR` env var.

## Sinks

`network.broadcast(channel, payload)` fans out through the host's
`SinkRouter`. The companion ships three:

- **console** (always on) — pretty-prints to stdout.
- **file** (`TURDMOD_COMPANION_LOG=path`) — appends NDJSON.
- **discord-webhook** (`TURDMOD_DISCORD_WEBHOOK=https://discord.com/...`) —
  posts an embed. Payload conventions: a string becomes `content`; an
  object with `embed` / `content` keys is forwarded as-is; anything
  else gets JSON-stringified into a code block.

More sinks (websocket fan-out, scummap web push) plug into `SinkRouter`
without touching mods. See
[`apps/turdmod-companion/src/sinks.ts`](../../apps/turdmod-companion/src/sinks.ts).
