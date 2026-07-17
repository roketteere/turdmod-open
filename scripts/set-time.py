#!/usr/bin/env python3
"""Walk SCUM's time-of-day to any target hour, safely.

WHY a loop: the bridge `setTimeOfDay` handler hard-clamps to ±2h per call ON
PURPOSE — a single large jump crosses a replication boundary
(OnRep_NighttimeDarkness) and crashes SCUMServer ~50s later. So we RAMP: call
the handler repeatedly (each step ≤2h, the safe primitive) with a short pause
between, until it reports it arrived (`clamped:false`). Honors requests up to 24h.

Usage:  python scripts/set-time.py <hour 0..24> [--pause 1.5] [--max-steps 16]
        (run from the turdmod repo root)
"""
import argparse, json, subprocess, sys, time

CLI = "tools/engine-rpc-test.mjs"


def set_time(hours: float) -> dict:
    out = subprocess.run(
        ["node", CLI, "setTimeOfDay", json.dumps({"hours": hours})],
        capture_output=True, text=True,
    ).stdout
    for line in out.splitlines():
        line = line.strip()
        if line.startswith("{"):
            try:
                d = json.loads(line)
                return d.get("result", d)
            except Exception:
                pass
    return {}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("hour", type=float, help="target hour 0..24")
    ap.add_argument("--pause", type=float, default=1.5, help="seconds between 2h steps")
    ap.add_argument("--max-steps", type=int, default=16, help="safety cap (24h ≈ 12 steps)")
    a = ap.parse_args()
    if not (0.0 <= a.hour <= 24.0):
        sys.exit("hour must be 0..24")

    target = a.hour % 24.0
    print(f"[set-time] ramping to {target:.1f} (≤2h/step, {a.pause}s pause — crash-safe)")
    for i in range(1, a.max_steps + 1):
        r = set_time(target)
        if r.get("error"):
            sys.exit(f"[set-time] handler error: {r['error']}")
        got = r.get("hours")
        clamped = r.get("clamped")
        print(f"  step {i}: now {got:.1f}  (clamped={clamped})")
        if not clamped:
            print(f"[set-time] arrived at {got:.1f}.")
            return
        time.sleep(a.pause)
    print(f"[set-time] hit max-steps ({a.max_steps}) before arriving — raise --max-steps?")


if __name__ == "__main__":
    main()
