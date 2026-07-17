# SCUM Custom Zone Format — fully cracked (build v23451409)

The in-game **Server Settings → Custom Zones** admin menu persists everything to **3 tables in
`SCUM.db`** (the SQLite save). No JSON config file. Captured + decoded 2026-06-07 by having an admin
draw one zone + Apply, then diffing the DB. **We can author zones/markers programmatically by writing
these rows — no menu, no player.**

> @inv: write while the server is STOPPED (clean), then start → server loads zones on boot. SCUM.db is
> WAL-mode + locked live; live writes risk corruption. @dep: scumdb readers (TOOL-REGISTRY).

## Tables

### `custom_zone_region` — geometry / map placement
| col | meaning |
|---|---|
| `id`, `map_id` | row id; map_id=1 |
| `name` | region label |
| `location_x`, `location_y` | center (SCUM world cm) |
| `size_x`, `size_y` | radius (circle: `size_y=0`) or half-extents (rectangle) — see `ECustomZoneShape` |
| `configuration_index` | which `custom_zone_configuration` it uses (links by config index, not id) |
| `default_region_name`, `default_region_state` | for built-in regions (`EDefaultCustomZoneState`: NotDefault=0/Unmodified=1/Modified=2/Deleted=3) |

### `custom_zone_configuration` — identity, color, behavior
| col | meaning |
|---|---|
| `id`, `map_id` | config id (1000 = "Global configuration" baseline) |
| `name` | config name |
| `color_red/green/blue` | float 0..1 RGB. **RED zone = red≈0.8, green≈0.08, blue≈0.11** (the live "Zone 1" config) |
| `settings` | bitfield of `ECustomZoneSetting`: VisibleOnMap=1, NotificationsOnEntry=2 → **3 = both on** |
| `handling_methods` | packed per-event rules (below) |

### `custom_zone_configuration_damage_handling_methods` — PvP / damage
| col | meaning |
|---|---|
| `custom_zone_configuration_id` | parent config |
| `damage_actor_type` | `EDamageActorType` (0=General,1=Player,2=Puppet,…,9=BaseBuilding,11=Vehicle,…) |
| `damage_handling_methods` | packed per-channel Allow/Block (below) |

## Bit-packing (decoded from live captures)

- **`handling_methods`** = **2 bits per event**, 15 events of `ECustomZoneEvent`, low 30 bits; plus a
  constant `0x6000` marker at bit 48. Per-event value = `ECustomZoneEventHandlingMethod`
  **Ignore=0, Allow=1, Block=2**. `field(event_i) = (hm >> (i*2)) & 0x3`.
  - Events order: `PlayerLockpicking, WorldLockpicking, BaseBuilding, FlagOvertake, VehicleParking,
    AvailabilityGrid, ChestParking, DropshipEncounterSpawning, AutoCloseVehicleDoorsOnExit,
    DisablingGeneral, DisablingSentry, DisablingPlayerElectronics, PlayerAntiBlockPhasing,
    BlueprintPlacement, BaseDecay`.
  - **No-rules PvP zone** = all events **Allow** (raid/rob/lockpick/build ON). **Safe zone** = all
    **Block** (live "Outposts" config does exactly this).
- **`damage_handling_methods`** = per `damage_actor_type` row, **2 bits per damage channel**, value
  Allow/Block/Ignore. Live "vs Player" = `0x15555555` = every channel **Allow** = full PvP.
  Block it = no PvP (safe zone). `0x0` = Ignore (use default).
- **`settings`** = `ECustomZoneSetting` bitfield (VisibleOnMap | NotificationsOnEntry).

## Worked example — a RED no-rules PvP zone over a POI
1. INSERT `custom_zone_configuration`: `name='RedDeath'`, `color_red=0.8, color_green=0.08,
   color_blue=0.11`, `settings=3`, `handling_methods=` (all 15 events Allow → `0x6000` | sum of
   `1<<(i*2)` for all i) .
2. INSERT 16 `custom_zone_configuration_damage_handling_methods` rows (one per `EDamageActorType`),
   Player + BaseBuilding + Vehicle = all-Allow (`0x15555555`-style) for full PvP/raid.
3. INSERT `custom_zone_region`: `name='RedDeath'`, `location_x/y=` POI center, `size_x=` radius,
   `size_y=0` (circle), `configuration_index=` the new config's index.
4. Restart server → red no-rules PvP zone live + visible on map.

## What this controls (and doesn't)
- ✅ Per-zone **PvP** (damage handling), **raid/rob** (lockpicking + flag-overtake + building),
  **encounter spawning** (Dropship), **color**, **map visibility**, **map markers** (regions ARE the
  markers).
- ❌ Loot **density/quality** per zone — that's the **separate** `ItemSpawnerVolume` system
  (`zone, ItemHealthMultiplier, ProbabilityMultiplier`). `ItemSpawnerVolume.zone` may link to these
  custom zones — open thread.

## Next: build the zone-writer
A tool/handler that INSERTs these rows (region + config + 16 damage rows) from params
(name, x, y, radius, color, pvp on/off, raid on/off). Write server-off, restart. Then "drop a red
PvP zone over the airfield" is one command — no menu, no player.

_Cracked 2026-06-07. Source: live SCUM.db capture (admin drew 1 zone) + scumdump v23451409 enums._
