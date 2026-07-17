// Shared engine-bridge client. Reads the SAME config the Manager uses and
// POSTs to the turdmod-service /engine/rpc relay. Used by engine-rpc.mjs
// (CLI) and regen-admin-catalog.mjs.
//   remote → %LOCALAPPDATA%\TurdMOD\remote.json  { host, port, token }
//   local  → C:\TurdMOD\service.json             { port, token }  (host 127.0.0.1)

import { readFileSync } from 'node:fs';
import { join } from 'node:path';

export function loadTarget(target) {
  if (target === 'local') {
    const j = JSON.parse(readFileSync('C:/TurdMOD/service.json', 'utf8'));
    return { host: '127.0.0.1', port: j.port ?? 9090, token: j.token ?? '' };
  }
  const base = process.env.LOCALAPPDATA || join(process.env.USERPROFILE || '', 'AppData/Local');
  const j = JSON.parse(readFileSync(join(base, 'TurdMOD', 'remote.json'), 'utf8'));
  return { host: j.host, port: j.port ?? 9090, token: j.token ?? '' };
}

// Call a bridge RPC. Returns the unwrapped result payload (service wraps as
// { result: … }). Throws on transport / HTTP / non-OK.
export async function callEngine(target, method, params = {}) {
  const cfg = loadTarget(target === 'local' ? 'local' : 'remote');
  const url = `http://${cfg.host}:${cfg.port}/engine/rpc`;
  const res = await fetch(url, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${cfg.token}` },
    body: JSON.stringify({ method, params }),
  });
  const text = await res.text();
  if (!res.ok) throw new Error(`HTTP ${res.status}: ${text}`);
  const j = JSON.parse(text);
  return 'result' in j ? j.result : j;
}
