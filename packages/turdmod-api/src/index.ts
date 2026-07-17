/**
 * @turdmod/turdmod-api — the TurdMOD mod API surface (v1).
 *
 * Mod authors write against these types. The runtime (Strategy C loader,
 * server-side runtime) implements them. The SDK reads these types to
 * scaffold project templates, and the auto-doc generator reads the JSDoc
 * to produce the public API reference.
 *
 * v1 stability: any function tagged `@since 1.0` is part of the v1 contract
 * and stays backward-compatible until v2. Functions tagged `@since-experimental`
 * may change shape between minor releases.
 *
 * This package is interface-only. It carries no runtime — calling the
 * functions in a Node.js process throws `TurdMODError("not running inside
 * the loader")`. The actual implementation is provided by the host
 * (apps/turdmod-loader for Strategy C, apps/turdmod-server for Strategy B).
 */

export * as content from "./content.js";
export * as player from "./player.js";
export * as world from "./world.js";
export * as ui from "./ui.js";
export * as persistence from "./persistence.js";
export * as network from "./network.js";

export { TurdMODError, type Disposable, type Vec3, type RGBA, getRuntime, setRuntime, withRuntime, type Runtime } from "./runtime.js";
