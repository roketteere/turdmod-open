#!/usr/bin/env node
// engine-events-tail.mjs — long-lived tail of the TurdMOD engine event firehose.
//
// Opens the named pipe and reads frames forever, printing each event to
// stdout. Stays subscribed across the bridge's emit-driven push channel
// (turdmod_engine_emit_event → admin_api::EventBroadcaster → every pipe client
// receives a copy).
//
// Usage:
//   node tools/engine-events-tail.mjs
//   node tools/engine-events-tail.mjs --filter chat,login
//   node tools/engine-events-tail.mjs --raw           (don't pretty-print payload)
//
// Frames on the pipe are length-prefixed:  [4 bytes LE length][JSON body]
// Two body shapes:
//   { event: string, data: any }           ← event push (what we tail)
//   { id, result?, error? }                ← responses to other clients' requests
// We discriminate by presence of `event`. Anything else is ignored.

import { createConnection } from "node:net";
import { readFileSync, existsSync } from "node:fs";
import { join } from "node:path";

const args = process.argv.slice(2);
const filterSet = (() => {
  const i = args.indexOf("--filter");
  if (i < 0 || !args[i + 1]) return null;
  return new Set(args[i + 1].split(",").map((s) => s.trim()).filter(Boolean));
})();
const raw = args.includes("--raw");

const base = process.env.LOCALAPPDATA || process.env.APPDATA;
const pipeFile = join(base, "TurdMOD", "engine", "pipe.txt");
if (!existsSync(pipeFile)) {
  console.error(`no pipe.txt at ${pipeFile} — is the engine running?`);
  process.exit(3);
}
const pipeName = readFileSync(pipeFile, "utf-8").trim();
console.error(`[events-tail] pipe: ${pipeName}`);
if (filterSet) console.error(`[events-tail] filter: ${[...filterSet].join(",")}`);

const sock = createConnection(pipeName);

let buf = Buffer.alloc(0);
let totalSeen = 0;
let totalPrinted = 0;

sock.on("connect", () => {
  console.error(`[events-tail] connected — waiting for events (Ctrl+C to quit)`);
});

sock.on("data", (chunk) => {
  buf = Buffer.concat([buf, chunk]);
  while (buf.length >= 4) {
    const frameLen = buf.readUInt32LE(0);
    if (buf.length < 4 + frameLen) break;
    const body = buf.subarray(4, 4 + frameLen).toString("utf-8");
    buf = buf.subarray(4 + frameLen);

    let parsed;
    try {
      parsed = JSON.parse(body);
    } catch {
      console.error(`[events-tail] non-JSON frame, len=${frameLen}`);
      continue;
    }
    if (parsed && typeof parsed === "object" && typeof parsed.event === "string") {
      totalSeen++;
      if (filterSet && !filterSet.has(parsed.event)) continue;
      totalPrinted++;
      const ts = new Date().toISOString();
      if (raw) {
        process.stdout.write(`${ts} ${parsed.event} ${JSON.stringify(parsed.data)}\n`);
      } else {
        const data = parsed.data === null || parsed.data === undefined
          ? "—"
          : typeof parsed.data === "object"
            ? JSON.stringify(parsed.data, null, 0)
            : String(parsed.data);
        process.stdout.write(`[${ts}] \x1b[36m${parsed.event}\x1b[0m ${data}\n`);
      }
    }
    // else: response frame from some other client's RPC — ignore
  }
});

sock.on("close", () => {
  console.error(
    `[events-tail] pipe closed — saw ${totalSeen} events, printed ${totalPrinted}`,
  );
  process.exit(0);
});

sock.on("error", (e) => {
  console.error(`[events-tail] pipe error: ${e.message}`);
  process.exit(4);
});

process.on("SIGINT", () => {
  console.error(`\n[events-tail] sigint — saw ${totalSeen} events, printed ${totalPrinted}`);
  sock.destroy();
  process.exit(0);
});
