#!/usr/bin/env node
// engine-rpc-test.mjs — one-shot RPC tester for the TurdMOD engine pipe.
//
// Usage:
//   node tools/engine-rpc-test.mjs <method> [paramsJson]
//
// Examples:
//   node tools/engine-rpc-test.mjs ping
//   node tools/engine-rpc-test.mjs dumpUFunctions
//
// Reads %LOCALAPPDATA%\TurdMOD\engine\pipe.txt, connects, fires a single
// JSON-RPC request, prints the response, exits.

import { createConnection } from "node:net";
import { readFileSync, existsSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { randomUUID } from "node:crypto";

// Argv shapes accepted:
//   node engine-rpc-test.mjs <method>
//   node engine-rpc-test.mjs <method> <paramsJson>
//   node engine-rpc-test.mjs <method> --grep <substr> [--limit N]
// PowerShell mangles inner double quotes in single-quoted args, so the
// --grep form sidesteps quoting altogether for the common findFunctions case.
const method = process.argv[2];
if (!method) {
  console.error("usage: node tools/engine-rpc-test.mjs <method> [paramsJson | --grep STR [--limit N]]");
  process.exit(2);
}
let paramsJson;
const rest = process.argv.slice(3);
const hasFlag = rest.some((a) => a.startsWith("--"));
if (hasFlag) {
  // Generic --key value passthrough. Every --foo bar pair becomes
  // params.foo = "bar". Strings only — bridge handlers parse as needed.
  const params = {};
  for (let i = 0; i < rest.length; i++) {
    if (rest[i].startsWith("--") && i + 1 < rest.length) {
      const key = rest[i].slice(2);
      params[key] = rest[++i];
    }
  }
  paramsJson = JSON.stringify(params);
} else if (rest.length > 0) {
  paramsJson = rest[0];
}

// Resolve the live bridge pipe. Enumerate \\.\pipe\ for turdmod-engine-<pid>
// FIRST — self-correcting across restarts (pipe.txt goes stale when the SCUM
// pid changes). Fall back to the pipe.txt discovery file only if enumeration
// finds nothing. @brk: assumes a single turdmod-engine pipe; picks the last.
function resolvePipe() {
  try {
    const live = readdirSync("\\\\.\\pipe\\").filter((p) => p.startsWith("turdmod-engine-"));
    if (live.length) return "\\\\.\\pipe\\" + live.sort()[live.length - 1];
  } catch {}
  const base = process.env.LOCALAPPDATA || process.env.APPDATA;
  const pf = join(base, "TurdMOD", "engine", "pipe.txt");
  if (existsSync(pf)) return readFileSync(pf, "utf-8").trim();
  return null;
}
const pipeName = resolvePipe();
if (!pipeName) {
  console.error("no turdmod-engine pipe found — is the engine running?");
  process.exit(3);
}
console.error(`[test] pipe: ${pipeName}`);

const id = randomUUID();
const body = JSON.stringify({
  id,
  method,
  params: paramsJson ? JSON.parse(paramsJson) : undefined,
});
const bodyBuf = Buffer.from(body, "utf-8");
const lenBuf = Buffer.alloc(4);
lenBuf.writeUInt32LE(bodyBuf.length, 0);
const frame = Buffer.concat([lenBuf, bodyBuf]);

const sock = createConnection(pipeName);
let read = Buffer.alloc(0);
sock.on("connect", () => {
  console.error(`[test] connected, sending ${bodyBuf.length}-byte request`);
  sock.write(frame);
});
sock.on("data", (chunk) => {
  read = Buffer.concat([read, chunk]);
  while (read.length >= 4) {
    const expected = read.readUInt32LE(0);
    if (read.length < 4 + expected) break;
    const msg = read.subarray(4, 4 + expected).toString("utf-8");
    read = read.subarray(4 + expected);
    let parsed;
    try { parsed = JSON.parse(msg); } catch { parsed = msg; }
    if (parsed && parsed.id === id) {
      console.log(JSON.stringify(parsed, null, 2));
      sock.end();
      process.exit(0);
    } else {
      console.error("[test] event/other frame:", JSON.stringify(parsed));
    }
  }
});
sock.on("error", (e) => {
  console.error(`[test] socket error: ${e.message}`);
  process.exit(4);
});
setTimeout(() => {
  console.error("[test] timeout after 30s");
  process.exit(5);
}, 30_000);
