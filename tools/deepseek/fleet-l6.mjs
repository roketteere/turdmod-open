#!/usr/bin/env node
// L6 SYSTEMS-MODEL fleet — the next rung. Synthesizes the per-class L5 digests
// into per-SYSTEM models ("the whole damage system, wired together"). Cloud
// (deepseek-chat), no GPU. Its own budget/state (independent of L5).
//
// Usage: node tools/deepseek/fleet-l6.mjs [--cap 3] [--conc 8]
import { readFileSync, writeFileSync, existsSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';

const arg = (n, d) => { const i = process.argv.indexOf(`--${n}`); return i >= 0 && process.argv[i + 1] ? process.argv[i + 1] : d; };
const KEY = process.env.DEEPSEEK_API_KEY; if (!KEY) { console.error('no DEEPSEEK_API_KEY'); process.exit(2); }
const BUILD = arg('build', 'v23451409');
const L5 = `C:/Development/Claude/turdmod/docs/engine/research/l5-digests/${BUILD}/digests.jsonl`;
const OUT = `C:/Development/Claude/turdmod/docs/engine/research/l6-systems/${BUILD}`;
const STATE = join(OUT, 'l6-state.json');
const MODEL = arg('model', 'deepseek-chat');
const CAP = parseFloat(arg('cap', '3'));
const CONC = parseInt(arg('conc', '8'), 10);
const PRICE_IN_MISS = 0.27, PRICE_IN_HIT = 0.07, PRICE_OUT = 1.10;

const SYSTEMS = [
  { key: 'damage-combat', name: 'Damage & Combat', terms: ['damage', 'wound', 'melee', 'ranged', 'ballistic', 'combat', 'attack', 'weapon', 'firearm', 'recoil'] },
  { key: 'vehicles', name: 'Vehicles', terms: ['vehicle', 'wheel', 'gearbox', 'drivetrain', 'dcx', 'tank', 'tire', 'chassis', 'engine'] },
  { key: 'prisoner-physiology', name: 'Prisoner Physiology & Health', terms: ['bodysymptom', 'bodycondition', 'metabolism', 'disease', 'symptom', 'nutrition', 'hydration', 'immune', 'sickness', 'vitamin'] },
  { key: 'economy-trading', name: 'Economy & Trading', terms: ['economy', 'currency', 'trade', 'trader', 'money', 'price', 'shop', 'market', 'fund'] },
  { key: 'inventory-items', name: 'Inventory & Items', terms: ['inventory', 'container', 'equip', 'attachment', 'magazine', 'storage', 'itemstack', 'slot'] },
  { key: 'crafting', name: 'Crafting', terms: ['craft', 'recipe', 'blueprint', 'workbench', 'fabricat', 'cooking', 'squeeze'] },
  { key: 'building-base', name: 'Building & Base Raiding', terms: ['building', 'construct', 'baseelement', 'flag', 'barrier', 'raid', 'lockpick', 'fortification'] },
  { key: 'loot-spawning', name: 'Loot & Spawning', terms: ['loot', 'spawn', 'lootbox', 'encounter', 'respawn', 'pickup', 'cache'] },
  { key: 'npc-ai', name: 'NPC & AI', terms: ['npc', 'zombie', 'puppet', 'animal', 'sentry', 'dropship', 'patrol', 'behavior', 'aicontroller'] },
  { key: 'squad-social', name: 'Squad & Social', terms: ['squad', 'team', 'social', 'friend', 'party', 'fameflag'] },
  { key: 'progression-fame', name: 'Progression, Fame & Skills', terms: ['fame', 'skill', 'attribute', 'progression', 'experience', 'perk', 'mastery'] },
  { key: 'networking-rpc', name: 'Networking & RPC', terms: ['rpcchannel', 'replicat', 'packet', 'netid', 'playercontroller', 'serverrpc', 'multicast'] },
  { key: 'chat-comms', name: 'Chat & Comms', terms: ['chat', 'voicechat', 'message', 'broadcast', 'admincommand'] },
  { key: 'world-environment', name: 'World & Environment', terms: ['weather', 'timeofday', 'landscape', 'climate', 'temperature', 'environment', 'sector'] },
  { key: 'quests-events', name: 'Quests & Events', terms: ['quest', 'gameevent', 'mission', 'objective'] },
];

const sysSection = (digest) => { const m = digest.match(/## Systems\s*\n([\s\S]*?)(\n## |$)/i); return m ? m[1].toLowerCase() : ''; };
const whatLine = (digest) => { const m = digest.match(/## What it is\s*\n([^#\n]+)/i); return m ? m[1].replace(/\s+/g, ' ').trim() : ''; };

const SYS = (sys, members) => [
  `You are mapping the "${sys.name}" system of a UE 4.27 survival game from per-class behavior digests (our analysis, inferred from class layout). Below are ${members.length} classes our digests associate with this system.`,
  `Synthesize a SYSTEM MODEL in Markdown with sections:`,
  `## Overview (2-3 sentences: what this system does as a whole)`,
  `## Spine (the 5-12 most important classes and the role each plays — the backbone)`,
  `## Data & control flow (how the classes connect — who drives whom, server vs client, what state flows where)`,
  `## Interaction points (the key functions/RPCs an external tool would hook or call to observe or drive this system)`,
  `## To rebuild it (what an ORIGINAL implementation of this system needs — components, state, the hard parts — for building our own game, NOT copying source)`,
  `## Confidence (high/med/low + biggest gap)`,
  `Be concrete, ~700-1000 words, mark inference vs fact. Don't invent offsets or output engine source.`,
].join('\n');

if (!existsSync(OUT)) mkdirSync(OUT, { recursive: true });
const digests = readFileSync(L5, 'utf8').trimEnd().split('\n').filter(Boolean).map(l => { try { return JSON.parse(l); } catch { return null; } }).filter(Boolean);
const state = existsSync(STATE) ? JSON.parse(readFileSync(STATE, 'utf8')) : { done: {}, spentUsd: 0 };

function membersFor(sys) {
  const seen = new Set(); const out = [];
  for (const d of digests) {
    const hay = (d.name + ' ' + sysSection(d.digest)).toLowerCase();
    if (sys.terms.some(t => hay.includes(t)) && !seen.has(d.name)) { seen.add(d.name); out.push(d); }
  }
  return out.slice(0, 70);
}

async function synth(sys, members) {
  const list = members.map(m => `- ${m.name} (<-${m.parent}): ${whatLine(m.digest).slice(0, 140)}`).join('\n');
  const body = { model: MODEL, temperature: 0.25, max_tokens: 1600,
    messages: [{ role: 'system', content: SYS(sys, members) }, { role: 'user', content: 'Member classes:\n' + list }] };
  const res = await fetch('https://api.deepseek.com/chat/completions', { method: 'POST', headers: { 'content-type': 'application/json', authorization: `Bearer ${KEY}` }, body: JSON.stringify(body) });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  const j = await res.json(); const u = j.usage || {};
  const inHit = u.prompt_cache_hit_tokens ?? 0, inMiss = u.prompt_cache_miss_tokens ?? ((u.prompt_tokens ?? 0) - inHit);
  const cost = (inMiss * PRICE_IN_MISS + inHit * PRICE_IN_HIT + (u.completion_tokens ?? 0) * PRICE_OUT) / 1e6;
  return { text: j.choices?.[0]?.message?.content ?? '', cost, n: members.length };
}

const pending = SYSTEMS.filter(s => !state.done[s.key]);
console.error(`L6: ${SYSTEMS.length} systems, ${pending.length} pending, cap $${CAP}`);
let spent = state.spentUsd, idx = 0, done = 0;
const save = () => writeFileSync(STATE, JSON.stringify({ ...state, spentUsd: spent }, null, 2));
async function worker() {
  while (idx < pending.length && spent < CAP) {
    const sys = pending[idx++]; const mem = membersFor(sys);
    if (!mem.length) { state.done[sys.key] = 1; continue; }
    try {
      const r = await synth(sys, mem); spent += r.cost; done++;
      writeFileSync(join(OUT, sys.key + '.md'), `# ${sys.name} — system model (build ${BUILD})\n\n> L6 synthesis over ${r.n} L5 class digests. Our analysis.\n\n` + r.text + '\n');
      state.done[sys.key] = 1; save();
      console.error(`  ✓ ${sys.key} (${r.n} classes, $${spent.toFixed(4)})`);
    } catch (e) { console.error(`  ! ${sys.key}: ${String(e.message).slice(0, 80)}`); }
  }
}
await Promise.all(Array.from({ length: CONC }, worker));
save();
console.error(`L6 DONE: ${done} systems modeled, $${spent.toFixed(4)} spent -> ${OUT}`);
