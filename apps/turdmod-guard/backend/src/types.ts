/**
 * Wire types for the TurdMOD Guard backend.
 *
 * `DetectorReport` mirrors the Rust struct in
 * `apps/turdmod-guard/src/detectors/mod.rs`. Serde's default external-tagged
 * enum representation means a `Verdict::Ok` variant serializes as the bare
 * string `"Ok"`, and the struct variants `Warn { reason }` /
 * `Flag { reason, severity, evidence }` serialize as
 * `{ "Warn": { ... } }` / `{ "Flag": { ... } }`. We accept both cases here.
 *
 * Severity is the externally-tagged variant of the Rust `Severity` enum —
 * because all variants are unit, serde renders them as bare strings
 * `"Low" | "Medium" | "High" | "Critical"`.
 */

import { z } from "zod";

export const severitySchema = z.enum(["Low", "Medium", "High", "Critical"]);
export type Severity = z.infer<typeof severitySchema>;

export const verdictSchema = z.union([
  z.literal("Ok"),
  z.object({
    Warn: z.object({ reason: z.string() }),
  }),
  z.object({
    Flag: z.object({
      reason: z.string(),
      severity: severitySchema,
      evidence: z.unknown(),
    }),
  }),
]);
export type Verdict = z.infer<typeof verdictSchema>;

/**
 * Body accepted on POST /reports. The Rust daemon serializes
 * `DetectorReport` directly with serde, so this matches that wire shape.
 *
 * The `serverId` field is added on the receiving side — guard daemons can
 * supply it via header (`x-guard-server`) to tag which server produced the
 * report. It's optional in the body so old daemons keep working.
 */
export const detectorReportSchema = z.object({
  detector: z.string().min(1),
  ts: z.string().min(1),
  verdict: verdictSchema,
  steam: z.string().nullable().optional(),
  player: z.string().nullable().optional(),
  serverId: z.string().optional(),
});
export type DetectorReport = z.infer<typeof detectorReportSchema>;

/**
 * What we persist (one line per report in NDJSON). Keeps the original wire
 * shape plus a backend-assigned `receivedAt` for auditing clock skew.
 */
export interface StoredReport extends DetectorReport {
  receivedAt: string;
}

export const banRecordSchema = z.object({
  steam: z.string().min(1),
  reason: z.string().min(1).max(2000),
  /** Server that originally banned the player — free-form identifier. */
  sourceServer: z.string().min(1),
  /** Optional admin / actor name for audit. */
  bannedBy: z.string().optional(),
  since: z.string(),
});
export type BanRecord = z.infer<typeof banRecordSchema>;

export const verdictKind = (v: Verdict): "Ok" | "Warn" | "Flag" => {
  if (v === "Ok") return "Ok";
  if ("Warn" in v) return "Warn";
  return "Flag";
};
