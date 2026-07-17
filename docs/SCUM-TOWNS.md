# SCUM Towns / Settlements — extracted coordinates

**120 settlements** with game-cm centroids, extracted 2026-06-07 from the scummap
ScummyExtractor catalog (`scummap/data/extracted/v23396794/catalog.db`). These were NOT
in `vanilla-zones.json` (which only had military/bunker/trader POIs) — confirmed absent
before extracting, per the "don't redo" rule.

## How (reproducible)
`tools/extract-scum-towns.py` → `tools/data/scum-towns.json`. SCUM encodes locations in
umap sub-level **paths** (`Maps/The_Island/A_2_Tisno`, `A_1_Apatija_3`, …). The script:
1. groups `actors` (1M+ rows) by umap_path, summing x/y for a centroid;
2. consolidates sub-levels to a base town name (strips `_N`, `_Interior`, `_INT`);
3. excludes infra (mines, splines, water, military/bunker/trader already zoned);
4. keeps settlements with ≥300 located actors.

Re-run on each SCUM map update (point at the newest `data/extracted/<ver>/catalog.db`).

## Data
`tools/data/scum-towns.json` — `[{name, sector, x, y, actors}]`, sorted by size.
Top towns: Samobor, Novigrad, Prvo_Selo, Rogoznica, Preko, Ston, Tisno, Vrsar,
Spickovina, Prkno, Gornji_Humac, Porat, Mirkovci, Veliki_Tabor, Zaton, Lokve, Blato…

## Use
Drives the second-tier (non-major-city) no-build custom zones — see
`tools/scum-zone-labels-nobuild.py` + `docs/CUSTOM-ZONE-FORMAT.md`. Coords are game-cm,
same system as the custom_zone_region location_x/y.
