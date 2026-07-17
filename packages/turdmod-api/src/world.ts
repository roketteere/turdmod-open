/**
 * `turdmod.world` — entity spawn/despawn + environment manipulation.
 *
 * Server-authoritative. Strategy C calls into the server when available; in
 * pure offline mode the local player IS the server. POIs added here surface
 * to the scummap interactive map (when published) via the scummap web hook.
 *
 * @since 1.0
 */

import { type Disposable, requireNamespace, TurdMODError, type Vec3 } from "./runtime.js";

export interface SpawnRequest {
  classPath: string;
  position: Vec3;
  rotationYawDegrees?: number;
  /** Server tags applied at spawn time. */
  tags?: ReadonlyArray<string>;
}

export interface SpawnResult {
  /** Stable id for despawn / relocate calls. */
  entityId: string;
}

export interface CustomPOI {
  /** Stable id; mods own the namespace under their own modId. */
  id: string;
  position: Vec3;
  /** Display name; passed through locres if it matches a key. */
  name: string;
  iconClass?: string;     // optional reference to a scummap icon slug
  category?: string;      // optional category for the legend
}

export interface WeatherState {
  /** Internal rate or label. Implementation-defined; pass-through. */
  preset?: string;
  temperatureC?: number;
  humidity?: number;
  precipitationMmHr?: number;
  windKph?: number;
  windHeadingDegrees?: number;
}

interface WorldRuntime {
  spawn(req: SpawnRequest): Promise<SpawnResult>;
  despawn(entityId: string): Promise<boolean>;
  teleport(entityId: string, to: Vec3): Promise<void>;

  setTimeOfDay(hour24: number): Promise<void>;
  setWeather(state: WeatherState): Disposable;
  getWeather(): Promise<WeatherState>;

  addPOI(poi: CustomPOI): Disposable;
  listPOIs(): CustomPOI[];

  /** Fired once per game tick at server rate. Use sparingly. */
  onTick(handler: (deltaMs: number) => void): Disposable;
}

function rt(): WorldRuntime {
  return requireNamespace<WorldRuntime>("world");
}

/** @since 1.0 */
export async function spawn(req: SpawnRequest): Promise<SpawnResult> {
  if (!req.classPath || !req.position) {
    throw new TurdMODError("spawn: classPath and position are required", "BAD_ARGS");
  }
  return rt().spawn(req);
}

/** @since 1.0 */
export async function despawn(entityId: string): Promise<boolean> {
  if (!entityId) throw new TurdMODError("despawn: entityId is required", "BAD_ARGS");
  return rt().despawn(entityId);
}

/** @since 1.0 */
export async function teleport(entityId: string, to: Vec3): Promise<void> {
  if (!entityId || !to) throw new TurdMODError("teleport: entityId and to are required", "BAD_ARGS");
  return rt().teleport(entityId, to);
}

/** @since 1.0 */
export async function setTimeOfDay(hour24: number): Promise<void> {
  if (hour24 < 0 || hour24 >= 24) throw new TurdMODError("setTimeOfDay: hour24 must be [0, 24)", "BAD_HOUR");
  return rt().setTimeOfDay(hour24);
}

/** Set a weather state. Returns disposable; dispose to revert to game-driven weather. @since 1.0 */
export const setWeather = (state: WeatherState): Disposable => rt().setWeather(state);
/** @since 1.0 */
export const getWeather = (): Promise<WeatherState> => rt().getWeather();

/** Surface a custom POI on the map (and the scummap web map when published). @since 1.0 */
export function addPOI(poi: CustomPOI): Disposable {
  if (!poi.id || !poi.position || !poi.name) {
    throw new TurdMODError("addPOI: id, position, and name are required", "BAD_POI");
  }
  return rt().addPOI(poi);
}

/** @since 1.0 */
export const listPOIs = (): CustomPOI[] => rt().listPOIs();

/** @since 1.0 */
export const onTick = (handler: (deltaMs: number) => void): Disposable => rt().onTick(handler);
