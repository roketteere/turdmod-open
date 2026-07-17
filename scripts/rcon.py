#!/usr/bin/env python3
"""Minimal Source-RCON CLI for the SCUM dedicated server.

SCUM ships Valve's Source RCON (TCP). This runs admin commands through SCUM's
REAL server-side pipeline with full console authority — the working path for
admin commands (the in-process bridge's ProcessEvent path does NOT execute
them; see HANDBOOK.md sections 2.2-2.5).

Config resolution (first hit wins): CLI flags > env > C:\\TurdMOD\\data\\rcon.json
  host : --host / RCON_HOST / json.host   (default 127.0.0.1)
  port : --port / RCON_PORT / json.port   (default 30016, the LOCAL dev port)
  pass : --pass / RCON_PASS / json.password
Secrets are NOT hardcoded here — keep the password in env or the gitignored
rcon.json / .secrets/credentials.md.

Usage:
  python scripts/rcon.py "SetTime 6"
  RCON_PASS=... python scripts/rcon.py "SpawnVehicle BPC_Kinglet_Duster"
  python scripts/rcon.py --host 127.0.0.1 --port 30016 --pass <pw> "ListPlayers"
"""
import argparse, json, os, socket, struct, sys

AUTH, EXEC = 3, 2  # SERVERDATA_AUTH, SERVERDATA_EXECCOMMAND


def _cfg():
    host, port, pw = "127.0.0.1", 30016, None
    try:
        with open(r"C:\TurdMOD\data\rcon.json", encoding="utf-8-sig") as f:
            j = json.load(f)
        host = j.get("host", host); port = int(j.get("port", port)); pw = j.get("password", pw)
    except Exception:
        pass
    return host, port, pw


def _pkt(i, t, body):
    b = body.encode() + b"\x00\x00"
    return struct.pack("<iii", 4 + 4 + len(b), i, t) + b


def _read(s):
    size = struct.unpack("<i", s.recv(4))[0]
    data = b""
    while len(data) < size:
        chunk = s.recv(size - len(data))
        if not chunk:
            break
        data += chunk
    rid = struct.unpack("<i", data[:4])[0]
    rtype = struct.unpack("<i", data[4:8])[0]
    body = data[8:].split(b"\x00")[0].decode("utf-8", "replace")
    return rid, rtype, body


def rcon(host, port, password, command, timeout=8.0):
    with socket.create_connection((host, port), timeout=timeout) as s:
        s.settimeout(timeout)
        s.sendall(_pkt(1, AUTH, password))
        rid, rtype, _ = _read(s)
        if rtype != EXEC:          # some builds emit an empty RESPONSE_VALUE first
            rid, rtype, _ = _read(s)
        if rid == -1:
            raise RuntimeError("RCON auth failed (bad password or RCON disabled)")
        s.sendall(_pkt(2, EXEC, command))
        _, _, body = _read(s)
        return body


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("command", help='e.g. "SetTime 6"')
    ap.add_argument("--host"); ap.add_argument("--port", type=int); ap.add_argument("--pass", dest="pw")
    a = ap.parse_args()
    h, p, pw = _cfg()
    host = a.host or os.environ.get("RCON_HOST") or h
    port = a.port or (int(os.environ["RCON_PORT"]) if os.environ.get("RCON_PORT") else None) or p
    password = a.pw or os.environ.get("RCON_PASS") or pw
    if not password:
        sys.exit("no RCON password — pass --pass, set RCON_PASS, or put it in C:\\TurdMOD\\data\\rcon.json")
    print(f"[rcon] {host}:{port}  <- {a.command!r}")
    try:
        print(f"[rcon] response: {rcon(host, port, password, a.command)!r}")
    except Exception as e:
        sys.exit(f"[rcon] error: {e}")


if __name__ == "__main__":
    main()
