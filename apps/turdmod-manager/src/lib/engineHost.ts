// Global engine-host selector: which SCUM server the manager talks to —
// the LOCAL box or the remote production server. One module-level store so a
// single sidebar dropdown drives every page; engineRpc() reads it to route
// each bridge call to engine_rpc (local pipe) or remote_engine_rpc (remote tunnel).
//
// @inv values are 'local' | 'remote' to match the Rust RemoteClient::for_target
//      + the scumdb_* tauri cmds. UI labels them Local / remote server.
// @dep tauri-engine.ts:engineRpc + engineRpcWithLog.ts read getEngineHost().

import { useSyncExternalStore } from 'react';

export type EngineHost = 'local' | 'remote';

export const ENGINE_HOSTS: { value: EngineHost; label: string }[] = [
  { value: 'local', label: '🏠 Local' },
  { value: 'remote', label: '☁ Remote' },
];

const KEY = 'turdmod.engineHost';

function read(): EngineHost {
  // Default to local — user configures remote explicitly via the host switcher.
  const v = (typeof localStorage !== 'undefined' && localStorage.getItem(KEY)) || 'local';
  return v === 'remote' ? 'remote' : 'local';
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
