/**
 * Build-diff -> mod-compat flagging.
 *
 * Reads the latest entry under <repo>/data/version-diffs/v{old}_to_v{new}.md
 * (produced by scripts/diff-builds.py — see scummap memory
 * `feedback_log_game_build_diffs.md`). Cross-references the
 * "Added/Removed/Changed" sections with each installed mod's sidecar (the
 * targeted DataTable rows and class names declared in dependencies / referenced
 * in entrypoint).
 *
 * Today's heuristic is keyword-based: we extract a flat set of "tokens" from
 * the diff (added/removed class names, added/removed datatable row names,
 * added/removed locres keys) and a flat set of tokens from each sidecar
 * (id, mod name, anything in dependencies, anything in description).
 *
 * Future work (#171's runtime impl will help here): when content.setDataTableOverride
 * is recorded in a sidecar's "manifest hints" section, the matching becomes
 * exact instead of fuzzy.
 *
 * The default scummap repo location is a sibling assumption — Joel's setup has
 * apps/turdmod-cli inside the scummap repo, so the diffs live two dirs up
 * under data/version-diffs/. Override with --diff-dir if running standalone.
 */

import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import { type Sidecar, isCompatibleWithBuild } from "@turdmod/turdmod-manifest";

export interface CompatFlag {
  modId: string;
  modName: string;
  installedVersion: string;
  diffFile: string;
  /** "out-of-range" if min/max bounds exclude the new build; "at-risk" if a token match was found. */
  reason: "out-of-range" | "at-risk" | "ok";
  matches: string[]; // tokens that overlapped
  detail: string;
}

export interface CompatReport {
  ts: string;
  diffFile: string | null;
  oldBuild: string | null;
  newBuild: string | null;
  scanned: number;
  flagged: number;
  flags: CompatFlag[];
}

/** Walk up from a starting dir until we find data/version-diffs/, or null. */
function findDiffDir(start: string): string | null {
  let dir = start;
  for (let i = 0; i < 6; i++) {
    const candidate = join(dir, "data", "version-diffs");
    if (existsSync(candidate)) return candidate;
    const parent = resolve(dir, "..");
    if (parent === dir) return null;
    dir = parent;
  }
  return null;
}

function newestDiff(diffDir: string): string | null {
  const files = readdirSync(diffDir)
    .filter(n => n.startsWith("v") && n.endsWith(".md"))
    .map(n => ({ name: n, mtime: statSync(join(diffDir, n)).mtimeMs }))
    .sort((a, b) => b.mtime - a.mtime);
  return files.length ? join(diffDir, files[0]!.name) : null;
}

/** Parse buildIds out of "v{old}_to_v{new}.md" filename. */
function parseBuildsFromFilename(name: string): { old: string; next: string } | null {
  const m = name.match(/^v(\d+)_to_v(\d+)\.md$/);
  return m ? { old: m[1]!, next: m[2]! } : null;
}

/** Pull a flat set of tokens from a build-diff markdown. */
function extractDiffTokens(text: string): Set<string> {
  const tokens = new Set<string>();
  // Backtick-quoted identifiers — the diff renders class names + datatable rows + locres keys this way.
  for (const m of text.matchAll(/`([A-Za-z_][\w./:-]{2,})`/g)) {
    tokens.add(m[1]!);
  }
  return tokens;
}

/** Pull tokens from a mod's sidecar that the diff might mention. */
function extractSidecarTokens(s: Sidecar): Set<string> {
  const out = new Set<string>([s.id]);
  for (const dep of Object.keys(s.dependencies)) out.add(dep);
  if (s.entrypoint) out.add(s.entrypoint);
  if (s.description) {
    for (const m of s.description.matchAll(/[A-Z][A-Za-z_]{2,}/g)) out.add(m[0]);
  }
  return out;
}

export function checkCompat(
  installed: { sidecar: Sidecar | null }[],
  opts: { diffDir?: string; cwd?: string } = {},
): CompatReport {
  const cwd = opts.cwd || process.cwd();
  const diffDir = opts.diffDir || findDiffDir(cwd);
  const ts = new Date().toISOString();

  if (!diffDir) {
    return {
      ts, diffFile: null, oldBuild: null, newBuild: null,
      scanned: installed.length, flagged: 0, flags: [],
    };
  }

  const diffFile = newestDiff(diffDir);
  if (!diffFile) {
    return {
      ts, diffFile: null, oldBuild: null, newBuild: null,
      scanned: installed.length, flagged: 0, flags: [],
    };
  }

  const builds = parseBuildsFromFilename(diffFile.split(/[/\\]/).pop()!);
  const newBuild = builds?.next ?? null;
  const text = readFileSync(diffFile, "utf-8");
  const diffTokens = extractDiffTokens(text);

  const flags: CompatFlag[] = [];
  for (const m of installed) {
    const s = m.sidecar;
    if (!s) continue;
    let outOfRange = false;
    if (newBuild) outOfRange = !isCompatibleWithBuild(s, newBuild);

    const sidecarTokens = extractSidecarTokens(s);
    const matches: string[] = [];
    for (const t of sidecarTokens) if (diffTokens.has(t)) matches.push(t);

    if (outOfRange) {
      flags.push({
        modId: s.id,
        modName: s.name,
        installedVersion: s.version,
        diffFile,
        reason: "out-of-range",
        matches: [],
        detail: `Mod targets builds ${s.minBuild || "*"}..${s.maxBuild || "*"}; current build ${newBuild} is outside that range.`,
      });
    } else if (matches.length > 0) {
      flags.push({
        modId: s.id,
        modName: s.name,
        installedVersion: s.version,
        diffFile,
        reason: "at-risk",
        matches,
        detail: `Latest build-diff touches ${matches.length} token(s) referenced by this mod's sidecar.`,
      });
    }
  }

  return {
    ts,
    diffFile,
    oldBuild: builds?.old ?? null,
    newBuild,
    scanned: installed.length,
    flagged: flags.length,
    flags,
  };
}
