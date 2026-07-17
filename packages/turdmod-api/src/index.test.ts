/**
 * Surface-area tests — these don't validate runtime behaviour (the runtime
 * isn't bound), they validate that:
 * 1. Calling any namespace function without a runtime throws TurdMODError.
 * 2. Setting a stub runtime with the right namespaces lets calls through.
 * 3. Argument-validation rejects bad inputs before reaching the runtime.
 */

import { describe, it, beforeEach, afterEach } from "node:test";
import assert from "node:assert/strict";
import {
  content, player, world, ui, persistence, network,
  setRuntime, TurdMODError, type Runtime,
} from "./index.js";

function noopDisposable() { return { dispose() { /* noop */ } }; }

const STUB_RUNTIME: Runtime = {
  modId: "test-mod",
  buildId: "23128448",
  capabilities: [],
  log: () => {},
  content: {
    setDataTableOverride: () => noopDisposable(),
    getDataTableRow: async () => null,
    setBlueprintOverride: () => noopDisposable(),
    setLocresOverride: () => noopDisposable(),
    listOverrides: () => ({ dataTables: [], blueprints: [], locres: [] }),
  },
  player: {
    list: () => [],
    getState: async () => null,
    getInventory: async () => [],
    giveItem: async () => {},
    removeItem: async () => 0,
    onSpawn: () => noopDisposable(),
    onDeath: () => noopDisposable(),
    onDamage: () => noopDisposable(),
    onInventoryChange: () => noopDisposable(),
  },
  world: {
    spawn: async () => ({ entityId: "e1" }),
    despawn: async () => true,
    teleport: async () => {},
    setTimeOfDay: async () => {},
    setWeather: () => noopDisposable(),
    getWeather: async () => ({}),
    addPOI: () => noopDisposable(),
    listPOIs: () => [],
    onTick: () => noopDisposable(),
  },
  ui: {
    setHud: () => noopDisposable(),
    removeHud: () => true,
    showMenu: () => noopDisposable(),
    closeMenu: () => true,
    bindHotkey: () => noopDisposable(),
    toast: () => {},
  },
  persistence: {
    get: async () => null,
    set: async () => {},
    keys: async () => [],
    clearAll: async () => {},
    getForPlayer: async () => null,
    setForPlayer: async () => {},
  },
  network: {
    on: () => noopDisposable(),
    broadcast: async () => {},
    send: async () => {},
    rpc: async () => null,
  },
};

describe("runtime binding", () => {
  beforeEach(() => setRuntime(null));
  afterEach(() => setRuntime(null));

  it("throws TurdMODError when runtime is unbound", () => {
    assert.throws(() => content.listOverrides(), (e: unknown) => e instanceof TurdMODError && (e as TurdMODError).code === "NO_RUNTIME");
  });

  it("succeeds once runtime is bound", () => {
    setRuntime(STUB_RUNTIME);
    assert.deepEqual(content.listOverrides(), { dataTables: [], blueprints: [], locres: [] });
    assert.deepEqual(player.list(), []);
    assert.deepEqual(world.listPOIs(), []);
  });
});

describe("argument validation", () => {
  beforeEach(() => setRuntime(STUB_RUNTIME));
  afterEach(() => setRuntime(null));

  it("content.setDataTableOverride rejects missing ref fields", () => {
    assert.throws(
      () => content.setDataTableOverride({ ref: { table: "", row: "" }, values: {} }),
      (e: unknown) => e instanceof TurdMODError,
    );
  });

  it("player.removeItem rejects non-positive count", async () => {
    await assert.rejects(
      () => player.removeItem("p1", "Item", 0),
      (e: unknown) => e instanceof TurdMODError,
    );
  });

  it("world.setTimeOfDay rejects out-of-range hour", async () => {
    await assert.rejects(
      () => world.setTimeOfDay(24),
      (e: unknown) => e instanceof TurdMODError,
    );
  });

  it("ui.setHud rejects bad anchor", () => {
    assert.throws(
      () => ui.setHud({ id: "x", text: "y", anchor: { x: 1.5, y: 0 } }),
      (e: unknown) => e instanceof TurdMODError,
    );
  });

  it("persistence.set rejects empty key", async () => {
    await assert.rejects(
      () => persistence.set("", { x: 1 }),
      (e: unknown) => e instanceof TurdMODError,
    );
  });

  it("network channels must match the regex", () => {
    assert.throws(
      () => network.on("Bad Channel With Spaces", () => {}),
      (e: unknown) => e instanceof TurdMODError,
    );
  });

  it("network channels accept the dotted form", () => {
    const d = network.on("my-mod.events.player_joined", () => {});
    d.dispose();
  });
});
