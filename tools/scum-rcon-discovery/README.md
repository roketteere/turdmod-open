# SCUM RCON Discovery

Two-phase research harness for building a definitive SCUM admin-command reference.

## Phase 1 — `discover.py`

Brute-force every candidate command in `commands.json` against a live SCUM server. For each command:

- Tries the bare form (`ListPlayers`) AND the chat-prefixed form (`#ListPlayers`)
- Records server response
- Classifies as `accepted` / `empty` / `rejected` / `error`
- Writes a markdown reference doc

```sh
python discover.py --host 127.0.0.1 --port 30016 --password XXXXX
python discover.py --host ... --include-destructive   # adds kick/ban/etc
python discover.py --host ... --only-category vehicles
python discover.py --host ... --out ../../docs/scum-rcon-reference.md
```

Defaults to writing `scum-rcon-reference.md` next to the script. Pass `--out` to redirect.

## Phase 2 — `vehicle_explore.py`

Once Phase 1 confirms which vehicle commands the server accepts, edit the `LIST_CMD` / `RESPAWN_CMD` / `DESPAWN_CMD` constants and run:

```sh
python vehicle_explore.py --host 127.0.0.1 --port 30016 \
    --password XXX --iterations 5 --interval 30
```

Captures raw snapshots between forced respawn cycles. Output: `data/scum-vehicle-spawns.json` for follow-up analysis.

## Prerequisites

1. SCUM dedicated server running locally OR on a host you control
2. RCON enabled in `ServerSettings.ini`:
   ```ini
   [RCON]
   bEnabled=True
   RCONPassword=yourpassword
   RCONPort=30016
   MaxConnectionCount=2
   ```
3. Server restarted after the .ini change
4. Confirm port is open: `Test-NetConnection -ComputerName 127.0.0.1 -Port 30016`

Python 3.10+. No external dependencies — uses only stdlib.

## Why this exists

SCUM has no published admin-command reference. Phase B audit of TurdMOD's architecture confirmed RCON is the only viable real-time write-back path, but we don't know the exact command syntax for kick / ban / teleport / vehicle ops. This harness empirically discovers what works, what doesn't, and what the responses look like — driving:

- Phase C's `runtime.rcon.*` typed API (so mods don't guess command strings)
- Future mods like PatrolWaves, vehicle-management, AdvancedSpawnControl
- A definitive `docs/scum-rcon-reference.md` for the community
