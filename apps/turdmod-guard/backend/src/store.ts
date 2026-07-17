/**
 * File-backed storage for the TurdMOD Guard backend MVP.
 *
 * Two stores share a base directory:
 *
 *   <baseDir>/reports.ndjson  — append-only flag history. One JSON object
 *                               per line. Cheap to write, easy to grep, and
 *                               trivially shippable to a real DB later.
 *   <baseDir>/bans.json       — full file rewrite on every edit. Small data
 *                               (one entry per banned steam id), so cost
 *                               isn't a concern, and the all-or-nothing
 *                               write avoids partial-update windows.
 *
 * When we move to Postgres, the NDJSON file becomes the import source for
 * the `guard_reports` table and `bans.json` becomes seed data for
 * `guard_bans`. We're not putting drizzle in here yet because v0 deployments
 * may run on a single VPS without a DB — the file store keeps that path
 * open.
 */

import { existsSync, mkdirSync, readFileSync, writeFileSync, appendFileSync, createReadStream } from "node:fs";
import { join } from "node:path";
import { createInterface } from "node:readline";
import {
  type BanRecord,
  type DetectorReport,
  type StoredReport,
  type Verdict,
  verdictKind,
} from "./types.js";

export interface ReportQuery {
  steam?: string;
  detector?: string;
  /** "Ok" / "Warn" / "Flag" — case-sensitive, matches the verdict kind. */
  verdict?: "Ok" | "Warn" | "Flag";
  /** 1-indexed page (default 1). */
  page?: number;
  /** Defaults to 50, capped at 500. */
  pageSize?: number;
}

export interface ReportPage {
  reports: StoredReport[];
  page: number;
  pageSize: number;
  /** Total across the whole file (after filters). Returned for UI paging. */
  total: number;
}

interface BanFile {
  bans: Record<string, BanRecord>;
}

const EMPTY_BAN_FILE: BanFile = { bans: {} };

export class GuardStore {
  readonly reportsPath: string;
  readonly bansPath: string;

  constructor(public readonly baseDir: string) {
    mkdirSync(baseDir, { recursive: true });
    this.reportsPath = join(baseDir, "reports.ndjson");
    this.bansPath = join(baseDir, "bans.json");
    if (!existsSync(this.reportsPath)) {
      writeFileSync(this.reportsPath, "", "utf-8");
    }
    if (!existsSync(this.bansPath)) {
      writeFileSync(this.bansPath, JSON.stringify(EMPTY_BAN_FILE, null, 2), "utf-8");
    }
  }

  // ----- Reports -----

  appendReport(report: DetectorReport): StoredReport {
    const stored: StoredReport = { ...report, receivedAt: new Date().toISOString() };
    appendFileSync(this.reportsPath, JSON.stringify(stored) + "\n", "utf-8");
    return stored;
  }

  async listReports(q: ReportQuery = {}): Promise<ReportPage> {
    const page = Math.max(1, q.page ?? 1);
    const pageSize = Math.min(500, Math.max(1, q.pageSize ?? 50));

    const matched: StoredReport[] = [];
    for await (const r of this.iterReports()) {
      if (q.steam && r.steam !== q.steam) continue;
      if (q.detector && r.detector !== q.detector) continue;
      if (q.verdict && verdictKind(r.verdict as Verdict) !== q.verdict) continue;
      matched.push(r);
    }

    // Newest first — order in the file is append-time chronological.
    matched.reverse();

    const start = (page - 1) * pageSize;
    const reports = matched.slice(start, start + pageSize);
    return { reports, page, pageSize, total: matched.length };
  }

  /** Iterate every stored report. Used for tests + the page endpoint. */
  async *iterReports(): AsyncGenerator<StoredReport> {
    if (!existsSync(this.reportsPath)) return;
    const stream = createReadStream(this.reportsPath, { encoding: "utf-8" });
    const rl = createInterface({ input: stream, crlfDelay: Infinity });
    for await (const line of rl) {
      const trimmed = line.trim();
      if (!trimmed) continue;
      try {
        yield JSON.parse(trimmed) as StoredReport;
      } catch {
        // Tolerate the rare corrupt line (partial write, manual edit) —
        // we'd rather skip a bad row than fail the whole listing.
      }
    }
  }

  // ----- Bans -----

  private readBans(): BanFile {
    try {
      return JSON.parse(readFileSync(this.bansPath, "utf-8")) as BanFile;
    } catch {
      return { bans: {} };
    }
  }

  private writeBans(file: BanFile): void {
    writeFileSync(this.bansPath, JSON.stringify(file, null, 2), "utf-8");
  }

  upsertBan(input: { steam: string; reason: string; sourceServer: string; bannedBy?: string }): BanRecord {
    const file = this.readBans();
    const record: BanRecord = {
      steam: input.steam,
      reason: input.reason,
      sourceServer: input.sourceServer,
      bannedBy: input.bannedBy,
      since: file.bans[input.steam]?.since ?? new Date().toISOString(),
    };
    file.bans[input.steam] = record;
    this.writeBans(file);
    return record;
  }

  removeBan(steam: string): boolean {
    const file = this.readBans();
    if (!file.bans[steam]) return false;
    delete file.bans[steam];
    this.writeBans(file);
    return true;
  }

  checkBan(steam: string): BanRecord | null {
    return this.readBans().bans[steam] ?? null;
  }

  listBans(): BanRecord[] {
    return Object.values(this.readBans().bans).sort((a, b) => a.since.localeCompare(b.since));
  }
}

export function defaultBaseDir(): string {
  return join(process.cwd(), "data", "guard-backend");
}
