#!/usr/bin/env node
// Standing extraction-team QUEUE processor. Drains pending RE/mapping jobs
// through tools/deepseek/run.mjs (cheap DeepSeek), writing dossiers AHEAD of
// need so the engine/mod work always has its answers waiting. Pure cloud — no
// GPU, no lobe — so it is safe to run on a schedule alongside live sessions.
//
// Usage:
//   node tools/deepseek/queue.mjs            # run the next pending job
//   node tools/deepseek/queue.mjs --all      # drain every pending job
//   node tools/deepseek/queue.mjs --list     # show the queue
//
// Add a job: append to docs/engine/research/_queue/queue.json with a
// prompt_file (self-contained — bake the context in) + out path, status:"pending".
// @dep: tools/deepseek/run.mjs  @inv: prompts are self-contained (no GPU/lobe).
import { readFileSync, writeFileSync, existsSync, appendFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dir = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(__dir, '..', '..'); // turdmod root
const QUEUE = join(ROOT, 'docs/engine/research/_queue/queue.json');
const LOG = join(ROOT, 'docs/engine/research/_queue/queue.log');
const RUN = join(__dir, 'run.mjs');

const load = () => JSON.parse(readFileSync(QUEUE, 'utf8'));
const save = (q) => writeFileSync(QUEUE, JSON.stringify(q, null, 2) + '\n', 'utf8');
const log = (m) => { try { appendFileSync(LOG, `[${new Date().toISOString()}] ${m}\n`); } catch {} };

const args = process.argv.slice(2);
const q = load();

if (args.includes('--list')) {
  for (const j of q.jobs) console.log(`${String(j.status).padEnd(8)} ${j.id}  —  ${j.title}`);
  process.exit(0);
}

const pending = q.jobs.filter((j) => j.status === 'pending');
if (!pending.length) { console.log('queue empty — nothing pending'); process.exit(0); }
const todo = args.includes('--all') ? pending : [pending[0]];

let ok = 0, fail = 0;
for (const job of todo) {
  const promptPath = join(ROOT, job.prompt_file);
  const outPath = join(ROOT, job.out);
  if (!existsSync(promptPath)) {
    job.status = 'error'; job.note = `missing prompt ${job.prompt_file}`; save(q);
    console.error(`SKIP ${job.id}: ${job.note}`); fail++; continue;
  }
  console.log(`running ${job.id} (${job.model || 'deepseek-reasoner'})…`);
  job.status = 'running'; save(q);
  const r = spawnSync('node', [
    RUN, '--in', promptPath, '--out', outPath,
    '--model', job.model || 'deepseek-reasoner', '--max', String(job.max || 9000),
  ], { stdio: 'inherit' });
  if (r.status === 0) {
    job.status = 'done'; job.completed_unix = Math.floor(Date.now() / 1000);
    log(`done ${job.id} -> ${job.out}`); console.log(`✓ ${job.id} -> ${job.out}`); ok++;
  } else {
    job.status = 'error'; job.note = `run.mjs exit ${r.status}`;
    log(`error ${job.id} exit ${r.status}`); console.error(`✗ ${job.id} failed`); fail++;
  }
  save(q);
}
console.log(`queue: ${ok} done, ${fail} failed, ${q.jobs.filter((j) => j.status === 'pending').length} still pending`);
process.exit(fail ? 1 : 0);
