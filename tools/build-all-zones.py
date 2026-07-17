import sqlite3, json, sys, os
DB=sys.argv[1]
MARKER=0x6000<<48
def hm(allow_all=False, nobuild=False, raid=False):
    v=MARKER
    if raid or allow_all:
        for i in range(15): v|=(1<<(i*2))          # everything Allow (build+raid+pvp)
    elif nobuild:
        for i in (2,13): v|=(2<<(i*2))              # block BaseBuilding + BlueprintPlacement
    return v
def hm_pvp_circle():                                 # PvP on, building blocked (no camping)
    v=MARKER
    for i in range(15): v|=((2 if i in (2,13) else 1)<<(i*2))
    return v
PVP_TYPES={1,2,9,11}
def dmg_allow():
    v=0
    for ch in range(15): v|=(1<<(ch*2))
    return v
# colors
RED=(0.80,0.08,0.11); ORANGE=(1.0,0.45,0.0); YELLOW=(1.0,0.92,0.0); BLUE=(0.13,0.45,0.95)

# tight PvP POI circles (radius)
PVP=[("A4 War Harbour",497000,-552000,70000),("Zeljava Airfield",505000,555000,65000),
 ("The Airport",-256000,-40000,75000),("C2 Prison & Power",-40000,200000,80000),
 ("Refinery & B3 Mil",175000,-100000,90000),("Weapons Factory",-442000,-682000,60000)]
# Raid/PVP rectangle (build+raid+pvp ON) = all of D0+D1+D2 (D row, cols 0-2).
# x[-904800,9600] y[314400,619200] -> center(-447600,466800) half(457200,152400).
RAID=[("D0-D2 RAID",-447600,466800,457200,152400)]
RAID_X=(-904800,9600); RAID_Y=(314400,619200)
def in_raid(x,y): return RAID_X[0]<=x<=RAID_X[1] and RAID_Y[0]<=y<=RAID_Y[1]
# "The City" (= Samobor area, what players call it) — blue no-build, good loot.
THE_CITY=("The City",315674,379351,60000)
# bunkers (yellow, tight boxes, half-extent)
BUNKERS=[("Bunker Z1",-565002,-723731,16000),("Bunker D1",-543500,544489,16000),
 ("Bunker A1",-341556,-470921,16000),("Bunker C2",-218428,194182,16000),
 ("Bunker A3",227064,-440711,16000),("Bunker C4",448327,269948,16000)]
# blue no-build POIs (loot POIs + junkyard), tight boxes
POIS=[("D0 Military Barracks",-862164,526844,32000),("Z0 TV Base",-698751,-793030,32000),
 ("B1 Factory",-407968,-5143,32000),("Z1 Trainyard",-311561,-754073,30000),
 ("A2 Port",-63859,-500713,30000),("B0 Junkyard",-439981,-102550,35000)]
towns=json.load(open(os.path.join(os.path.dirname(os.path.abspath(__file__)),'data','scum-towns.json')))
SKIP=('Nuclear','Radio','Sanitarium','Castle','Medsave','Airfield','Observatory','Hospital','Block','Surround','Reactor','Junkyard','Samobor')
towns=[t for t in towns if not any(s.lower() in t['name'].lower() for s in SKIP)][:35]

con=sqlite3.connect(DB); c=con.cursor()
# wipe all non-default, non-global configs + their damage rows + non-default regions
for cid in [r[0] for r in c.execute("SELECT id FROM custom_zone_configuration WHERE id NOT IN (1,1000)")]:
    c.execute("DELETE FROM custom_zone_configuration_damage_handling_methods WHERE custom_zone_configuration_id=?",(cid,))
c.execute("DELETE FROM custom_zone_configuration WHERE id NOT IN (1,1000)")
c.execute("DELETE FROM custom_zone_region WHERE default_region_state=0")
rid_dmg=(c.execute("SELECT MAX(id) FROM custom_zone_configuration_damage_handling_methods").fetchone()[0] or 0)
reg_id=(c.execute("SELECT MAX(id) FROM custom_zone_region").fetchone()[0] or 0)
cid=2; idx=1; allow=dmg_allow()
def emit(name,x,y,sx,sy,color,handling,pvp_damage):
    global cid,idx,rid_dmg,reg_id
    c.execute("INSERT INTO custom_zone_configuration (id,map_id,name,color_red,color_green,color_blue,handling_methods,settings) VALUES (?,?,?,?,?,?,?,?)",
              (cid,1,name,color[0],color[1],color[2],handling,3))
    for at in range(16):
        rid_dmg+=1
        val=(allow if (pvp_damage and at in PVP_TYPES) else 0)
        c.execute("INSERT INTO custom_zone_configuration_damage_handling_methods (id,custom_zone_configuration_id,map_id,damage_actor_type,damage_handling_methods) VALUES (?,?,?,?,?)",(rid_dmg,cid,1,at,val))
    reg_id+=1
    c.execute("INSERT INTO custom_zone_region (id,map_id,name,location_x,location_y,size_x,size_y,configuration_index,default_region_name,default_region_state) VALUES (?,?,?,?,?,?,?,?,?,?)",
              (reg_id,1,name,x,y,sx,sy,idx,'None',0))
    cid+=1; idx+=1
# Every zone name carries BOTH its PvP/PvE status AND its build status, so the
# entry notification reads e.g. "Tisno - PVE / NO BUILD ZONE".
# no-build zones inside the raid rectangle are skipped (the raid area is buildable).
for n,x,y,r in PVP:               emit(f"{n} - PVP / NO BUILD ZONE",x,y,r,0,RED,hm_pvp_circle(),True)
for n,x,y,hx,hy in RAID:          emit(f"{n} - PVP RAID / BUILD ZONE",x,y,hx,hy,ORANGE,hm(raid=True),True)
emit(f"{THE_CITY[0]} - PVE / NO BUILD ZONE",THE_CITY[1],THE_CITY[2],THE_CITY[3],THE_CITY[3],BLUE,hm(nobuild=True),False)
skipped=0
for n,x,y,h in BUNKERS:
    if in_raid(x,y): skipped+=1; continue
    emit(f"{n} - PVE / NO BUILD ZONE",x,y,h,h,YELLOW,hm(nobuild=True),False)
for n,x,y,h in POIS:
    if in_raid(x,y): skipped+=1; continue
    emit(f"{n} - PVE / NO BUILD ZONE",x,y,h,h,BLUE,hm(nobuild=True),False)
for t in towns:
    if in_raid(t['x'],t['y']): skipped+=1; continue
    emit(f"{t['name']} - PVE / NO BUILD ZONE",t['x'],t['y'],28000,28000,BLUE,hm(nobuild=True),False)
# rename the default Outpost safe zones to carry both labels too
c.execute("UPDATE custom_zone_configuration SET name='Outposts - SAFE / NO BUILD ZONE' WHERE id=1")
for rid,rnm in c.execute("SELECT id,name FROM custom_zone_region WHERE configuration_index=0").fetchall():
    base=rnm.split(' - ')[0]
    c.execute("UPDATE custom_zone_region SET name=? WHERE id=?", (base+" - SAFE / NO BUILD ZONE", rid))
print(f"skipped {skipped} no-build zones inside the D0-D2 raid area")
con.commit(); con.execute("PRAGMA wal_checkpoint(TRUNCATE)")
print(f"built: {len(PVP)} PvP + {len(RAID)} raid + {len(BUNKERS)} yellow bunkers + {len(POIS)} blue POIs(+junkyard) + {len(towns)} blue towns")
print("regions:", c.execute("SELECT COUNT(*) FROM custom_zone_region").fetchone()[0])
