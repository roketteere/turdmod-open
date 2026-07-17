import { listen } from '@tauri-apps/api/event';

export interface WireEntry {
  id: string;
  type: 'engine-event' | 'rpc-call' | 'rpc-reply' | 'lifecycle' | 'error';
  name: string;
  payload: unknown;
  ts: number;
  durationMs?: number;
}

interface WireStats {
  total: number;
  byType: Record<string, number>;
  eps: number;
}

const MAX_ENTRIES = 5000;
const EPS_WINDOW_MS = 10000;

class WireStore {
  private entries: WireEntry[] = [];
  private listeners: Set<() => void> = new Set();
  private paused = false;
  private stats: WireStats = { total: 0, byType: {}, eps: 0 };
  private epsTimestamps: number[] = [];
  private unlistenFns: (() => void)[] = [];

  async init() {
    // Listen to engine events
    const unlistenEngine = await listen<{ event: string; data: unknown; receivedAtMs: number }>(
      'engine://event',
      (event) => {
        this.addEntry({
          id: `${event.payload.receivedAtMs}-${Math.random().toString(36).slice(2, 8)}`,
          type: 'engine-event',
          name: event.payload.event,
          payload: event.payload.data,
          ts: event.payload.receivedAtMs,
        });
      }
    );
    this.unlistenFns.push(unlistenEngine);

    // Listen to RPC calls
    const unlistenRpcCall = await listen<{ id: string; method: string; params: unknown; ts: number }>(
      'wire://rpc-call',
      (event) => {
        this.addEntry({
          id: event.payload.id,
          type: 'rpc-call',
          name: event.payload.method,
          payload: event.payload.params,
          ts: event.payload.ts,
        });
      }
    );
    this.unlistenFns.push(unlistenRpcCall);

    // Listen to RPC replies
    const unlistenRpcReply = await listen<{
      id: string;
      method: string;
      ok: boolean;
      result?: unknown;
      error?: string;
      ts: number;
      durationMs: number;
    }>('wire://rpc-reply', (event) => {
      this.addEntry({
        id: event.payload.id,
        type: 'rpc-reply',
        name: event.payload.method,
        payload: event.payload.ok ? event.payload.result : event.payload.error,
        ts: event.payload.ts,
        durationMs: event.payload.durationMs,
      });
    });
    this.unlistenFns.push(unlistenRpcReply);
  }

  destroy() {
    this.unlistenFns.forEach((fn) => fn());
    this.unlistenFns = [];
  }

  private addEntry(entry: WireEntry) {
    if (this.paused) return;
    this.entries.unshift(entry);
    if (this.entries.length > MAX_ENTRIES) {
      this.entries.pop();
    }
    this.updateStats(entry);
    this.notify();
  }

  private updateStats(entry: WireEntry) {
    this.stats.total++;
    this.stats.byType[entry.type] = (this.stats.byType[entry.type] || 0) + 1;
    // Update EPS
    const now = Date.now();
    this.epsTimestamps.push(now);
    // Remove timestamps older than window
    while (this.epsTimestamps.length > 0 && this.epsTimestamps[0] < now - EPS_WINDOW_MS) {
      this.epsTimestamps.shift();
    }
    const count = this.epsTimestamps.length;
    this.stats.eps = count / (EPS_WINDOW_MS / 1000);
  }

  getEntries(): WireEntry[] {
    return this.entries;
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private notify() {
    this.listeners.forEach((fn) => fn());
  }

  clear() {
    this.entries = [];
    this.stats = { total: 0, byType: {}, eps: 0 };
    this.epsTimestamps = [];
    this.notify();
  }

  setPaused(p: boolean) {
    this.paused = p;
  }

  isPaused(): boolean {
    return this.paused;
  }

  getStats(): WireStats {
    return { ...this.stats, byType: { ...this.stats.byType } };
  }
}

export const wireStore = new WireStore();

export function initWireStore() {
  wireStore.init();
}