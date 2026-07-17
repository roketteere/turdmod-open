// scummap-zone-brief.mjs — dispatch the scummap custom-zone-format update to DeepSeek (cheap).
// Feeds the cracked SCUM custom-zone format + scummap's current zone code; DeepSeek returns a
// knowledge doc + concrete proposed tool updates. Parent (Opus) reviews + applies the writes.
// Run: node scummap-zone-brief.mjs   (writes ./scummap-zone-deepseek-output.md)
import { readFileSync, writeFileSync, existsSync } from "node:fs";

const KEY = process.env.DEEPSEEK_API_KEY;
if (!KEY) { console.error("no DEEPSEEK_API_KEY"); process.exit(1); }

const read = (p, max = 16000) => existsSync(p) ? readFileSync(p, "utf8").slice(0, max) : `(missing: ${p})`;
const R = "C:/Development/Claude/";

const ctx = {
  format: read(R + "turdmod/docs/CUSTOM-ZONE-FORMAT.md"),
  zonesPy: read(R + "scummap/apps/auto-map/src/scummy_auto_map/zones.py"),
  vanillaZones: read(R + "scummap/apps/auto-map/data/vanilla-zones.json", 6000),
  zonePanel: read(R + "scummap/apps/web/src/components/ZoneEditPanel.tsx"),
};

const system = "You are a senior engineer updating the 'scummap' project (a SCUM interactive-map app with custom-map-making tools). " +
  "We just reverse-engineered SCUM's native Custom Zone persistence format (stored in SCUM.db). " +
  "Update scummap's understanding so its zone/custom-map tools can read, represent, and (eventually) export SCUM custom zones. " +
  "Be concrete and surgical. Output ONLY: (1) a Markdown knowledge section for scummap docs explaining the SCUM.db custom-zone format in scummap's terms, " +
  "(2) specific proposed changes to scummap's zone code (zones.py model + ZoneEditPanel.tsx + the vanilla-zones.json schema) to support the new fields " +
  "(per-zone config: color RGB, handling_methods per ECustomZoneEvent Allow/Block/Ignore, damage_handling per EDamageActorType for PvP, region geometry circle/rect). " +
  "Give code as diffs or full snippets the maintainer can paste. Do not invent APIs not implied by the provided code.";

const user =
  `## Cracked SCUM Custom Zone format (source of truth)\n${ctx.format}\n\n` +
  `## scummap current: auto-map zones.py\n\`\`\`python\n${ctx.zonesPy}\n\`\`\`\n\n` +
  `## scummap current: vanilla-zones.json (sample)\n\`\`\`json\n${ctx.vanillaZones}\n\`\`\`\n\n` +
  `## scummap current: web ZoneEditPanel.tsx\n\`\`\`tsx\n${ctx.zonePanel}\n\`\`\`\n\n` +
  `Produce the two deliverables now.`;

const body = {
  model: "deepseek-chat",
  messages: [{ role: "system", content: system }, { role: "user", content: user }],
  temperature: 0.2, max_tokens: 8000,
};

console.error(`[deepseek] dispatching scummap-zone brief (${user.length} chars in)...`);
const t0 = Date.now();
const resp = await fetch("https://api.deepseek.com/chat/completions", {
  method: "POST",
  headers: { "Content-Type": "application/json", Authorization: `Bearer ${KEY}` },
  body: JSON.stringify(body),
});
if (!resp.ok) { console.error(`[deepseek] HTTP ${resp.status}: ${await resp.text()}`); process.exit(2); }
const data = await resp.json();
const out = data.choices?.[0]?.message?.content ?? "(no content)";
const usage = data.usage ?? {};
const cost = ((usage.prompt_tokens || 0) * 0.27 + (usage.completion_tokens || 0) * 1.10) / 1e6;
writeFileSync("scummap-zone-deepseek-output.md", out, "utf8");
console.error(`[deepseek] done in ${((Date.now() - t0) / 1000).toFixed(1)}s | tokens in=${usage.prompt_tokens} out=${usage.completion_tokens} | ~$${cost.toFixed(4)} | wrote scummap-zone-deepseek-output.md`);
