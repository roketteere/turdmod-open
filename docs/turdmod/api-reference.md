# TurdMOD API reference

*Generated 2026-05-09T01:44:27.502Z from `packages/turdmod-api/src/` by `scripts/gen-docs.ts`. Do not edit by hand — re-run `pnpm --filter @turdmod/turdmod-api docs` after changing the API.*

Mod authors `import { content, player, world, ui, persistence, network } from "@turdmod/turdmod-api"` and call the namespaced functions documented below. Argument validation throws `TurdMODError` before the runtime is touched. The host (Strategy B/C loader) binds the runtime via `setRuntime()`.

See also:
- [Manifest spec v1](manifest-spec.md) — what authors write in `turdmod.json`
- [Compatibility policy](compatibility-policy.md) — when each delivery mode is allowed
- [BattlEye safety](battleye-safety.md) — Strategy C runtime gating
- [Loader architecture](loader-architecture.md) — Strategy C internals

## `turdmod.content`

DataTable / Blueprint / locres overrides.

### Functions

### `setDataTableOverride`

```ts
export function setDataTableOverride(o: DataTableOverride): Disposable
```

Override one or more fields on a DataTable row. Returns a disposable —
call `.dispose()` to revert.

```ts
import { content } from "@turdmod/turdmod-api";
content.setDataTableOverride({
ref: { table: "ItemSpawningParameters", row: "MemoryModule_Level4" },
values: { MaxOccurrences: 99 },
});
```

*Since 1.0*

### `getDataTableRow`

```ts
export async function getDataTableRow(ref: DataTableRef): Promise<Record<string, unknown> | null>
```

Read a DataTable row's current effective values (after any active overrides).
Returns null if the row doesn't exist.

*Since 1.0*

### `setBlueprintOverride`

```ts
export function setBlueprintOverride(o: BlueprintOverride): Disposable
```

Override one or more fields on a Blueprint's Class Default Object.

```ts
content.setBlueprintOverride({
klass: "BP_DepositorySmall_C",
values: { MaxLootTier: 4 },
});
```

*Since 1.0*

### `setLocresOverride`

```ts
export function setLocresOverride(o: LocresOverride): Disposable
```

Add or replace a localized string for a given locale.

*Since 1.0*

### `listOverrides`

```ts
export function listOverrides()
```

Listing of every override registered by the current mod. Useful for debug.

### Types

### `ClassPath`

```ts
export type ClassPath = string;
```

Identifier for an item / actor / blueprint class as it lives in the .pak.

### `RowName`

```ts
export type RowName = string;
```

Identifier for a row inside a DataTable, e.g. "MemoryModule_Level1".

### `DataTableRef`

```ts
export interface DataTableRef { table: string; // e.g. "ItemSpawningParameters" or full asset path row: RowName; }
```

A DataTable identifier — usually the asset path inside the pak.

### `DataTableOverride`

```ts
export interface DataTableOverride { ref: DataTableRef; /** Sparse override — only keys you set are changed; everything else stays. */ values: Record<string, unknown>; }
```



### `BlueprintOverride`

```ts
export interface BlueprintOverride { /** Class path of the BP, e.g. "BP_DepositorySmall_C". */ klass: ClassPath; /** Sparse property map. */ values: Record<string, unknown>; }
```



### `LocresOverride`

```ts
export interface LocresOverride { /** Locale code, e.g. "en-US". */ locale: string; /** Locres key, e.g. "UI_Items::MemoryModuleLevel4". */ key: string; value: string; }
```



## `turdmod.player`

Player lifecycle, inventory, stats hooks.

### Functions

### `list`

```ts
export function list(): PlayerInfo[]
```

Every player currently visible to this mod. In Strategy C this is `[local]`;
in Strategy B this is everyone on the server.

*Since 1.0*

### `getState`

```ts
export async function getState(id: PlayerId): Promise<PlayerState | null>
```


*Since 1.0*

### `getInventory`

```ts
export async function getInventory(id: PlayerId): Promise<InventoryItem[]>
```


*Since 1.0*

### `giveItem`

```ts
export async function giveItem(id: PlayerId, item: InventoryItem): Promise<void>
```

Spawn an item into the player's inventory. Returns when the spawn is
acknowledged. Throws if the player has no room and `count` is positive.

*Since 1.0*

### `removeItem`

```ts
export async function removeItem(id: PlayerId, classPath: string, count: number): Promise<number>
```

Remove items from the player's inventory by class. Returns the actual
count removed (may be less than `count` if they didn't have that many).

*Since 1.0*

### `onSpawn`

```ts
export const onSpawn = (handler: (p: PlayerInfo)
```


*Since 1.0*

### `onDeath`

```ts
export const onDeath = (handler: (p: PlayerInfo, cause: string)
```


*Since 1.0*

### `onDamage`

```ts
export const onDamage = (handler: (e: DamageEvent)
```


*Since 1.0*

### `onInventoryChange`

```ts
export const onInventoryChange = (handler: (e: InventoryEvent)
```


*Since 1.0*

### Types

### `PlayerId`

```ts
export type PlayerId = string;
```

Stable identifier — Steam64 if available, else a server-local id.

### `PlayerInfo`

```ts
export interface PlayerInfo { id: PlayerId; steamId?: string; name: string; isLocal: boolean; // true in Strategy C; varies in Strategy B }
```



### `PlayerState`

```ts
export interface PlayerState { position: Vec3; health: number; maxHealth: number; /** Tag set used by the game (e.g. "infected", "stunned"). Read-only here. */ tags: ReadonlyArray<string>; }
```



### `InventoryItem`

```ts
export interface InventoryItem { classPath: string; count: number; uniqueId?: string; // server-side persistence id when available }
```



### `DamageEvent`

```ts
export interface DamageEvent { player: PlayerInfo; amount: number; /** Source's class path, or "world" / "fall" / "hunger" / etc. */ cause: string; attacker?: PlayerInfo; }
```



### `InventoryEvent`

```ts
export interface InventoryEvent { player: PlayerInfo; added: InventoryItem[]; removed: InventoryItem[]; }
```



## `turdmod.world`

Spawn / despawn entities, weather, time, custom POIs.

### Functions

### `spawn`

```ts
export async function spawn(req: SpawnRequest): Promise<SpawnResult>
```


*Since 1.0*

### `despawn`

```ts
export async function despawn(entityId: string): Promise<boolean>
```


*Since 1.0*

### `teleport`

```ts
export async function teleport(entityId: string, to: Vec3): Promise<void>
```


*Since 1.0*

### `setTimeOfDay`

```ts
export async function setTimeOfDay(hour24: number): Promise<void>
```


*Since 1.0*

### `setWeather`

```ts
export const setWeather = (state: WeatherState): Disposable
```

Set a weather state. Returns disposable; dispose to revert to game-driven weather. @since 1.0

### `getWeather`

```ts
export const getWeather = (): Promise<WeatherState>
```


*Since 1.0*

### `addPOI`

```ts
export function addPOI(poi: CustomPOI): Disposable
```

Surface a custom POI on the map (and the scummap web map when published). @since 1.0

### `listPOIs`

```ts
export const listPOIs = (): CustomPOI[]
```


*Since 1.0*

### `onTick`

```ts
export const onTick = (handler: (deltaMs: number)
```


*Since 1.0*

### Types

### `SpawnRequest`

```ts
export interface SpawnRequest { classPath: string; position: Vec3; rotationYawDegrees?: number; /** Server tags applied at spawn time. */ tags?: ReadonlyArray<string>; }
```



### `SpawnResult`

```ts
export interface SpawnResult { /** Stable id for despawn / relocate calls. */ entityId: string; }
```



### `CustomPOI`

```ts
export interface CustomPOI { /** Stable id; mods own the namespace under their own modId. */ id: string; position: Vec3; /** Display name; passed through locres if it matches a key. */ name: string; iconClass?: string; // optional refere...
```



### `WeatherState`

```ts
export interface WeatherState { /** Internal rate or label. Implementation-defined; pass-through. */ preset?: string; temperatureC?: number; humidity?: number; precipitationMmHr?: number; windKph?: number; windHeadingDegrees?: number; }
```



## `turdmod.ui`

HUD widgets, custom menus, hotkeys, toasts (Strategy C only).

### Functions

### `setHud`

```ts
export function setHud(widget: HudTextWidget): Disposable
```

Render or update a HUD text widget. The widget keeps its id; calling with
the same id replaces the existing widget atomically (no flicker).

*Since 1.0*

### `removeHud`

```ts
export const removeHud = (id: string): boolean
```


*Since 1.0*

### `showMenu`

```ts
export function showMenu(menu: CustomMenu): Disposable
```

Open a custom radial / list menu. Returns disposable; dispose to close.

*Since 1.0*

### `closeMenu`

```ts
export const closeMenu = (id: string): boolean
```


*Since 1.0*

### `bindHotkey`

```ts
export function bindHotkey(key: Hotkey, handler: () => void): Disposable
```

Bind a global hotkey. Hotkeys are mod-scoped — collisions across mods are
resolved by load order (last-loaded wins) and surfaced in the manager UI.

*Since 1.0*

### `toast`

```ts
export const toast = (text: string, durationMs = 3000): void
```


*Since 1.0*

### Types

### `HudTextWidget`

```ts
export interface HudTextWidget { id: string; text: string; /** Anchor on the screen, normalized [0..1] with origin top-left. */ anchor: { x: number; y: number }; fontPx?: number; color?: RGBA; visible?: boolean; }
```



### `MenuItem`

```ts
export interface MenuItem { id: string; label: string; /** Optional submenu. Either action or items, not both. */ items?: MenuItem[]; action?: () => void; enabled?: boolean; }
```



### `CustomMenu`

```ts
export interface CustomMenu { id: string; title: string; items: MenuItem[]; }
```



### `Hotkey`

```ts
export type Hotkey = string;
```

Standard SCUM input bindings: keyboard chord like "Ctrl+M", "F8".

## `turdmod.persistence`

Per-mod KV slot. Top-level + per-player scoped.

### Functions

### `get`

```ts
export async function get<T = unknown>(key: string): Promise<T | null>
```


*Since 1.0*

### `set`

```ts
export async function set<T = unknown>(key: string, value: T): Promise<void>
```


*Since 1.0*

### `keys`

```ts
export async function keys(): Promise<string[]>
```


*Since 1.0*

### `clearAll`

```ts
export async function clearAll(): Promise<void>
```

Atomic clear of the mod's namespace. Use for "reset progress." @since 1.0

### `getForPlayer`

```ts
export async function getForPlayer<T = unknown>(playerId: string, key: string): Promise<T | null>
```


*Since 1.0*

### `setForPlayer`

```ts
export async function setForPlayer<T = unknown>(playerId: string, key: string, value: T): Promise<void>
```


*Since 1.0*

## `turdmod.network`

Server-authoritative event dispatch. Private servers only.

### Functions

### `on`

```ts
export function on<T = unknown>(channel: string, handler: (msg: NetMessage<T>) => void): Disposable
```


*Since 1.0*

### `broadcast`

```ts
export async function broadcast<T = unknown>(channel: string, payload: T): Promise<void>
```

Server-side mods only. @since 1.0

### `send`

```ts
export async function send<T = unknown>(channel: string, playerId: string, payload: T): Promise<void>
```


*Since 1.0*

### `rpc`

```ts
export async function rpc<TReq = unknown, TRes = unknown>(channel: string, payload: TReq): Promise<TRes>
```


*Since 1.0*

### Types

### `NetMessage`

```ts
export interface NetMessage<T = unknown> { channel: string; /** Steam id of the sender, or "server" for server-emitted events. */ from: string; payload: T; /** Server timestamp (ms since epoch). */ ts: number; }
```



## Shared (`@turdmod/turdmod-api`)

### `TurdMODError`

```ts
class TurdMODError
```

Runtime contract — the host (loader / server runtime) injects an
implementation of `Runtime`; every API namespace reads from it via
`getRuntime()`. In a non-host process (e.g. a mod author writing tests),
`getRuntime()` throws unless they've called `setRuntime(stub)` with a
test double.

### `Disposable`

```ts
export interface Disposable { dispose(): void; }
```

Anything that can be released later (subscription handle).

### `Vec3`

```ts
export interface Vec3 { x: number; y: number; z: number; }
```

SCUM uses centimeters; +X = west, +Y = north (per scummap apps/web/src/lib/coords.ts).

### `RGBA`

```ts
export interface RGBA { r: number; g: number; b: number; a?: number; }
```



### `Runtime`

```ts
export interface Runtime { // Lifecycle. readonly modId: string; readonly buildId: string; log(level: "trace" | "debug" | "info" | "warn" | "error", msg: string, data?: unknown): void; // Capabilities — populated from the manifest at loa...
```

Runtime is the union of all namespace-specific runtime hooks. The host
binds one implementation; `getRuntime()` returns it. We intentionally
keep every method asynchronous — Strategy B (server-side) needs RPC,
Strategy C (in-game) does not, but uniform shape lets the SDK paper
over the difference.

### `setRuntime`

```ts
export function setRuntime(rt: Runtime | null): void
```

Host calls this once during mod-load.

### `getRuntime`

```ts
export function getRuntime(): Runtime
```



### `requireNamespace`

```ts
export function requireNamespace<T>(name: keyof Runtime): T
```

Type-narrow helper for namespace impls.
