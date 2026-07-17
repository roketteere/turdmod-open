import sqlite3, sys
DB = sys.argv[1]
MARKER = 0x6000 << 48
def hm_pvp_nobuild():   # all Allow(1) except BaseBuilding(2)+BlueprintPlacement(13)=Block(2)
    v = MARKER
    for i in range(15): v |= ((2 if i in (2,13) else 1) << (i*2))
    return v
def hm_nobuild():       # all Ignore(0) except 2,13 = Block(2)
    v = MARKER
    for i in (2,13): v |= (2 << (i*2))
    return v
GOLD = (1.0, 0.84, 0.0)   # gold/yellow for major loot POIs + bunkers (boxed)

NOBUILD = [  # (name, x, y, radius)
    ("Bunker Z1", -565002.0, -723731.0, 45000.0),
    ("Bunker D1", -543500.0,  544489.0, 45000.0),
    ("Bunker A1", -341556.0, -470921.0, 45000.0),
    ("Bunker C2", -218428.0,  194182.0, 45000.0),
    ("Bunker A3",  227064.0, -440711.0, 45000.0),
    ("Bunker C4",  448327.0,  269948.0, 45000.0),
    ("D0 Military Barracks", -862164.0, 526844.0, 60000.0),
    ("Z0 TV Base",           -698751.0,-793030.0, 60000.0),
    ("B1 Factory",           -407968.0,  -5143.0, 60000.0),
    ("Z1 Trainyard",         -311561.0,-754073.0, 60000.0),
    ("A2 Port",               -63859.0,-500713.0, 60000.0),
]

con = sqlite3.connect(DB); c = con.cursor()

# 1) PvP zones (ids 2-7): rename + add no-build to handling
pvp_hm = hm_pvp_nobuild()
for cid in range(2, 8):
    row = c.execute("SELECT name FROM custom_zone_configuration WHERE id=?", (cid,)).fetchone()
    if not row: continue
    nm = row[0]
    if " - " not in nm: nm = nm + " - PVP ZONE"
    c.execute("UPDATE custom_zone_configuration SET name=?, handling_methods=? WHERE id=?", (nm, pvp_hm, cid))
    c.execute("UPDATE custom_zone_region SET name=? WHERE configuration_index=? AND default_region_state=0",
              (nm, cid-1))

# 2) Outposts (id 1, regions): rename "- SAFE ZONE"
c.execute("UPDATE custom_zone_configuration SET name='Outposts - SAFE ZONE' WHERE id=1")
for r in c.execute("SELECT id,name FROM custom_zone_region WHERE configuration_index=0").fetchall():
    rid, rnm = r
    if " - " not in rnm:
        c.execute("UPDATE custom_zone_region SET name=? WHERE id=?", (rnm + " - SAFE ZONE", rid))

# 3) clear any prior no-build configs (id>=8) then add fresh
old = [r[0] for r in c.execute("SELECT id FROM custom_zone_configuration WHERE id>=8 AND id<1000")]
for cid in old:
    c.execute("DELETE FROM custom_zone_configuration_damage_handling_methods WHERE custom_zone_configuration_id=?", (cid,))
c.execute("DELETE FROM custom_zone_configuration WHERE id>=8 AND id<1000")
c.execute("DELETE FROM custom_zone_region WHERE configuration_index>=7 AND default_region_state=0")

nb_hm = hm_nobuild()
rid_dmg = (c.execute("SELECT MAX(id) FROM custom_zone_configuration_damage_handling_methods").fetchone()[0] or 0)
reg_id  = (c.execute("SELECT MAX(id) FROM custom_zone_region").fetchone()[0] or 0)
for n,(name,x,y,r) in enumerate(NOBUILD):
    cid = 8 + n
    cfg_index = 7 + n
    full = name + " - NO BUILD"
    c.execute("INSERT INTO custom_zone_configuration (id,map_id,name,color_red,color_green,color_blue,handling_methods,settings) VALUES (?,?,?,?,?,?,?,?)",
              (cid,1,full,GOLD[0],GOLD[1],GOLD[2],nb_hm,3))
    for at in range(16):
        rid_dmg += 1
        c.execute("INSERT INTO custom_zone_configuration_damage_handling_methods (id,custom_zone_configuration_id,map_id,damage_actor_type,damage_handling_methods) VALUES (?,?,?,?,?)",
                  (rid_dmg,cid,1,at,0))  # 0 = ignore (no PvP change)
    reg_id += 1
    c.execute("INSERT INTO custom_zone_region (id,map_id,name,location_x,location_y,size_x,size_y,configuration_index,default_region_name,default_region_state) VALUES (?,?,?,?,?,?,?,?,?,?)",
              (reg_id,1,full,x,y,r,r,cfg_index,'None',0))  # size_y=r => SQUARE box

con.commit()
con.execute("PRAGMA wal_checkpoint(TRUNCATE)")
print("=== configs after ===")
for r in c.execute("SELECT id,name,color_red,color_green,color_blue FROM custom_zone_configuration ORDER BY id"):
    print(f"  id={r[0]:<4} {r[1]:<32} rgb=({r[2]:.2f},{r[3]:.2f},{r[4]:.2f})")
print(f"=== regions: {c.execute('SELECT COUNT(*) FROM custom_zone_region').fetchone()[0]} total ===")
