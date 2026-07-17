// Typed wrappers for the in-game turdmod-loader RPC surface.
//
// The loader's inbound pipe is exposed by the Tauri `loader_rpc` command
// (src-tauri/src/loader_rpc.rs). The loader is injected into SCUM.exe via
// the dxgi.dll proxy and writes its pipe discovery file at
// %LOCALAPPDATA%/TurdMOD/loader/pipe.txt.
//
// Methods supported in V1:
//   - ping            → { ok, version, pid }
//   - enqueuePanel    → { queued: true } (params = the panel JSON itself)

import { invoke } from '@tauri-apps/api/core';

export interface LoaderPing {
  ok: boolean;
  version: string;
  pid: number;
}

export interface EnqueuePanelResult {
  queued: boolean;
}

/** Generic JSON-RPC pass-through to the in-game loader's named pipe.
 *  Throws if the loader isn't injected (no pipe.txt) or the RPC times
 *  out after 30 s. */
export function loaderRpc<T = unknown>(
  method: string,
  params?: unknown,
): Promise<T> {
  return invoke<T>('loader_rpc', { method, params: params ?? null });
}

/** Liveness ping. Resolves with `{ ok, version, pid }` when the loader is
 *  online; rejects with the pipe error string otherwise. */
export function loaderPing(): Promise<LoaderPing> {
  return loaderRpc<LoaderPing>('ping');
}

/** Enqueue a panel into the in-game ImGui render queue. The payload shape
 *  is whatever `hooks::enqueue_panel` understands — for V1 that's
 *  free-form JSON; the render loop pattern-matches at draw time. */
export function enqueuePanel(
  payload: Record<string, unknown>,
): Promise<EnqueuePanelResult> {
  return loaderRpc<EnqueuePanelResult>('enqueuePanel', payload);
}
