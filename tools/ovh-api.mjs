#!/usr/bin/env node
// ovh-api — authed GET/POST to the turdmod-service HTTP API, reading the
// bearer token from the Manager's config (never inline on the command line).
//   node tools/ovh-api.mjs <remote|local> <GET|POST> <path> [bodyJson]
//   node tools/ovh-api.mjs remote GET /health
//   node tools/ovh-api.mjs remote POST /server/start
import { loadTarget } from './lib/engine-client.mjs';

const [, , targetArg, methodArg, path, bodyArg] = process.argv;
if (!path) { console.error('usage: ovh-api <remote|local> <GET|POST> <path> [bodyJson]'); process.exit(2); }
const method = (methodArg || 'GET').toUpperCase();
const cfg = loadTarget(targetArg === 'local' ? 'local' : 'remote');
const url = `http://${cfg.host}:${cfg.port}${path}`;
const init = { method, headers: { Authorization: `Bearer ${cfg.token}` } };
if (bodyArg) { init.headers['Content-Type'] = 'application/json'; init.body = bodyArg; }

const res = await fetch(url, init).catch((e) => { console.error('request failed:', e.message); process.exit(1); });
const text = await res.text();
console.log(`HTTP ${res.status}`);
try { console.log(JSON.stringify(JSON.parse(text), null, 2)); } catch { console.log(text); }
if (!res.ok) process.exit(1);
