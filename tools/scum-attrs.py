#!/usr/bin/env python3
"""scum-attrs.py — read / set a SCUM prisoner's BASE attributes directly in SCUM.db.

Bypasses the in-game `#SetAttributes` admin command, which is gated above the
SuperAdmin/owner executor level (EExecutorStatus: Regular<Admin<SuperAdmin<
Elevated<Developer) and so returns "not authorized" even for the server owner.

Attributes live as UE DoubleProperty values inside the `prisoner.body_simulation`
blob — UE class `PrisonerBodySimulationSave` (the in-game "Metabolism" struct):
  BaseStrength  BaseConstitution  BaseDexterity  BaseIntelligence
Property layout: [i32 namelen][name\\0][i32 typelen]"DoubleProperty\\0"[i64 size][1b guid][f64 value]
Offsets are stable across prisoners (header is fixed) but we re-derive them per
blob by name to be safe.

⚠ The SCUM server MUST be stopped before writing. It holds the prisoner in memory
and rewrites the blob on save/logout, clobbering live edits. Procedure:
  stop server -> (pull db+wal+shm together so sqlite applies WAL) -> set ->
  wal_checkpoint(TRUNCATE) -> push db, delete server's stale wal/shm -> start.

Usage:
  scum-attrs.py read  <SCUM.db> [prisoner_id|all]
  scum-attrs.py set   <SCUM.db> <prisoner_id> str con dex int   # e.g. 8 5 5 5
  scum-attrs.py whois <SCUM.db>                                  # id -> name/steam
"""
import sqlite3, struct, sys

ATTRS = ('BaseStrength', 'BaseConstitution', 'BaseDexterity', 'BaseIntelligence')

def val_offset(blob, prop):
    """Return byte offset of the f64 value for DoubleProperty `prop` in `blob`."""
    name = prop.encode('ascii')
    i = blob.find(name)
    if i < 0: raise ValueError(f'{prop} not found in blob')
    pos = i + len(name) + 1                                  # past name + null
    typelen = struct.unpack_from('<i', blob, pos)[0]; pos += 4
    typ = blob[pos:pos+typelen-1].decode('ascii'); pos += typelen
    if typ != 'DoubleProperty': raise ValueError(f'{prop} is {typ}')
    pos += 8                                                  # i64 size
    pos += 1                                                  # guid flag byte
    return pos

def read_attrs(blob):
    return {a: struct.unpack_from('<d', blob, val_offset(blob, a))[0] for a in ATTRS}

def whois(cur):
    rows = cur.execute('SELECT prisoner_id,name,user_id FROM user_profile WHERE prisoner_id IS NOT NULL ORDER BY prisoner_id').fetchall()
    return {r[0]: (r[1], r[2]) for r in rows}

def cmd_read(db, who):
    con = sqlite3.connect(db); cur = con.cursor()
    names = whois(cur)
    ids = list(names) if who in (None, 'all') else [int(who)]
    for pid in ids:
        row = cur.execute('SELECT body_simulation FROM prisoner WHERE id=?', (pid,)).fetchone()
        if not row: print(f'prisoner {pid}: not found'); continue
        nm = names.get(pid, ('?', '?'))
        v = read_attrs(bytes(row[0]))
        print(f'prisoner {pid:<3} {nm[0]:<18} {nm[1]:<18} ' +
              ' '.join(f'{k.replace("Base",""):3}={v[k]:.2f}' for k in ATTRS))
    con.close()

def cmd_set(db, pid, s, c, d, i):
    pid = int(pid); tgt = dict(zip(ATTRS, (float(s), float(c), float(d), float(i))))
    con = sqlite3.connect(db); cur = con.cursor()
    row = cur.execute('SELECT body_simulation FROM prisoner WHERE id=?', (pid,)).fetchone()
    if not row: print(f'prisoner {pid}: not found'); sys.exit(1)
    blob = bytearray(row[0])
    for a, nv in tgt.items():
        off = val_offset(blob, a)
        old = struct.unpack_from('<d', blob, off)[0]
        struct.pack_into('<d', blob, off, nv)
        print(f'  {a:18} {old:7.4f} -> {nv:.1f}  @off {off}')
    cur.execute('UPDATE prisoner SET body_simulation=? WHERE id=?', (bytes(blob), pid))
    con.commit()
    con.execute('PRAGMA wal_checkpoint(TRUNCATE)'); con.commit()
    v = read_attrs(bytes(cur.execute('SELECT body_simulation FROM prisoner WHERE id=?', (pid,)).fetchone()[0]))
    print('  verify: ' + ' '.join(f'{k.replace("Base","")}={v[k]:.1f}' for k in ATTRS))
    con.close()

def cmd_whois(db):
    con = sqlite3.connect(db); cur = con.cursor()
    for pid, (nm, uid) in whois(cur).items():
        print(f'  prisoner {pid:<3} {nm:<20} {uid}')
    con.close()

if __name__ == '__main__':
    a = sys.argv
    if len(a) < 3: print(__doc__); sys.exit(1)
    op = a[1]
    if   op == 'read':  cmd_read(a[2], a[3] if len(a) > 3 else 'all')
    elif op == 'set':   cmd_set(a[2], a[3], a[4], a[5], a[6], a[7])
    elif op == 'whois': cmd_whois(a[2])
    else: print(__doc__); sys.exit(1)
