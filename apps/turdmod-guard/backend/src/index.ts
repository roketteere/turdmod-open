/**
 * TurdMOD Guard backend — Hono REST service.
 *
 * Endpoints:
 *
 *   GET    /health                          { ok, ts, baseDir }
 *
 *   POST   /reports                         body: DetectorReport
 *                                           201 { ok: true, report }
 *   GET    /reports?steam=&detector=&...    paginated list
 *
 *   POST   /bans   (admin)                  body: { steam, reason, sourceServer, bannedBy? }
 *                                           201 { ok: true, ban }
 *   DELETE /bans/:steam   (admin)           204 on hit, 404 on miss
 *   GET    /bans          (admin)           full ban list
 *   GET    /bans/check?steam=…              public; { banned: bool, ban? }
 *
 * Auth: admin endpoints require `Authorization: Bearer $GUARD_ADMIN_TOKEN`.
 * The lookup endpoint (`/bans/check`) is unauthenticated by design — every
 * server's guard daemon has to be able to verify a player on login without
 * being granted ban-list edit privileges.
 *
 * Port handling: PORT env var if set; otherwise we let the OS pick a free
 * port and write `~/.scummy-map/turdmod-guard-backend.json` so other tools
 * (the Rust daemon, the future admin dashboard) can discover us. The
 * discovery file is removed on graceful exit.
 *
 * Storage: file-backed for v0 (see store.ts). `data/guard-backend/` under
 * the cwd by default; override with `GUARD_BACKEND_DIR`.
 */

import { serve } from "@hono/node-server";
import { Hono } from "hono";
import { logger } from "hono/logger";
import { z } from "zod";
import { zValidator } from "@hono/zod-validator";
import { mkdirSync, unlinkSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { GuardStore, defaultBaseDir } from "./store.js";
import { detectorReportSchema } from "./types.js";
import { requireAdmin } from "./auth.js";

export interface BuildAppOptions {
  store: GuardStore;
  /**
   * Token getter — read at request time so tests can swap it without
   * rebuilding the app. Returns `undefined` when unset.
   */
  getAdminToken: () => string | undefined;
}

export function buildApp(opts: BuildAppOptions): Hono {
  const { store, getAdminToken } = opts;
  const app = new Hono();
  app.use(logger());

  app.get("/health", (c) =>
    c.json({ ok: true, ts: new Date().toISOString(), baseDir: store.baseDir }),
  );

  // ----- Reports -----

  app.post("/reports", zValidator("json", detectorReportSchema), (c) => {
    const body = c.req.valid("json");
    // Pull the optional server tag from the header if the body didn't
    // include it. Lets the Rust daemon set `x-guard-server: <name>` once
    // at startup instead of stamping every report.
    const serverHeader = c.req.header("x-guard-server");
    const serverId = body.serverId ?? serverHeader;
    const stored = store.appendReport({ ...body, serverId });
    return c.json({ ok: true, report: stored }, 201);
  });

  const listReportsQuery = z.object({
    steam: z.string().optional(),
    detector: z.string().optional(),
    verdict: z.enum(["Ok", "Warn", "Flag"]).optional(),
    page: z.coerce.number().int().positive().optional(),
    pageSize: z.coerce.number().int().positive().max(500).optional(),
  });

  app.get("/reports", zValidator("query", listReportsQuery), async (c) => {
    const q = c.req.valid("query");
    const result = await store.listReports(q);
    return c.json(result);
  });

  // ----- Bans -----

  const adminGuard = requireAdmin(getAdminToken);

  const upsertBanBody = z.object({
    steam: z.string().min(1),
    reason: z.string().min(1).max(2000),
    sourceServer: z.string().min(1),
    bannedBy: z.string().optional(),
  });

  app.post("/bans", adminGuard, zValidator("json", upsertBanBody), (c) => {
    const ban = store.upsertBan(c.req.valid("json"));
    return c.json({ ok: true, ban }, 201);
  });

  app.delete("/bans/:steam", adminGuard, (c) => {
    const steam = c.req.param("steam");
    const removed = store.removeBan(steam);
    if (!removed) return c.json({ error: "not_found" }, 404);
    return c.body(null, 204);
  });

  app.get("/bans", adminGuard, (c) => c.json({ bans: store.listBans() }));

  const checkQuery = z.object({ steam: z.string().min(1) });
  app.get("/bans/check", zValidator("query", checkQuery), (c) => {
    const { steam } = c.req.valid("query");
    const ban = store.checkBan(steam);
    if (!ban) return c.json({ banned: false });
    return c.json({
      banned: true,
      reason: ban.reason,
      since: ban.since,
      sourceServer: ban.sourceServer,
    });
  });

  app.notFound((c) => c.json({ error: "not_found" }, 404));
  app.onError((err, c) => {
    console.error("[guard-backend]", err);
    return c.json({ error: "internal_error" }, 500);
  });

  return app;
}

// ----- Entrypoint wiring -----

const DISCOVERY_DIR = join(homedir(), ".scummy-map");
const DISCOVERY_FILE = join(DISCOVERY_DIR, "turdmod-guard-backend.json");

function writeDiscovery(port: number): void {
  mkdirSync(DISCOVERY_DIR, { recursive: true });
  const payload = {
    app: "turdmod-guard-backend",
    port,
    url: `http://localhost:${port}`,
    pid: process.pid,
    startedAt: new Date().toISOString(),
  };
  writeFileSync(DISCOVERY_FILE, JSON.stringify(payload, null, 2), "utf-8");
}

function clearDiscovery(): void {
  try {
    unlinkSync(DISCOVERY_FILE);
  } catch {
    /* ignore */
  }
}

function isMain(): boolean {
  const argv = process.argv[1];
  if (!argv) return false;
  const url = `file://${argv.replace(/\\/g, "/")}`;
  if (import.meta.url === url) return true;
  return argv.endsWith("index.ts") || argv.endsWith("index.js");
}

if (isMain()) {
  const baseDir = process.env.GUARD_BACKEND_DIR || defaultBaseDir();
  const store = new GuardStore(baseDir);
  const port = Number(process.env.PORT || 0); // 0 → OS picks a free port

  const app = buildApp({
    store,
    getAdminToken: () => process.env.GUARD_ADMIN_TOKEN,
  });

  const server = serve({ fetch: app.fetch, port }, (info) => {
    writeDiscovery(info.port);
    console.log(`[guard-backend] listening on http://localhost:${info.port}`);
    console.log(`[guard-backend] base dir: ${baseDir}`);
    console.log(`[guard-backend] discovery: ${DISCOVERY_FILE}`);
    if (!process.env.GUARD_ADMIN_TOKEN) {
      console.warn("[guard-backend] GUARD_ADMIN_TOKEN unset — admin endpoints will return 503");
    }
  });

  const shutdown = (signal: string) => {
    console.log(`[guard-backend] received ${signal}, shutting down`);
    clearDiscovery();
    server.close(() => process.exit(0));
    // Hard fallback if close hangs.
    setTimeout(() => process.exit(0), 2000).unref();
  };
  process.on("SIGINT", () => shutdown("SIGINT"));
  process.on("SIGTERM", () => shutdown("SIGTERM"));
  process.on("exit", () => clearDiscovery());
}
