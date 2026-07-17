#!/usr/bin/env node
// regen-admin-catalog — refresh src/data/admin-commands.json from the LIVE
// game's dumpAdminCommands (the authoritative ~231-verb BP walk). Run after
// every SCUM update to keep the autocomplete catalog current.
//
//   node tools/regen-admin-catalog.mjs [remote|local]   (default: remote)
//
// Preserves each verb's existing category; new verbs are slotted by the same
// inferCategory() heuristic the Admin Commands page uses, so the file stays
// consistent with the app. Reports the added/removed delta. Native commands
// (e.g. SetWeather) are NOT in the BP dump — TMM learns those on first use.

import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { callEngine } from './lib/engine-client.mjs';

const CATALOG_PATH = fileURLToPath(new URL('../apps/turdmod-manager/src/data/admin-commands.json', import.meta.url));

// Mirror of AdminCommandsPage.tsx inferCategory(). Keep in sync.
function inferCategory(verb) {
  if (/^(Spawn|Create)/.test(verb)) return 'spawn';
  if (/^(Destroy|Remove|Reset|Clear|Cancel|Ban|Kick|Shutdown)/.test(verb)) return 'destructive';
  if (/^(Print|Dump|List|Show|Visualize|Get|Find|Check|Report|Track|Draw)/.test(verb)) return 'inspect';
  if (/(Toggle|Enable|Disable|Debug)/.test(verb)) return 'debug';
  if (/(Garden|Plant|Farming)/.test(verb)) return 'garden';
  if (/(ToAll|ToAllOnline)/.test(verb)) return 'bulk';
  if (/(Weather|Time|Map|Tournament|Vote|Encounter|Hunt|Cargo|Quest|Notification|Reload)/.test(verb)) return 'world';
  if (/(Player|Prisoner|Squad|Body|Skill|Currency|Fame|Teleport|Mute|Silence|Sleep|Inventory|Loot|Bleeding|Injury)/.test(verb)) return 'player';
  return 'misc';
}

async function main() {
  const target = process.argv[2] === 'local' ? 'local' : 'remote';

  const existing = JSON.parse(readFileSync(CATALOG_PATH, 'utf8'));
  const catOrder = Object.keys(existing); // preserve file's category order
  const prevVerbs = new Set(Object.values(existing).flat());
  const categoryByVerb = new Map();
  for (const [cat, verbs] of Object.entries(existing)) {
    for (const v of verbs) categoryByVerb.set(v, cat);
  }

  console.log(`pulling dumpAdminCommands from ${target}…`);
  const dump = await callEngine(target, 'dumpAdminCommands', {});
  const liveVerbs = (dump.commands ?? []).map((c) => c.verb).filter(Boolean);
  if (liveVerbs.length === 0) throw new Error('dump returned 0 verbs — engine reachable? player not required.');

  const liveSet = new Set(liveVerbs);
  const added = liveVerbs.filter((v) => !prevVerbs.has(v)).sort();
  const removed = [...prevVerbs].filter((v) => !liveSet.has(v)).sort();

  // Rebuild grouped catalog: keep existing category, infer for new verbs.
  const grouped = Object.fromEntries(catOrder.map((c) => [c, []]));
  for (const verb of liveVerbs) {
    const cat = categoryByVerb.get(verb) ?? inferCategory(verb);
    (grouped[cat] ??= []).push(verb);
  }
  for (const c of Object.keys(grouped)) grouped[c] = [...new Set(grouped[c])].sort();

  writeFileSync(CATALOG_PATH, JSON.stringify(grouped, null, 2) + '\n');

  console.log(`\nwrote ${CATALOG_PATH}`);
  console.log(`total verbs: ${liveVerbs.length}  (was ${prevVerbs.size})`);
  console.log(`added (${added.length}): ${added.join(', ') || '—'}`);
  console.log(`removed (${removed.length}): ${removed.join(', ') || '—'}`);
  for (const c of catOrder) console.log(`  ${c}: ${grouped[c].length}`);
}

main().catch((e) => { console.error(e.message); process.exit(1); });
