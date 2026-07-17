# World-Asset Catalog — capturable in-world SCUM assets for spawn-anywhere deployables

Captured **offline** from the SCUM client paks (no server, no crashes) on 2026-06-22.
Source: `UnrealPak -List` across `pakchunk0-WindowsNoEditor.pak` + patch paks `pakchunk0_s1..s17`
(AES-encrypted; key in `tmp/crypto.json`). Full listing: 68,029 assets (`tmp/all-client-assets.txt`).

**The `_ES` (Entity System) variants are the FUNCTIONAL versions** — spawn those for working
deployables (usable gas pump / fridge), not the bare `SM_` static meshes.

## Gas stations / fuel pumps
| Asset | In-pak path | Notes |
|---|---|---|
| `SM_AC_GasStation_01` | `Gas_station/AC_Gas_Station/` | AC station mesh |
| `BP_AC_Gas_Station_01` | `Gas_station/` | AC station BP |
| `SM_Continental_Gas_Station_01` | `Gas_station/Continental_Gas_Station_01/Objects/` | mesh |
| **`SM_Continental_Gas_Station_01_ES`** | `Gas_station/Continental_Gas_Station_01/Objects/` | **functional (entity-system)** |
| `BP_Cont_Gas_Station_01_EXT_*` | `Gas_station/Continental_Gas_Station_01/` | MASTER + CHILD_01/02/03 |
| `BP_AirfieldFuelStation` + `SM_AirfieldFuelStation` (+`_part1`) | `Airfield/A_4_Airfield/AirfieldFuelStation/` | airfield pump; `MI_GasPumpAirfield` material |
| `BP_River_Pier_GasStation_01` | `Gas_station/` | river pier variant |
| Sea / Stone gas stations | `Gas_station/Sea_Gas_Station_01_RW/`, `Gas_station/Stone_Gas_Station/` | more variants |

## Large refrigerators (in-building / town)
| Asset | In-pak path | Notes |
|---|---|---|
| **`SM_SuperMarket_Refrigerator`** (+`_02`) | `City/SuperMarket_Large/Objects/` | the big supermarket fridges |
| **`Refrigerator_ES`** | `Items/Equipment/` | **functional deployable fridge** |
| `Refrigerator`, `Refrigerator_Portable_Small`(+`_ES`), `Refrigerator_Unusable` | `Items/Equipment/` | item/deployable forms |
| `SM_Fridge_Portable_MERGED` | `Items/Fridge_Portable/` | portable fridge mesh |
| `SM_Refrigerator01_Closed_Ruined` | `Items/Soviet_kitchen/Meshes/` | ruined kitchen fridge |

SDK classes (scumdump `SCUM_classes.hpp`): `URefrigerator_ES_C`, `URefrigerator_Portable_Small_ES_C`,
`ARefrigeratorItem`, `ABP_*_Gas_Station_*_C`, `ABP_AirfieldFuelStation_C`.

## Next steps (not yet done)
1. Resolve the exact `/Game/...` mount path per asset (check pak mount point; the s-paks list bare
   top-level folders like `Gas_station/...`). SCUM convention is usually `/Game/ConZ_Files/...`.
2. Spawn = ONE safe bridge call: `loadAsset` the `_ES` class → `BeginDeferredActorSpawnFromClass` at a
   target transform. **Never** use `getNearbyActors`/bulk scans on the live server — they ProcessEvent-
   storm the game thread and crash it.
3. Persist placements + re-spawn on world-load (death-marker pattern); gate by premium tier.
