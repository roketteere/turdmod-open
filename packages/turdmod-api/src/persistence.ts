/**
 * `turdmod.persistence` — per-mod savefile slot.
 *
 * Each mod gets a private namespaced key/value store. Strategy C writes to
 * `<appdata>/TurdMOD/<modId>/state.json`; Strategy B to a server-managed
 * Postgres row keyed by (modId, serverId, playerId|null).
 *
 * Conflict-free with vanilla saves — the host stores TurdMOD state OUTSIDE
 * the SCUM save format. Removing a mod cleans up its directory; vanilla
 * progression is unaffected.
 *
 * @since 1.0
 */

import { requireNamespace, TurdMODError } from "./runtime.js";

interface PersistenceRuntime {
  /** Read a value. Returns null if absent. */
  get<T = unknown>(key: string): Promise<T | null>;
  /** Set a value. Setting `null` deletes the key. */
  set<T = unknown>(key: string, value: T): Promise<void>;
  /** All top-level keys in this mod's namespace. */
  keys(): Promise<string[]>;
  /** Atomic clear of the mod's namespace. Use for "reset progress." */
  clearAll(): Promise<void>;

  /**
   * Per-player scoped value. In Strategy B the host keys by player Steam id;
   * in Strategy C this is equivalent to top-level (single local player).
   */
  getForPlayer<T = unknown>(playerId: string, key: string): Promise<T | null>;
  setForPlayer<T = unknown>(playerId: string, key: string, value: T): Promise<void>;
}

function rt(): PersistenceRuntime {
  return requireNamespace<PersistenceRuntime>("persistence");
}

function checkKey(key: string): void {
  if (!key || typeof key !== "string") {
    throw new TurdMODError("persistence key must be a non-empty string", "BAD_KEY");
  }
  if (key.length > 256) {
    throw new TurdMODError("persistence key max length is 256", "KEY_TOO_LONG");
  }
}

/** @since 1.0 */
export async function get<T = unknown>(key: string): Promise<T | null> {
  checkKey(key);
  return rt().get<T>(key);
}

/** @since 1.0 */
export async function set<T = unknown>(key: string, value: T): Promise<void> {
  checkKey(key);
  return rt().set<T>(key, value);
}

/** @since 1.0 */
export async function keys(): Promise<string[]> {
  return rt().keys();
}

/** Atomic clear of the mod's namespace. Use for "reset progress." @since 1.0 */
export async function clearAll(): Promise<void> {
  return rt().clearAll();
}

/** @since 1.0 */
export async function getForPlayer<T = unknown>(playerId: string, key: string): Promise<T | null> {
  if (!playerId) throw new TurdMODError("getForPlayer: playerId is required", "BAD_ID");
  checkKey(key);
  return rt().getForPlayer<T>(playerId, key);
}

/** @since 1.0 */
export async function setForPlayer<T = unknown>(playerId: string, key: string, value: T): Promise<void> {
  if (!playerId) throw new TurdMODError("setForPlayer: playerId is required", "BAD_ID");
  checkKey(key);
  return rt().setForPlayer<T>(playerId, key, value);
}
