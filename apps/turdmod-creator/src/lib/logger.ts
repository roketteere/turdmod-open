import { appendFileSync } from "node:fs";
import { join } from "node:path";
import { logsDir } from "./config.js";

/** Append a structured event to today's NDJSON log. */
export function logEvent(event: Record<string, unknown>): void {
  const ts = new Date();
  const filename = `${ts.toISOString().slice(0, 10)}.jsonl`;
  const path = join(logsDir(), filename);
  const line = JSON.stringify({ ts: ts.toISOString(), ...event }) + "\n";
  try {
    appendFileSync(path, line, "utf8");
  } catch {
    // Don't fail the command if logging fails.
  }
}

export function info(msg: string): void {
  if (!process.argv.includes("--quiet")) console.log(msg);
}

export function ok(msg: string): void {
  if (!process.argv.includes("--quiet")) console.log(`✓ ${msg}`);
}

export function warn(msg: string): void {
  console.warn(`⚠ ${msg}`);
}

export function err(msg: string): void {
  console.error(`✗ ${msg}`);
}

/** True if `--json` flag was passed. */
export function jsonMode(): boolean {
  return process.argv.includes("--json");
}

/** True if `--advanced` flag was passed. */
export function advancedMode(): boolean {
  return process.argv.includes("--advanced");
}

/** Parse a `--key value` flag from argv slice. */
export function flag(args: string[], name: string): string | undefined {
  const i = args.indexOf(`--${name}`);
  if (i < 0 || i + 1 >= args.length) return undefined;
  return args[i + 1];
}
