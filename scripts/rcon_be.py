#!/usr/bin/env python3
"""BattlEye RCON (BERcon) client — the REAL admin channel for a SCUM server.

SCUM uses BattlEye, and its admin/RCON console is BattlEye RCON (BERcon), a
UDP protocol — NOT Valve Source RCON (which scripts/rcon.py and the Rust
turdmod-service/src/rcon.rs assume; that assumption appears wrong for direct
SCUM and is why nothing bound on the Source-RCON port).

Configured in <server>/BattlEye/BEServer_x64.cfg:
    RConPassword <pw>
    RConPort     <udp-port>
    RConIP       127.0.0.1            # local-only is safest

BERcon packet:  'B''E' | CRC32(payload) LE 4B | payload
  payload = 0xFF <type> <data>
    type 0x00 login   : data = password
    type 0x01 command : data = <seq:1B> <command ascii>
    type 0x02 server msg (server->client keepalive/multipart)
  login reply : 0xFF 0x00 <0x01 ok | 0x00 fail>
  cmd reply   : 0xFF 0x01 <seq> [0x00 <nPkts> <idx>]? <ascii response>

Usage:
  python scripts/rcon_be.py --port 30016 --pass <pw> "SetTime 6"
"""
import argparse, socket, struct, sys, zlib


def _frame(ptype: int, data: bytes) -> bytes:
    payload = b"\xff" + bytes([ptype]) + data
    crc = struct.pack("<I", zlib.crc32(payload) & 0xFFFFFFFF)
    return b"BE" + crc + payload


def _payload(pkt: bytes) -> bytes:
    # strip 'BE' + 4-byte crc, return the 0xFF... payload
    if len(pkt) < 7 or pkt[:2] != b"BE":
        raise RuntimeError(f"bad BERcon header: {pkt[:8]!r}")
    return pkt[6:]


def exec_cmd(host: str, port: int, password: str, command: str, timeout=6.0) -> str:
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.settimeout(timeout)
    try:
        s.sendto(_frame(0x00, password.encode("latin-1")), (host, port))
        p = _payload(s.recvfrom(2048)[0])               # login reply
        if not (p[0] == 0xFF and p[1] == 0x00):
            raise RuntimeError(f"unexpected login reply: {p[:4]!r}")
        if p[2] != 0x01:
            raise RuntimeError("BERcon login FAILED (bad RCON password)")
        # send command, seq 0
        s.sendto(_frame(0x01, b"\x00" + command.encode("latin-1")), (host, port))
        out = []
        while True:
            try:
                p = _payload(s.recvfrom(4096)[0])
            except socket.timeout:
                break
            if p[1] == 0x01:                            # command response
                body = p[3:]
                # multipart header: 0x00 <total> <index>
                if body[:1] == b"\x00" and len(body) >= 3:
                    body = body[3:]
                out.append(body.decode("latin-1", "replace"))
            elif p[1] == 0x02:                          # server keepalive — ack it
                s.sendto(_frame(0x02, bytes([p[2]])), (host, port))
        return "".join(out).strip()
    finally:
        s.close()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("command")
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", type=int, required=True)
    ap.add_argument("--pass", dest="pw", required=True)
    a = ap.parse_args()
    print(f"[bercon] {a.host}:{a.port} (udp) <- {a.command!r}")
    try:
        r = exec_cmd(a.host, a.port, a.pw, a.command)
        print(f"[bercon] response: {r!r}" if r else "[bercon] (empty response — command accepted)")
    except Exception as e:
        sys.exit(f"[bercon] error: {e}")


if __name__ == "__main__":
    main()
