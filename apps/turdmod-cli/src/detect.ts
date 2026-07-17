/**
 * Strategy C precondition: detect whether SCUM is running under BattlEye and
 * decide whether a Lua-scripting DLL is allowed to inject. This module is
 * load-bearing safety code — anything that wants to inject MUST call it
 * first, and any "uncertain" result MUST fail closed (no inject).
 *
 * Cross-platform stub: only Windows is supported. On non-Windows platforms
 * we return `{ supported: false }` and the caller refuses by default.
 *
 * Detection sources:
 *   1. Process names — BattlEye artifacts (BEService.exe, BEClient_x64.exe,
 *      BEDaisy.sys is a kernel driver but its loader process shows up too).
 *   2. SCUM process command line — looks for `-NoBattlEye` or single-player
 *      flags via `Get-CimInstance Win32_Process`. Distinguishes solo and
 *      no-BE private-server launches from a default (BE-on) launch.
 *   3. Fail-closed default — when SCUM is running but cmdline is unreadable,
 *      we assume BE is on.
 */

import { spawnSync } from "node:child_process";
import { appendFileSync, existsSync, mkdirSync, renameSync, statSync } from "node:fs";
import { homedir, platform } from "node:os";
import { dirname, join } from "node:path";

export type Mode =
  | "solo"
  | "private-be-off"
  | "private-be-on"
  | "official"   // currently same gate as private-be-on; reserved for server-list lookup
  | "unknown";

export interface DetectionResult {
  supported: boolean;
  ts: string;
  os: NodeJS.Platform;
  beActive: boolean;
  beProcessesFound: string[];
  scumRunning: boolean;
  scumPid: number | null;
  scumCmdline: string | null;
  mode: Mode;
  /** Absolute go/no-go for Strategy C injection. */
  allowedToInject: boolean;
  /** Why we landed where we did — surface this in audit logs. */
  reason: string;
}

const BE_PROCESS_NAMES = [
  "BEService.exe",
  "BEService_x64.exe",
  "BEClient_x64.exe",
  "BattlEyeLauncher.exe",
];

const SCUM_PROCESS_NAMES = ["SCUM.exe", "SCUM-Win64-Shipping.exe"];

/** Run a tasklist enumeration and return image names. Cheap; no cmdlines. */
function listProcessNames(): string[] {
  if (platform() !== "win32") return [];
  // tasklist /FO CSV /NH → "Image Name","PID","Session","Session#","Mem"
  const r = spawnSync("tasklist", ["/FO", "CSV", "/NH"], { encoding: "utf8" });
  if (r.status !== 0 || !r.stdout) return [];
  const names: string[] = [];
  for (const line of r.stdout.split(/\r?\n/)) {
    const m = line.match(/^"([^"]+)"/);
    if (m && m[1]) names.push(m[1]);
  }
  return names;
}

/** Returns the SCUM process pid + cmdline, or null if SCUM isn't running. */
function getScumProcess(): { pid: number; cmdline: string } | null {
  if (platform() !== "win32") return null;
  const filter = SCUM_PROCESS_NAMES.map(n => `Name='${n}'`).join(" OR ");
  const ps = `Get-CimInstance -ClassName Win32_Process -Filter "${filter}" | Select-Object -First 1 ProcessId, CommandLine | ConvertTo-Json -Compress`;
  const r = spawnSync("powershell", ["-NoProfile", "-NonInteractive", "-Command", ps], { encoding: "utf8" });
  if (r.status !== 0 || !r.stdout) return null;
  const text = r.stdout.trim();
  if (!text) return null;
  try {
    const obj = JSON.parse(text);
    if (!obj || typeof obj !== "object" || !("ProcessId" in obj)) return null;
    return {
      pid: Number((obj as { ProcessId: number }).ProcessId),
      cmdline: String((obj as { CommandLine?: string }).CommandLine ?? ""),
    };
  } catch {
    return null;
  }
}

/** Determine launch mode from SCUM's command line. Defaults to private-be-on (most conservative). */
function inferModeFromCmdline(cmdline: string | null): { mode: Mode; reason: string } {
  if (!cmdline) return { mode: "unknown", reason: "SCUM not running or cmdline unavailable" };
  const c = cmdline.toLowerCase();
  if (c.includes("-singleplayer") || c.includes("-offline")) return { mode: "solo", reason: "SCUM cmdline contains -singleplayer/-offline" };
  if (c.includes("-nobattleye")) return { mode: "private-be-off", reason: "SCUM cmdline contains -NoBattlEye (private server, BE off)" };
  return { mode: "private-be-on", reason: "SCUM cmdline lacks -NoBattlEye / -singleplayer; BE is presumed active" };
}

// ---------- audit log ----------

const AUDIT_LOG_MAX_BYTES = 5 * 1024 * 1024; // 5MB rotation
const AUDIT_LOG_DIR_ENV = "TURDMOD_LOG_DIR";

function defaultAuditDir(): string {
  // Prefer %LOCALAPPDATA%\TurdMOD on Windows; ~/.turdmod elsewhere.
  if (platform() === "win32") {
    const app = process.env.LOCALAPPDATA;
    if (app) return join(app, "TurdMOD");
  }
  return join(homedir(), ".turdmod");
}

function auditLogPath(): string {
  const dir = process.env[AUDIT_LOG_DIR_ENV] || defaultAuditDir();
  return join(dir, "loader.log");
}

/** Rotate `loader.log` to `loader.log.1` if it's grown past the cap. */
function rotateIfNeeded(path: string): void {
  if (!existsSync(path)) return;
  let size = 0;
  try { size = statSync(path).size; } catch { return; }
  if (size < AUDIT_LOG_MAX_BYTES) return;
  try { renameSync(path, path + ".1"); } catch { /* noop */ }
}

/**
 * Append one detection result to the persistent audit log. Writes are
 * fire-and-forget — if the disk is full or the path is unwritable, we
 * silently skip rather than crash a sanity check.
 */
export function appendAuditEntry(r: DetectionResult): void {
  try {
    const path = auditLogPath();
    mkdirSync(dirname(path), { recursive: true });
    rotateIfNeeded(path);
    appendFileSync(path, JSON.stringify(r) + "\n", "utf-8");
  } catch {
    // best-effort; never fail a detection because the log is broken.
  }
}

/** Synchronous detection. Cheap (~100ms on a warm Windows box); safe to call from a CLI. */
export function detectScumEnvironment(opts: { writeAudit?: boolean } = {}): DetectionResult {
  const ts = new Date().toISOString();
  const os = platform();

  if (os !== "win32") {
    return {
      supported: false, ts, os,
      beActive: false, beProcessesFound: [],
      scumRunning: false, scumPid: null, scumCmdline: null,
      mode: "unknown",
      allowedToInject: false,
      reason: `Strategy C is Windows-only (current platform: ${os})`,
    };
  }

  const procs = listProcessNames();
  const beProcessesFound = procs.filter(p => BE_PROCESS_NAMES.some(b => p.toLowerCase() === b.toLowerCase()));
  const beActive = beProcessesFound.length > 0;

  const scum = getScumProcess();
  const scumRunning = scum !== null;
  const { mode, reason: cmdReason } = inferModeFromCmdline(scum?.cmdline ?? null);

  // Decision tree (fail-closed):
  // - If BE is active → refuse, period. The detection layer is the gate.
  // - If SCUM is running and cmdline says solo or private-be-off → allow.
  // - Otherwise → refuse with a clear reason.
  let allowedToInject = false;
  let reason = "";
  if (beActive) {
    allowedToInject = false;
    reason = `BattlEye process(es) detected: ${beProcessesFound.join(", ")} — REFUSED.`;
  } else if (!scumRunning) {
    allowedToInject = false;
    reason = "SCUM is not running. Inject only happens after game start.";
  } else if (mode === "solo" || mode === "private-be-off") {
    allowedToInject = true;
    reason = cmdReason;
  } else {
    allowedToInject = false;
    reason = `SCUM running but mode is '${mode}'. Refusing per fail-closed policy. (${cmdReason})`;
  }

  const result: DetectionResult = {
    supported: true, ts, os,
    beActive, beProcessesFound,
    scumRunning, scumPid: scum?.pid ?? null, scumCmdline: scum?.cmdline ?? null,
    mode, allowedToInject, reason,
  };
  if (opts.writeAudit) appendAuditEntry(result);
  return result;
}

// ---------- watcher ----------

export interface WatcherOptions {
  intervalMs?: number;
  /** Only emit on state change; default true. False emits every tick. */
  changesOnly?: boolean;
  /** Append every observed result to the audit log. Default true. */
  audit?: boolean;
  /** Notification sink (callback). The CLI passes a stdout printer. */
  onResult: (r: DetectionResult, changed: boolean) => void;
}

/**
 * Poll the detection layer at a steady cadence and notify on state changes.
 * The Strategy C loader runs an in-process equivalent of this every 30s
 * post-inject; the standalone CLI version is for ops / pre-launch checks
 * that want to watch the gate before invoking inject.
 *
 * Returns a `stop` function that halts the watcher.
 */
export function watchScumEnvironment(opts: WatcherOptions): () => void {
  const intervalMs = Math.max(1000, opts.intervalMs || 30_000);
  const changesOnly = opts.changesOnly !== false;
  const audit = opts.audit !== false;

  let prev: DetectionResult | null = null;
  let stopped = false;

  const tick = () => {
    if (stopped) return;
    const r = detectScumEnvironment({ writeAudit: audit });
    const changed = !prev
      || prev.beActive !== r.beActive
      || prev.scumRunning !== r.scumRunning
      || prev.mode !== r.mode
      || prev.allowedToInject !== r.allowedToInject;
    if (!changesOnly || changed) opts.onResult(r, changed);
    prev = r;
  };
  tick(); // emit initial state immediately
  const handle = setInterval(tick, intervalMs);
  return () => {
    stopped = true;
    clearInterval(handle);
  };
}
