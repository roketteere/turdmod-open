/**
 * Live tailer probe — opens the SCUM logs dir, prints every line as it
 * comes in. Used to isolate "no events fire" from log-tail.ts itself.
 */
import { startTailer } from "./log-tail.js";
import { parseSingleLine } from "./parsers.js";

const LOGS = "C:/Program Files (x86)/Steam/steamapps/common/SCUM Server/SCUM/Saved/SaveFiles/Logs";
console.log("[tail-test] watching", LOGS);

const stop = startTailer({
  logsDir: LOGS,
  pollMs: 500,
  onLine(line, kind, src) {
    const ev = parseSingleLine(line);
    console.log(`[tail-test] ${kind} ${src.split(/[\\/]/).pop()}: ${line.slice(0, 120)}${ev ? ` => ${ev.kind}` : ""}`);
  },
  onKillBlock(block, src) {
    console.log(`[tail-test] kill-block from ${src.split(/[\\/]/).pop()}: ${block.split("\n")[0]}...`);
  },
});

process.on("SIGINT", () => { stop(); process.exit(0); });
console.log("[tail-test] now log in/out on the SCUM server. Ctrl+C to stop.");
