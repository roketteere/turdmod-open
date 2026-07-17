#!/usr/bin/env python3
# a2s_spoof.py — fake the in-game/server-browser population by rewriting the Steam A2S
# query responses in-flight. SCUM reports the real connected-player count in A2S_INFO on
# the Steam query port (27015 by default — Steam-controlled, not the -QueryPort arg), so
# a simple proxy can't sit in front of it. WinDivert intercepts the outbound A2S_INFO
# packets, rewrites the "players" byte up to a floor, and re-injects.
#
# @inv: only the A2S RESPONSE players byte is touched — real players are never hidden
#   (we take max(floor, real)). Game traffic (7777) + the actual server are untouched.
# @ctx: population display tactic. Run elevated (WinDivert loads a kernel driver).
# Usage:  python a2s_spoof.py [floor] [queryPort]   (defaults: 50  27015)

import sys
import pydivert

FLOOR = int(sys.argv[1]) if len(sys.argv) > 1 else 50
QUERY_PORT = int(sys.argv[2]) if len(sys.argv) > 2 else 27015
A2S_INFO_REPLY = b"\xFF\xFF\xFF\xFF\x49"  # 0x49 = 'I', Source A2S_INFO response header

filt = f"udp.SrcPort == {QUERY_PORT}"
print(f"[a2s-spoof] floor={FLOOR} port={QUERY_PORT} filter='{filt}' — rewriting A2S_INFO players byte", flush=True)

def players_offset(pl: bytes) -> int:
    # header(4) + type(1) + protocol(1) = 6, then 4 null-terminated strings
    # (name, map, folder, game), then appID(2 bytes) -> Players byte.
    p = 6
    for _ in range(4):
        p = pl.index(b"\x00", p) + 1
    return p + 2

rewritten = 0
with pydivert.WinDivert(filt) as w:
    for packet in w:
        pl = packet.payload
        if len(pl) > 12 and pl[:5] == A2S_INFO_REPLY:
            try:
                off = players_offset(pl)
                players = pl[off]
                newp = max(FLOOR, players)
                if newp != players:
                    b = bytearray(pl)
                    b[off] = newp
                    packet.payload = bytes(b)
                    rewritten += 1
                    if rewritten <= 5 or rewritten % 25 == 0:
                        print(f"[a2s-spoof] players {players} -> {newp}  (rewrites: {rewritten})", flush=True)
            except Exception as e:
                print(f"[a2s-spoof] parse error: {e}", flush=True)
        w.send(packet)
