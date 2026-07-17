/**
 * `turdmod.player` — player lifecycle + inventory + stats hooks.
 *
 * Strategy B (server-side) gets every player on the server. Strategy C
 * (offline) gets the local player only. Mods that target both should not
 * assume player count > 1 unless they declare server-side mode.
 *
 * @since 1.0
 */

import { type Disposable, requireNamespace, TurdMODError, type Vec3 } from "./runtime.js";

/** Stable identifier — Steam64 if available, else a server-local id. */
export type PlayerId = string;

export interface PlayerInfo {
  id: PlayerId;
  steamId?: string;
  name: string;
  isLocal: boolean;       // true in Strategy C; varies in Strategy B
}

export interface PlayerState {
  position: Vec3;
  health: number;
  maxHealth: number;
  /** Tag set used by the game (e.g. "infected", "stunned"). Read-only here. */
  tags: ReadonlyArray<string>;
}

export interface InventoryItem {
  classPath: string;
  count: number;
  uniqueId?: string;      // server-side persistence id when available
}

export interface DamageEvent {
  player: PlayerInfo;
  amount: number;
  /** Source's class path, or "world" / "fall" / "hunger" / etc. */
  cause: string;
  attacker?: PlayerInfo;
}

export interface InventoryEvent {
  player: PlayerInfo;
  added: InventoryItem[];
  removed: InventoryItem[];
}

interface PlayerRuntime {
  list(): PlayerInfo[];
  getState(id: PlayerId): Promise<PlayerState | null>;
  getInventory(id: PlayerId): Promise<InventoryItem[]>;
  giveItem(id: PlayerId, item: InventoryItem): Promise<void>;
  removeItem(id: PlayerId, classPath: string, count: number): Promise<number>; // returns # actually removed

  // Events.
  onSpawn(handler: (p: PlayerInfo) => void): Disposable;
  onDeath(handler: (p: PlayerInfo, cause: string) => void): Disposable;
  onDamage(handler: (e: DamageEvent) => void): Disposable;
  onInventoryChange(handler: (e: InventoryEvent) => void): Disposable;
}

function rt(): PlayerRuntime {
  return requireNamespace<PlayerRuntime>("player");
}

/**
 * Every player currently visible to this mod. In Strategy C this is `[local]`;
 * in Strategy B this is everyone on the server.
 *
 * @since 1.0
 */
export function list(): PlayerInfo[] {
  return rt().list();
}

/** @since 1.0 */
export async function getState(id: PlayerId): Promise<PlayerState | null> {
  if (!id) throw new TurdMODError("getState: id is required", "BAD_ID");
  return rt().getState(id);
}

/** @since 1.0 */
export async function getInventory(id: PlayerId): Promise<InventoryItem[]> {
  if (!id) throw new TurdMODError("getInventory: id is required", "BAD_ID");
  return rt().getInventory(id);
}

/**
 * Spawn an item into the player's inventory. Returns when the spawn is
 * acknowledged. Throws if the player has no room and `count` is positive.
 *
 * @since 1.0
 */
export async function giveItem(id: PlayerId, item: InventoryItem): Promise<void> {
  if (!id || !item.classPath) {
    throw new TurdMODError("giveItem: id and item.classPath are required", "BAD_ARGS");
  }
  return rt().giveItem(id, item);
}

/**
 * Remove items from the player's inventory by class. Returns the actual
 * count removed (may be less than `count` if they didn't have that many).
 *
 * @since 1.0
 */
export async function removeItem(id: PlayerId, classPath: string, count: number): Promise<number> {
  if (!id || !classPath || count <= 0) {
    throw new TurdMODError("removeItem: id, classPath, and positive count required", "BAD_ARGS");
  }
  return rt().removeItem(id, classPath, count);
}

/** @since 1.0 */
export const onSpawn  = (handler: (p: PlayerInfo) => void): Disposable => rt().onSpawn(handler);
/** @since 1.0 */
export const onDeath  = (handler: (p: PlayerInfo, cause: string) => void): Disposable => rt().onDeath(handler);
/** @since 1.0 */
export const onDamage = (handler: (e: DamageEvent) => void): Disposable => rt().onDamage(handler);
/** @since 1.0 */
export const onInventoryChange = (handler: (e: InventoryEvent) => void): Disposable => rt().onInventoryChange(handler);
