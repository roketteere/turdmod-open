# Generate scummymap.com deploy SQL: (1) town POIs on the default map, (2) the
# "ScummyMap Official" map row with a zone_overrides overlay built from the SAME
# zone definitions used by build-all-zones.py (the live SCUM.db writer).
# @inv: pois served from the LATEST extractor dataset only (apps/api .../pois.ts);
#       town pois MUST carry that dataset_id or they never surface.
# @inv: a custom map shows UNION(default-map pois, its own) -> towns on the default
#       map (...0001) appear on the official map too; only zones are per-map.
# @inv: zone fill comes from kind's palette (4 kinds: safe/pve/pvp/kos). "NO BUILD"
#       rides in the zone NAME (renders in the popup). SCUM half-extents -> full cm.
import json, os, sys

HERE = os.path.dirname(os.path.abspath(__file__))
TOWNS = json.load(open(os.path.join(HERE, 'data', 'scum-towns.json')))

DATASET_ID = '8cee6b1d-b43b-4557-b070-b5ee8c82a4b8'  # extracted-v23128448 (active)
DEFAULT_MAP = '00000000-0000-0000-0000-000000000001'
OFFICIAL_MAP = '00000000-0000-0000-0000-000000000002'

# ── zone defs (verbatim from build-all-zones.py) ──────────────────────────────
RED=(0.80,0.08,0.11); ORANGE=(1.0,0.45,0.0); YELLOW=(1.0,0.92,0.0); BLUE=(0.13,0.45,0.95)
PVP=[("A4 War Harbour",497000,-552000,70000),("Zeljava Airfield",505000,555000,65000),
 ("The Airport",-256000,-40000,75000),("C2 Prison & Power",-40000,200000,80000),
 ("Refinery & B3 Mil",175000,-100000,90000),("Weapons Factory",-442000,-682000,60000)]
RAID=[("D0-D2 RAID",-447600,466800,457200,152400)]
RAID_X=(-904800,9600); RAID_Y=(314400,619200)
def in_raid(x,y): return RAID_X[0]<=x<=RAID_X[1] and RAID_Y[0]<=y<=RAID_Y[1]
THE_CITY=("The City",315674,379351,60000)
BUNKERS=[("Bunker Z1",-565002,-723731,16000),("Bunker D1",-543500,544489,16000),
 ("Bunker A1",-341556,-470921,16000),("Bunker C2",-218428,194182,16000),
 ("Bunker A3",227064,-440711,16000),("Bunker C4",448327,269948,16000)]
POIS=[("D0 Military Barracks",-862164,526844,32000),("Z0 TV Base",-698751,-793030,32000),
 ("B1 Factory",-407968,-5143,32000),("Z1 Trainyard",-311561,-754073,30000),
 ("A2 Port",-63859,-500713,30000),("B0 Junkyard",-439981,-102550,35000)]

def hexof(c): return '#%02x%02x%02x' % tuple(round(v*255) for v in c)
def kind_of(color):
    return {RED:'kos', ORANGE:'pvp', YELLOW:'pve', BLUE:'pve'}[color]

zones = {}
n = 0
def add_circle(name, x, y, r, color):
    global n; n += 1
    zones['custom-%d' % n] = {"center":{"x":x,"y":y},"name":name,"faction":"Other",
        "shape":"circle","radiusCm":r,"kind":kind_of(color),"color":hexof(color)}
def add_rect(name, x, y, sx, sy, color):
    global n; n += 1
    zones['custom-%d' % n] = {"center":{"x":x,"y":y},"name":name,"faction":"Other",
        "shape":"rectangle","widthCm":sx*2,"heightCm":sy*2,"radiusCm":sx,
        "kind":kind_of(color),"color":hexof(color)}

for nm,x,y,r in PVP:            add_circle(f"{nm} - PVP / NO BUILD ZONE",x,y,r,RED)
for nm,x,y,hx,hy in RAID:       add_rect(f"{nm} - PVP RAID / BUILD ZONE",x,y,hx,hy,ORANGE)
add_rect(f"{THE_CITY[0]} - PVE / NO BUILD ZONE",THE_CITY[1],THE_CITY[2],THE_CITY[3],THE_CITY[3],BLUE)
for nm,x,y,h in BUNKERS:
    if not in_raid(x,y): add_rect(f"{nm} - PVE / NO BUILD ZONE",x,y,h,h,YELLOW)
for nm,x,y,h in POIS:
    if not in_raid(x,y): add_rect(f"{nm} - PVE / NO BUILD ZONE",x,y,h,h,BLUE)

zone_json = json.dumps(zones, separators=(',',':'))

# ── SQL helpers ───────────────────────────────────────────────────────────────
def sq(s): return "'" + s.replace("'", "''") + "'"

# (1) towns.sql — idempotent: drop prior towns for this dataset, re-insert.
town_lines = [
    "-- ScummyMap: town/city name POIs on the default map (inherited by all maps).",
    f"DELETE FROM pois WHERE category='towns' AND dataset_id='{DATASET_ID}';",
]
for t in TOWNS:
    name = t['name'].replace('_', ' ')
    slug = t['name'].lower().replace(' ', '_')
    sector = (t.get('sector') or '').replace('_', '')  # "D_4" -> "D4"
    meta = json.dumps({"source":"scum-towns","sector":sector,"actors":t.get('actors')}, separators=(',',':'))
    town_lines.append(
        "INSERT INTO pois (id,dataset_id,class_key,display_name,category,sector,pos_x,pos_y,pos_z,map_id,metadata) "
        f"VALUES (gen_random_uuid(),'{DATASET_ID}',{sq('town-'+slug)},{sq(name)},'towns',{sq(sector)},"
        f"{int(t['x'])},{int(t['y'])},0,'{DEFAULT_MAP}',{sq(meta)}::jsonb);")
open(os.path.join(HERE,'..','tmp','scummymap-towns.sql'),'w',encoding='utf-8').write('\n'.join(town_lines)+'\n')

# (2) official-map.sql — upsert the map + its zone overlay.
desc = 'The official www.ScummyMap.com SCUM server. Direct connect YOUR_SERVER_IP:7042. PvP/Raid red+orange, PvE yellow, safe outposts green.'
official_sql = f"""-- ScummyMap Official map (2nd pinned default) + gameplay zone overlay.
INSERT INTO maps (id,code,name,owner_discord_id,accent_color,is_public,zone_overrides,description,website_url)
VALUES ('{OFFICIAL_MAP}','official','ScummyMap Official','','#dc2626',true,{sq(zone_json)}::jsonb,{sq(desc)},'https://www.scummymap.com')
ON CONFLICT (id) DO UPDATE SET
  code=EXCLUDED.code, name=EXCLUDED.name, accent_color=EXCLUDED.accent_color,
  is_public=EXCLUDED.is_public, zone_overrides=EXCLUDED.zone_overrides,
  description=EXCLUDED.description, website_url=EXCLUDED.website_url, updated_at=now();
"""
open(os.path.join(HERE,'..','tmp','scummymap-official.sql'),'w',encoding='utf-8').write(official_sql)

print(f"towns: {len(TOWNS)} rows -> tmp/scummymap-towns.sql")
print(f"zones: {len(zones)} overlay zones -> tmp/scummymap-official.sql")
print("zone kinds:", {k:[z['kind'] for z in zones.values()].count(k) for k in ('safe','pve','pvp','kos')})
print("sample zone:", json.dumps(list(zones.values())[0]))
