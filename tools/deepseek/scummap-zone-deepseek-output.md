## 1. Knowledge Section: SCUM.db Custom Zone Format

### SCUM.db Custom Zone Persistence Format

SCUM stores custom zones (admin-created via Server Settings → Custom Zones) in three SQLite tables within `SCUM.db`. This is the **only** persistence mechanism — no JSON config files. Zones are loaded on server boot from these tables.

#### Table: `custom_zone_region` — Geometry & Map Placement

| Column | Type | Description |
|--------|------|-------------|
| `id` | INTEGER | Row ID |
| `map_id` | INTEGER | Always 1 (single map) |
| `name` | TEXT | Region label shown on map |
| `location_x` | REAL | Center X in SCUM world cm |
| `location_y` | REAL | Center Y in SCUM world cm |
| `size_x` | REAL | Radius (circle) or half-width (rectangle) in cm |
| `size_y` | REAL | 0 for circle, half-height for rectangle |
| `configuration_index` | INTEGER | Index into `custom_zone_configuration` (NOT the config's `id`) |
| `default_region_name` | TEXT | For built-in regions, the original name |
| `default_region_state` | INTEGER | `EDefaultCustomZoneState`: 0=NotDefault, 1=Unmodified, 2=Modified, 3=Deleted |

#### Table: `custom_zone_configuration` — Identity, Color, Behavior

| Column | Type | Description |
|--------|------|-------------|
| `id` | INTEGER | Config ID (1000 = "Global configuration" baseline) |
| `map_id` | INTEGER | Always 1 |
| `name` | TEXT | Config name |
| `color_red` | REAL | Red channel 0.0–1.0 |
| `color_green` | REAL | Green channel 0.0–1.0 |
| `color_blue` | REAL | Blue channel 0.0–1.0 |
| `settings` | INTEGER | Bitfield of `ECustomZoneSetting`: 1=VisibleOnMap, 2=NotificationsOnEntry, 3=both |
| `handling_methods` | INTEGER | Packed per-event rules (see below) |

#### Table: `custom_zone_configuration_damage_handling_methods` — PvP/Damage Rules

| Column | Type | Description |
|--------|------|-------------|
| `custom_zone_configuration_id` | INTEGER | Parent config ID |
| `damage_actor_type` | INTEGER | `EDamageActorType`: 0=General, 1=Player, 2=Puppet, …, 9=BaseBuilding, 11=Vehicle |
| `damage_handling_methods` | INTEGER | Packed per-channel Allow/Block/Ignore |

### Bit-Packing Details

#### `handling_methods` — 2 bits per event, 15 events

```
Bits 0-29: 15 events × 2 bits each
Bit 48:    Constant 0x6000 marker
```

Per-event value (`ECustomZoneEventHandlingMethod`): 0=Ignore, 1=Allow, 2=Block

Event order (index 0-14):
1. PlayerLockpicking
2. WorldLockpicking
3. BaseBuilding
4. FlagOvertake
5. VehicleParking
6. AvailabilityGrid
7. ChestParking
8. DropshipEncounterSpawning
9. AutoCloseVehicleDoorsOnExit
10. DisablingGeneral
11. DisablingSentry
12. DisablingPlayerElectronics
13. PlayerAntiBlockPhasing
14. BlueprintPlacement
15. BaseDecay

**PvP zone** (no rules): All events = Allow → `0x6000 | sum(1 << (i*2) for i in range(15))`
**Safe zone**: All events = Block → `0x6000 | sum(2 << (i*2) for i in range(15))`

#### `damage_handling_methods` — 2 bits per damage channel

Per `damage_actor_type` row, same encoding: 0=Ignore, 1=Allow, 2=Block

**Full PvP** (Player vs Player): `0x15555555` = all channels Allow
**Safe zone** (no PvP): All channels Block

### Worked Example: Red PvP Zone

```sql
-- 1. Create configuration
INSERT INTO custom_zone_configuration (map_id, name, color_red, color_green, color_blue, settings, handling_methods)
VALUES (1, 'RedDeath', 0.8, 0.08, 0.11, 3, <all_events_allow_bitfield>);

-- 2. Create damage handling rows (one per EDamageActorType)
INSERT INTO custom_zone_configuration_damage_handling_methods (custom_zone_configuration_id, damage_actor_type, damage_handling_methods)
VALUES (<config_id>, 0, 0x15555555), (<config_id>, 1, 0x15555555), ...;

-- 3. Create region
INSERT INTO custom_zone_region (map_id, name, location_x, location_y, size_x, size_y, configuration_index)
VALUES (1, 'RedDeath', <center_x>, <center_y>, <radius>, 0, <config_index>);
```

### Key Constraints
- Write to SCUM.db **only when server is stopped** (WAL mode + locked live)
- `configuration_index` in `custom_zone_region` links by **index** (0-based position in `custom_zone_configuration`), not by config `id`
- `size_y = 0` indicates circle; non-zero indicates rectangle with half-extents `(size_x, size_y)`

---

## 2. Proposed Code Changes

### A. `zones.py` — Add SCUM.db Zone Model + Writer

```python
# Add to zones.py after existing imports

from dataclasses import dataclass, field
from enum import IntEnum
from typing import Optional

# ── SCUM.db Custom Zone Enums ──────────────────────────────────────────

class ECustomZoneEvent(IntEnum):
    """Order matches bit-packing in handling_methods (index 0-14)."""
    PlayerLockpicking = 0
    WorldLockpicking = 1
    BaseBuilding = 2
    FlagOvertake = 3
    VehicleParking = 4
    AvailabilityGrid = 5
    ChestParking = 6
    DropshipEncounterSpawning = 7
    AutoCloseVehicleDoorsOnExit = 8
    DisablingGeneral = 9
    DisablingSentry = 10
    DisablingPlayerElectronics = 11
    PlayerAntiBlockPhasing = 12
    BlueprintPlacement = 13
    BaseDecay = 14

class ECustomZoneEventHandlingMethod(IntEnum):
    Ignore = 0
    Allow = 1
    Block = 2

class EDamageActorType(IntEnum):
    General = 0
    Player = 1
    Puppet = 2
    # ... other types as needed
    BaseBuilding = 9
    Vehicle = 11

class ECustomZoneSetting(IntEnum):
    VisibleOnMap = 1
    NotificationsOnEntry = 2

class ECustomZoneShape(IntEnum):
    Circle = 0  # size_y == 0
    Rectangle = 1  # size_x = half-width, size_y = half-height

# ── SCUM.db Zone Data Model ────────────────────────────────────────────

@dataclass
class ScumDbZoneConfig:
    """Corresponds to custom_zone_configuration row."""
    name: str
    color_rgb: tuple[float, float, float]  # 0..1 each
    settings: int = 3  # VisibleOnMap | NotificationsOnEntry
    handling_methods: dict[ECustomZoneEvent, ECustomZoneEventHandlingMethod] = field(default_factory=dict)
    
    def pack_handling_methods(self) -> int:
        """Pack per-event rules into 30-bit bitfield + 0x6000 marker."""
        result = 0x6000  # constant marker at bit 48
        for event, method in self.handling_methods.items():
            result |= (int(method) & 0x3) << (int(event) * 2)
        return result
    
    @classmethod
    def unpack_handling_methods(cls, packed: int) -> dict[ECustomZoneEvent, ECustomZoneEventHandlingMethod]:
        """Unpack 30-bit bitfield into per-event dict."""
        methods = {}
        for event in ECustomZoneEvent:
            value = (packed >> (int(event) * 2)) & 0x3
            methods[event] = ECustomZoneEventHandlingMethod(value)
        return methods

@dataclass
class ScumDbZoneDamageHandling:
    """Corresponds to custom_zone_configuration_damage_handling_methods row."""
    damage_actor_type: EDamageActorType
    damage_handling_methods: int  # packed per-channel Allow/Block/Ignore
    
    @staticmethod
    def pack_all_allow() -> int:
        """0x15555555 = all channels Allow."""
        return 0x15555555
    
    @staticmethod
    def pack_all_block() -> int:
        """All channels Block = 2 per channel."""
        result = 0
        for i in range(16):  # 16 channels × 2 bits
            result |= 2 << (i * 2)
        return result

@dataclass
class ScumDbZoneRegion:
    """Corresponds to custom_zone_region row."""
    name: str
    location_cm: tuple[float, float]  # (x, y)
    size_x: float  # radius (circle) or half-width (rectangle)
    size_y: float  # 0 for circle, half-height for rectangle
    configuration_index: int  # index into custom_zone_configuration (0-based)
    default_region_name: str = ""
    default_region_state: int = 0  # NotDefault

# ── SCUM.db Zone Writer ────────────────────────────────────────────────

def write_scumdb_zone(
    db_path: str,
    region: ScumDbZoneRegion,
    config: ScumDbZoneConfig,
    damage_handling: list[ScumDbZoneDamageHandling],
) -> None:
    """Write a custom zone to SCUM.db. Server must be STOPPED.
    
    Args:
        db_path: Path to SCUM.db
        region: Region geometry/placement
        config: Zone configuration (color, behavior)
        damage_handling: List of damage handling rows (one per EDamageActorType)
    """
    import sqlite3
    
    conn = sqlite3.connect(db_path)
    try:
        cur = conn.cursor()
        
        # 1. Insert configuration
        cur.execute("""
            INSERT INTO custom_zone_configuration 
                (map_id, name, color_red, color_green, color_blue, settings, handling_methods)
            VALUES (1, ?, ?, ?, ?, ?, ?)
        """, (
            config.name,
            config.color_rgb[0],
            config.color_rgb[1],
            config.color_rgb[2],
            config.settings,
            config.pack_handling_methods(),
        ))
        config_id = cur.lastrowid
        
        # 2. Get config index (0-based position in table)
        cur.execute("SELECT COUNT(*) FROM custom_zone_configuration WHERE map_id=1")
        config_count = cur.fetchone()[0]
        config_index = config_count - 1  # 0-based
        
        # 3. Insert damage handling rows
        for dh in damage_handling:
            cur.execute("""
                INSERT INTO custom_zone_configuration_damage_handling_methods
                    (custom_zone_configuration_id, damage_actor_type, damage_handling_methods)
                VALUES (?, ?, ?)
            """, (config_id, int(dh.damage_actor_type), dh.damage_handling_methods))
        
        # 4. Insert region
        cur.execute("""
            INSERT INTO custom_zone_region
                (map_id, name, location_x, location_y, size_x, size_y, configuration_index,
                 default_region_name, default_region_state)
            VALUES (1, ?, ?, ?, ?, ?, ?, ?, ?)
        """, (
            region.name,
            region.location_cm[0],
            region.location_cm[1],
            region.size_x,
            region.size_y,
            config_index,
            region.default_region_name,
            region.default_region_state,
        ))
        
        conn.commit()
    finally:
        conn.close()

# ── Helper: Create PvP Zone ────────────────────────────────────────────

def create_pvp_zone(
    name: str,
    center_cm: tuple[float, float],
    radius_cm: float,
    color_rgb: tuple[float, float, float] = (0.8, 0.08, 0.11),  # Red
    db_path: str = "SCUM.db",
) -> None:
    """Create a full-PvP custom zone (no rules, all damage allowed)."""
    # All events Allow
    handling = {event: ECustomZoneEventHandlingMethod.Allow for event in ECustomZoneEvent}
    
    config = ScumDbZoneConfig(
        name=name,
        color_rgb=color_rgb,
        handling_methods=handling,
    )
    
    # Damage handling: Player + BaseBuilding + Vehicle = full PvP
    damage_handling = [
        ScumDbZoneDamageHandling(
            damage_actor_type=EDamageActorType.Player,
            damage_handling_methods=ScumDbZoneDamageHandling.pack_all_allow(),
        ),
        ScumDbZoneDamageHandling(
            damage_actor_type=EDamageActorType.BaseBuilding,
            damage_handling_methods=ScumDbZoneDamageHandling.pack_all_allow(),
        ),
        ScumDbZoneDamageHandling(
            damage_actor_type=EDamageActorType.Vehicle,
            damage_handling_methods=ScumDbZoneDamageHandling.pack_all_allow(),
        ),
    ]
    
    region = ScumDbZoneRegion(
        name=name,
        location_cm=center_cm,
        size_x=radius_cm,
        size_y=0,  # circle
        configuration_index=0,  # will be set by writer
    )
    
    write_scumdb_zone(db_path, region, config, damage_handling)

def create_safe_zone(
    name: str,
    center_cm: tuple[float, float],
    radius_cm: float,
    color_rgb: tuple[float, float, float] = (0.1, 0.6, 0.3),  # Green
    db_path: str = "SCUM.db",
) -> None:
    """Create a safe zone (all events Block, no PvP)."""
    handling = {event: ECustomZoneEventHandlingMethod.Block for event in ECustomZoneEvent}
    
    config = ScumDbZoneConfig(
        name=name,
        color_rgb=color_rgb,
        handling_methods=handling,
    )
    
    damage_handling = [
        ScumDbZoneDamageHandling(
            damage_actor_type=EDamageActorType.Player,
            damage_handling_methods=ScumDbZoneDamageHandling.pack_all_block(),
        ),
    ]
    
    region = ScumDbZoneRegion(
        name=name,
        location_cm=center_cm,
        size_x=radius_cm,
        size_y=0,
        configuration_index=0,
    )
    
    write_scumdb_zone(db_path, region, config, damage_handling)
```

### B. `vanilla-zones.json` — Add Custom Zone Schema

Add to the schema definition:

```json
{
  "build": "v23451409",
  "source": "SCUM.db (custom_zone_region + custom_zone_configuration)",
  "coord_system": "game_cm (matches apps/auto-map/src/scummy_auto_map/coords.py)",
  "custom_zones": [
    {
      "kind": "custom",
      "name": "RedDeath",
      "game_cm_x": -255823.0,
      "game_cm_y": -39603.0,
      "radius_cm": 50000.0,
      "shape": "circle",
      "color_rgb": [0.8, 0.08, 0.11],
      "settings": 3,
      "handling_methods": {
        "PlayerLockpicking": "Allow",
        "WorldLockpicking": "Allow",
        "BaseBuilding": "Allow",
        "FlagOvertake": "Allow",
        "VehicleParking": "Allow",
        "AvailabilityGrid": "Allow",
        "ChestParking": "Allow",
        "DropshipEncounterSpawning": "Allow",
        "AutoCloseVehicleDoorsOnExit": "Allow",
        "DisablingGeneral": "Allow",
        "DisablingSentry": "Allow",
        "DisablingPlayerElectronics": "Allow",
        "PlayerAntiBlockPhasing": "Allow",
        "BlueprintPlacement": "Allow",
        "BaseDecay": "Allow"
      },
      "damage_handling": {
        "Player": "Allow",
        "BaseBuilding": "Allow",
        "Vehicle": "Allow"
      },
      "source_table": "custom_zone_region",
      "source_config_id": 1001
    }
  ]
}
```

### C. `ZoneEditPanel.tsx` — Add SCUM.db Export Support

```tsx
// Add to imports
import type { ScumDbZoneConfig, ScumDbZoneDamageHandling, ScumDbZoneRegion } from '@scummy/shared';

// Add to Props
type Props = {
  // ... existing props ...
  // New: SCUM.db export support
  onExportToScumDb?: (zone: ScumDbExportPayload) => Promise<void>;
};

// Add export payload type
export type ScumDbExportPayload = {
  region: {
    name: string;
    location_cm: [number, number];
    size_x: number;
    size_y: number;
  };
  config: {
    name: string;
    color_rgb: [number, number, number];
    settings: number;
    handling_methods: Record<string, 'Allow' | 'Block' | 'Ignore'>;
  };
  damage_handling: Array<{
    damage_actor_type: number;
    damage_handling_methods: number;
  }>;
};

// Add export button to the panel (after the Save button group)
{onExportToScumDb && (
  <button
    type="button"
    onClick={async () => {
      const override = buildOverride();
      const payload: ScumDbExportPayload = {
        region: {
          name: override.name || zone.name,
          location_cm: [zone.gameCmX, zone.gameCmY],
          size_x: override.radiusCm,
          size_y: shape === 'rectangle' ? (override.heightCm || override.radiusCm) : 0,
        },
        config: {
          name: override.name || zone.name,
          color_rgb: hexToRgb(fill),
          settings: 3, // VisibleOnMap | NotificationsOnEntry
          handling_methods: kind === 'safe' 
            ? Object.fromEntries(EVENTS.map(e => [e, 'Block']))
            : Object.fromEntries(EVENTS.map(e => [e, 'Allow'])),
        },
        damage_handling: [
          {
            damage_actor_type: 1, // Player
            damage_handling_methods: kind === 'safe' ? 0xAAAAAAAA : 0x15555555,
          },
        ],
      };
      await onExportToScumDb(payload);
    }}
    className="rounded bg-green-700 px-3 py-1 text-[11px] font-semibold text-white hover:bg-green-600"
  >
    Export to SCUM.db
  </button>
)}

// Helper function
function hexToRgb(hex: string): [number, number, number] {
  const result = /^#?([a-f\d]{2})([a-f\d]{2})([a-f\d]{2})$/i.exec(hex);
  if (!result) return [0, 0, 0];
  return [
    parseInt(result[1], 16) / 255,
    parseInt(result[2], 16) / 255,
    parseInt(result[3], 16) / 255,
  ];
}

const EVENTS = [
  'PlayerLockpicking', 'WorldLockpicking', 'BaseBuilding', 'FlagOvertake',
  'VehicleParking', 'AvailabilityGrid', 'ChestParking', 'DropshipEncounterSpawning',
  'AutoCloseVehicleDoorsOnExit', 'DisablingGeneral', 'DisablingSentry',
  'DisablingPlayerElectronics', 'PlayerAntiBlockPhasing', 'BlueprintPlacement', 'BaseDecay',
];
```

### D. `shared` Package — Add SCUM.db Types

```typescript
// Add to @scummy/shared/src/types.ts

export interface ScumDbZoneConfig {
  name: string;
  colorRgb: [number, number, number]; // 0..1
  settings: number; // bitfield: 1=VisibleOnMap, 2=NotificationsOnEntry
  handlingMethods: Record<string, 'Allow' | 'Block' | 'Ignore'>;
}

export interface ScumDbZoneDamageHandling {
  damageActorType: number; // EDamageActorType
  damageHandlingMethods: number; // packed bitfield
}

export interface ScumDbZoneRegion {
  name: string;
  locationCm: [number, number];
  sizeX: number; // radius or half-width
  sizeY: number; // 0 for circle, half-height for rectangle
  configurationIndex: number;
}

export interface ScumDbExportPayload {
  region: ScumDbZoneRegion;
  config: ScumDbZoneConfig;
  damageHandling: ScumDbZoneDamageHandling[];
}
```