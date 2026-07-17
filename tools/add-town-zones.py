import sqlite3, json, sys
DB=sys.argv[1]
MARKER=0x6000<<48
def hm_nobuild():
    v=MARKER
    for i in (2,13): v|=(2<<(i*2))   # BaseBuilding + BlueprintPlacement = Block
    return v
MAGENTA=(0.73,0.21,0.46)   # Joel's color -> MAJOR cities
CYAN=(0.06,0.63,0.78)      # Joel's color -> minor towns
import os; towns=json.load(open(os.path.join(os.path.dirname(os.path.abspath(__file__)),'data','scum-towns.json')))
SKIP=('Nuclear','Radio','Sanitarium','Castle','Medsave','Airfield','Observatory','Hospital','Block','Surround','Reactor')
towns=[t for t in towns if not any(s.lower() in t['name'].lower() for s in SKIP)]
MAJOR=towns[:10]; MINOR=towns[10:35]
con=sqlite3.connect(DB); c=con.cursor()
# remove Joel's placeholder configs (>=19) + their regions, and any non-default region at cfg_index>=18
old=[r[0] for r in c.execute("SELECT id FROM custom_zone_configuration WHERE id>=19 AND id<1000")]
for cid in old: c.execute("DELETE FROM custom_zone_configuration_damage_handling_methods WHERE custom_zone_configuration_id=?",(cid,))
c.execute("DELETE FROM custom_zone_configuration WHERE id>=19 AND id<1000")
c.execute("DELETE FROM custom_zone_region WHERE configuration_index>=18 AND default_region_state=0")
hm=hm_nobuild()
rid_dmg=(c.execute("SELECT MAX(id) FROM custom_zone_configuration_damage_handling_methods").fetchone()[0] or 0)
reg_id=(c.execute("SELECT MAX(id) FROM custom_zone_region").fetchone()[0] or 0)
state={'cid':19,'idx':18}
def add(name,x,y,half,color,tier):
    full=f"{name} - NO BUILD ({tier})"
    cid=state['cid']; idx=state['idx']
    c.execute("INSERT INTO custom_zone_configuration (id,map_id,name,color_red,color_green,color_blue,handling_methods,settings) VALUES (?,?,?,?,?,?,?,?)",
              (cid,1,full,color[0],color[1],color[2],hm,3))
    nonlocal_dmg=rid_dmg
    for at in range(16):
        state.setdefault('d',rid_dmg)
    return cid,idx,full
# simpler: inline loop
for tier,group,half,color in (('CITY',MAJOR,55000,MAGENTA),('TOWN',MINOR,40000,CYAN)):
    for t in group:
        cid=state['cid']; idx=state['idx']
        full=f"{t['name']} - NO BUILD ({tier})"
        c.execute("INSERT INTO custom_zone_configuration (id,map_id,name,color_red,color_green,color_blue,handling_methods,settings) VALUES (?,?,?,?,?,?,?,?)",
                  (cid,1,full,color[0],color[1],color[2],hm,3))
        for at in range(16):
            rid_dmg+=1
            c.execute("INSERT INTO custom_zone_configuration_damage_handling_methods (id,custom_zone_configuration_id,map_id,damage_actor_type,damage_handling_methods) VALUES (?,?,?,?,?)",(rid_dmg,cid,1,at,0))
        reg_id+=1
        c.execute("INSERT INTO custom_zone_region (id,map_id,name,location_x,location_y,size_x,size_y,configuration_index,default_region_name,default_region_state) VALUES (?,?,?,?,?,?,?,?,?,?)",
                  (reg_id,1,full,t['x'],t['y'],half,half,idx,'None',0))
        state['cid']+=1; state['idx']+=1
con.commit(); con.execute("PRAGMA wal_checkpoint(TRUNCATE)")
print(f"added {len(MAJOR)} magenta CITY + {len(MINOR)} cyan TOWN zones")
print("major:", ", ".join(t['name'] for t in MAJOR))
