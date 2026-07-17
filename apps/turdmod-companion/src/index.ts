/**
 * TurdMOD companion entrypoint.
 *
 * Loads mod modules from `<modsDir>` (default: examples/turdmod), spins up
 * the runtime, and either:
 *   - tails the SCUM dedicated-server logs at SCUM_SERVER_LOGS_DIR, or
 *   - emits a synthetic event stream when --demo is passed (no server needed).
 *
 * The mod loader looks for `<modId>/scripts/main.ts` (TS) or `main.js` and
 * imports it. Each module exports `{ on_load, on_unload }` plus optional
 * `manifest` (re-read from turdmod.json next to it).
 *
 * Sinks: console (always on), file (TURDMOD_COMPANION_LOG=path), Discord
 * (TURDMOD_DISCORD_WEBHOOK=https://discord.com/...).
 */

import { existsSync, readdirSync, readFileSync } from "node:fs";
import { resolve, join } from "node:path";
import { pathToFileURL } from "node:url";
import { validateManifest, type Manifest } from "@turdmod/turdmod-manifest";
import { createCompanionRuntime, type ModBinding } from "./runtime.js";
import { startTailer } from "./log-tail.js";
import { ConsoleSink, DiscordWebhookSink, FileSink, SinkRouter } from "./sinks.js";
import { parseSingleLine, parseKillBlock, type ServerEvent } from "./parsers.js";
import { IpcServer } from "./ipc-server.js";
import { EngineClient } from "./engine-client.js";

interface CliFlags {
  demo?: boolean;
  modsDir?: string;
  logsDir?: string;
}

function parseArgs(argv: string[]): CliFlags {
  const out: CliFlags = {};
  for (let i = 2; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--demo") out.demo = true;
    else if (a === "--mods-dir") out.modsDir = argv[++i];
    else if (a === "--logs-dir") out.logsDir = argv[++i];
  }
  return out;
}

interface DiscoveredMod {
  id: string;
  manifest: Manifest;
  entryFile: string;
}

function discoverMods(modsDir: string): DiscoveredMod[] {
  if (!existsSync(modsDir)) return [];
  const out: DiscoveredMod[] = [];
  for (const entry of readdirSync(modsDir, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    const dir = join(modsDir, entry.name);
    const manifestPath = join(dir, "turdmod.json");
    if (!existsSync(manifestPath)) continue;
    let raw: unknown;
    try { raw = JSON.parse(readFileSync(manifestPath, "utf-8")); }
    catch { continue; }
    const v = validateManifest(raw);
    if (!v.ok) {
      console.warn(`[companion] skipping ${entry.name}: invalid manifest`);
      continue;
    }
    const manifest = v.value;
    // Companion hosts only server-side TS/JS mods. pak-content (no scripts),
    // offline-only (loader DLL + Lua), and external-tool (separate process)
    // live elsewhere — silently skip them so the startup log only flags
    // genuinely malformed mods.
    if (manifest.mode !== "server-side") continue;
    const candidates = ["scripts/main.ts", "scripts/main.mts", "scripts/main.js", "scripts/main.mjs"];
    const entryFile = candidates.map(c => join(dir, c)).find(p => existsSync(p));
    if (!entryFile) {
      console.warn(`[companion] skipping ${manifest.id}: server-side mode requires scripts/main.{ts,mts,js,mjs}`);
      continue;
    }
    out.push({ id: manifest.id, manifest, entryFile });
  }
  return out;
}

async function loadMod(disc: DiscoveredMod): Promise<ModBinding> {
  return {
    id: disc.id,
    capabilities: disc.manifest.capabilities.map((c: Record<string, string>) => Object.keys(c)[0]!).filter(Boolean),
    load: async () => {
      const url = pathToFileURL(disc.entryFile).href;
      const m = await import(url) as Record<string, unknown>;
      return {
        on_load: typeof m.on_load === "function" ? m.on_load as () => void : undefined,
        on_unload: typeof m.on_unload === "function" ? m.on_unload as () => void : undefined,
      };
    },
  };
}

// <modsDir>/mod-state.json holds the frozen-mods set the Manager writes.
// Fails open: a missing or malformed file means "load every mod".
function readFrozenSlugs(modsDir: string): Set<string> {
  const stateFile = join(modsDir, "mod-state.json");
  if (!existsSync(stateFile)) return new Set();
  try {
    const raw = JSON.parse(readFileSync(stateFile, "utf-8")) as unknown;
    if (
      raw !== null &&
      typeof raw === "object" &&
      "frozen" in raw &&
      Array.isArray((raw as Record<string, unknown>)["frozen"])
    ) {
      return new Set(
        (raw as { frozen: unknown[] }).frozen.filter(
          (s): s is string => typeof s === "string",
        ),
      );
    }
  } catch {
    console.warn("[companion] could not parse mod-state.json, treating all mods as active");
  }
  return new Set();
}

// Branding banner — every TurdMOD process prints the same art at startup.
// Keep this string in sync with apps/turdmod-loader/src/lib.rs:BANNER and
// apps/turdmod-guard/src/main.rs:BANNER so all three processes log the
// same TurdMOD-is-running moment.
const BANNER = `
████████╗██╗   ██╗██████╗ ██████╗ ███╗   ███╗ ██████╗ ██████╗
╚══██╔══╝██║   ██║██╔══██╗██╔══██╗████╗ ████║██╔═══██╗██╔══██╗
   ██║   ██║   ██║██████╔╝██║  ██║██╔████╔██║██║   ██║██║  ██║
   ██║   ██║   ██║██╔══██╗██║  ██║██║╚██╔╝██║██║   ██║██║  ██║
   ██║   ╚██████╔╝██║  ██║██████╔╝██║ ╚═╝ ██║╚██████╔╝██████╔╝
   ╚═╝    ╚═════╝ ╚═╝  ╚═╝╚═════╝ ╚═╝     ╚═╝ ╚═════╝ ╚═════╝

                  >>> TurdMOD is running! <<<
                companion v0.0.1 — log-tail driver
                  github.com/roketteere/scummymap
`;

async function main() {
  console.log(BANNER);
  const argv = parseArgs(process.argv);

  const repoRoot = resolve(import.meta.url.replace(/^file:\/+/, "/").replace(/^\/+([A-Za-z]:)/, "$1"), "..", "..", "..", "..");
  const modsDir = argv.modsDir
    ? resolve(argv.modsDir)
    : process.env.TURDMOD_MODS_DIR
      ? resolve(process.env.TURDMOD_MODS_DIR)
      : resolve(repoRoot, "examples/turdmod");
  const logsDir = argv.logsDir || process.env.SCUM_SERVER_LOGS_DIR;
  const storeDir = process.env.TURDMOD_COMPANION_STORE || resolve(repoRoot, "data/turdmod-companion");

  const router = new SinkRouter();
  router.add(new ConsoleSink());
  if (process.env.TURDMOD_COMPANION_LOG) router.add(new FileSink(resolve(process.env.TURDMOD_COMPANION_LOG)));
  if (process.env.TURDMOD_DISCORD_WEBHOOK) router.add(new DiscordWebhookSink(process.env.TURDMOD_DISCORD_WEBHOOK));

  const runtime = createCompanionRuntime({
    buildId: process.env.SCUM_BUILD_ID || "23128448",
    storeDir,
    router,
  });

  // IPC server: peers (loader, guard daemon) subscribe to /events and
  // receive every parsed ServerEvent as SSE. Also hosts mod management API.
  const ipc = new IpcServer();
  const ipcInfo = await ipc.start();
  console.log(`[companion] ipc listening at ${ipcInfo.url} (subscribe at ${ipcInfo.url}/events)`);
  console.log(`[companion] mod management: GET ${ipcInfo.url}/mods | POST ${ipcInfo.url}/mods/{load|unload|reload}`);

  // Engine RPC client — discover + connect to the loader's named pipe.
  // Non-fatal: mods that don't need the engine still work without it.
  const engineClient = new EngineClient();
  try {
    await engineClient.connect();
    globalThis.__turdmod_engine_client = engineClient;
    try {
      const pong = await engineClient.ping();
      console.log(`[companion] engine ping OK: ${JSON.stringify(pong)}`);
    } catch (e) {
      console.warn(`[companion] engine ping failed: ${(e as Error).message}`);
    }
  } catch (e) {
    console.warn(`[companion] engine not reachable: ${(e as Error).message}`);
    console.warn(`[companion] engine-dependent mod calls will throw NOT_SUPPORTED`);
  }

  // Wire mod manager into IPC so Manager GUI can load/unload mods at runtime
  const discoveredMap = new Map<string, DiscoveredMod>();
  ipc.modManager = {
    runtime,
    async loadMod(modId: string) {
      const disc = discoveredMap.get(modId);
      if (!disc) throw new Error(`mod '${modId}' not found in mods directory`);
      if (runtime.loadedMods().includes(modId)) throw new Error(`mod '${modId}' already loaded`);
      await runtime.bindMod(await loadMod(disc));
      console.log(`[companion] hot-loaded mod: ${modId}`);
    },
    async unloadMod(modId: string) {
      if (!runtime.loadedMods().includes(modId)) throw new Error(`mod '${modId}' not loaded`);
      await runtime.unbindMod(modId);
    },
    async reloadMod(modId: string) {
      if (runtime.loadedMods().includes(modId)) await runtime.unbindMod(modId);
      const disc = discoveredMap.get(modId);
      if (!disc) throw new Error(`mod '${modId}' not found in mods directory`);
      await runtime.bindMod(await loadMod(disc));
      console.log(`[companion] hot-reloaded mod: ${modId}`);
    },
    listMods() {
      return {
        loaded: runtime.loadedMods(),
        available: Array.from(discoveredMap.keys()),
      };
    },
  };

  const frozenSlugs = readFrozenSlugs(modsDir);
  if (frozenSlugs.size > 0) {
    console.log(`[companion] frozen mods (will skip): ${[...frozenSlugs].join(", ")}`);
  }

  const discovered = discoverMods(modsDir);
  for (const disc of discovered) discoveredMap.set(disc.id, disc);
  if (discovered.length === 0) {
    console.warn(`[companion] no server-side / offline-only mods discovered under ${modsDir}`);
    console.warn(`[companion] expected layout: <modsDir>/<modId>/turdmod.json + scripts/main.{ts,js}`);
  }
  for (const disc of discovered) {
    if (frozenSlugs.has(disc.id)) {
      console.log(`[companion] mod ${disc.id} is frozen, skipping`);
      continue;
    }
    try {
      console.log(`[companion] loading ${disc.id} (${disc.manifest.mode}) from ${disc.entryFile}`);
      await runtime.bindMod(await loadMod(disc));
    } catch (e) {
      console.warn(`[companion] failed to load ${disc.id}: ${(e as Error).message}`);
    }
  }

  if (argv.demo) {
    runDemoLoop(runtime);
    return;
  }

  if (!logsDir) {
    console.warn(`[companion] no logs dir configured. Set SCUM_SERVER_LOGS_DIR or pass --logs-dir, or run with --demo to see synthetic events.`);
    process.exit(1);
  }
  if (!existsSync(logsDir)) {
    console.warn(`[companion] logs dir does not exist: ${logsDir}`);
    process.exit(1);
  }

  console.log(`[companion] tailing ${logsDir}`);
  const stop = startTailer({
    logsDir,
    onLine(line, kind) {
      const evt = parseSingleLine(line);
      if (process.env.TURDMOD_TAIL_DEBUG) console.log(`[companion] line ${kind} parsed=${evt?.kind ?? "null"} <${line.slice(0, 160)}>`);
      if (evt) {
        runtime.dispatch(evt);
        ipc.broadcast(evt);
      }
    },
    onKillBlock(block) {
      const evt = parseKillBlock(block);
      if (process.env.TURDMOD_TAIL_DEBUG) console.log(`[companion] killblock parsed=${evt?.kind ?? "null"}`);
      if (evt) {
        runtime.dispatch(evt);
        ipc.broadcast(evt);
      }
    },
  });

  process.on("SIGINT",  async () => { engineClient.disconnect(); stop(); await runtime.shutdown(); process.exit(0); });
  process.on("SIGTERM", async () => { engineClient.disconnect(); stop(); await runtime.shutdown(); process.exit(0); });
}

/** Synthetic event stream for `--demo`. Emits a representative slice of activity over ~30s. */
function runDemoLoop(runtime: ReturnType<typeof createCompanionRuntime>) {
  console.log("[companion] DEMO MODE — synthetic events every ~3s. Ctrl+C to stop.");
  const players = [
    { steam: "76561198000000001", player: "PlayerAlpha" },
    { steam: "76561198000000002", player: "YOUR_OWNER_NAME" },
    { steam: "76561198000000003", player: "PlayerBeta"      },
  ];
  const events: ServerEvent[] = [
    { kind: "login",  ts: "2026.05.09-00.00.00", ip: "127.0.0.1", steam: players[0]!.steam, player: players[0]!.player, pos: { x: -200000, y:  100000, z: 9000 } },
    { kind: "login",  ts: "2026.05.09-00.00.05", ip: "127.0.0.1", steam: players[1]!.steam, player: players[1]!.player, pos: { x: -180000, y:  120000, z: 9100 } },
    { kind: "login",  ts: "2026.05.09-00.00.10", ip: "127.0.0.1", steam: players[2]!.steam, player: players[2]!.player, pos: { x:   50000, y: -200000, z: 9050 } },
    { kind: "kill",   ts: "2026.05.09-00.01.00", victim: "PlayerAlpha", victimSteam: players[0]!.steam, killer: "YOUR_OWNER_NAME", killerSteam: players[1]!.steam, weapon: "AK-47", distanceM: 152, headshot: true },
    { kind: "vehicle", ts: "2026.05.09-00.02.00", event: "Destroyed", vclass: "Kinglet_Duster_ES", vid: "160035", owner: players[1]!.steam, pos: { x: 399256, y: -39481, z: 13724 } },
    { kind: "bunker",  ts: "2026.05.09-00.03.00", bid: "C4", state: "Active", pos: { x: 446323, y: 263051, z: 18552 } },
    { kind: "logout",  ts: "2026.05.09-00.04.00", steam: players[0]!.steam, player: players[0]!.player },
  ];
  let i = 0;
  const tick = setInterval(() => {
    if (i >= events.length) { clearInterval(tick); console.log("[companion] demo stream complete"); return; }
    const e = events[i++]!;
    console.log(`[demo] dispatch ${e.kind}`);
    runtime.dispatch(e);
  }, 3000);
}

main().catch(e => { console.error(e); process.exit(1); });
