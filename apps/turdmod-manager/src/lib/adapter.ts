import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import {
  EngineRpcAdapter,
  type ServerAdapter,
} from '@turdmod/turdmod-core';

// Manager's adapter instance — engine-tier route into the local
// companion + named-pipe bridge. Lite/Pro will construct their own
// adapter instances (RemoteFtpAdapter for Lite, EngineRpcAdapter
// pointed at remote SSH-tunneled bridge for Pro).
//
// We pass Tauri's invoke + listen via the adapter config so the
// turdmod-core package doesn't have to import @tauri-apps/api itself
// (keeps it Tauri-version-agnostic).
//
// This is the FIRST migration callsite — only the engineRpc method is
// routed through the adapter for now. Other Manager flows
// (manager_read_text_file, manager_server_*) continue to invoke()
// directly. Migrating each one to call the adapter is incremental
// work; the goal here is to prove the abstraction works end-to-end.

let cached: ServerAdapter | null = null;

export function getEngineAdapter(): ServerAdapter {
  if (cached) return cached;
  cached = new EngineRpcAdapter({
    host: 'localhost',
    // Wrap Tauri's invoke to match the adapter's looser signature
    // (Tauri adds an InvokeArgs constraint + an options param we don't
    // forward).
    invoke: <T = unknown>(cmd: string, args?: object) =>
      invoke<T>(cmd, args as Record<string, unknown> | undefined),
    listenEvent: async (event, handler) => {
      const unlisten = await listen(event, (e) => handler(e.payload));
      return unlisten;
    },
  });
  return cached;
}

// Convenience wrapper matching the existing tauri-engine.ts engineRpc
// signature so callers can switch without changing their code shape.
export function adapterEngineRpc<T = unknown>(
  method: string,
  params?: Record<string, unknown>,
): Promise<T> {
  return getEngineAdapter().engineRpc<T>(method, params);
}
