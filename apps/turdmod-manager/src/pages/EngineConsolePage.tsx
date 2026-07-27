import { useEffect, useMemo, useRef, useState } from 'react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { engineRpc } from '../lib/tauri-engine';

// Engine Console — the "raw" sister to TurdCOM Terminal. Lets you call
// ANY registered bridge RPC by method name with a JSON param payload.
// Use TurdCOM Terminal for the 228 SCUM admin verbs; use this for
// direct engine-level capabilities (teleportPlayer, dumpClasses,
// describeWidget, spawnVehicle, listClassInstances, etc.).
//
// The method picker auto-populates from the bridge's listHandlers RPC,
// so any new handler we add shows up automatically.

interface ListHandlersResponse {
  ok?: boolean;
  count?: number;
  handlers?: string[];
}

interface HistoryEntry {
  id: string;
  timestamp: Date;
  method: string;
  paramsJson: string;
  resultJson: string;
  isError: boolean;
  durationMs: number;
}

interface StoredEntry {
  timestamp: number;
  method: string;
  paramsJson: string;
  resultJson: string;
  isError: boolean;
  durationMs: number;
}

const STORAGE_KEY = 'turdmod.engineConsole.v1';
const MAX_HISTORY = 100;

function loadHistory(): HistoryEntry[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const stored = JSON.parse(raw) as StoredEntry[];
    return stored.slice(-MAX_HISTORY).map((s, i) => ({
      id: `${s.timestamp}-${i}`,
      timestamp: new Date(s.timestamp),
      method: s.method,
      paramsJson: s.paramsJson,
      resultJson: s.resultJson,
      isError: s.isError,
      durationMs: s.durationMs,
    }));
  } catch {
    return [];
  }
}

function saveHistory(entries: HistoryEntry[]) {
  try {
    const toStore: StoredEntry[] = entries.slice(-MAX_HISTORY).map((e) => ({
      timestamp: e.timestamp.getTime(),
      method: e.method,
      paramsJson: e.paramsJson,
      resultJson: e.resultJson,
      isError: e.isError,
      durationMs: e.durationMs,
    }));
    localStorage.setItem(STORAGE_KEY, JSON.stringify(toStore));
  } catch {
    // ignore — full storage shouldn't break the page
  }
}

// Fallback list used when listHandlers RPC isn't available (e.g. older
// bridge DLL still deployed). Keep roughly in sync with regs[] in the
// bridge but allow listHandlers to override when present.
const FALLBACK_HANDLERS = [
  'ping',
  'broadcastChat',
  'teleportPlayer',
  'getOnlinePlayers',
  'spawnVehicle',
  'dumpUFunctions',
  'findFunctions',
  'dumpClasses',
  'runAdminCommand',
  'runTestAdminCommand',
  'sendChat',
  'dumpWidgets',
  'describeWidget',
  'describeFunction',
  'dumpAllClasses',
  'dumpAllEnums',
  'dumpAllStructs',
  'broadcastRaidBanner',
  'setFamePoints',
  'setCurrencyBalance',
  'probeQuestHandlers',
  'setEconomy',
  'listClassInstances',
  'dumpAdminCommands',
  'sendChatLineToPlayer',
  'listHandlers',
];

export function EngineConsolePage() {
  const [history, setHistory] = useState<HistoryEntry[]>(loadHistory);
  const [selectedMethod, setSelectedMethod] = useState<string>('ping');
  const [methodFilter, setMethodFilter] = useState<string>('');
  const [paramsJson, setParamsJson] = useState<string>('{}');
  const [paramsError, setParamsError] = useState<string | null>(null);

  const handlersQuery = useQuery<ListHandlersResponse, Error>({
    queryKey: ['engine', 'listHandlers'],
    queryFn: () => engineRpc<ListHandlersResponse>('listHandlers', {}),
    retry: false,
    staleTime: 5 * 60_000,
  });

  // Live handlers from the bridge, with fallback to the hardcoded list
  // when the RPC fails (older bridge DLL).
  const handlers = useMemo(() => {
    const live = handlersQuery.data?.handlers;
    if (live && live.length > 0) return [...live].sort();
    return [...FALLBACK_HANDLERS].sort();
  }, [handlersQuery.data]);

  const filteredHandlers = useMemo(() => {
    if (!methodFilter.trim()) return handlers;
    const q = methodFilter.toLowerCase();
    return handlers.filter((m) => m.toLowerCase().includes(q));
  }, [handlers, methodFilter]);

  const mutation = useMutation<
    { result: unknown; durationMs: number },
    Error,
    { method: string; params: Record<string, unknown> }
  >({
    mutationFn: async ({ method, params }) => {
      const t0 = performance.now();
      const result = await engineRpc<unknown>(method, params);
      const durationMs = performance.now() - t0;
      return { result, durationMs };
    },
  });

  const validateParams = (text: string): Record<string, unknown> | null => {
    const trimmed = text.trim();
    if (!trimmed) {
      setParamsError(null);
      return {};
    }
    try {
      const parsed = JSON.parse(trimmed) as unknown;
      if (
        typeof parsed !== 'object' ||
        parsed === null ||
        Array.isArray(parsed)
      ) {
        setParamsError('params must be a JSON object (not array/primitive)');
        return null;
      }
      setParamsError(null);
      return parsed as Record<string, unknown>;
    } catch (err) {
      setParamsError((err as Error).message);
      return null;
    }
  };

  const handleParamsChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
    setParamsJson(e.target.value);
    validateParams(e.target.value);
  };

  const send = async () => {
    if (!selectedMethod) return;
    const params = validateParams(paramsJson);
    if (params === null) return; // params didn't parse

    const entryId = `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
    try {
      const { result, durationMs } = await mutation.mutateAsync({
        method: selectedMethod,
        params,
      });
      const newEntry: HistoryEntry = {
        id: entryId,
        timestamp: new Date(),
        method: selectedMethod,
        paramsJson: paramsJson.trim() || '{}',
        resultJson: JSON.stringify(result, null, 2),
        isError: false,
        durationMs,
      };
      const updated = [...history, newEntry].slice(-MAX_HISTORY);
      setHistory(updated);
      saveHistory(updated);
    } catch (err) {
      const newEntry: HistoryEntry = {
        id: entryId,
        timestamp: new Date(),
        method: selectedMethod,
        paramsJson: paramsJson.trim() || '{}',
        resultJson: String((err as Error)?.message ?? err),
        isError: true,
        durationMs: 0,
      };
      const updated = [...history, newEntry].slice(-MAX_HISTORY);
      setHistory(updated);
      saveHistory(updated);
    }
  };

  const clearHistory = () => {
    if (!window.confirm('Clear Engine Console history?')) return;
    setHistory([]);
    saveHistory([]);
  };

  const replayFromHistory = (entry: HistoryEntry) => {
    setSelectedMethod(entry.method);
    setParamsJson(entry.paramsJson);
    setParamsError(null);
  };

  // Auto-scroll history on new entries.
  const historyEndRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    historyEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [history]);

  const isLive = !!handlersQuery.data?.handlers?.length;

  return (
    <div className="flex h-full flex-col gap-3">
      <header>
        <p className="font-display text-xs tracking-[0.3em] text-turd-mustard">
          TurdMOD
        </p>
        <h1 className="mt-1 font-display text-3xl text-turd-cream">
          Engine Console
        </h1>
        <p className="mt-1 text-xs text-turd-cream-dim">
          Raw bridge RPC console. Call any registered handler with JSON
          params — engine-level access, bypasses SCUM&apos;s admin parser.
          Use TurdCOM Terminal for `#`-prefixed admin verbs.
        </p>
      </header>

      <div className="grid min-h-0 flex-1 grid-cols-[260px_1fr] gap-3 overflow-hidden">
        {/* Left: method picker */}
        <aside className="flex min-h-0 flex-col overflow-hidden glass rounded-xl">
          <div className="border-b border-turd-bronze/30 p-3">
            <div className="flex items-center justify-between">
              <h2 className="font-display text-xs uppercase tracking-widest text-turd-mustard">
                Methods · {handlers.length}
              </h2>
              <span
                className={`text-[9px] uppercase tracking-wider ${
                  isLive ? 'text-green-400' : 'text-turd-cream-dim/60'
                }`}
                title={
                  isLive
                    ? 'Methods came from listHandlers RPC'
                    : 'Bridge listHandlers unavailable — showing built-in fallback list'
                }
              >
                {isLive ? 'live' : 'fallback'}
              </span>
            </div>
            <input
              type="text"
              value={methodFilter}
              onChange={(e) => setMethodFilter(e.target.value)}
              placeholder="Filter…"
              className="mt-2 w-full rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-2 py-1 font-mono text-[11px] text-turd-cream placeholder:text-turd-cream-dim/40 focus:border-turd-mustard-bright focus:outline-none"
            />
          </div>
          <div className="flex-1 overflow-auto p-2">
            {filteredHandlers.map((m) => (
              <button
                key={m}
                type="button"
                onClick={() => setSelectedMethod(m)}
                className={`block w-full rounded px-2 py-1 text-left font-mono text-[11px] transition-colors ${
                  selectedMethod === m
                    ? 'bg-turd-mustard/20 text-turd-mustard-bright'
                    : 'text-turd-cream hover:bg-turd-bg-soft'
                }`}
              >
                {m}
              </button>
            ))}
            {filteredHandlers.length === 0 && (
              <p className="px-2 py-3 text-[11px] text-turd-cream-dim">
                No methods match &quot;{methodFilter}&quot;.
              </p>
            )}
          </div>
        </aside>

        {/* Right: params + result + history */}
        <main className="flex min-h-0 flex-col gap-3 overflow-hidden">
          {/* Request panel */}
          <section className="glass rounded-xl p-4">
            <div className="mb-3 flex items-baseline justify-between gap-2">
              <div className="flex items-baseline gap-2">
                <code className="font-mono text-sm text-turd-mustard-bright">
                  {selectedMethod}
                </code>
                <span className="text-[10px] text-turd-cream-dim/70">
                  JSON params below
                </span>
              </div>
              <button
                type="button"
                onClick={() => void send()}
                disabled={mutation.isPending || paramsError !== null}
                className="rounded border border-turd-mustard/60 bg-turd-mustard/20 px-4 py-1 font-display text-xs uppercase tracking-wider text-turd-mustard-bright transition-colors hover:bg-turd-mustard/30 disabled:cursor-not-allowed disabled:opacity-30"
              >
                {mutation.isPending ? 'Sending…' : 'Send'}
              </button>
            </div>
            <textarea
              value={paramsJson}
              onChange={handleParamsChange}
              spellCheck={false}
              className={`h-24 w-full resize-y rounded border bg-turd-bg-deep/60 px-2 py-1.5 font-mono text-[11px] text-turd-cream outline-none ${
                paramsError
                  ? 'border-red-500/60 focus:border-red-400'
                  : 'border-turd-bronze/40 focus:border-turd-mustard'
              }`}
              placeholder='{"name":"SampleAdmin","x":428844,"y":586315,"z":200}'
            />
            {paramsError && (
              <p className="mt-1 font-mono text-[10px] text-red-400">
                JSON parse error: {paramsError}
              </p>
            )}
          </section>

          {/* History */}
          <section className="flex min-h-0 flex-1 flex-col overflow-hidden glass rounded-xl">
            <div className="flex items-center justify-between border-b border-turd-bronze/30 px-4 py-2">
              <h2 className="font-display text-sm uppercase tracking-widest text-turd-mustard">
                History · {history.length}
              </h2>
              <button
                type="button"
                onClick={clearHistory}
                disabled={history.length === 0}
                className="text-[11px] text-turd-cream-dim transition-colors hover:text-turd-cream disabled:opacity-30"
              >
                Clear
              </button>
            </div>
            {history.length === 0 ? (
              <div className="flex flex-1 items-center justify-center p-6">
                <p className="text-xs text-turd-cream-dim">
                  No calls yet. Pick a method and Send.
                </p>
              </div>
            ) : (
              <div className="flex-1 overflow-auto p-3">
                <div className="space-y-2">
                  {history.map((entry) => (
                    <details
                      key={entry.id}
                      className="rounded border border-turd-bronze/20 bg-turd-bg-deep/60"
                    >
                      <summary className="flex cursor-pointer items-baseline justify-between gap-2 px-2 py-1.5 hover:bg-turd-bg-soft/30">
                        <div className="flex items-baseline gap-2 overflow-hidden">
                          <span className="font-mono text-[10px] text-turd-cream-dim/60">
                            {entry.timestamp.toLocaleTimeString('en-US', {
                              hour12: false,
                              hour: '2-digit',
                              minute: '2-digit',
                              second: '2-digit',
                            })}
                          </span>
                          <code
                            className={`truncate font-mono text-xs ${
                              entry.isError
                                ? 'text-red-400'
                                : 'text-turd-mustard-bright'
                            }`}
                          >
                            {entry.method}
                          </code>
                          {!entry.isError && entry.durationMs > 0 && (
                            <span className="text-[10px] text-turd-cream-dim/60">
                              {entry.durationMs.toFixed(0)}ms
                            </span>
                          )}
                        </div>
                        <button
                          type="button"
                          onClick={(e) => {
                            e.preventDefault();
                            replayFromHistory(entry);
                          }}
                          className="shrink-0 rounded border border-turd-bronze/30 px-1.5 py-0.5 text-[10px] text-turd-cream-dim hover:border-turd-bronze hover:text-turd-cream"
                          title="Load into editor"
                        >
                          load
                        </button>
                      </summary>
                      <div className="space-y-2 border-t border-turd-bronze/20 p-2">
                        <div>
                          <p className="text-[9px] uppercase tracking-wider text-turd-cream-dim/60">
                            Params
                          </p>
                          <pre className="mt-0.5 overflow-x-auto rounded bg-turd-bg-deep/80 p-1.5 font-mono text-[10px] text-turd-cream">
                            {entry.paramsJson}
                          </pre>
                        </div>
                        <div>
                          <p className="text-[9px] uppercase tracking-wider text-turd-cream-dim/60">
                            {entry.isError ? 'Error' : 'Result'}
                          </p>
                          <pre
                            className={`mt-0.5 max-h-96 overflow-auto rounded bg-turd-bg-deep/80 p-1.5 font-mono text-[10px] ${
                              entry.isError ? 'text-red-300' : 'text-turd-cream'
                            }`}
                          >
                            {entry.resultJson}
                          </pre>
                        </div>
                      </div>
                    </details>
                  ))}
                  <div ref={historyEndRef} />
                </div>
              </div>
            )}
          </section>
        </main>
      </div>
    </div>
  );
}
