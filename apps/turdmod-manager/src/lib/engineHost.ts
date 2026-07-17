// Global engine-host selector: which SCUM server the manager talks to —
// the LOCAL box or the OVH production server. One module-level store so a
// single sidebar dropdown drives every page; engineRpc() reads it to route
// each bridge call to engine_rpc (local pipe) or remote_engine_rpc (OVH tunnel).
//
// @inv values are 'local' | 'remote' to match the Rust RemoteClient::for_target
//      + the scumdb_* tauri cmds. UI labels them Local / OVH.
// @dep tauri-engine.ts:engineRpc + engineRpcWithLog.ts read getEngineHost().

import { useSyncExternalStore } from 'react';

export type EngineHost = 'local' | 'remote';

export const ENGINE_HOSTS: { value: EngineHost; label: string }[] = [
  { value: 'local', label: '🏠 Local' },
  { value: 'remote', label: '☁ OVH' },
];

const KEY = 'turdmod.engineHost';

function read(): EngineHost {
  // Default to OVH (the live server) — local has no running engine, so a Local
  // default silently routed everything to a dead pipe (no spawn, no live data).
  const v = (typeof localStorage !== 'undefined' && localStorage.getItem(KEY)) || 'remote';
  return v === 'local' ? 'local' : 'remote';
}

let current: EngineHost = read();
const subs = new Set<() => void>();

export function getEngineHost(): EngineHost {
  return current;
}

export function setEngineHost(h: EngineHost): void {
  if (h === current) return;
  current = h;
  try { localStorage.setItem(KEY, h); } catch { /* ignore */ }
  subs.forEach((f) => f());
}

function subscribe(cb: () => void): () => void {
  subs.add(cb);
  return () => { subs.delete(cb); };
}

// React binding — components re-render when the host flips.
export function useEngineHost(): EngineHost {
  return useSyncExternalStore(subscribe, getEngineHost, getEngineHost);
}
