#!/usr/bin/env python3
"""
SCUM RCON command discovery harness.

Reads candidate commands from commands.json, fires each against the
configured server, records which commands the server accepts and what
they return. Outputs a markdown reference doc.

Usage:
    python discover.py --host 127.0.0.1 --port 30016 --password XXX
    python discover.py --host ... --include-destructive
    python discover.py --host ... --only-category vehicles
    python discover.py --host ... --out ../../docs/scum-rcon-reference.md

The harness tries each candidate BOTH as-given AND with a leading '#'
(since SCUM chat-mode commands use '#' but RCON-mode may not require it).
Whichever form returns a non-empty / non-error response is recorded as
canonical.
"""

import argparse
import json
import socket
import struct
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent

# ---- Source RCON protocol ---------------------------------------------------

SERVERDATA_AUTH = 3
SERVERDATA_AUTH_RESPONSE = 2
SERVERDATA_EXECCOMMAND = 2
SERVERDATA_RESPONSE_VALUE = 0


class RconError(Exception):
    pass


class Rcon:
    def __init__(self, host: str, port: int, password: str, timeout: float = 8.0):
        self.host, self.port, self.password, self.timeout = host, port, password, timeout
        self.sock: socket.socket | None = None
        self.next_id = 1

    def connect(self) -> None:
        self.sock = socket.socket()
        self.sock.settimeout(self.timeout)
        self.sock.connect((self.host, self.port))
        auth_id = self._send_packet(SERVERDATA_AUTH, self.password.encode("utf-8"))
        resp_id, _kind, _body = self._read_packet()
        if resp_id == -1:
            raise RconError("auth rejected — wrong password?")
        if resp_id != auth_id:
            raise RconError(f"auth id mismatch (sent {auth_id}, got {resp_id})")

    def close(self) -> None:
        if self.sock:
            try:
                self.sock.close()
            except Exception:
                pass
            self.sock = None

    def execute(self, command: str) -> str:
        if self.sock is None:
            raise RconError("not connected")
        sent_id = self._send_packet(SERVERDATA_EXECCOMMAND, command.encode("utf-8"))
        # Some commands return empty body. Read one packet; if it's not ours
        # (rare in SCUM), keep reading up to 3 times.
        for _ in range(3):
            resp_id, _kind, body = self._read_packet()
            if resp_id == sent_id:
                return body.decode("utf-8", errors="replace")
        return ""

    def _send_packet(self, kind: int, body: bytes) -> int:
        if self.sock is None:
            raise RconError("not connected")
        pkt_id = self.next_id
        self.next_id += 1
        size = 4 + 4 + len(body) + 2  # id + kind + body + null + null
        pkt = struct.pack("<iii", size, pkt_id, kind) + body + b"\x00\x00"
        self.sock.sendall(pkt)
        return pkt_id

    def _read_packet(self) -> tuple[int, int, bytes]:
        if self.sock is None:
            raise RconError("not connected")
        size_bytes = self._read_exact(4)
        size = struct.unpack("<i", size_bytes)[0]
        if size < 10 or size > 4096 * 4:
            raise RconError(f"packet size out of range: {size}")
        rest = self._read_exact(size)
        pkt_id, kind = struct.unpack("<ii", rest[:8])
        body = rest[8:-2]  # strip the two null terminators
        return pkt_id, kind, body

    def _read_exact(self, n: int) -> bytes:
        assert self.sock is not None
        buf = b""
        while len(buf) < n:
            chunk = self.sock.recv(n - len(buf))
            if not chunk:
                raise RconError("connection closed by server")
            buf += chunk
        return buf


# ---- Discovery -------------------------------------------------------------


def classify_response(body: str) -> str:
    """Heuristic: figure out whether the server rejected the command."""
    if not body.strip():
        return "empty"  # accepted with no output OR silently rejected
    low = body.lower()
    if any(
        marker in low
        for marker in [
            "unknown command",
            "command not found",
            "invalid command",
            "syntax",
            "usage:",
            "not recognized",
        ]
    ):
        return "rejected"
    return "accepted"


def try_command(rcon: Rcon, command: str) -> tuple[str, str]:
    """Send a command, return (raw_body, classification)."""
    try:
        body = rcon.execute(command)
    except RconError as e:
        return f"<rcon error: {e}>", "error"
    return body, classify_response(body)


def main() -> int:
    ap = argparse.ArgumentParser(description="SCUM RCON command discovery")
    ap.add_argument("--host", required=True)
    ap.add_argument("--port", type=int, default=30016)
    ap.add_argument("--password", required=True)
    ap.add_argument("--commands", default=str(HERE / "commands.json"))
    ap.add_argument("--out", default=str(HERE / "scum-rcon-reference.md"))
    ap.add_argument("--include-destructive", action="store_true")
    ap.add_argument("--only-category", default=None)
    ap.add_argument("--delay", type=float, default=0.2, help="seconds between commands")
    args = ap.parse_args()

    with open(args.commands, "r", encoding="utf-8") as f:
        catalog = json.load(f)

    commands = list(catalog["commands"])
    if args.include_destructive:
        commands.extend(catalog.get("destructive_commands", []))
    if args.only_category:
        commands = [c for c in commands if c["category"] == args.only_category]

    print(f"[discover] connecting to {args.host}:{args.port}")
    rcon = Rcon(args.host, args.port, args.password)
    try:
        rcon.connect()
    except RconError as e:
        print(f"[discover] FATAL: {e}", file=sys.stderr)
        return 1
    print(f"[discover] auth OK — firing {len(commands)} candidates")

    results = []
    for i, entry in enumerate(commands, 1):
        cmd = entry["cmd"]
        # Try the bare form first.
        body_bare, cls_bare = try_command(rcon, cmd)
        # Try with '#' prefix if bare looked empty or rejected.
        body_hash, cls_hash = "", "skipped"
        if cls_bare in ("empty", "rejected") and not cmd.startswith("#"):
            time.sleep(args.delay)
            body_hash, cls_hash = try_command(rcon, "#" + cmd)

        # Pick the better result for the canonical form.
        if cls_bare == "accepted":
            canonical = cmd
            body, cls = body_bare, cls_bare
        elif cls_hash == "accepted":
            canonical = "#" + cmd
            body, cls = body_hash, cls_hash
        elif cls_bare == "empty" and not body_bare:
            # Empty response — recorded as "possibly accepted, no output".
            canonical = cmd
            body, cls = "", "empty"
        else:
            canonical = cmd
            body, cls = body_bare, cls_bare

        results.append(
            {
                "category": entry["category"],
                "candidate": cmd,
                "canonical": canonical,
                "classification": cls,
                "response": body[:2000],  # cap for the report
                "note": entry.get("note", ""),
            }
        )
        print(f"[{i:3d}/{len(commands)}] {cls:9s}  {canonical}")
        time.sleep(args.delay)

    rcon.close()

    # ---- Write the markdown reference ---------------------------------------

    accepted = [r for r in results if r["classification"] == "accepted"]
    empty = [r for r in results if r["classification"] == "empty"]
    rejected = [r for r in results if r["classification"] == "rejected"]
    errored = [r for r in results if r["classification"] == "error"]

    out_path = Path(args.out)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    with open(out_path, "w", encoding="utf-8") as f:
        f.write("# SCUM RCON Command Reference\n\n")
        f.write(
            f"_Auto-generated by `tools/scum-rcon-discovery/discover.py` "
            f"on {datetime.now(timezone.utc).strftime('%Y-%m-%d %H:%M UTC')}._\n\n"
        )
        f.write(
            f"Probed **{len(results)}** candidate commands against `{args.host}:{args.port}`. "
            f"**{len(accepted)} accepted**, {len(empty)} returned empty bodies (possibly "
            f"accepted with no output), {len(rejected)} rejected, {len(errored)} errored.\n\n"
        )

        def section(title: str, items: list, show_response: bool = True) -> None:
            if not items:
                return
            f.write(f"## {title} ({len(items)})\n\n")
            by_cat: dict[str, list] = {}
            for r in items:
                by_cat.setdefault(r["category"], []).append(r)
            for cat in sorted(by_cat):
                f.write(f"### `{cat}`\n\n")
                for r in by_cat[cat]:
                    f.write(f"- **`{r['canonical']}`**")
                    if r["note"]:
                        f.write(f" — {r['note']}")
                    f.write("\n")
                    if show_response and r["response"]:
                        f.write("  ```\n")
                        for line in r["response"].splitlines()[:20]:
                            f.write(f"  {line}\n")
                        f.write("  ```\n")
                f.write("\n")

        section("Accepted", accepted, show_response=True)
        section("Empty response (possibly silent success)", empty, show_response=False)
        section("Rejected by server", rejected, show_response=True)
        section("Errored (network or protocol)", errored, show_response=True)

    print(f"\n[discover] wrote report → {out_path}")
    print(
        f"[discover] {len(accepted)} accepted | {len(empty)} empty | "
        f"{len(rejected)} rejected | {len(errored)} errored"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
