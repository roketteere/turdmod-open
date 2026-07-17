import { describe, it, beforeEach, afterEach } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, rmSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { GuardStore } from "./store.js";
import { buildApp } from "./index.js";
import type { DetectorReport } from "./types.js";

const sample = (over: Partial<DetectorReport> = {}): DetectorReport => ({
  detector: "kill_distance",
  ts: "2026-05-08T12:34:56Z",
  verdict: { Flag: { reason: "AK-47 hit @ 800m", severity: "High", evidence: { distance_m: 812 } } },
  steam: "76561198000000001",
  player: "Alice",
  ...over,
});

describe("GuardStore", () => {
  let dir: string;
  let store: GuardStore;

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "guard-be-"));
    store = new GuardStore(dir);
  });
  afterEach(() => rmSync(dir, { recursive: true, force: true }));

  it("appends a report and reads it back", async () => {
    store.appendReport(sample());
    const page = await store.listReports();
    assert.equal(page.total, 1);
    assert.equal(page.reports[0]!.detector, "kill_distance");
    assert.ok(page.reports[0]!.receivedAt);
  });

  it("filters by steam and detector", async () => {
    store.appendReport(sample({ steam: "A", detector: "kill_distance" }));
    store.appendReport(sample({ steam: "B", detector: "speed" }));
    store.appendReport(sample({ steam: "A", detector: "speed" }));

    assert.equal((await store.listReports({ steam: "A" })).total, 2);
    assert.equal((await store.listReports({ detector: "speed" })).total, 2);
    assert.equal((await store.listReports({ steam: "A", detector: "speed" })).total, 1);
  });

  it("filters by verdict kind across the union shape", async () => {
    store.appendReport(sample({ verdict: "Ok" }));
    store.appendReport(sample({ verdict: { Warn: { reason: "borderline" } } }));
    store.appendReport(sample());

    assert.equal((await store.listReports({ verdict: "Ok" })).total, 1);
    assert.equal((await store.listReports({ verdict: "Warn" })).total, 1);
    assert.equal((await store.listReports({ verdict: "Flag" })).total, 1);
  });

  it("paginates newest-first", async () => {
    for (let i = 0; i < 25; i++) {
      store.appendReport(sample({ player: `P${i}`, ts: `2026-05-08T12:${String(i).padStart(2, "0")}:00Z` }));
    }
    const p1 = await store.listReports({ page: 1, pageSize: 10 });
    assert.equal(p1.total, 25);
    assert.equal(p1.reports.length, 10);
    assert.equal(p1.reports[0]!.player, "P24");
    const p3 = await store.listReports({ page: 3, pageSize: 10 });
    assert.equal(p3.reports.length, 5);
  });

  it("upserts a ban and preserves `since` on edit", () => {
    const a = store.upsertBan({ steam: "S1", reason: "wallhack", sourceServer: "srv-A" });
    assert.equal(a.steam, "S1");
    const since = a.since;

    // Re-ban with a new reason → since should not change.
    const b = store.upsertBan({ steam: "S1", reason: "wallhack confirmed", sourceServer: "srv-A", bannedBy: "joel" });
    assert.equal(b.since, since);
    assert.equal(b.reason, "wallhack confirmed");
    assert.equal(b.bannedBy, "joel");
  });

  it("removes a ban and reports hit/miss", () => {
    store.upsertBan({ steam: "S1", reason: "x", sourceServer: "srv-A" });
    assert.equal(store.removeBan("S1"), true);
    assert.equal(store.removeBan("S1"), false);
    assert.equal(store.checkBan("S1"), null);
  });

  it("checks a ban", () => {
    assert.equal(store.checkBan("never"), null);
    store.upsertBan({ steam: "S2", reason: "speed", sourceServer: "srv-B" });
    const hit = store.checkBan("S2");
    assert.equal(hit?.reason, "speed");
  });

  it("survives a corrupt NDJSON line", async () => {
    store.appendReport(sample({ steam: "ok-1" }));
    // Inject garbage between two valid lines.
    const { appendFileSync } = await import("node:fs");
    appendFileSync(store.reportsPath, "this is not json\n", "utf-8");
    store.appendReport(sample({ steam: "ok-2" }));
    const page = await store.listReports();
    assert.equal(page.total, 2);
  });

  it("persists bans as readable JSON", () => {
    store.upsertBan({ steam: "S3", reason: "teleport", sourceServer: "srv-C" });
    const file = JSON.parse(readFileSync(store.bansPath, "utf-8"));
    assert.equal(file.bans.S3.reason, "teleport");
  });
});

describe("buildApp", () => {
  let dir: string;
  let store: GuardStore;
  let token: string | undefined;

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "guard-be-app-"));
    store = new GuardStore(dir);
    token = "test-token-1234";
  });
  afterEach(() => rmSync(dir, { recursive: true, force: true }));

  const app = () => buildApp({ store, getAdminToken: () => token });

  it("POST /reports stores a report", async () => {
    const res = await app().request("/reports", {
      method: "POST",
      headers: { "content-type": "application/json", "x-guard-server": "srv-A" },
      body: JSON.stringify(sample()),
    });
    assert.equal(res.status, 201);
    const body = (await res.json()) as { ok: boolean; report: { serverId?: string } };
    assert.equal(body.ok, true);
    assert.equal(body.report.serverId, "srv-A");
  });

  it("GET /reports filters", async () => {
    store.appendReport(sample({ steam: "A" }));
    store.appendReport(sample({ steam: "B" }));
    const res = await app().request("/reports?steam=A");
    assert.equal(res.status, 200);
    const body = (await res.json()) as { total: number };
    assert.equal(body.total, 1);
  });

  it("rejects POST /bans without bearer", async () => {
    const res = await app().request("/bans", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ steam: "S", reason: "r", sourceServer: "srv" }),
    });
    assert.equal(res.status, 401);
  });

  it("503 when GUARD_ADMIN_TOKEN unset", async () => {
    token = undefined;
    const res = await app().request("/bans", {
      method: "POST",
      headers: { "content-type": "application/json", authorization: "Bearer whatever" },
      body: JSON.stringify({ steam: "S", reason: "r", sourceServer: "srv" }),
    });
    assert.equal(res.status, 503);
  });

  it("POST /bans then GET /bans/check (public) and DELETE", async () => {
    const post = await app().request("/bans", {
      method: "POST",
      headers: { "content-type": "application/json", authorization: "Bearer test-token-1234" },
      body: JSON.stringify({ steam: "S9", reason: "speed-hack", sourceServer: "srv-A", bannedBy: "joel" }),
    });
    assert.equal(post.status, 201);

    const check = await app().request("/bans/check?steam=S9");
    assert.equal(check.status, 200);
    const cb = (await check.json()) as { banned: boolean; reason?: string };
    assert.equal(cb.banned, true);
    assert.equal(cb.reason, "speed-hack");

    const del = await app().request("/bans/S9", {
      method: "DELETE",
      headers: { authorization: "Bearer test-token-1234" },
    });
    assert.equal(del.status, 204);

    const check2 = await app().request("/bans/check?steam=S9");
    const cb2 = (await check2.json()) as { banned: boolean };
    assert.equal(cb2.banned, false);
  });

  it("GET /health is unauth and reports baseDir", async () => {
    const res = await app().request("/health");
    assert.equal(res.status, 200);
    const body = (await res.json()) as { ok: boolean; baseDir: string };
    assert.equal(body.ok, true);
    assert.equal(body.baseDir, dir);
  });

  it("rejects malformed POST /reports body", async () => {
    const res = await app().request("/reports", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ detector: "x" }), // missing ts + verdict
    });
    assert.equal(res.status, 400);
  });

  it("accepts the bare 'Ok' verdict shape", async () => {
    const res = await app().request("/reports", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ detector: "x", ts: "2026-05-08T00:00:00Z", verdict: "Ok" }),
    });
    assert.equal(res.status, 201);
  });
});
