#!/usr/bin/env python3
"""
SCUM vehicle spawn-point deep dive.

After discover.py confirms which vehicle-related commands the server
accepts, run this to:
  1. Capture the current vehicle list / spawn-point snapshot.
  2. Force respawns N times.
  3. Diff each snapshot vs the first to detect whether spawn points
     are deterministic, location-pool, or fully random.

Output: data/scum-vehicle-spawns.json + a brief markdown summary.

Usage (after editing the *_CMD constants below to match what discover.py
found accepted):

    python vehicle_explore.py --host 127.0.0.1 --port 30016 \
        --password XXX --iterations 5 --interval 30

This is a thin scaffold — the LIST_CMD / RESPAWN_CMD constants are
placeholders. Update them once we know SCUM's exact vehicle command set.
"""

import argparse
import json
import sys
import time
from pathlib import Path

from discover import Rcon, RconError

# Adjust these after `discover.py` confirms what's accepted.
LIST_CMD = "ListVehicles"        # OR "#ListVehicles" — patch after discovery
RESPAWN_CMD = "RespawnAllVehicles"  # placeholder
DESPAWN_CMD = "DespawnAllVehicles"  # placeholder

HERE = Path(__file__).resolve().parent


def snapshot(rcon: Rcon, cmd: str) -> str:
    return rcon.execute(cmd)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--host", required=True)
    ap.add_argument("--port", type=int, default=30016)
    ap.add_argument("--password", required=True)
    ap.add_argument("--iterations", type=int, default=5)
    ap.add_argument("--interval", type=float, default=30.0,
                    help="seconds between respawn cycles (let SCUM settle)")
    ap.add_argument("--out", default=str(HERE.parent.parent / "data" / "scum-vehicle-spawns.json"))
    ap.add_argument("--list-cmd", default=LIST_CMD)
    ap.add_argument("--respawn-cmd", default=RESPAWN_CMD)
    ap.add_argument("--despawn-cmd", default=DESPAWN_CMD)
    args = ap.parse_args()

    print(f"[vehicles] connecting to {args.host}:{args.port}")
    rcon = Rcon(args.host, args.port, args.password)
    try:
        rcon.connect()
    except RconError as e:
        print(f"[vehicles] FATAL: {e}", file=sys.stderr)
        return 1

    snapshots = []
    for i in range(args.iterations):
        print(f"[vehicles] iteration {i+1}/{args.iterations}")
        raw = snapshot(rcon, args.list_cmd)
        snapshots.append({"iter": i, "raw": raw, "ts": time.time()})

        if i < args.iterations - 1:
            # Trigger a respawn cycle for the next iteration.
            print(f"[vehicles]   → despawn")
            rcon.execute(args.despawn_cmd)
            time.sleep(3)
            print(f"[vehicles]   → respawn")
            rcon.execute(args.respawn_cmd)
            print(f"[vehicles]   → sleeping {args.interval}s for SCUM to settle")
            time.sleep(args.interval)

    rcon.close()

    # Persist raw snapshots; analysis is left for follow-up tooling once we
    # see the actual response format SCUM returns for the list command.
    out_path = Path(args.out)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    with open(out_path, "w", encoding="utf-8") as f:
        json.dump(
            {
                "host": args.host,
                "iterations": args.iterations,
                "list_cmd": args.list_cmd,
                "respawn_cmd": args.respawn_cmd,
                "despawn_cmd": args.despawn_cmd,
                "snapshots": snapshots,
            },
            f,
            indent=2,
        )
    print(f"\n[vehicles] wrote raw snapshots → {out_path}")
    print("[vehicles] next: write a parser for SCUM's vehicle-list format")
    return 0


if __name__ == "__main__":
    sys.exit(main())
