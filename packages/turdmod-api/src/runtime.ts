/**
 * Runtime contract — the host (loader / server runtime) injects an
 * implementation of `Runtime`; every API namespace reads from it via
 * `getRuntime()`. In a non-host process (e.g. a mod author writing tests),
 * `getRuntime()` throws unless they've called `setRuntime(stub)` with a
 * test double.
 *
 * Uses AsyncLocalStorage so async work spawned from a handler retains
 * access to the same Runtime that was bound when the handler was invoked.
 * The host calls `withRuntime(rt, () => fn())` around each event dispatch;
 * everything that fn() does — synchronous or asynchronous — sees that
 * runtime. The simpler `setRuntime(rt)` form still works for module-global
 * binding (tests, single-mod hosts).
 */

export class TurdMODError extends Error {
  constructor(message: string, public readonly code?: string) {
    super(message);
    this.name = "TurdMODError";
  }
}

/** Anything that can be released later (subscription handle). */
export interface Disposable {
  dispose(): void;
}

/** SCUM uses centimeters; +X = west, +Y = north (per scummap apps/web/src/lib/coords.ts). */
export interface Vec3 {
  x: number;
  y: number;
  z: number;
}

export interface RGBA {
  r: number; g: number; b: number; a?: number;
}

/**
 * Runtime is the union of all namespace-specific runtime hooks. The host
 * binds one implementation; `getRuntime()` returns it. We intentionally
 * keep every method asynchronous — Strategy B (server-side) needs RPC,
 * Strategy C (in-game) does not, but uniform shape lets the SDK paper
 * over the difference.
 */
export interface Runtime {
  // Lifecycle.
  readonly modId: string;
  readonly buildId: string;
  log(level: "trace" | "debug" | "info" | "warn" | "error", msg: string, data?: unknown): void;

  // Capabilities — populated from the manifest at load time.
  readonly capabilities: ReadonlyArray<string>;

  // Namespace-specific extensions injected by the host. Each namespace
  // module reads from `runtime[<namespace>]` to find its impl.
  // Intentionally typed as `unknown` here so each namespace can declare
  // its own concrete shape without the runtime contract knowing all of them.
  readonly content?: unknown;
  readonly player?: unknown;
  readonly world?: unknown;
  readonly ui?: unknown;
  readonly persistence?: unknown;
  readonly network?: unknown;
}

import { AsyncLocalStorage } from "node:async_hooks";

let _moduleRuntime: Runtime | null = null;
const _storage = new AsyncLocalStorage<Runtime>();

// globalThis bridge — ESM module caching with pnpm symlinks can create
// separate module instances of this file. globalThis is the one truly
// shared space across all instances in the same process.
const _G = globalThis as unknown as { __turdmod_runtime?: Runtime | null };

/** Module-level binding (works for synchronous tests + single-mod hosts). */
export function setRuntime(rt: Runtime | null): void {
  _moduleRuntime = rt;
  _G.__turdmod_runtime = rt;
}

/**
 * Run `fn` with the given runtime visible to anything called from inside it
 * (including any async work). Multi-mod hosts use this to isolate per-mod
 * runtime visibility so async handlers don't see each other's bindings.
 */
export function withRuntime<T>(rt: Runtime, fn: () => T): T {
  return _storage.run(rt, fn);
}

export function getRuntime(): Runtime {
  const rt = _storage.getStore() ?? _moduleRuntime ?? _G.__turdmod_runtime;
  if (!rt) {
    throw new TurdMODError(
      "TurdMOD runtime is not bound. This API can only be called from inside the TurdMOD loader (Strategy B/C). For tests, call setRuntime(stub) first.",
      "NO_RUNTIME",
    );
  }
  return rt;
}

/** Type-narrow helper for namespace impls. */
export function requireNamespace<T>(name: keyof Runtime): T {
  const rt = getRuntime();
  const ns = rt[name];
  if (!ns) {
    throw new TurdMODError(
      `Namespace '${String(name)}' is not provided by the current runtime. The host (Strategy B/C) must populate it.`,
      "NO_NAMESPACE",
    );
  }
  return ns as T;
}
