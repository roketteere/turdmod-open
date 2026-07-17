import sqlite3, os, re, json
con=sqlite3.connect('scummap/data/extracted/v23396794/catalog.db'); c=con.cursor()
rows=c.execute("""SELECT umap_path, COUNT(*) n, SUM(x) sx, SUM(y) sy
 FROM actors WHERE has_location=1 AND x IS NOT NULL AND ABS(x)<1500000 AND ABS(y)<1500000
 AND umap_path LIKE '%The_Island%' GROUP BY umap_path""").fetchall()
def base(p):
    b=os.path.basename(p)
    b=re.sub(r'_INT$|_Int(erior)?(_\w+)?$','',b)
    b=re.sub(r'_\d+[a-z]?$','',b)
    b=re.sub(r'_[a-z]$','',b)
    return b
# exclude non-settlement infra (these are POIs we either already zoned or aren't towns)
EXCL=('Landscape','Gameplay','Audio','Nav_','VFX','Foliage','Persistent','Spline','Water','The_Island',
 'Safe_Zone','Mine','Radar','Observatory','Junkyard','Mill','Range','Track','Turbine','Saltworks',
 'Motocross','Shooting','Bridge','Shipwreck','Railroad','Sightseeing','Bunker','Abandoned','Military',
 'Trader','Fisherman','Outpost','Airport','Prison','Refinery','Factory','Harbour','Power','Trainyard',
 'Barracks','TV_Base','Weapons','Zeljava','Dam','Cabin','Site','Spawn','Entrance','Cave','Lighthouse',
 'Hospital_Surr','RadarStation','CoalMine','Stone','Saw','Wind','Vineyard','Farm','Surroundings')
acc={}
for p,n,sx,sy in rows:
    b=base(p)
    if any(e.lower() in b.lower() for e in EXCL): continue
    if b not in acc: acc[b]=[0,0,0]
    acc[b][0]+=sx; acc[b][1]+=sy; acc[b][2]+=n
out=[]
for b,(sx,sy,n) in acc.items():
    if n<300: continue
    out.append({"name":re.sub(r'^[A-Z]_\d+_','',b),"sector":b[:3],"x":round(sx/n),"y":round(sy/n),"actors":n})
out.sort(key=lambda t:-t["actors"])
print(f"{len(out)} towns/settlements (centroid):")
for t in out: print(f"  {t['sector']} {t['name']:<22} ({t['x']:>8},{t['y']:>8}) [{t['actors']}]")
json.dump(out, open('turdmod/tmp/towns-extracted.json','w'), indent=2)
