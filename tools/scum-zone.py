#!/usr/bin/env python3
# scum-zone.py — author SCUM custom zones directly in SCUM.db (no menu, no player).
# Write while the server is STOPPED, then restart to apply. See docs/CUSTOM-ZONE-FORMAT.md.
#
#   python scum-zone.py <SCUM.db> add-pvp "RedDeath" <x> <y> <radius> [r,g,b]
#   python scum-zone.py <SCUM.db> list
#
# Decoded packing (verified vs live "Global" config 6917529028015773013):
#   handling_methods = (0x6000 << 48) | sum(method << (i*2)) for 15 ECustomZoneEvent
#     method: Ignore=0, Allow=1, Block=2
#   damage_handling_methods (per EDamageActorType) = sum(1 << (ch*2)) for N channels => all-Allow
#   settings = VisibleOnMap(1) | NotificationsOnEntry(2) = 3
import sqlite3, sys

EVENTS = 15
MARKER = 0x6000 << 48
DAMAGE_ACTOR_TYPES = list(range(16))  # EDamageActorType General..CargoDrop

def handling(method):  # uniform method across all 15 events
    v = MARKER
    for i in range(EVENTS):
        v |= (method << (i * 2))
    return v

def all_allow_damage(channels=15):
    v = 0
    for ch in range(channels):
        v |= (1 << (ch * 2))
    return v  # 0x15555555 family

def list_zones(db):
    con = sqlite3.connect(db); con.row_factory = sqlite3.Row; c = con.cursor()
    print("CONFIGS:")
    for r in c.execute("SELECT id,name,color_red,color_green,color_blue,settings FROM custom_zone_configuration ORDER BY id"):
        print(f"  id={r['id']} '{r['name']}' rgb=({r['color_red']:.2f},{r['color_green']:.2f},{r['color_blue']:.2f}) settings={r['settings']}")
    print("REGIONS:")
    for r in c.execute("SELECT id,name,location_x,location_y,size_x,configuration_index FROM custom_zone_region ORDER BY id"):
        print(f"  id={r['id']} '{r['name']}' @({r['location_x']:.0f},{r['location_y']:.0f}) r={r['size_x']:.0f} cfg_idx={r['configuration_index']}")

def add_pvp(db, name, x, y, radius, rgb=(0.8, 0.08, 0.11)):
    con = sqlite3.connect(db); c = con.cursor()
    # New config id (avoid the 1000 global + existing).
    cfg_id = (c.execute("SELECT MAX(id) FROM custom_zone_configuration WHERE id < 1000").fetchone()[0] or 0) + 1
    # configuration_index: regions link to non-global configs by 0-based order of id ascending.
    # Index of our new config = count of existing non-global configs (we're appending).
    cfg_index = c.execute("SELECT COUNT(*) FROM custom_zone_configuration WHERE id < 1000").fetchone()[0]
    hm = handling(1)  # all events Allow => raid/rob/lockpick/build ON (no rules)
    c.execute("INSERT INTO custom_zone_configuration (id,map_id,name,color_red,color_green,color_blue,handling_methods,settings) VALUES (?,?,?,?,?,?,?,?)",
              (cfg_id, 1, name, rgb[0], rgb[1], rgb[2], hm, 3))
    # Damage handling per actor type: Player(1)/Puppet(2)/Vehicle(11)/BaseBuilding(9) => all-Allow (full PvP+raid); rest Ignore(0).
    allow = all_allow_damage()
    pvp_types = {1, 2, 9, 11}
    rid = (c.execute("SELECT MAX(id) FROM custom_zone_configuration_damage_handling_methods").fetchone()[0] or 0)
    for at in DAMAGE_ACTOR_TYPES:
        rid += 1
        c.execute("INSERT INTO custom_zone_configuration_damage_handling_methods (id,custom_zone_configuration_id,map_id,damage_actor_type,damage_handling_methods) VALUES (?,?,?,?,?)",
                  (rid, cfg_id, 1, at, allow if at in pvp_types else 0))
    # Region (circle: size_y=0).
    reg_id = (c.execute("SELECT MAX(id) FROM custom_zone_region").fetchone()[0] or 0) + 1
    c.execute("INSERT INTO custom_zone_region (id,map_id,name,location_x,location_y,size_x,size_y,configuration_index,default_region_name,default_region_state) VALUES (?,?,?,?,?,?,?,?,?,?)",
              (reg_id, 1, name, x, y, radius, 0.0, cfg_index, 'None', 0))
    con.commit()
    print(f"ADDED red PvP zone '{name}' @({x},{y}) r={radius} | config id={cfg_id} idx={cfg_index} handling_methods={hm}")

if __name__ == '__main__':
    db = sys.argv[1]; op = sys.argv[2]
    if op == 'list':
        list_zones(db)
    elif op == 'add-pvp':
        name, x, y, r = sys.argv[3], float(sys.argv[4]), float(sys.argv[5]), float(sys.argv[6])
        rgb = tuple(float(v) for v in sys.argv[7].split(',')) if len(sys.argv) > 7 else (0.8, 0.08, 0.11)
        add_pvp(db, name, x, y, r, rgb)
    else:
        print("ops: list | add-pvp <name> <x> <y> <radius> [r,g,b]")
