# Refresh TMM's item icons from a fresh ScummyExtractor run. Converts the extractor's long
# asset-path PNG names to the short ico_<exportname>.png convention TMM expects (ItemIcon.tsx +
# item-icons.ts), thumbnailed to 64px (parity with the prior set), and rewrites _index.json.
# Usage: python refresh-item-icons.py <extracted-icons-dir> <tmm-item-icons-dir>
import json, os, sys
from PIL import Image

SRC = sys.argv[1] if len(sys.argv) > 1 else r"C:\Development\Claude\scummap\data\extracted\v23623039\icons"
DST = sys.argv[2] if len(sys.argv) > 2 else r"C:\Development\Claude\turdmod\apps\turdmod-manager\public\item-icons"

idx = json.load(open(os.path.join(SRC, "_index.json"), encoding="utf-8"))
os.makedirs(DST, exist_ok=True)
# Clear the old set (we rebuild it + _index.json from the fresh extraction).
for f in os.listdir(DST):
    try: os.remove(os.path.join(DST, f))
    except OSError: pass

new_idx, written, missing = [], 0, 0
for e in idx:
    exp, fil = e.get("exportName", ""), e.get("file", "")
    if not exp or not fil:
        continue
    short = exp.lower() + ".png"                       # ICO_AK47_Strap -> ico_ak47_strap.png
    src_file = os.path.join(SRC, os.path.basename(fil))
    if not os.path.exists(src_file):
        missing += 1
        continue
    try:
        img = Image.open(src_file).convert("RGBA")
        img.thumbnail((64, 64), Image.LANCZOS)          # keep aspect, fit within 64x64
        img.save(os.path.join(DST, short), "PNG")
        new_idx.append({"assetPath": e.get("assetPath", ""), "exportName": exp,
                        "width": img.width, "height": img.height, "file": "icons/" + short})
        written += 1
    except Exception as ex:
        missing += 1

json.dump(new_idx, open(os.path.join(DST, "_index.json"), "w", encoding="utf-8"), indent=1)
print(f"wrote {written} icons, {missing} skipped; _index.json {len(new_idx)} entries")
