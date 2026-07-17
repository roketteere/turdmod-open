/**
 * `turdmod-companion verify <mod-id>` — smoke-test a server-side mod.
 *
 * Runs the mod through a self-contained companion runtime, dispatches one
 * synthetic event of every kind the live runtime emits, and reports:
 *
 *   - load result (manifest valid, entry resolvable, on_load runs clean)
 *   - per-kind event count + handler invocations
 *   - every `network.broadcast` the mod made (channel + payload preview)
 *   - persistence keys the mod read or wrote during the run
 *   - any errors caught from inside mod handlers
 *
 * Exits 0 on PASS, 1 on FAIL. The `--json` flag emits a machine-readable
 * report for CI / contract evidence packs.
 */

import { existsSync, readdirSync, readFileSync, mkdtempSync, rmSync } from "node:fs";
import { resolve, join } from "node:path";
import { tmpdir } from "node:os";
import { pathToFileURL } from "node:url";
import { validateManifest } from "@turdmod/turdmod-manifest";
import { createCompanionRuntime } from "./runtime.js";
import { SinkRouter, type Sink, type SinkContext, type SinkPayload } from "./sinks.js";
import type { ServerEvent } from "./parsers.js";

interface VerifyArgs {
  modId: string;
  modsDir: string;
  buildId: string;
  json: boolean;
}

function parseArgs(argv: string[]): VerifyArgs {
  let modId: string | undefined;
  let modsDir: string | undefined;
  let buildId = "23128448";
  let json = false;
  for (let i = 2; i < argv.length; i++) {
    const a = argv[i]!;
    if (a === "--mods-dir") modsDir = argv[++i] ?? modsDir;
    else if (a === "--build-id") buildId = argv[++i] ?? buildId;
    else if (a === "--json") json = true;
    else if (a === "-h" || a === "--help") { usage(); process.exit(0); }
    else if (!a.startsWith("--") && !modId) modId = a;
  }
  if (!modId) { usage(); process.exit(1); }
  if (!modsDir) {
    const repoRoot = resolve(import.meta.url.replace(/^file:\/+/, "/").replace(/^\/+([A-Za-z]:)/, "$1"), "..", "..", "..", "..");
    modsDir = process.env.TURDMOD_MODS_DIR ? resolve(process.env.TURDMOD_MODS_DIR) : resolve(repoRoot, "examples/turdmod");
  }
  return { modId, modsDir, buildId, json };
}

function usage(): void {
  console.log("usage: verify <mod-id> [--mods-dir <path>] [--build-id <id>] [--json]");
}

class RecordingSink implements Sink {
  name = "recording";
  events: { ts: number; modId: string; channel: string; payload: unknown }[] = [];
  publish(ctx: SinkContext, payload: SinkPayload): void {
    this.events.push({ ts: ctx.ts, modId: ctx.modId, channel: ctx.channel, payload });
  }
}

const A = { steam: "76561198000000001", player: "VerifyAlice" };
const B = { steam: "76561198000000002", player: "VerifyBob" };

function syntheticEvents(): ServerEvent[] {
  return [
    { kind: "login", ts: "2026.05.09-00.00.00", ip: "127.0.0.1", steam: A.steam, player: A.player, pos: { x: -200000, y: 100000, z: 9000 } },
    { kind: "login", ts: "2026.05.09-00.00.05", ip: "127.0.0.1", steam: B.steam, player: B.player, pos: { x: -180000, y: 120000, z: 9100 } },
    { kind: "chat",  ts: "2026.05.09-00.00.30", channel: "Global", player: A.player, steam: A.steam, text: "verify-test global hello" },
    { kind: "chat",  ts: "2026.05.09-00.00.35", channel: "Squad",  player: A.player, steam: A.steam, text: "verify-test squad hello" },
    { kind: "kill",  ts: "2026.05.09-00.01.00", victim: A.player, victimSteam: A.steam, killer: B.player, killerSteam: B.steam, weapon: "AK-47", distanceM: 152, headshot: true },
    { kind: "vehicle", ts: "2026.05.09-00.02.00", event: "Destroyed",       vclass: "Kinglet_Duster_ES", vid: "160035", owner: B.steam, pos: { x: 399256, y: -39481, z: 13724 } },
    { kind: "vehicle", ts: "2026.05.09-00.02.30", event: "Disappeared",     vclass: "Wolfswagen_ES",     vid: "160040", owner: A.steam, pos: { x: 200000, y: 100000, z: 9000 } },
    { kind: "vehicle", ts: "2026.05.09-00.02.45", event: "Failed to spawn", vclass: "Rager_ES",          vid: "160041", owner: A.steam, pos: { x: 0, y: 0, z: 0 } },
    { kind: "bunker", ts: "2026.05.09-00.03.00", bid: "C4", state: "Active", pos: { x: 446323, y: 263051, z: 18552 } },
    { kind: "admin",  ts: "2026.05.09-00.04.00", steam: A.steam, player: A.player, cmd: "#announce hello from verify" },
    { kind: "logout", ts: "2026.05.09-00.05.00", steam: A.steam, player: A.player },
    { kind: "logout", ts: "2026.05.09-00.05.05", steam: B.steam, player: B.player },
  ];
}

interface VerifyResult {
  ok: boolean;
  mod: { id: string; name: string; version: string; mode: string; entry: string };
  events: { dispatched: number; byKind: Record<string, number> };
  broadcasts: { count: number; byChannel: Record<string, number>; sample: { channel: string; payloadType: string; preview: string }[] };
  persistenceKeys: string[];
  seededFixtureKeys: string[];
  handlerErrors: string[];
}

async function main(): Promise<void> {
  const args = parseArgs(process.argv);

  const dir = join(args.modsDir, args.modId);
  if (!existsSync(dir)) return failExit({ stage: "discover", error: `mod dir not found: ${dir}` }, args.json);
  const manifestPath = join(dir, "turdmod.json");
  if (!existsSync(manifestPath)) return failExit({ stage: "discover", error: `no turdmod.json at ${manifestPath}` }, args.json);

  let raw: unknown;
  try { raw = JSON.parse(readFileSync(manifestPath, "utf-8")); }
  catch (e) { return failExit({ stage: "manifest-parse", error: (e as Error).message }, args.json); }
  const v = validateManifest(raw);
  if (!v.ok) return failExit({ stage: "manifest-validate", errors: v.errors }, args.json);
  const manifest = v.value;

  if (manifest.mode !== "server-side") {
    return failExit({ stage: "mode-check", error: `mode '${manifest.mode}' is not hosted by the companion runtime (only 'server-side' is). pak-content / offline-only / external-tool live elsewhere.` }, args.json);
  }
  const candidates = ["scripts/main.ts", "scripts/main.mts", "scripts/main.js", "scripts/main.mjs"];
  const entryFile = candidates.map(c => join(dir, c)).find(p => existsSync(p));
  if (!entryFile) return failExit({ stage: "entry-resolve", error: `no scripts/main.{ts,mts,js,mjs} in ${dir}` }, args.json);

  const sink = new RecordingSink();
  const router = new SinkRouter();
  router.add(sink);
  const storeDir = mkdtempSync(join(tmpdir(), `turdmod-verify-${manifest.id}-`));

  // Optional pre-state: if the mod ships a test-fixtures/persistence.json,
  // seed each top-level key into the per-mod store dir before bindMod. Lets
  // mods that depend on admin-configured data (e.g. squad mappings) verify
  // their full broadcast path under synthetic events.
  const fixturePath = join(dir, "test-fixtures", "persistence.json");
  let seededKeys: string[] = [];
  if (existsSync(fixturePath)) {
    let fixture: Record<string, unknown>;
    try { fixture = JSON.parse(readFileSync(fixturePath, "utf-8")) as Record<string, unknown>; }
    catch (e) { return failExit({ stage: "fixture-parse", error: (e as Error).message, fixture: fixturePath }, args.json); }
    seededKeys = Object.keys(fixture);
    const modStoreDir = join(storeDir, manifest.id);
    const { mkdirSync, writeFileSync } = await import("node:fs");
    mkdirSync(modStoreDir, { recursive: true });
    for (const [k, v] of Object.entries(fixture)) {
      writeFileSync(join(modStoreDir, encodeURIComponent(k) + ".json"), JSON.stringify(v, null, 2), "utf-8");
    }
  }

  const runtime = createCompanionRuntime({ buildId: args.buildId, storeDir, router });

  // The companion runtime swallows mod handler errors with console.warn —
  // hook the function so we can surface them in the verify report.
  const handlerErrors: string[] = [];
  const origWarn = console.warn;
  console.warn = (...m: unknown[]) => {
    const line = m.map(x => typeof x === "string" ? x : String(x)).join(" ");
    if (line.startsWith("mod handler error")) handlerErrors.push(line);
    else origWarn(...m);
  };

  try {
    await runtime.bindMod({
      id: manifest.id,
      capabilities: manifest.capabilities.flatMap(c => Object.keys(c)),
      load: async () => {
        const url = pathToFileURL(entryFile).href;
        const m = await import(url) as Record<string, unknown>;
        return {
          on_load:   typeof m.on_load   === "function" ? m.on_load   as () => void : undefined,
          on_unload: typeof m.on_unload === "function" ? m.on_unload as () => void : undefined,
        };
      },
    });
  } catch (e) {
    console.warn = origWarn;
    rmSync(storeDir, { recursive: true, force: true });
    return failExit({ stage: "load", error: (e as Error).stack || (e as Error).message }, args.json);
  }

  const events = syntheticEvents();
  for (const ev of events) runtime.dispatch(ev);
  // network.broadcast is async; let the queued promises settle before we tally.
  await new Promise(r => setTimeout(r, 200));

  await runtime.shutdown();
  console.warn = origWarn;

  const persisted: string[] = [];
  const modStoreDir = join(storeDir, manifest.id);
  if (existsSync(modStoreDir)) walk(modStoreDir, "", persisted);
  rmSync(storeDir, { recursive: true, force: true });

  const byKind = events.reduce((a, e) => { a[e.kind] = (a[e.kind] ?? 0) + 1; return a; }, {} as Record<string, number>);
  const byChannel = sink.events.reduce((a, e) => { a[e.channel] = (a[e.channel] ?? 0) + 1; return a; }, {} as Record<string, number>);
  const sample = sink.events.slice(0, 5).map(e => ({
    channel: e.channel,
    payloadType: typeof e.payload,
    preview: typeof e.payload === "string"
      ? e.payload.slice(0, 120)
      : JSON.stringify(e.payload).slice(0, 120),
  }));

  const result: VerifyResult = {
    ok: handlerErrors.length === 0,
    mod: { id: manifest.id, name: manifest.name, version: manifest.version, mode: manifest.mode, entry: entryFile },
    events: { dispatched: events.length, byKind },
    broadcasts: { count: sink.events.length, byChannel, sample },
    persistenceKeys: persisted,
    seededFixtureKeys: seededKeys,
    handlerErrors,
  };

  if (args.json) {
    console.log(JSON.stringify(result, null, 2));
  } else {
    const status = result.ok ? "PASS" : "FAIL";
    console.log("");
    console.log(`=== verify ${manifest.id} v${manifest.version} → ${status} ===`);
    console.log(`mode:        ${manifest.mode}`);
    console.log(`entry:       ${entryFile}`);
    console.log(`events:      ${events.length} dispatched (${Object.entries(byKind).map(([k, v]) => `${k}=${v}`).join(" ")})`);
    if (seededKeys.length) console.log(`seeded:      ${seededKeys.length} fixture keys (${seededKeys.join(", ")})`);
    console.log(`broadcasts:  ${sink.events.length} (${Object.entries(byChannel).map(([c, n]) => `${c}=${n}`).join(", ") || "none"})`);
    if (sample.length) {
      console.log(`broadcast preview:`);
      for (const s of sample) console.log(`   ${s.channel} (${s.payloadType}): ${s.preview}`);
    }
    console.log(`persistence: ${persisted.length} keys${persisted.length ? "\n   " + persisted.join("\n   ") : ""}`);
    if (handlerErrors.length) {
      console.log(`\nERRORS (${handlerErrors.length}):`);
      for (const e of handlerErrors) console.log(`   ${e}`);
    }
    console.log("");
  }
  process.exit(result.ok ? 0 : 1);
}

function walk(d: string, rel: string, out: string[]): void {
  for (const e of readdirSync(d, { withFileTypes: true })) {
    const sub = join(d, e.name);
    const subRel = rel ? `${rel}/${e.name}` : e.name;
    if (e.isDirectory()) walk(sub, subRel, out);
    else out.push(decodeURIComponent(subRel.replace(/\.json$/, "")));
  }
}

function failExit(detail: Record<string, unknown>, json: boolean): void {
  if (json) console.log(JSON.stringify({ ok: false, ...detail }, null, 2));
  else console.error(`FAIL [${detail.stage}]: ${JSON.stringify(detail)}`);
  process.exit(1);
}

main().catch(e => { console.error(e); process.exit(1); });
