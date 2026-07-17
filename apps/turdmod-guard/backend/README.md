# turdmod-guard-backend

Central REST API for the TurdMOD Guard fleet. Receives flag reports from
each server's running guard daemon, stores a shared history, and serves a
cross-server ban-list lookup so a player banned on one TurdMOD server can
be flagged when joining another.

This is the v0 backend — file-backed JSON storage, single shared-secret
admin auth. The shape is deliberately small so it can move to Postgres +
drizzle (and proper per-server auth tokens) without churning the wire
protocol.

## Where it sits

```
guard daemon (Rust)  ──POST /reports────►  guard-backend  ──read───►  admin dashboard (web)
                                                  │
guard daemon (Rust)  ──GET  /bans/check ◄─────────┘
   on player login
```

## Running it

```bash
# from the repo root
pnpm install
pnpm --filter @turdmod/turdmod-guard-backend dev
```

The dev script reads `.env` from the repo root and watches `src/`.
Override the port (otherwise the OS picks a free one):

```bash
PORT=4567 pnpm --filter @turdmod/turdmod-guard-backend dev
```

On start-up the server writes a discovery file at
`~/.scummy-map/turdmod-guard-backend.json`:

```json
{
  "app": "turdmod-guard-backend",
  "port": 51234,
  "url": "http://localhost:51234",
  "pid": 12345,
  "startedAt": "2026-05-08T20:34:56.000Z"
}
```

The Rust daemon reads that file (or `TURDMOD_GUARD_BACKEND` env override)
to find the running backend — never hardcode the port.

## Environment

| Var                  | Required          | Default                     | Notes                                                                     |
| -------------------- | ----------------- | --------------------------- | ------------------------------------------------------------------------- |
| `PORT`               | no                | OS-picked free port         | Set explicitly only when you need a deterministic port (e.g. behind nginx) |
| `GUARD_ADMIN_TOKEN`  | for admin routes  | (unset → admin routes 503)  | Bearer token for `POST /bans`, `DELETE /bans/:steam`, `GET /bans`         |
| `GUARD_BACKEND_DIR`  | no                | `<cwd>/data/guard-backend`  | Directory holding `reports.ndjson` + `bans.json`. Both files gitignored.  |

## Storage layout

```
<GUARD_BACKEND_DIR>/
├── reports.ndjson   # append-only, one DetectorReport per line
└── bans.json        # full file rewrite on every edit
```

`reports.ndjson` lines are exactly the JSON the Rust daemon serializes
from `DetectorReport` (see `apps/turdmod-guard/src/detectors/mod.rs`),
plus a backend-stamped `receivedAt`.

## Endpoints

### `GET /health` (public)

```json
{ "ok": true, "ts": "...", "baseDir": "/path/to/data/guard-backend" }
```

### `POST /reports` (public — daemons post here)

Body: a `DetectorReport` JSON. The serde external-tagged enum format is
accepted as-is — both the bare-string `"Ok"` verdict and the object-form
`{"Warn": {…}}` / `{"Flag": {…}}` work.

```bash
curl -sS -X POST http://localhost:$PORT/reports \
  -H 'content-type: application/json' \
  -H 'x-guard-server: srv-eu-1' \
  -d '{
    "detector": "kill_distance",
    "ts": "2026-05-08T20:00:00Z",
    "verdict": { "Flag": { "reason": "AK-47 hit @ 812m", "severity": "High", "evidence": { "distance_m": 812 } } },
    "steam": "76561198000000001",
    "player": "Alice"
  }'
```

### `GET /reports?steam=&detector=&verdict=&page=&pageSize=` (public)

```bash
curl -sS "http://localhost:$PORT/reports?steam=76561198000000001&pageSize=20"
```

Returns `{ reports: StoredReport[], page, pageSize, total }`. Newest-first.
`pageSize` capped at 500; `verdict` is one of `Ok | Warn | Flag`.

### `POST /bans` (admin)

```bash
curl -sS -X POST http://localhost:$PORT/bans \
  -H "authorization: Bearer $GUARD_ADMIN_TOKEN" \
  -H 'content-type: application/json' \
  -d '{ "steam": "76561198000000001", "reason": "speed-hack", "sourceServer": "srv-eu-1", "bannedBy": "joel" }'
```

Idempotent: re-posting the same steam updates the reason / actor without
resetting the original `since` timestamp.

### `GET /bans/check?steam=…` (public — daemons hit this on login)

```bash
curl -sS "http://localhost:$PORT/bans/check?steam=76561198000000001"
# → { "banned": true, "reason": "speed-hack", "since": "2026-05-08T…", "sourceServer": "srv-eu-1" }
```

### `DELETE /bans/:steam` (admin)

```bash
curl -sS -X DELETE "http://localhost:$PORT/bans/76561198000000001" \
  -H "authorization: Bearer $GUARD_ADMIN_TOKEN"
# → 204 on hit, 404 on miss
```

### `GET /bans` (admin)

Full ban list. Sorted by `since` ascending.

## Tests

```bash
pnpm --filter @turdmod/turdmod-guard-backend test
```

Uses `node:test` + `tsx`, no extra runner. Tests cover the store and the
Hono routes (request-level, no live socket).

## Roadmap (not in v0)

- Postgres + drizzle migration. Schema mirrors the file shapes; the
  NDJSON file becomes the import source for `guard_reports`.
- Per-server tokens instead of one shared admin secret. Each registered
  server gets its own token + scope (its own reports, read-only on
  others).
- Webhook for high-severity flags so admins are paged in real-time.
- A `GET /bans/check` batch variant so a daemon can verify a whole
  player connect-storm in one call.
