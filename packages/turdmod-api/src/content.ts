/**
 * `turdmod.content` — DataTable / Blueprint / locres overrides.
 *
 * The Content namespace is what most "I want to change loot rates" / "I want
 * to add a new recipe" / "I want to retext this asset" mods touch. It maps
 * cleanly to UE 4.27's data-driven systems:
 *
 * - DataTable overrides: write into rows of `ItemSpawningParameters`,
 *   `ILTN_*`, recipe tables, etc. Pak content mods (Strategy A) do this
 *   declaratively at startup; scripted mods (Strategy C) do it at runtime.
 * - Blueprint overrides: replace a Default__BP_*_C value (e.g. raise a
 *   container's max-loot-tier).
 * - Locres injects: add or replace strings in a locale (key + value).
 *
 * Strategy A (pak content) provides a declarative version of all three via
 * the manifest; this scripted form is for mods that need conditional logic
 * ("if event X, swap loot table Y") and is only available to Strategy B/C.
 *
 * @since 1.0
 */

import { type Disposable, requireNamespace, TurdMODError } from "./runtime.js";

/** Identifier for an item / actor / blueprint class as it lives in the .pak. */
export type ClassPath = string;

/** Identifier for a row inside a DataTable, e.g. "MemoryModule_Level1". */
export type RowName = string;

/** A DataTable identifier — usually the asset path inside the pak. */
export interface DataTableRef {
  table: string;          // e.g. "ItemSpawningParameters" or full asset path
  row: RowName;
}

export interface DataTableOverride {
  ref: DataTableRef;
  /** Sparse override — only keys you set are changed; everything else stays. */
  values: Record<string, unknown>;
}

export interface BlueprintOverride {
  /** Class path of the BP, e.g. "BP_DepositorySmall_C". */
  klass: ClassPath;
  /** Sparse property map. */
  values: Record<string, unknown>;
}

export interface LocresOverride {
  /** Locale code, e.g. "en-US". */
  locale: string;
  /** Locres key, e.g. "UI_Items::MemoryModuleLevel4". */
  key: string;
  value: string;
}

interface ContentRuntime {
  setDataTableOverride(o: DataTableOverride): Disposable;
  getDataTableRow(ref: DataTableRef): Promise<Record<string, unknown> | null>;
  setBlueprintOverride(o: BlueprintOverride): Disposable;
  setLocresOverride(o: LocresOverride): Disposable;
  /** List every override the current mod has registered; useful for /turdmod debug. */
  listOverrides(): {
    dataTables: DataTableOverride[];
    blueprints: BlueprintOverride[];
    locres: LocresOverride[];
  };
}

function rt(): ContentRuntime {
  return requireNamespace<ContentRuntime>("content");
}

/**
 * Override one or more fields on a DataTable row. Returns a disposable —
 * call `.dispose()` to revert.
 *
 * @example
 * import { content } from "@turdmod/turdmod-api";
 * content.setDataTableOverride({
 *   ref: { table: "ItemSpawningParameters", row: "MemoryModule_Level4" },
 *   values: { MaxOccurrences: 99 },
 * });
 *
 * @since 1.0
 */
export function setDataTableOverride(o: DataTableOverride): Disposable {
  validateDataTableOverride(o);
  return rt().setDataTableOverride(o);
}

/**
 * Read a DataTable row's current effective values (after any active overrides).
 * Returns null if the row doesn't exist.
 *
 * @since 1.0
 */
export async function getDataTableRow(ref: DataTableRef): Promise<Record<string, unknown> | null> {
  if (!ref.table || !ref.row) {
    throw new TurdMODError("getDataTableRow: ref.table and ref.row are required", "BAD_REF");
  }
  return rt().getDataTableRow(ref);
}

/**
 * Override one or more fields on a Blueprint's Class Default Object.
 *
 * @example
 * content.setBlueprintOverride({
 *   klass: "BP_DepositorySmall_C",
 *   values: { MaxLootTier: 4 },
 * });
 *
 * @since 1.0
 */
export function setBlueprintOverride(o: BlueprintOverride): Disposable {
  if (!o.klass) throw new TurdMODError("setBlueprintOverride: klass is required", "BAD_OVERRIDE");
  return rt().setBlueprintOverride(o);
}

/**
 * Add or replace a localized string for a given locale.
 *
 * @since 1.0
 */
export function setLocresOverride(o: LocresOverride): Disposable {
  if (!o.locale || !o.key || typeof o.value !== "string") {
    throw new TurdMODError("setLocresOverride: locale, key, and value are required", "BAD_LOCRES");
  }
  return rt().setLocresOverride(o);
}

/** Listing of every override registered by the current mod. Useful for debug. */
export function listOverrides() {
  return rt().listOverrides();
}

function validateDataTableOverride(o: DataTableOverride): void {
  if (!o?.ref?.table || !o?.ref?.row) {
    throw new TurdMODError("DataTableOverride.ref.table and ref.row are required", "BAD_OVERRIDE");
  }
  if (!o.values || typeof o.values !== "object") {
    throw new TurdMODError("DataTableOverride.values must be an object", "BAD_OVERRIDE");
  }
}
