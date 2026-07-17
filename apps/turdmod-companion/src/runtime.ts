/**
 * Companion runtime — implements the Runtime contract from
 * @turdmod/turdmod-api by synthesizing API events from log-tail data.
 *
 * Mods are TS modules that look like:
 *
 *   import { player, network, persistence } from "@turdmod/turdmod-api";
 *   export function on_load() { player.onDeath(e => network.broadcast("kill", e)); }
 *
 * The companion calls `bindMod(mod)` once per mod, which:
 *   1. Sets a per-mod runtime via setRuntime() (each mod sees its own)
 *   2. Awaits the mod's on_load
 *   3. Records its disposables for shutdown
 *
 * Events flow: log-tail line -> parsers -> ServerEvent -> dispatch on
 * registered handlers (per mod). network.broadcast publishes through the
 * shared SinkRouter.
 */

import { mkdirSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { join } from "node:path";
import { setRuntime, withRuntime, type Runtime, type Disposable, TurdMODError } from "@turdmod/turdmod-api";
import type { ServerEvent } from "./parsers.js";
import { SinkRouter } from "./sinks.js";

// ---- per-mod state ----

interface ModHandlers {
  onSpawn: ((p: PlayerInfo) => void)[];
  onDeath: ((p: PlayerInfo, cause: string) => void)[];
  onDamage: ((e: DamageEvent) => void)[];
  onTick: ((deltaMs: number) => void)[];
  netListeners: Map<string, ((msg: NetMessage) => void)[]>;
  disposables: Disposable[];
}

interface PlayerInfo {
  id: string;
  steamId?: string;
  name: string;
  isLocal: boolean;
}

interface DamageEvent {
  player: PlayerInfo;
  amount: number;
  cause: string;
  attacker?: PlayerInfo;
}

interface NetMessage<T = unknown> {
  channel: string;
  from: string;
  payload: T;
  ts: number;
}

function newHandlers(): ModHandlers {
  return {
    onSpawn: [], onDeath: [], onDamage: [], onTick: [],
    netListeners: new Map(),
    disposables: [],
  };
}

// ---- companion runtime ----

export interface CompanionConfig {
  buildId: string;
  storeDir: string;       // base dir for per-mod persistence files
  router: SinkRouter;
}

export interface CompanionRuntime {
  bindMod(mod: ModBinding): Promise<void>;
  unbindMod(modId: string): Promise<void>;
  dispatch(event: ServerEvent): void;
  shutdown(): Promise<void>;
  players(): PlayerInfo[];
  loadedMods(): string[];
}

export interface ModBinding {
  id: string;
  capabilities?: string[];
  load: () => Promise<{ on_load?: () => void | Promise<void>; on_unload?: () => void | Promise<void> }>;
}

export function createCompanionRuntime(cfg: CompanionConfig): CompanionRuntime {
  const handlersByMod = new Map<string, ModHandlers>();
  const runtimeByMod = new Map<string, Runtime>();
  const onLoadCallbacks = new Map<string, () => void | Promise<void>>();
  const onUnloadCallbacks = new Map<string, () => void | Promise<void>>();
  const players = new Map<string, PlayerInfo>(); // keyed by steam id
  let lastTickTs = Date.now();

  /**
   * Bind a mod's runtime, run a handler, then restore. Mods reach into the
   * global runtime via the namespace shims (player.list(), etc.) — the
   * callsite-pattern is "host runs `setRuntime(mod.rt)` -> mod handler
   * runs -> host restores". This is synchronous; we never await between
   * the bind and the unbind.
   */
  function callForMod(modId: string, fn: () => unknown): void {
    const rt = runtimeByMod.get(modId);
    if (!rt) return;
    try {
      withRuntime(rt, () => { void fn(); });
    } catch (e) {
      console.warn(`mod handler error in ${modId}: ${(e as Error).message}`);
    }
  }

  const tickIntervalMs = 5000;
  const tickHandle = setInterval(() => {
    const now = Date.now();
    const delta = now - lastTickTs;
    lastTickTs = now;
    for (const [modId, h] of handlersByMod) {
      for (const fn of h.onTick) callForMod(modId, () => fn(delta));
    }
  }, tickIntervalMs);

  function persistencePathFor(modId: string, key: string): string {
    const dir = join(cfg.storeDir, modId);
    mkdirSync(dir, { recursive: true });
    return join(dir, encodeURIComponent(key) + ".json");
  }

  function readKey<T>(modId: string, key: string): T | null {
    const p = persistencePathFor(modId, key);
    if (!existsSync(p)) return null;
    try { return JSON.parse(readFileSync(p, "utf-8")) as T; } catch { return null; }
  }
  function writeKey(modId: string, key: string, value: unknown): void {
    const p = persistencePathFor(modId, key);
    writeFileSync(p, JSON.stringify(value, null, 2), "utf-8");
  }

  function makeRuntime(modId: string, capabilities: string[]): Runtime {
    const handlers = handlersByMod.get(modId)!;
    const dispose = (arr: unknown[], fn: unknown): Disposable => {
      const d = { dispose() { const i = arr.indexOf(fn); if (i >= 0) arr.splice(i, 1); } };
      handlers.disposables.push(d);
      return d;
    };

    return {
      modId,
      buildId: cfg.buildId,
      capabilities,
      log(level, msg, data) {
        const ts = new Date().toISOString();
        const line = `${ts} [${modId}] ${level.toUpperCase()} ${msg}` + (data !== undefined ? " " + JSON.stringify(data) : "");
        if (level === "error" || level === "warn") console.warn(line); else console.log(line);
      },

      // Event subscriptions return disposables that pop the handler from the array.
      player: {
        list: () => Array.from(players.values()),
        getState: async () => null,
        getInventory: async () => [],
        giveItem: async () => { throw new TurdMODError("player.giveItem requires the in-process loader (not available in log-tail runtime)", "NOT_SUPPORTED"); },
        removeItem: async () => { throw new TurdMODError("player.removeItem requires the in-process loader", "NOT_SUPPORTED"); },
        onSpawn(handler: (p: PlayerInfo) => void) { handlers.onSpawn.push(handler); return dispose(handlers.onSpawn, handler); },
        onDeath(handler: (p: PlayerInfo, cause: string) => void) { handlers.onDeath.push(handler); return dispose(handlers.onDeath, handler); },
        onDamage(handler: (e: DamageEvent) => void) { handlers.onDamage.push(handler); return dispose(handlers.onDamage, handler); },
        onInventoryChange() { return { dispose() {} }; },
      },

      world: {
        spawn: async () => { throw new TurdMODError("world.spawn requires the in-process loader", "NOT_SUPPORTED"); },
        despawn: async () => { throw new TurdMODError("world.despawn requires the in-process loader", "NOT_SUPPORTED"); },
        teleport: async () => { throw new TurdMODError("world.teleport requires the in-process loader", "NOT_SUPPORTED"); },
        setTimeOfDay: async () => { throw new TurdMODError("world.setTimeOfDay requires the in-process loader", "NOT_SUPPORTED"); },
        setWeather: () => ({ dispose() {} }),
        getWeather: async () => ({}),
        addPOI: () => ({ dispose() {} }),
        listPOIs: () => [],
        onTick(handler: (deltaMs: number) => void) { handlers.onTick.push(handler); return dispose(handlers.onTick, handler); },
      },

      ui: {
        setHud: () => ({ dispose() {} }),
        removeHud: () => false,
        showMenu: () => ({ dispose() {} }),
        closeMenu: () => false,
        bindHotkey: () => ({ dispose() {} }),
        toast: () => {},
      },

      persistence: {
        get: async <T>(key: string) => readKey<T>(modId, key),
        set: async (key: string, value: unknown) => writeKey(modId, key, value),
        keys: async () => [], // simple v0
        clearAll: async () => {},
        getForPlayer: async <T>(playerId: string, key: string) => readKey<T>(modId, `p/${playerId}/${key}`),
        setForPlayer: async (playerId: string, key: string, value: unknown) => writeKey(modId, `p/${playerId}/${key}`, value),
      },

      network: {
        on(channel: string, handler: (m: NetMessage) => void) {
          let arr = handlers.netListeners.get(channel);
          if (!arr) { arr = []; handlers.netListeners.set(channel, arr); }
          arr.push(handler);
          return dispose(arr, handler);
        },
        broadcast: async (channel: string, payload: unknown) => {
          await cfg.router.publish({ modId, channel, ts: Date.now() }, payload);
        },
        send: async (channel: string, _playerId: string, payload: unknown) => {
          await cfg.router.publish({ modId, channel, ts: Date.now() }, payload);
        },
        rpc: async (method: string, params?: unknown) => {
          const client = globalThis.__turdmod_engine_client;
          if (!client) {
            throw new TurdMODError(
              "network.rpc requires the engine (loader DLL not attached, or pipe discovery failed). " +
              "Run SCUMServer with turdmod-launcher injecting the loader DLL.",
              "NOT_SUPPORTED",
            );
          }
          return client.call(method, params);
        },
      },
    };
  }

  function fanOutChannel(channel: string, payload: unknown): void {
    for (const [modId, h] of handlersByMod) {
      const arr = h.netListeners.get(channel);
      if (arr) for (const fn of arr) callForMod(modId, () => fn({ channel, from: "server", payload, ts: Date.now() }));
    }
  }

  function dispatch(event: ServerEvent): void {
    switch (event.kind) {
      case "login": {
        const p: PlayerInfo = { id: event.steam, steamId: event.steam, name: event.player, isLocal: false };
        players.set(event.steam, p);
        for (const [modId, h] of handlersByMod) for (const fn of h.onSpawn) callForMod(modId, () => fn(p));
        fanOutChannel("system.login", event);
        break;
      }
      case "logout": {
        const p = players.get(event.steam);
        if (p) {
          fanOutChannel("system.logout", { ...event, ...p });
          players.delete(event.steam);
        } else {
          fanOutChannel("system.logout", event);
        }
        break;
      }
      case "kill": {
        const victim: PlayerInfo = {
          id: event.victimSteam || event.victim,
          steamId: event.victimSteam,
          name: event.victim,
          isLocal: false,
        };
        const cause = event.weapon ? `weapon:${event.weapon}` : (event.killer ? `killer:${event.killer}` : "unknown");
        for (const [modId, h] of handlersByMod) for (const fn of h.onDeath) callForMod(modId, () => fn(victim, cause));
        fanOutChannel("system.kill", event);
        break;
      }
      case "vehicle": {
        const slug = event.event.toLowerCase().replace(/\s+/g, "_");
        fanOutChannel(`vehicle.${slug}`, event);
        fanOutChannel("vehicle.any", event);
        break;
      }
      case "bunker":
      case "chat":
      case "admin": {
        fanOutChannel(`system.${event.kind}`, event);
        break;
      }
    }
  }

  async function bindMod(mod: ModBinding): Promise<void> {
    handlersByMod.set(mod.id, newHandlers());
    const rt = makeRuntime(mod.id, mod.capabilities ?? []);
    runtimeByMod.set(mod.id, rt);
    // setRuntime as module-level fallback — AsyncLocalStorage can lose
    // context across dynamic import() in some Node/tsx versions.
    // globalThis bridge: set module-level runtime so deferred callbacks
    // (setInterval, promises) from mods can still reach the runtime.
    // NOT cleared after bindMod — mods share the companion's runtime.
    setRuntime(rt);
    await withRuntime(rt, async () => {
      const m = await mod.load();
      if (m.on_load) {
        onLoadCallbacks.set(mod.id, m.on_load);
        await m.on_load();
      }
      if (m.on_unload) onUnloadCallbacks.set(mod.id, m.on_unload);
    });
  }

  async function unbindMod(modId: string): Promise<void> {
    const unloadFn = onUnloadCallbacks.get(modId);
    const rt = runtimeByMod.get(modId);
    if (unloadFn && rt) {
      await withRuntime(rt, async () => { await unloadFn(); });
    }
    const h = handlersByMod.get(modId);
    if (h) for (const d of h.disposables) try { d.dispose(); } catch { /* noop */ }
    handlersByMod.delete(modId);
    runtimeByMod.delete(modId);
    onLoadCallbacks.delete(modId);
    onUnloadCallbacks.delete(modId);
    console.log(`[companion] mod ${modId} unloaded`);
  }

  async function shutdown(): Promise<void> {
    clearInterval(tickHandle);
    for (const [modId, fn] of onUnloadCallbacks) {
      const rt = runtimeByMod.get(modId);
      if (rt) await withRuntime(rt, async () => { await fn(); });
    }
    for (const [, h] of handlersByMod) for (const d of h.disposables) try { d.dispose(); } catch { /* noop */ }
  }

  return {
    bindMod, unbindMod, dispatch, shutdown,
    players: () => Array.from(players.values()),
    loadedMods: () => Array.from(handlersByMod.keys()),
  };
}

function safeCall(fn: () => void): void {
  try { fn(); } catch (e) { console.warn(`mod handler error: ${(e as Error).message}`); }
}
