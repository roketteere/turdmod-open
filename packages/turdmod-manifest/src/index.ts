/**
 * TurdMOD manifest spec v1 — canonical schema for mod sidecars and manifests.
 *
 * Used by:
 * - apps/turdmod-cli — validates at install time and via `turdmod validate`
 * - apps/turdmod-loader (future) — validates at load/inject time
 * - apps/turdmod-registry (future) — validates on publish
 *
 * See docs/turdmod/manifest-spec.md for the human-readable spec.
 */

import { z } from "zod";

// ---------- primitive shapes ----------

/** Mod identifier: lowercase, kebab-case, must start with a letter. */
export const ModIdSchema = z
  .string()
  .min(2)
  .max(64)
  .regex(/^[a-z][a-z0-9-]*[a-z0-9]$/, "must be lowercase kebab-case starting with a letter");

/** Semver-ish: looser than full semver to be friendly to authors. */
export const VersionSchema = z
  .string()
  .min(1)
  .max(32)
  .regex(/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/, "must be like 1.2.3 or 1.2.3-beta.1");

/** SCUM buildId from the manifest.json (e.g. "23128448"). Empty = no bound. */
export const BuildIdSchema = z.string().regex(/^\d{4,12}$/, "must be the numeric SCUM buildId").or(z.literal(""));

/**
 * Delivery strategies. Mirrors docs/turdmod/compatibility-policy.md.
 * - pak-content: drop a .pak into ~mods/. UE 4.27 native pak loading. No injection.
 * - server-side: runs in the dedicated server process.
 * - offline-only: DLL + Lua, refuses to inject when BattlEye is active.
 * - external-tool: companion app, never touches the game process.
 */
export const ModeSchema = z.enum(["pak-content", "server-side", "offline-only", "external-tool"]);

/** Capabilities a mod can declare. Sandbox enforces these at runtime. */
export const CapabilitySchema = z.union([
  z.object({ filesystem: z.enum(["none", "read", "readwrite"]) }),
  z.object({ network: z.enum(["none", "localhost", "any"]) }),
  z.object({ process: z.enum(["none"]) }),
]);

// ---------- the manifest ----------

/**
 * The full manifest a mod author writes.
 * Lives at the root of a mod project as `turdmod.json` and inside the published
 * pak as `<id>.turdmod.json` (the sidecar in apps/turdmod-cli/#135).
 */
export const ManifestSchema = z
  .object({
    /** Manifest schema version. Bump when we make breaking changes. */
    schema: z.literal(1).default(1),

    id: ModIdSchema,
    name: z.string().min(1).max(120),
    version: VersionSchema,
    author: z.string().min(1).max(120).optional(),
    description: z.string().max(2000).optional(),

    mode: ModeSchema,

    /** SCUM compatibility window. Empty `maxBuild` means no upper bound. */
    minBuild: BuildIdSchema.optional(),
    maxBuild: BuildIdSchema.optional(),

    /** Other mods this one depends on (id ranges). */
    dependencies: z.record(ModIdSchema, VersionSchema).default({}),

    /** Sandbox capability declarations — only meaningful for offline-only/server-side modes. */
    capabilities: z.array(CapabilitySchema).default([]),

    /** Mode-specific entrypoint hints (Lua files, server modules, etc.). */
    entrypoint: z.string().max(512).optional(),

    /** Author-supplied tags, used by the registry (KTask #151) for browsing. */
    tags: z.array(z.string().max(40)).max(16).default([]),

    /** Author-supplied homepage / repo link. */
    homepage: z.string().url().optional(),

    /**
     * Declarative config schema. If present, TurdMOD Manager renders a
     * visual form that writes to <mod-dir>/mod.config.json. Mods read that
     * file themselves at startup.
     */
    configSchema: z
      .array(
        z.discriminatedUnion("type", [
          z.object({
            type: z.literal("string"),
            key: z.string().min(1).max(64),
            default: z.string(),
            label: z.string().max(120).optional(),
            description: z.string().max(500).optional(),
          }),
          z.object({
            type: z.literal("number"),
            key: z.string().min(1).max(64),
            default: z.number(),
            label: z.string().max(120).optional(),
            description: z.string().max(500).optional(),
            min: z.number().optional(),
            max: z.number().optional(),
          }),
          z.object({
            type: z.literal("boolean"),
            key: z.string().min(1).max(64),
            default: z.boolean(),
            label: z.string().max(120).optional(),
            description: z.string().max(500).optional(),
          }),
          z.object({
            type: z.literal("enum"),
            key: z.string().min(1).max(64),
            default: z.string(),
            options: z.array(z.string().max(64)).min(1).max(32),
            label: z.string().max(120).optional(),
            description: z.string().max(500).optional(),
          }),
        ]),
      )
      .max(64)
      .optional(),
  })
  .strict();

export type Manifest = z.infer<typeof ManifestSchema>;

/** A single field in a mod's configSchema. */
export type ConfigField = NonNullable<Manifest["configSchema"]>[number];

// ---------- the sidecar ----------

/**
 * Sidecar metadata written next to a pak inside SCUM/Content/Paks/~mods/.
 * Superset of Manifest with install-time fields the CLI captures.
 */
export const SidecarSchema = ManifestSchema.extend({
  /** ISO-8601 timestamp from `turdmod install`. */
  installedAt: z.string().datetime(),
  /** Original path the pak was copied from, for audit. */
  sourcePath: z.string().max(2048).optional(),
}).strict();

export type Sidecar = z.infer<typeof SidecarSchema>;

// ---------- helpers ----------

export type ValidationResult<T> =
  | { ok: true; value: T }
  | { ok: false; errors: { path: string; message: string }[] };

/** Parse + validate an unknown value against the manifest schema. */
export function validateManifest(input: unknown): ValidationResult<Manifest> {
  const r = ManifestSchema.safeParse(input);
  return r.success
    ? { ok: true, value: r.data }
    : { ok: false, errors: r.error.errors.map(e => ({ path: e.path.join("."), message: e.message })) };
}

/** Parse + validate an unknown value against the sidecar schema. */
export function validateSidecar(input: unknown): ValidationResult<Sidecar> {
  const r = SidecarSchema.safeParse(input);
  return r.success
    ? { ok: true, value: r.data }
    : { ok: false, errors: r.error.errors.map(e => ({ path: e.path.join("."), message: e.message })) };
}

/** Whether a mod's compatibility window covers a given SCUM buildId. */
export function isCompatibleWithBuild(m: Pick<Manifest, "minBuild" | "maxBuild">, buildId: string): boolean {
  const n = Number(buildId);
  if (!Number.isFinite(n)) return false;
  if (m.minBuild && m.minBuild !== "" && Number(m.minBuild) > n) return false;
  if (m.maxBuild && m.maxBuild !== "" && Number(m.maxBuild) < n) return false;
  return true;
}

/**
 * Whether a mod's mode is allowed in the given environment.
 * Mirrors the matrix in docs/turdmod/compatibility-policy.md.
 */
export type Environment =
  | "solo"
  | "private-be-off"
  | "private-be-on"
  | "official";

export function isModeAllowedInEnvironment(mode: Manifest["mode"], env: Environment): boolean {
  switch (env) {
    case "solo":
      return mode === "pak-content" || mode === "offline-only" || mode === "external-tool";
    case "private-be-off":
      return true; // pak-content, server-side, offline-only, external-tool — all allowed
    case "private-be-on":
      return mode === "pak-content" || mode === "server-side" || mode === "external-tool";
    case "official":
      return mode === "external-tool";
  }
}
