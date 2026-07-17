import { invoke } from '@tauri-apps/api/core';
import { emit } from '@tauri-apps/api/event';
import { getEngineHost } from './engineHost';

export async function engineRpcWithLog<T = unknown>(
  method: string,
  params?: Record<string, unknown>
): Promise<T> {
  const ts = Date.now();
  const id = `${ts}-${Math.random().toString(36).slice(2, 8)}`;
  await emit('wire://rpc-call', { id, method, params, ts });
  const cmd = getEngineHost() === 'remote' ? 'remote_engine_rpc' : 'engine_rpc';
  try {
    const result = await invoke<T>(cmd, { method, params: params ?? {} });
    await emit('wire://rpc-reply', {
      id,
      method,
      ok: true,
      result,
      ts: Date.now(),
      durationMs: Date.now() - ts,
    });
    return result;
  } catch (e) {
    await emit('wire://rpc-reply', {
      id,
      method,
      ok: false,
      error: String(e),
      ts: Date.now(),
      durationMs: Date.now() - ts,
    });
    throw e;
  }
}