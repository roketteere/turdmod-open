#!/usr/bin/env node
// L5 digest FLEET — climbs the RE ladder's behavior/semantics rung at scale.
// Reads the OFFLINE SCUM SDK class dump and, per meaningful class, asks
// deepseek-chat for a BEHAVIOR/UNDERSTANDING digest (what the class does + its
// systems role + key fields/methods, in OUR analysis) — the raw knowledge that
// feeds an original game, NOT a reproduction of source.
//
// Cloud + parallel (no GPU). Concurrency pool + checkpoint/resume + a HARD $ cap
// computed from real per-call usage, so it physically cannot overrun.
//
// Usage:
//   node tools/deepseek/fleet.mjs --calibrate           # 20 units, print real $/unit + extrapolation
//   node tools/deepseek/fleet.mjs --cap 20 --conc 14     # the blitz, halts at $20
//   node tools/deepseek/fleet.mjs --cap 20 --resume      # continue where it stopped
//
// Env: DEEPSEEK_API_KEY.  @dep: data/extracted/<build>/classes.json (scumdump)
import { readFileSync, writeFileSync, appendFileSync, existsSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';

const arg = (n, d) => { const i = process.argv.indexOf(`--${n}`); return i >= 0 && process.argv[i + 1] && !process.argv[i + 1].startsWith('--') ? process.argv[i + 1] : d; };
const flag = (n) => process.argv.includes(`--${n}`);

const KEY = process.env.DEEPSEEK_API_KEY;
if (!KEY) { console.error('DEEPSEEK_API_KEY not set'); process.exit(2); }

const BUILD = arg('build', 'v23451409');
const DUMP = `C:/Development/Claude/scumdump/data/extracted/${BUILD}/classes.json`;
const OUTDIR = `C:/Development/Claude/turdmod/docs/engine/research/l5-digests/${BUILD}`;
const DIGESTS = join(OUTDIR, 'digests.jsonl');
const STATE = join(OUTDIR, 'fleet-state.json');
const MODEL = arg('model', 'deepseek-chat');
const CAP = parseFloat(arg('cap', flag('calibrate') ? '999' : '20'));   // USD hard ceiling
const CONC = parseInt(arg('conc', '14'), 10);
const LIMIT = flag('calibrate') ? 20 : (arg('limit') ? parseInt(arg('limit'), 10) : Infinity);

// deepseek-chat pricing (USD per 1M tokens) — used for the meter; conservative.
const PRICE_IN_MISS = 0.27, PRICE_IN_HIT = 0.07, PRICE_OUT = 1.10;

const SYSTEM = [
  'You are a senior Unreal Engine 4.27 reverse-engineer building a KNOWLEDGE BASE about a game\'s',
  'systems from its (factual) class layout — class name, parent, byte size, property names+types+offsets,',
  'and function names+signatures. Your job is ANALYSIS/UNDERSTANDING, never reproducing source code.',
  'For the given class, output a compact Markdown digest with EXACTLY these sections:',
  '## What it is  (one line: the class\'s role in the game)',
  '## Systems  (which gameplay systems it belongs to / connects to — inventory, damage, vehicle, AI, economy, UI, networking, etc.)',
  '## Key state  (the 3-8 most telling properties and what they represent — infer purpose from name+type)',
  '## Behaviour  (what its notable functions do, inferred from their names+params; note server vs client, replication)',
  '## Relationships  (parent/likely-siblings; what it owns or is owned by)',
  '## Confidence  (high/med/low + the single biggest unknown)',
  'Be concise (~600-900 words max). The class layout pasted below is your ONLY ground truth — the field/',
  'function NAMES and OFFSETS are FACT; everything you say they DO is INFERENCE. Never invent a field,',
  'function, or offset not in the layout. Mark inference vs fact, and where you infer a behavior that an',
  'external tool should rely on, note it needs live validation. Do NOT output C++ or reproduce source.',
].join('\n');

function trimUnit(u) {
  const props = (u.properties || u.fields || u.members || []).slice(0, 70)
    .map(p => ({ n: p.name ?? p.n, t: p.type ?? p.t, o: p.offset ?? p.o }));
  const funcs = (u.functions || u.methods || []).slice(0, 70)
    .map(f => ({ n: f.name ?? f.n, p: (f.params || f.parameters || []).map(x => (x.name ? `${x.name}:${x.type}` : x)).slice(0, 10) }));
  return { name: u.name, kind: u.kind, parent: u.parent, size: u.size, properties: props, functions: funcs };
}
const isMeaningful = (u) => ((u.properties || u.fields || []).length > 0) || ((u.functions || u.methods || []).length > 0);

async function digest(unit) {
  const body = {
    model: MODEL, temperature: 0.2, max_tokens: 1200,
    messages: [{ role: 'system', content: SYSTEM },
               { role: 'user', content: 'Class layout (factual):\n```json\n' + JSON.stringify(trimUnit(unit)) + '\n```' }],
  };
  const res = await fetch('https://api.deepseek.com/chat/completions', {
    method: 'POST', headers: { 'content-type': 'application/json', authorization: `Bearer ${KEY}` },
    body: JSON.stringify(body),
  });
  if (!res.ok) throw new Error(`HTTP ${res.status}: ${(await res.text()).slice(0, 180)}`);
  const j = await res.json();
  const u = j.usage || {};
  const inHit = u.prompt_cache_hit_tokens ?? 0;
  const inMiss = u.prompt_cache_miss_tokens ?? ((u.prompt_tokens ?? 0) - inHit);
  const cost = (inMiss * PRICE_IN_MISS + inHit * PRICE_IN_HIT + (u.completion_tokens ?? 0) * PRICE_OUT) / 1e6;
  return { text: j.choices?.[0]?.message?.content ?? '', cost, out: u.completion_tokens ?? 0 };
}

// ---- load + worklist ----
if (!existsSync(OUTDIR)) mkdirSync(OUTDIR, { recursive: true });
const all = JSON.parse(readFileSync(DUMP, 'utf8'));
const unitsAll = Array.isArray(all) ? all : (all.classes || all.objects || all[Object.keys(all)[0]]);
const worklist = unitsAll.filter(isMeaningful);
const state = existsSync(STATE) ? JSON.parse(readFileSync(STATE, 'utf8')) : { done: {}, spentUsd: 0, started: null };
const pending = worklist.filter(u => !state.done[u.name]).slice(0, LIMIT === Infinity ? undefined : LIMIT);

console.error(`fleet: ${unitsAll.length} classes, ${worklist.length} meaningful, ${Object.keys(state.done).length} already done, ${pending.length} this run`);
console.error(`model=${MODEL} conc=${CONC} cap=$${CAP} spent so far=$${state.spentUsd.toFixed(4)}`);

let spent = state.spentUsd, done = 0, failed = 0, stopped = false;
const save = () => writeFileSync(STATE, JSON.stringify({ ...state, spentUsd: spent }, null, 2));

let idx = 0;
async function worker(id) {
  while (true) {
    if (stopped || spent >= CAP) { stopped = true; return; }
    const i = idx++; if (i >= pending.length) return;
    const unit = pending[i];
    try {
      const r = await digest(unit);
      spent += r.cost; done++;
      state.done[unit.name] = 1;
      appendFileSync(DIGESTS, JSON.stringify({ name: unit.name, parent: unit.parent, kind: unit.kind, build: BUILD, digest: r.text }) + '\n');
      if (done % 10 === 0) { save(); console.error(`  +${done} done | $${spent.toFixed(4)} | ~$${(spent / done).toFixed(5)}/unit | ${unit.name}`); }
    } catch (e) {
      failed++; if (failed <= 5) console.error(`  ! ${unit.name}: ${String(e.message).slice(0, 120)}`);
    }
  }
}

await Promise.all(Array.from({ length: CONC }, (_, k) => worker(k)));
save();
const perUnit = done ? spent / done : 0;
console.error(`\nDONE: ${done} digested, ${failed} failed, $${spent.toFixed(4)} spent (~$${perUnit.toFixed(5)}/unit)${stopped && spent >= CAP ? ' [HIT CAP]' : ''}`);
if (flag('calibrate')) {
  const remaining = worklist.length - Object.keys(state.done).length;
  console.error(`EXTRAPOLATION: ${worklist.length} meaningful units × $${perUnit.toFixed(5)} ≈ $${(worklist.length * perUnit).toFixed(2)} for the full SDK`);
  console.error(`remaining after this run: ${remaining} → ≈ $${(remaining * perUnit).toFixed(2)} more`);
}
