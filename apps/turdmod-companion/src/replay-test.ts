/**
 * One-shot replay: read the live login.log + gameplay.log from the running
 * server, run every line through the parser + runtime, prove the
 * mods-fire chain works on REAL data without depending on the live
 * tailer. Used to isolate a "no events fire" bug.
 */
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { parseSingleLine, parseKillBlock } from "./parsers.js";
import { createCompanionRuntime, type ModBinding } from "./runtime.js";
import { ConsoleSink, FileSink, SinkRouter } from "./sinks.js";
import { withRuntime } from "@turdmod/turdmod-api";

const LOGS = "C:/Program Files (x86)/Steam/steamapps/common/SCUM Server/SCUM/Saved/SaveFiles/Logs";
const REPO = "C:/Development/Claude/scummap";

function loadFileLines(p: string): string[] {
  const buf = readFileSync(p);
  const text = buf.toString("utf16le");
  return text.split(/\r?\n/);
}

async function main() {
  const router = new SinkRouter();
  router.add(new ConsoleSink());
  router.add(new FileSink("/tmp/replay-events.ndjson"));

  const runtime = createCompanionRuntime({ buildId: "23128448", storeDir: "/tmp/turdmod-replay-store", router });

  // Load the four mods inline.
  const modIds = ["kill-feed", "my-squad", "welcome-screen", "vehicle-manager"];
  for (const id of modIds) {
    const file = join(REPO, "examples/turdmod", id, "scripts/main.ts");
    const url = "file:///" + file.replace(/\\/g, "/").replace(/^([A-Za-z]:)/, "$1");
    const m = await import(url) as Record<string, unknown>;
    const binding: ModBinding = {
      id,
      load: async () => ({
        on_load: typeof m.on_load === "function" ? (m.on_load as () => void) : undefined,
      }),
    };
    await runtime.bindMod(binding);
    console.log(`[replay] loaded ${id}`);
  }

  // Replay every recognized log line.
  const files = readdirSync(LOGS).filter(n => /^(login|gameplay|chat|vehicle_destruction|admin)_/i.test(n) && n.endsWith(".log"));
  console.log(`[replay] replaying ${files.length} logs`);
  let count = 0;
  for (const f of files) {
    const path = join(LOGS, f);
    for (const line of loadFileLines(path)) {
      if (!line.trim()) continue;
      const ev = parseSingleLine(line);
      if (ev) {
        console.log(`[replay] dispatch ${ev.kind} ${(ev as any).player ?? (ev as any).victim ?? (ev as any).vid ?? ""}`);
        runtime.dispatch(ev);
        count++;
      }
    }
  }

  // Give async handlers a moment to flush.
  await new Promise(r => setTimeout(r, 500));
  console.log(`[replay] dispatched ${count} events`);
  process.exit(0);
}

main().catch(e => { console.error(e); process.exit(1); });

void withRuntime; // ensure import isn't dead-stripped
