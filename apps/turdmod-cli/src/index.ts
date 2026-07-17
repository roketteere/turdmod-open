#!/usr/bin/env node
/**
 * TurdMOD CLI — pak content mods (Strategy A).
 *
 * Manages the SCUM `~mods/` directory: install / list / enable / disable
 * paks, plus a Vanilla mode that moves all active paks aside before launch
 * so a player can safely connect to an official Gamepires server.
 *
 * No DLL, no injection. Strategy A is purely the UE 4.27 native pak loader
 * reading additional `.pak` files from `<game>/SCUM/Content/Paks/~mods/`.
 *
 * See docs/turdmod/compatibility-policy.md for the policy this enforces.
 */

import { Command } from "commander";
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, renameSync, rmSync, statSync, watch, writeFileSync } from "node:fs";
import { basename, extname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir, platform } from "node:os";
import { type Sidecar, validateSidecar, validateManifest } from "@turdmod/turdmod-manifest";
import { detectScumEnvironment, watchScumEnvironment } from "./detect.js";
import { checkCompat } from "./compat.js";

// ---------- types ----------

type ModSidecar = Sidecar;

type ModEntry = {
  id: string;
  pakPath: string;
  sidecarPath: string;
  state: "active" | "disabled";
  sidecar: ModSidecar | null;
  sidecarValid: boolean;
  pakSizeBytes: number;
};

// ---------- env / discovery ----------

const SIDECAR_EXT = ".turdmod.json";
const ACTIVE_DIR = "~mods";
const DISABLED_DIR = "~mods.disabled";

function envPaksDir(): string | null {
  return process.env.TURDMOD_PAKS_DIR || process.env.SCUM_PAKS_DIR || null;
}

function autoDiscoverPaksDir(): string | null {
  if (platform() !== "win32") return null;
  const candidates = [
    "C:/Program Files (x86)/Steam/steamapps/common/SCUM/SCUM/Content/Paks",
    "C:/Program Files/Steam/steamapps/common/SCUM/SCUM/Content/Paks",
    join(homedir(), "AppData/Local/SCUM/Content/Paks"),
  ];
  for (const c of candidates) if (existsSync(c)) return c;
  return null;
}

function resolvePaksDir(opts: { paks?: string }): string {
  const explicit = opts.paks || envPaksDir() || autoDiscoverPaksDir();
  if (!explicit) {
    fail(
      "Could not locate SCUM Paks dir.\n" +
      "  Pass --paks <path>, set TURDMOD_PAKS_DIR, or set SCUM_PAKS_DIR.\n" +
      "  Expected something like: <Steam>/steamapps/common/SCUM/SCUM/Content/Paks"
    );
  }
  if (!existsSync(explicit)) fail(`Paks dir does not exist: ${explicit}`);
  return resolve(explicit);
}

function ensureSubdirs(paksDir: string): { active: string; disabled: string } {
  const active = join(paksDir, ACTIVE_DIR);
  const disabled = join(paksDir, DISABLED_DIR);
  for (const d of [active, disabled]) if (!existsSync(d)) mkdirSync(d, { recursive: true });
  return { active, disabled };
}

// ---------- core ----------

function listMods(paksDir: string): ModEntry[] {
  const { active, disabled } = ensureSubdirs(paksDir);
  const out: ModEntry[] = [];
  for (const [dir, state] of [[active, "active"], [disabled, "disabled"]] as const) {
    for (const entry of readdirSync(dir)) {
      if (!entry.toLowerCase().endsWith(".pak")) continue;
      const pakPath = join(dir, entry);
      const id = basename(entry, extname(entry));
      const sidecarPath = join(dir, id + SIDECAR_EXT);
      const { sidecar, valid } = readAndValidateSidecar(sidecarPath);
      out.push({
        id,
        pakPath,
        sidecarPath,
        state,
        sidecar,
        sidecarValid: valid,
        pakSizeBytes: statSync(pakPath).size,
      });
    }
  }
  return out.sort((a, b) => a.id.localeCompare(b.id));
}

function readAndValidateSidecar(path: string): { sidecar: ModSidecar | null; valid: boolean } {
  if (!existsSync(path)) return { sidecar: null, valid: false };
  let raw: unknown;
  try { raw = JSON.parse(readFileSync(path, "utf-8")); }
  catch { return { sidecar: null, valid: false }; }
  const v = validateSidecar(raw);
  return v.ok ? { sidecar: v.value, valid: true } : { sidecar: raw as ModSidecar, valid: false };
}

function findById(paksDir: string, id: string): ModEntry | null {
  return listMods(paksDir).find(m => m.id === id) || null;
}

function moveModFiles(entry: ModEntry, toDir: string): ModEntry {
  const newPak = join(toDir, basename(entry.pakPath));
  renameSync(entry.pakPath, newPak);
  if (existsSync(entry.sidecarPath)) {
    const newSide = join(toDir, basename(entry.sidecarPath));
    renameSync(entry.sidecarPath, newSide);
    return { ...entry, pakPath: newPak, sidecarPath: newSide };
  }
  return { ...entry, pakPath: newPak };
}

// ---------- commands ----------

function cmdList(opts: { paks?: string; json?: boolean }) {
  const paksDir = resolvePaksDir(opts);
  const mods = listMods(paksDir);
  if (opts.json) { console.log(JSON.stringify(mods, null, 2)); return; }
  if (mods.length === 0) {
    console.log(`No mods installed at ${join(paksDir, ACTIVE_DIR)} or ${join(paksDir, DISABLED_DIR)}.`);
    return;
  }
  console.log(`SCUM Paks: ${paksDir}`);
  console.log(`Mods (${mods.length}):`);
  for (const m of mods) {
    const flag = m.state === "active" ? "✓" : "·";
    const sidecarTag = m.sidecar
      ? (m.sidecarValid ? `${m.sidecar.name} v${m.sidecar.version}` : `(invalid sidecar)`)
      : `(no sidecar)`;
    const size = formatSize(m.pakSizeBytes);
    console.log(`  ${flag} ${m.id.padEnd(28)} ${sidecarTag.padEnd(40)} ${size}`);
  }
  const active = mods.filter(m => m.state === "active").length;
  console.log(`\n${active} active / ${mods.length - active} disabled`);
}

function cmdInstall(pakArg: string, opts: { paks?: string; id?: string; name?: string; version?: string; author?: string; description?: string; minBuild?: string; maxBuild?: string }) {
  const paksDir = resolvePaksDir(opts);
  const { active } = ensureSubdirs(paksDir);
  const src = resolve(pakArg);
  if (!existsSync(src)) fail(`Source pak not found: ${src}`);
  if (!src.toLowerCase().endsWith(".pak")) fail(`Not a .pak file: ${src}`);

  const id = opts.id || basename(src, extname(src));
  if (findById(paksDir, id)) fail(`Mod '${id}' is already installed (use 'turdmod pak uninstall ${id}' first).`);

  const destPak = join(active, id + ".pak");
  const destSide = join(active, id + SIDECAR_EXT);
  copyFileSync(src, destPak);

  const draft = {
    schema: 1 as const,
    id,
    name: opts.name || id,
    version: opts.version || "0.0.1",
    author: opts.author,
    mode: "pak-content" as const,
    description: opts.description,
    minBuild: opts.minBuild,
    maxBuild: opts.maxBuild,
    dependencies: {},
    capabilities: [],
    tags: [],
    installedAt: new Date().toISOString(),
    sourcePath: src,
  };
  const v = validateSidecar(draft);
  if (!v.ok) {
    rmSync(destPak, { force: true });
    fail(`Generated sidecar failed validation:\n${v.errors.map(e => `  - ${e.path}: ${e.message}`).join("\n")}`);
  }
  writeFileSync(destSide, JSON.stringify(v.value, null, 2), "utf-8");

  console.log(`Installed '${id}' from ${src}`);
  console.log(`  pak:     ${destPak}`);
  console.log(`  sidecar: ${destSide}`);
  console.log(`  state:   active`);
}

function cmdCompatCheck(opts: { paks?: string; diffDir?: string; json?: boolean; strict?: boolean }) {
  const paksDir = resolvePaksDir(opts);
  const mods = listMods(paksDir);
  const report = checkCompat(mods, { diffDir: opts.diffDir });

  if (opts.json) {
    console.log(JSON.stringify(report, null, 2));
  } else if (!report.diffFile) {
    console.log("No build-diff found under data/version-diffs/.");
    console.log("Run scummap's `scripts/diff-builds.py --auto` (or the full extract pipeline) to produce one.");
  } else {
    console.log(`TurdMOD compat check (${report.ts})`);
    console.log(`  Diff:        ${report.diffFile}`);
    console.log(`  Old → new:   v${report.oldBuild} → v${report.newBuild}`);
    console.log(`  Scanned:     ${report.scanned} mod(s)`);
    console.log(`  Flagged:     ${report.flagged}`);
    if (report.flagged === 0) {
      console.log(`\n✓ No mods flagged. Proceed normally.`);
    } else {
      console.log("");
      for (const f of report.flags) {
        const icon = f.reason === "out-of-range" ? "✗" : "⚠";
        console.log(`${icon} ${f.modId} v${f.installedVersion} — ${f.reason}`);
        console.log(`  ${f.detail}`);
        if (f.matches.length) console.log(`  Matched tokens: ${f.matches.slice(0, 8).join(", ")}${f.matches.length > 8 ? "…" : ""}`);
      }
    }
  }

  if (opts.strict && report.flagged > 0) process.exit(1);
}

function cmdInit(modId: string, opts: { dir?: string; name?: string; author?: string; mode?: string; description?: string; minBuild?: string }) {
  const m = opts.mode || "pak-content";
  if (!["pak-content", "server-side", "offline-only", "external-tool"].includes(m)) {
    fail(`--mode must be one of: pak-content, server-side, offline-only, external-tool`);
  }
  const targetDir = resolve(opts.dir || join(process.cwd(), modId));
  if (existsSync(targetDir)) {
    const entries = readdirSync(targetDir);
    if (entries.length > 0) fail(`Target directory is not empty: ${targetDir}\nUse --dir <path> to point at a different location.`);
  } else {
    mkdirSync(targetDir, { recursive: true });
  }

  const manifestDraft = {
    schema: 1 as const,
    id: modId,
    name: opts.name || modId,
    version: "0.0.1",
    author: opts.author,
    mode: m,
    description: opts.description,
    minBuild: opts.minBuild,
    dependencies: {},
    capabilities: m === "offline-only" || m === "server-side" ? [{ filesystem: "read" }] : [],
    tags: [],
    entrypoint: m === "pak-content" || m === "external-tool" ? undefined : "scripts/main.lua",
  };
  const validated = validateManifest(manifestDraft);
  if (!validated.ok) {
    rmSync(targetDir, { recursive: true, force: true });
    fail(`Manifest validation failed:\n${validated.errors.map(e => `  - ${e.path}: ${e.message}`).join("\n")}`);
  }
  writeFileSync(join(targetDir, "turdmod.json"), JSON.stringify(validated.value, null, 2), "utf-8");

  // Asset folder for pak-content mods.
  if (m === "pak-content") {
    mkdirSync(join(targetDir, "assets"), { recursive: true });
    writeFileSync(
      join(targetDir, "assets", ".gitkeep"),
      "// Drop UE 4.27 .uasset / .uexp / .ubulk / texture files here.\n",
      "utf-8",
    );
  }

  // Scripts folder for offline-only / server-side mods.
  if (m === "offline-only" || m === "server-side") {
    mkdirSync(join(targetDir, "scripts"), { recursive: true });
    const sample = m === "server-side"
      ? sampleServerSideLua(modId)
      : sampleOfflineLua(modId);
    writeFileSync(join(targetDir, "scripts", "main.lua"), sample, "utf-8");
  }

  writeFileSync(join(targetDir, "README.md"), sampleReadme(modId, m), "utf-8");

  const ignore =
    "# TurdMOD mod project\n" +
    "dist/\n" +
    "*.pak\n" +
    "*.utoc\n" +
    "*.ucas\n" +
    ".turdmod-cache/\n";
  writeFileSync(join(targetDir, ".gitignore"), ignore, "utf-8");

  console.log(`Initialized TurdMOD mod project '${modId}' at ${targetDir}`);
  console.log(`  manifest: turdmod.json`);
  if (m === "pak-content")  console.log(`  assets:   assets/`);
  if (m === "offline-only" || m === "server-side") console.log(`  scripts:  scripts/main.lua`);
  console.log(`\nNext: edit turdmod.json, write your mod, then 'turdmod pak validate ./turdmod.json' to check.`);
}

function sampleOfflineLua(modId: string): string {
  return [
    `-- ${modId} — offline-only TurdMOD script.`,
    `-- Runs in the Strategy C loader's sandboxed Lua VM.`,
    `-- See @turdmod/turdmod-api for the full API surface.`,
    ``,
    `local turdmod = require("turdmod")`,
    ``,
    `local function on_load()`,
    `  turdmod.ui.toast("${modId} loaded", 3000)`,
    `  turdmod.player.onSpawn(function(p)`,
    `    if p.isLocal then`,
    `      turdmod.ui.toast("Welcome back, " .. p.name, 5000)`,
    `    end`,
    `  end)`,
    `end`,
    ``,
    `return { on_load = on_load }`,
    ``,
  ].join("\n");
}

function sampleServerSideLua(modId: string): string {
  return [
    `-- ${modId} — server-side TurdMOD script.`,
    `-- Runs in the dedicated server's TurdMOD runtime, not on clients.`,
    `-- See @turdmod/turdmod-api for the full API surface.`,
    ``,
    `local turdmod = require("turdmod")`,
    ``,
    `local function on_load()`,
    `  turdmod.player.onDeath(function(p, cause)`,
    `    turdmod.network.broadcast("${modId}.death", { who = p.name, cause = cause })`,
    `  end)`,
    `end`,
    ``,
    `return { on_load = on_load }`,
    ``,
  ].join("\n");
}

function sampleReadme(modId: string, mode: string): string {
  return [
    `# ${modId}`,
    ``,
    `A TurdMOD mod (\`${mode}\`).`,
    ``,
    `## Build & install`,
    ``,
    `\`\`\`bash`,
    `# validate the manifest`,
    `turdmod pak validate ./turdmod.json`,
    ``,
    mode === "pak-content"
      ? `# (todo) cook your assets into a .pak with the UE editor / unrealpak,\n# then install it:\nturdmod pak install ./dist/${modId}.pak`
      : `# (todo) bundle and ship — packaging spec lands with the loader (turdmod-cli #167-#169)`,
    `\`\`\``,
    ``,
    `## Files`,
    ``,
    mode === "pak-content"
      ? `- \`turdmod.json\` — manifest\n- \`assets/\` — UE 4.27 assets that get baked into a .pak`
      : `- \`turdmod.json\` — manifest\n- \`scripts/main.lua\` — entry point (Lua sandbox runtime)`,
    `- \`README.md\` — this file`,
    ``,
    `See [TurdMOD docs](../../docs/turdmod/) for the API surface and policy details.`,
    ``,
  ].join("\n");
}

function cmdValidate(target: string, opts: { paks?: string }) {
  // target may be a mod id or a path to a .turdmod.json (or a manifest turdmod.json file)
  let raw: unknown;
  let label: string;
  if (existsSync(target) && statSync(target).isFile()) {
    label = target;
    try { raw = JSON.parse(readFileSync(target, "utf-8")); }
    catch (e) { fail(`Invalid JSON at ${target}: ${(e as Error).message}`); }
  } else {
    const paksDir = resolvePaksDir(opts);
    const m = findById(paksDir, target);
    if (!m) fail(`No mod '${target}' installed and no file at that path.`);
    label = m.sidecarPath;
    if (!existsSync(m.sidecarPath)) fail(`Mod '${target}' has no sidecar at ${m.sidecarPath}.`);
    try { raw = JSON.parse(readFileSync(m.sidecarPath, "utf-8")); }
    catch (e) { fail(`Invalid JSON in sidecar ${m.sidecarPath}: ${(e as Error).message}`); }
  }

  // Try sidecar first (superset), then fall back to manifest (a published turdmod.json without install fields).
  const sidecarResult = validateSidecar(raw);
  if (sidecarResult.ok) {
    console.log(`✓ ${label}`);
    console.log(`  Schema: sidecar v${sidecarResult.value.schema}`);
    console.log(`  ${sidecarResult.value.id} — ${sidecarResult.value.name} v${sidecarResult.value.version} (${sidecarResult.value.mode})`);
    return;
  }
  const manifestResult = validateManifest(raw);
  if (manifestResult.ok) {
    console.log(`✓ ${label}`);
    console.log(`  Schema: manifest v${manifestResult.value.schema} (no install metadata)`);
    console.log(`  ${manifestResult.value.id} — ${manifestResult.value.name} v${manifestResult.value.version} (${manifestResult.value.mode})`);
    return;
  }
  console.error(`✗ ${label}`);
  console.error(`  Sidecar validation failed:`);
  for (const e of sidecarResult.errors) console.error(`    - ${e.path || "<root>"}: ${e.message}`);
  process.exit(1);
}

function cmdUninstall(id: string, opts: { paks?: string; force?: boolean }) {
  const paksDir = resolvePaksDir(opts);
  const m = findById(paksDir, id);
  if (!m) fail(`No mod '${id}' installed.`);
  if (!opts.force) {
    console.log(`About to remove:\n  ${m.pakPath}\n  ${m.sidecarPath}\nRe-run with --force to confirm.`);
    return;
  }
  rmSync(m.pakPath, { force: true });
  rmSync(m.sidecarPath, { force: true });
  console.log(`Uninstalled '${id}'`);
}

function cmdEnable(id: string, opts: { paks?: string }) {
  const paksDir = resolvePaksDir(opts);
  const { active } = ensureSubdirs(paksDir);
  const m = findById(paksDir, id);
  if (!m) fail(`No mod '${id}' installed.`);
  if (m.state === "active") { console.log(`'${id}' is already active.`); return; }
  moveModFiles(m, active);
  console.log(`Enabled '${id}' (active)`);
}

function cmdDisable(id: string, opts: { paks?: string }) {
  const paksDir = resolvePaksDir(opts);
  const { disabled } = ensureSubdirs(paksDir);
  const m = findById(paksDir, id);
  if (!m) fail(`No mod '${id}' installed.`);
  if (m.state === "disabled") { console.log(`'${id}' is already disabled.`); return; }
  moveModFiles(m, disabled);
  console.log(`Disabled '${id}' (will not load on next launch)`);
}

function cmdVanilla(opts: { paks?: string }) {
  const paksDir = resolvePaksDir(opts);
  const { disabled } = ensureSubdirs(paksDir);
  const mods = listMods(paksDir).filter(m => m.state === "active");
  if (mods.length === 0) { console.log("Already in Vanilla mode (no active mods)."); return; }
  for (const m of mods) moveModFiles(m, disabled);
  console.log(`Vanilla mode: moved ${mods.length} mod(s) aside.`);
  console.log(`Safe to connect to official servers. Run 'turdmod pak modded' to restore.`);
}

function cmdModded(opts: { paks?: string }) {
  const paksDir = resolvePaksDir(opts);
  const { active } = ensureSubdirs(paksDir);
  const mods = listMods(paksDir).filter(m => m.state === "disabled");
  if (mods.length === 0) { console.log("Already in Modded mode (no disabled mods to restore)."); return; }
  for (const m of mods) moveModFiles(m, active);
  console.log(`Modded mode: restored ${mods.length} mod(s).`);
  console.log(`⚠️  Do NOT connect to an official Gamepires server with mods active.`);
}

// ---------- watcher ----------

type WatchEvent =
  | { type: "added"; id: string; state: "active" | "disabled"; sidecarValid: boolean }
  | { type: "removed"; id: string }
  | { type: "enabled"; id: string }
  | { type: "disabled"; id: string }
  | { type: "sidecar-changed"; id: string; sidecarValid: boolean };

function snapshotById(mods: ModEntry[]): Map<string, ModEntry> {
  return new Map(mods.map(m => [m.id, m]));
}

function diffSnapshots(prev: Map<string, ModEntry>, next: Map<string, ModEntry>): WatchEvent[] {
  const events: WatchEvent[] = [];
  for (const [id, m] of next) {
    const old = prev.get(id);
    if (!old) {
      events.push({ type: "added", id, state: m.state, sidecarValid: m.sidecarValid });
      continue;
    }
    if (old.state !== m.state) {
      events.push({ type: m.state === "active" ? "enabled" : "disabled", id });
    }
    if (old.sidecarValid !== m.sidecarValid) {
      events.push({ type: "sidecar-changed", id, sidecarValid: m.sidecarValid });
    }
  }
  for (const [id] of prev) {
    if (!next.has(id)) events.push({ type: "removed", id });
  }
  return events;
}

function emitEvent(ev: WatchEvent, jsonMode: boolean) {
  if (jsonMode) {
    console.log(JSON.stringify({ ts: new Date().toISOString(), ...ev }));
    return;
  }
  const ts = new Date().toISOString();
  switch (ev.type) {
    case "added":
      console.log(`[${ts}] + ${ev.id} (${ev.state})${ev.sidecarValid ? "" : " ⚠ sidecar invalid"}`);
      break;
    case "removed":
      console.log(`[${ts}] - ${ev.id}`);
      break;
    case "enabled":
      console.log(`[${ts}] ✓ ${ev.id} (now active)`);
      break;
    case "disabled":
      console.log(`[${ts}] · ${ev.id} (now disabled)`);
      break;
    case "sidecar-changed":
      console.log(`[${ts}] ! ${ev.id} sidecar ${ev.sidecarValid ? "is now valid" : "became invalid"}`);
      break;
  }
}

function cmdWatch(opts: { paks?: string; json?: boolean; intervalMs?: string }) {
  const paksDir = resolvePaksDir(opts);
  const { active, disabled } = ensureSubdirs(paksDir);
  const debounceMs = Math.max(50, Number(opts.intervalMs || 200));
  const json = !!opts.json;

  let snap = snapshotById(listMods(paksDir));
  const startup = {
    type: "ready" as const,
    paksDir,
    active: [...snap.values()].filter(m => m.state === "active").length,
    disabled: [...snap.values()].filter(m => m.state === "disabled").length,
  };
  if (json) console.log(JSON.stringify({ ts: new Date().toISOString(), ...startup }));
  else console.log(`watching ${paksDir} (${startup.active} active, ${startup.disabled} disabled). Ctrl-C to stop.`);

  let pending: NodeJS.Timeout | null = null;
  const reconcile = () => {
    pending = null;
    const next = snapshotById(listMods(paksDir));
    for (const ev of diffSnapshots(snap, next)) emitEvent(ev, json);
    snap = next;
  };
  const schedule = () => {
    if (pending) clearTimeout(pending);
    pending = setTimeout(reconcile, debounceMs);
  };

  // node:fs watch — non-recursive so we register one watcher per directory.
  const watchers = [watch(active, { persistent: true }, schedule), watch(disabled, { persistent: true }, schedule)];
  const shutdown = () => {
    for (const w of watchers) try { w.close(); } catch { /* noop */ }
    if (pending) clearTimeout(pending);
    process.exit(0);
  };
  process.on("SIGINT", shutdown);
  process.on("SIGTERM", shutdown);
}

function cmdStatus(opts: { paks?: string }) {
  const paksDir = resolvePaksDir(opts);
  const mods = listMods(paksDir);
  const active = mods.filter(m => m.state === "active");
  const mode = active.length === 0 ? "VANILLA" : "MODDED";
  console.log(`SCUM Paks: ${paksDir}`);
  console.log(`Mode:      ${mode}`);
  console.log(`Active:    ${active.length}`);
  console.log(`Disabled:  ${mods.length - active.length}`);
  if (mode === "MODDED") {
    console.log(`\n⚠️  Active mods present. Do NOT connect to an official Gamepires server.`);
    console.log(`   Run 'turdmod pak vanilla' before connecting to official servers.`);
  }
}

// ---------- helpers ----------

function fail(msg: string): never {
  console.error(`error: ${msg}`);
  process.exit(1);
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

// ---------- entry ----------

const program = new Command()
  .name("turdmod")
  .description("TurdMOD CLI — manage SCUM mods (pak content + future strategies)")
  // Use --cli-version instead of the default --version so subcommands can take a --version flag.
  .version("0.0.1", "--cli-version", "Output the turdmod CLI version");

const paksOption = ["--paks <path>", "Path to SCUM Content/Paks directory (overrides TURDMOD_PAKS_DIR / SCUM_PAKS_DIR / autodetect)"] as const;

// ---- 'turdmod pak <subcommand>' — Strategy A (pak content mods) ---------------

const pak = program.command("pak").description("Manage pak content mods (Strategy A) under ~mods/");

pak.command("list")
  .description("List installed mods (active + disabled)")
  .option(...paksOption)
  .option("--json", "Emit JSON instead of a table")
  .action(cmdList);

pak.command("install <pak>")
  .description("Install a .pak as a new mod (active by default)")
  .option(...paksOption)
  .option("--id <id>", "Mod id (default: pak filename without extension)")
  .option("--name <name>", "Display name")
  .option("--version <ver>", "Version (default: 0.0.1)")
  .option("--author <author>", "Author handle")
  .option("--description <desc>", "Short description")
  .option("--min-build <id>", "Minimum compatible SCUM buildId")
  .option("--max-build <id>", "Maximum compatible SCUM buildId")
  .action(cmdInstall);

pak.command("uninstall <id>")
  .description("Remove an installed mod (pak + sidecar)")
  .option(...paksOption)
  .option("--force", "Confirm removal")
  .action(cmdUninstall);

pak.command("enable <id>")
  .description("Move a mod from ~mods.disabled/ to ~mods/")
  .option(...paksOption)
  .action(cmdEnable);

pak.command("disable <id>")
  .description("Move a mod from ~mods/ to ~mods.disabled/")
  .option(...paksOption)
  .action(cmdDisable);

pak.command("vanilla")
  .description("Move ALL active mods aside (safe to connect to official servers)")
  .option(...paksOption)
  .action(cmdVanilla);

pak.command("modded")
  .description("Restore previously disabled mods to active state")
  .option(...paksOption)
  .action(cmdModded);

pak.command("status")
  .description("Show current mode (Vanilla/Modded) and counts")
  .option(...paksOption)
  .action(cmdStatus);

pak.command("validate <id-or-path>")
  .description("Validate a sidecar (by mod id) or a manifest/sidecar JSON at the given path")
  .option(...paksOption)
  .action(cmdValidate);

pak.command("compat-check")
  .description("Cross-reference installed mods against the latest scummap build-diff and flag at-risk mods")
  .option(...paksOption)
  .option("--diff-dir <path>", "Override the path to data/version-diffs/")
  .option("--json", "Emit JSON instead of a table")
  .option("--strict", "Exit 1 if any mod is flagged (for use in CI / pre-launch checks)")
  .action(cmdCompatCheck);

pak.command("init <id>")
  .description("Scaffold a new TurdMOD mod project (turdmod.json + scripts/ or assets/ + README)")
  .option("--dir <path>", "Target directory (default: ./<id>)")
  .option("--name <name>", "Display name (default: id)")
  .option("--author <author>", "Author handle")
  .option("--mode <mode>", "Delivery mode: pak-content | server-side | offline-only | external-tool (default: pak-content)")
  .option("--description <desc>", "Short description")
  .option("--min-build <id>", "Minimum compatible SCUM buildId")
  .action(cmdInit);

pak.command("watch")
  .description("Watch ~mods/ + ~mods.disabled/ and emit add/remove/enable/disable events")
  .option(...paksOption)
  .option("--json", "Emit newline-delimited JSON (for piping into the manager UI)")
  .option("--interval-ms <n>", "Debounce window for fs.watch (default 200)")
  .action(cmdWatch);

// ---- top-level legacy aliases (deprecated, hidden, kept for backward compat) ----
// Each prints a one-line deprecation note then forwards to the new handler.

type AnyHandler = (...args: any[]) => any; // eslint-disable-line @typescript-eslint/no-explicit-any
const legacy = (name: string, action: AnyHandler) => {
  const cmdName = name.split(" ")[0]!;
  return program.command(name, { hidden: true })
    .allowUnknownOption(true)
    .allowExcessArguments(true)
    .action((...args: unknown[]) => {
      console.error(`note: 'turdmod ${cmdName}' is the legacy form; the new shape is 'turdmod pak ${cmdName}'.`);
      return action(...args);
    });
};

legacy("list", cmdList as AnyHandler);
legacy("install <pak>", cmdInstall as AnyHandler);
legacy("uninstall <id>", cmdUninstall as AnyHandler);
legacy("enable <id>", cmdEnable as AnyHandler);
legacy("disable <id>", cmdDisable as AnyHandler);
legacy("vanilla", cmdVanilla as AnyHandler);
legacy("modded", cmdModded as AnyHandler);
legacy("status", cmdStatus as AnyHandler);
legacy("validate <id-or-path>", cmdValidate as AnyHandler);
legacy("watch", cmdWatch as AnyHandler);

// ---- top-level slots reserved for future strategies (B / C / D) ----

function notYet(name: string, ktTask: string) {
  console.log(`'turdmod ${name}' is reserved for ${ktTask}. Not implemented yet.`);
  console.log(`See docs/turdmod/compatibility-policy.md for the planned shape.`);
}

const scripting = program.command("scripting").description("Strategy C: offline / single-player Lua scripting (KTask #136)");

scripting.command("check")
  .description("Run the BattlEye-detection precondition. Reports whether Strategy C injection is allowed right now.")
  .option("--json", "Emit the detection result as JSON")
  .option("--strict", "Exit 0 if injection is allowed, 1 if refused (for use as a precondition in scripts)")
  .option("--no-audit", "Don't append this check to the audit log")
  .action((opts: { json?: boolean; strict?: boolean; audit?: boolean }) => {
    const r = detectScumEnvironment({ writeAudit: opts.audit !== false });
    if (opts.json) {
      console.log(JSON.stringify(r, null, 2));
    } else {
      console.log(`TurdMOD Strategy C — detection report (${r.ts})`);
      console.log(`  OS:                 ${r.os}${r.supported ? "" : " (UNSUPPORTED)"}`);
      console.log(`  BattlEye active:    ${r.beActive}${r.beProcessesFound.length ? "  [" + r.beProcessesFound.join(", ") + "]" : ""}`);
      console.log(`  SCUM running:       ${r.scumRunning}${r.scumPid ? "  pid=" + r.scumPid : ""}`);
      console.log(`  Inferred mode:      ${r.mode}`);
      console.log(`  Allowed to inject:  ${r.allowedToInject ? "YES" : "NO"}`);
      console.log(`  Reason:             ${r.reason}`);
    }
    if (opts.strict && !r.allowedToInject) process.exit(1);
  });

scripting.command("watch")
  .description("Poll the BattlEye-detection layer at a fixed interval and emit state-change events")
  .option("--interval-ms <n>", "Poll interval (default 30000)")
  .option("--all", "Emit every tick instead of only on state changes")
  .option("--no-audit", "Don't append to ~/.turdmod/loader.log")
  .option("--json", "Emit NDJSON instead of human-readable lines")
  .action((opts: { intervalMs?: string; all?: boolean; audit?: boolean; json?: boolean }) => {
    const stop = watchScumEnvironment({
      intervalMs: opts.intervalMs ? Number(opts.intervalMs) : 30_000,
      changesOnly: !opts.all,
      audit: opts.audit !== false,
      onResult: (r, changed) => {
        if (opts.json) {
          console.log(JSON.stringify({ changed, ...r }));
          return;
        }
        const tag = changed ? "*" : " ";
        const allow = r.allowedToInject ? "ALLOWED" : "REFUSED";
        console.log(`[${r.ts}] ${tag} ${allow.padEnd(7)} mode=${r.mode.padEnd(15)} be=${r.beActive ? "yes" : "no "} scum=${r.scumRunning ? "running" : "stopped"}  | ${r.reason}`);
      },
    });
    process.on("SIGINT", () => { stop(); process.exit(0); });
    process.on("SIGTERM", () => { stop(); process.exit(0); });
  });

scripting.command("inject")
  .description("(stub) Inject the TurdMOD scripting DLL into a running SCUM process")
  .action(() => {
    const r = detectScumEnvironment();
    if (!r.allowedToInject) {
      console.error(`REFUSED: ${r.reason}`);
      console.error(`(Run 'turdmod scripting check --json' for the full report.)`);
      process.exit(1);
    }
    console.log("Pre-flight passed; injection is allowed.");
    console.log("DLL injection itself is not yet implemented — see docs/turdmod/loader-architecture.md");
    console.log("and the follow-up KTask sub-tasks for the Rust loader DLL + Lua VM work.");
    process.exit(0);
  });

program.command("server")
  .description("Strategy B: server-side mods running in the dedicated server (KTask #136)")
  .action(() => notYet("server", "KTask #136 (server-side runtime)"));

program.command("tool")
  .description("Strategy D: external tool framework (overlay / SendInput companion apps)")
  .action(() => notYet("tool", "post-v1 (Strategy D framework)"));

const isMain = process.argv[1] && (
  fileURLToPath(import.meta.url) === resolve(process.argv[1]) ||
  process.argv[1].endsWith("index.ts") ||
  process.argv[1].endsWith("index.js")
);
if (isMain) program.parseAsync(process.argv);

export { listMods, resolvePaksDir, type ModEntry, type ModSidecar };
