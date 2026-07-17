import { useMemo, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useQuery } from '@tanstack/react-query';

// Mirrors src-tauri/src/ollama_pool.rs response shapes.
type EndpointHealth = {
  name: string;
  url: string;
  tier: string;
  host: string;
  online: boolean;
  latencyMs: number;
  models: string[];
  modelCount: number;
  version: string | null;
  reason: string | null;
  costPerHr: number;
  tags: string[];
};

type HealthReport = {
  checkedAtMs: number;
  endpoints: EndpointHealth[];
};

type DispatchResult = {
  endpoint: string;
  endpointUrl: string;
  model: string;
  response: string | null;
  evalCount: number | null;
  evalDurationMs: number | null;
  tokensPerSec: number | null;
  totalDurationMs: number;
  promptEvalCount: number | null;
  doneReason: string | null;
  error: string | null;
};

type HistoryEntry = {
  ts: number;
  endpoint: string;
  model: string;
  promptPreview: string;
  result: DispatchResult;
};

const REFRESH_INTERVAL_MS = 5_000;
const HISTORY_LIMIT = 10;

function tierBadgeColor(tier: string): string {
  switch (tier) {
    case 'local':
      return 'border-turd-cream/40 text-turd-cream-dim';
    case 'lan':
      return 'border-turd-green/50 text-turd-green';
    case 'wifi':
      return 'border-turd-mustard/60 text-turd-mustard';
    case 'cloud':
      return 'border-turd-mustard/60 text-turd-mustard';
    default:
      return 'border-turd-bronze/40 text-turd-cream-dim';
  }
}

// ---------------------------------------------------------------------------
// Tier icons — pure inline SVG, no asset, no library. stroke="currentColor"
// so they tint with the parent's text color.
// ---------------------------------------------------------------------------

function IconLocal({ className = 'h-3.5 w-3.5' }: { className?: string }) {
  // Desktop monitor with stand.
  return (
    <svg
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <rect x="1.5" y="2.5" width="13" height="8.5" rx="1" />
      <line x1="5" y1="14" x2="11" y2="14" />
      <line x1="8" y1="11" x2="8" y2="14" />
    </svg>
  );
}

function IconLan({ className = 'h-3.5 w-3.5' }: { className?: string }) {
  // Two boxes connected by a line — wired LAN.
  return (
    <svg
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <rect x="1" y="9.5" width="4.5" height="4.5" rx="0.5" />
      <rect x="10.5" y="9.5" width="4.5" height="4.5" rx="0.5" />
      <path d="M3.25 9.5 V5 H12.75 V9.5" />
      <line x1="8" y1="5" x2="8" y2="2" />
      <circle cx="8" cy="1.75" r="0.75" />
    </svg>
  );
}

function IconWifi({ className = 'h-3.5 w-3.5' }: { className?: string }) {
  // Three concentric wifi arcs.
  return (
    <svg
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <path d="M1.5 6 Q8 0.5 14.5 6" />
      <path d="M3.75 8.5 Q8 4.5 12.25 8.5" />
      <path d="M6 11 Q8 9 10 11" />
      <circle cx="8" cy="13.25" r="0.85" fill="currentColor" />
    </svg>
  );
}

function IconCloud({ className = 'h-3.5 w-3.5' }: { className?: string }) {
  // Stylized cloud shape.
  return (
    <svg
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <path d="M4.5 12 Q1.5 12 1.5 9.25 Q1.5 6.75 4 6.5 Q4.5 3.5 7.75 3.5 Q11 3.5 11.5 6.75 Q14.5 6.75 14.5 9.5 Q14.5 12 12 12 Z" />
    </svg>
  );
}

function IconOffline({ className = 'h-3.5 w-3.5' }: { className?: string }) {
  // Diagonal slash through a circle — "no signal".
  return (
    <svg
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <circle cx="8" cy="8" r="6" />
      <line x1="3.75" y1="3.75" x2="12.25" y2="12.25" />
    </svg>
  );
}

function IconRefresh({ className = 'h-3.5 w-3.5' }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <path d="M2 8 A6 6 0 0 1 13.5 5.5" />
      <polyline points="13.5,2 13.5,5.5 10,5.5" />
      <path d="M14 8 A6 6 0 0 1 2.5 10.5" />
      <polyline points="2.5,14 2.5,10.5 6,10.5" />
    </svg>
  );
}

function IconSend({ className = 'h-3.5 w-3.5' }: { className?: string }) {
  // Paper-plane.
  return (
    <svg
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <path d="M14.5 1.5 L1.5 7 L6 9 L8 14 Z" />
      <line x1="14.5" y1="1.5" x2="6" y2="9" />
    </svg>
  );
}

function IconBroadcast({ className = 'h-3.5 w-3.5' }: { className?: string }) {
  // Radio waves emanating from a center.
  return (
    <svg
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <circle cx="8" cy="8" r="1.5" fill="currentColor" stroke="none" />
      <path d="M5.5 5.5 A3.5 3.5 0 0 0 5.5 10.5" />
      <path d="M10.5 5.5 A3.5 3.5 0 0 1 10.5 10.5" />
      <path d="M3 3 A7 7 0 0 0 3 13" />
      <path d="M13 3 A7 7 0 0 1 13 13" />
    </svg>
  );
}

function IconTrash({ className = 'h-3.5 w-3.5' }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <polyline points="2,4 14,4" />
      <path d="M3.5 4 L4 13.5 A1 1 0 0 0 5 14.5 H11 A1 1 0 0 0 12 13.5 L12.5 4" />
      <path d="M6 4 V2.5 A0.5 0.5 0 0 1 6.5 2 H9.5 A0.5 0.5 0 0 1 10 2.5 V4" />
    </svg>
  );
}

function IconHistory({ className = 'h-3.5 w-3.5' }: { className?: string }) {
  // Clock face with curved arrow on top.
  return (
    <svg
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <circle cx="8" cy="9" r="5" />
      <polyline points="8,6 8,9 10,10.5" />
      <path d="M3.5 5.5 A4.5 4.5 0 0 1 8 3" />
      <polyline points="3.5,3 3.5,5.5 6,5.5" />
    </svg>
  );
}

function IconSignal({ className = 'h-3.5 w-3.5' }: { className?: string }) {
  // Ascending bars.
  return (
    <svg
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <line x1="2.5" y1="13" x2="2.5" y2="11" />
      <line x1="6" y1="13" x2="6" y2="9" />
      <line x1="9.5" y1="13" x2="9.5" y2="6" />
      <line x1="13" y1="13" x2="13" y2="3" />
    </svg>
  );
}

function IconBridge({ className = 'h-3.5 w-3.5' }: { className?: string }) {
  // Stylized network/bridge — two nodes joined by a curve.
  return (
    <svg
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <circle cx="3" cy="11" r="2" />
      <circle cx="13" cy="11" r="2" />
      <path d="M3 9 Q8 1 13 9" />
    </svg>
  );
}

function IconInbox({ className = 'h-3.5 w-3.5' }: { className?: string }) {
  // Empty inbox tray — used in the empty-history slot.
  return (
    <svg
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <rect x="2" y="3" width="12" height="10" rx="1" />
      <path d="M2 9 H5 L6 11 H10 L11 9 H14" />
    </svg>
  );
}

// Pulsing live dot — used to show background polling is alive without
// flickering buttons. Pure CSS animation, no JS state.
function LivePulse({ className = 'h-2 w-2' }: { className?: string }) {
  return (
    <span className="relative inline-flex">
      <span
        className={`absolute inline-flex animate-ping rounded-full bg-turd-green/60 ${className}`}
      />
      <span className={`relative inline-flex rounded-full bg-turd-green ${className}`} />
    </span>
  );
}

function TierIcon({
  tier,
  className,
}: {
  tier: string;
  className?: string;
}) {
  switch (tier) {
    case 'local':
      return <IconLocal className={className} />;
    case 'lan':
      return <IconLan className={className} />;
    case 'wifi':
      return <IconWifi className={className} />;
    case 'cloud':
      return <IconCloud className={className} />;
    default:
      return <IconLocal className={className} />;
  }
}

function truncate(s: string, n: number): string {
  if (s.length <= n) return s;
  return s.slice(0, n - 1) + '…';
}

function formatLatency(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
}

function EndpointCard({
  ep,
  inFlight,
  lastDirection,
}: {
  ep: EndpointHealth;
  inFlight: boolean;
  lastDirection: 'tx' | 'rx' | null;
}) {
  const isOnline = ep.online;
  const ledColor = isOnline ? 'bg-turd-green' : 'bg-turd-red';
  const borderColor = inFlight
    ? 'border-turd-mustard animate-pulse'
    : isOnline
    ? 'border-turd-green/30'
    : 'border-turd-red/40';

  return (
    <div
      className={`rounded border ${borderColor} bg-turd-bg-soft p-4 transition-all`}
    >
      <div className="flex items-start justify-between gap-2">
        <div className="flex items-center gap-2">
          <span className={`h-3 w-3 rounded-full ${ledColor}`} />
          <span className="font-display text-sm text-turd-cream">{ep.name}</span>
          <span
            className={`inline-flex items-center gap-1 rounded border px-2 py-0.5 text-[10px] uppercase tracking-wider ${tierBadgeColor(
              ep.tier,
            )}`}
          >
            {isOnline ? (
              <TierIcon tier={ep.tier} className="h-3 w-3" />
            ) : (
              <IconOffline className="h-3 w-3" />
            )}
            <span>{ep.tier}</span>
          </span>
        </div>
        <div className="flex items-center gap-2">
          <TierIcon
            tier={ep.tier}
            className={`h-5 w-5 ${
              isOnline ? 'text-turd-cream-dim/60' : 'text-turd-red/60'
            }`}
          />
          {inFlight && (
            <span className="font-display text-[10px] uppercase tracking-wider text-turd-mustard">
              → tx
            </span>
          )}
          {!inFlight && lastDirection === 'rx' && (
            <span className="font-display text-[10px] uppercase tracking-wider text-turd-green">
              ← rx
            </span>
          )}
        </div>
      </div>

      <div className="mt-2 grid grid-cols-2 gap-1 font-mono text-[11px] text-turd-cream-dim">
        <div>host:</div>
        <div className="text-turd-cream">{ep.host}</div>
        <div>url:</div>
        <div className="truncate text-turd-cream" title={ep.url}>
          {ep.url}
        </div>
        <div>latency:</div>
        <div className="text-turd-cream">{formatLatency(ep.latencyMs)}</div>
        <div>models:</div>
        <div className="text-turd-cream">
          {ep.modelCount > 0 ? ep.modelCount : '—'}
        </div>
        {ep.version && (
          <>
            <div>ollama:</div>
            <div className="text-turd-cream">v{ep.version}</div>
          </>
        )}
        {ep.costPerHr > 0 && (
          <>
            <div>$/hr:</div>
            <div className="text-turd-mustard">${ep.costPerHr.toFixed(2)}</div>
          </>
        )}
      </div>

      {ep.models.length > 0 && (
        <details className="mt-2">
          <summary className="cursor-pointer font-display text-[10px] uppercase tracking-wider text-turd-cream-dim hover:text-turd-cream">
            Models ({ep.models.length})
          </summary>
          <ul className="mt-1 space-y-0.5 font-mono text-[11px] text-turd-cream-dim">
            {ep.models.map((m) => (
              <li key={m}>{m}</li>
            ))}
          </ul>
        </details>
      )}

      {!isOnline && ep.reason && (
        <p className="mt-2 font-mono text-[10px] text-turd-red">{ep.reason}</p>
      )}

      {ep.tags.length > 0 && (
        <div className="mt-2 flex flex-wrap gap-1">
          {ep.tags.map((t) => (
            <span
              key={t}
              className="rounded border border-turd-bronze/40 px-1.5 py-0.5 text-[9px] uppercase tracking-wider text-turd-cream-dim"
            >
              {t}
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

export function OllamaBridgePage() {
  const healthQuery = useQuery<HealthReport>({
    queryKey: ['ollama-pool-health'],
    queryFn: () => invoke<HealthReport>('ollama_pool_health'),
    refetchInterval: REFRESH_INTERVAL_MS,
    refetchIntervalInBackground: false,
  });

  const filePathQuery = useQuery<string>({
    queryKey: ['ollama-pool-endpoints-file'],
    queryFn: () => invoke<string>('ollama_pool_endpoints_file_path'),
  });

  const endpoints = healthQuery.data?.endpoints ?? [];

  const [selectedEndpoint, setSelectedEndpoint] = useState<string>('');
  const [model, setModel] = useState<string>('qwen2.5-coder:7b');
  const [prompt, setPrompt] = useState<string>(
    'Write a one-line Rust function that doubles its argument.',
  );
  const [numPredict, setNumPredict] = useState<number>(256);
  const [temperature, setTemperature] = useState<number>(0.2);

  const [inFlight, setInFlight] = useState<string | null>(null);
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const lastDirectionRef = useRef<Record<string, 'tx' | 'rx' | null>>({});
  // Separate from healthQuery.isFetching so the auto-poll every 5s
  // doesn't flicker the Refresh button. Only flips when YOU click.
  const [manualRefreshing, setManualRefreshing] = useState(false);

  async function manualRefresh() {
    setManualRefreshing(true);
    try {
      await healthQuery.refetch();
    } finally {
      setManualRefreshing(false);
    }
  }

  // Default the selected endpoint to the first online one once health loads.
  useMemo(() => {
    if (!selectedEndpoint && endpoints.length > 0) {
      const firstOnline = endpoints.find((e) => e.online) ?? endpoints[0];
      setSelectedEndpoint(firstOnline.name);
    }
  }, [endpoints, selectedEndpoint]);

  const availableModels = useMemo(() => {
    const ep = endpoints.find((e) => e.name === selectedEndpoint);
    return ep?.models ?? [];
  }, [endpoints, selectedEndpoint]);

  async function sendDispatch() {
    if (!selectedEndpoint || !prompt.trim()) return;
    setInFlight(selectedEndpoint);
    lastDirectionRef.current[selectedEndpoint] = 'tx';

    try {
      const result = await invoke<DispatchResult>('ollama_pool_dispatch', {
        endpoint: selectedEndpoint,
        prompt,
        model,
        numPredict,
        temperature,
      });

      lastDirectionRef.current[result.endpoint] = 'rx';
      setHistory((h) =>
        [
          {
            ts: Date.now(),
            endpoint: result.endpoint,
            model: result.model,
            promptPreview: truncate(prompt, 80),
            result,
          },
          ...h,
        ].slice(0, HISTORY_LIMIT),
      );
    } catch (e) {
      const errMsg = e instanceof Error ? e.message : String(e);
      setHistory((h) =>
        [
          {
            ts: Date.now(),
            endpoint: selectedEndpoint,
            model,
            promptPreview: truncate(prompt, 80),
            result: {
              endpoint: selectedEndpoint,
              endpointUrl: '',
              model,
              response: null,
              evalCount: null,
              evalDurationMs: null,
              tokensPerSec: null,
              totalDurationMs: 0,
              promptEvalCount: null,
              doneReason: null,
              error: errMsg,
            },
          },
          ...h,
        ].slice(0, HISTORY_LIMIT),
      );
    } finally {
      setInFlight(null);
      // Clear rx indicator after a moment so the card returns to idle.
      setTimeout(() => {
        lastDirectionRef.current[selectedEndpoint] = null;
        setHistory((h) => [...h]);
      }, 3000);
    }
  }

  async function broadcastToAll() {
    const onlineEps = endpoints.filter((e) => e.online).map((e) => e.name);
    for (const name of onlineEps) {
      setInFlight(name);
      lastDirectionRef.current[name] = 'tx';
      try {
        const result = await invoke<DispatchResult>('ollama_pool_dispatch', {
          endpoint: name,
          prompt,
          model,
          numPredict,
          temperature,
        });
        lastDirectionRef.current[result.endpoint] = 'rx';
        setHistory((h) =>
          [
            {
              ts: Date.now(),
              endpoint: result.endpoint,
              model: result.model,
              promptPreview: truncate(prompt, 80),
              result,
            },
            ...h,
          ].slice(0, HISTORY_LIMIT),
        );
      } catch (e) {
        const errMsg = e instanceof Error ? e.message : String(e);
        setHistory((h) =>
          [
            {
              ts: Date.now(),
              endpoint: name,
              model,
              promptPreview: truncate(prompt, 80),
              result: {
                endpoint: name,
                endpointUrl: '',
                model,
                response: null,
                evalCount: null,
                evalDurationMs: null,
                tokensPerSec: null,
                totalDurationMs: 0,
                promptEvalCount: null,
                doneReason: null,
                error: errMsg,
              },
            },
            ...h,
          ].slice(0, HISTORY_LIMIT),
        );
      }
    }
    setInFlight(null);
  }

  const btnBase =
    'rounded border px-4 py-2 font-display text-xs uppercase tracking-wider transition-colors disabled:opacity-40';
  const btnPrimary = `${btnBase} border-turd-mustard bg-turd-bg-soft text-turd-mustard-bright hover:bg-turd-bronze/30`;
  const btnSecondary = `${btnBase} border-turd-bronze/60 bg-turd-bg-soft text-turd-cream-dim hover:border-turd-cream hover:text-turd-cream`;

  return (
    <div className="space-y-6">
      <div>
        <h1 className="flex items-center gap-2 font-display text-2xl text-turd-mustard-bright">
          <IconBridge className="h-6 w-6" />
          Ollama Bridge
        </h1>
        <p className="mt-1 text-sm text-turd-cream-dim">
          Live status across the Ollama mesh. Dispatch test prompts to any endpoint;
          watch traffic light up the cards in real time.
        </p>
      </div>

      {/* Endpoint cards */}
      <section>
        <div className="mb-2 flex items-center justify-between">
          <h2 className="flex items-center gap-2 font-display text-xs uppercase tracking-wider text-turd-cream-dim">
            <IconSignal className="h-4 w-4" />
            Endpoints ({endpoints.filter((e) => e.online).length}/{endpoints.length} online)
            <span className="ml-1 inline-flex items-center" title="Live — auto-refreshes every 5s">
              <LivePulse />
            </span>
          </h2>
          <button
            className={`${btnSecondary} inline-flex items-center gap-1.5`}
            onClick={manualRefresh}
            disabled={manualRefreshing}
          >
            <IconRefresh
              className={`h-3.5 w-3.5 ${manualRefreshing ? 'animate-spin' : ''}`}
            />
            {manualRefreshing ? 'Checking…' : 'Refresh'}
          </button>
        </div>
        {healthQuery.isLoading ? (
          <p className="text-sm text-turd-cream-dim">Loading endpoints…</p>
        ) : healthQuery.isError ? (
          <p className="text-sm text-turd-red">
            Failed to load: {String(healthQuery.error)}
          </p>
        ) : endpoints.length === 0 ? (
          <p className="text-sm text-turd-cream-dim">
            No endpoints configured. Edit{' '}
            <span className="font-mono text-turd-cream">
              {filePathQuery.data ?? 'ollama-endpoints.json'}
            </span>{' '}
            to add some.
          </p>
        ) : (
          <div className="grid grid-cols-1 gap-3 md:grid-cols-2 lg:grid-cols-3">
            {endpoints.map((ep) => (
              <EndpointCard
                key={ep.name}
                ep={ep}
                inFlight={inFlight === ep.name}
                lastDirection={lastDirectionRef.current[ep.name] ?? null}
              />
            ))}
          </div>
        )}
        {filePathQuery.data && (
          <p className="mt-2 font-mono text-[10px] text-turd-cream-dim/60">
            registry: {filePathQuery.data}
          </p>
        )}
      </section>

      {/* Dispatch test box */}
      <section className="rounded border border-turd-bronze/40 bg-turd-bg-soft p-4">
        <h2 className="flex items-center gap-2 font-display text-xs uppercase tracking-wider text-turd-cream-dim">
          <IconSend className="h-4 w-4" />
          Dispatch test
        </h2>
        <div className="mt-3 grid grid-cols-1 gap-3 md:grid-cols-[200px_1fr]">
          <div className="space-y-3">
            <div>
              <label className="block font-display text-[10px] uppercase tracking-wider text-turd-cream-dim">
                Endpoint
              </label>
              <select
                className="mt-1 w-full rounded border border-turd-bronze/40 bg-turd-bg-mid px-2 py-1.5 font-mono text-xs text-turd-cream"
                value={selectedEndpoint}
                onChange={(e) => setSelectedEndpoint(e.target.value)}
              >
                {endpoints.map((ep) => (
                  <option key={ep.name} value={ep.name} disabled={!ep.online}>
                    {ep.name} {ep.online ? '' : '(offline)'}
                  </option>
                ))}
              </select>
            </div>
            <div>
              <label className="block font-display text-[10px] uppercase tracking-wider text-turd-cream-dim">
                Model
              </label>
              {availableModels.length > 0 ? (
                <select
                  className="mt-1 w-full rounded border border-turd-bronze/40 bg-turd-bg-mid px-2 py-1.5 font-mono text-xs text-turd-cream"
                  value={model}
                  onChange={(e) => setModel(e.target.value)}
                >
                  {availableModels.map((m) => (
                    <option key={m} value={m}>
                      {m}
                    </option>
                  ))}
                </select>
              ) : (
                <input
                  className="mt-1 w-full rounded border border-turd-bronze/40 bg-turd-bg-mid px-2 py-1.5 font-mono text-xs text-turd-cream"
                  value={model}
                  onChange={(e) => setModel(e.target.value)}
                  placeholder="qwen2.5-coder:7b"
                />
              )}
            </div>
            <div className="grid grid-cols-2 gap-2">
              <div>
                <label className="block font-display text-[10px] uppercase tracking-wider text-turd-cream-dim">
                  Max tokens
                </label>
                <input
                  type="number"
                  className="mt-1 w-full rounded border border-turd-bronze/40 bg-turd-bg-mid px-2 py-1.5 font-mono text-xs text-turd-cream"
                  value={numPredict}
                  onChange={(e) => setNumPredict(Number(e.target.value) || 256)}
                />
              </div>
              <div>
                <label className="block font-display text-[10px] uppercase tracking-wider text-turd-cream-dim">
                  Temp
                </label>
                <input
                  type="number"
                  step={0.1}
                  min={0}
                  max={2}
                  className="mt-1 w-full rounded border border-turd-bronze/40 bg-turd-bg-mid px-2 py-1.5 font-mono text-xs text-turd-cream"
                  value={temperature}
                  onChange={(e) => setTemperature(Number(e.target.value) || 0)}
                />
              </div>
            </div>
          </div>
          <div className="space-y-3">
            <div>
              <label className="block font-display text-[10px] uppercase tracking-wider text-turd-cream-dim">
                Prompt
              </label>
              <textarea
                className="mt-1 h-32 w-full rounded border border-turd-bronze/40 bg-turd-bg-mid px-2 py-1.5 font-mono text-xs text-turd-cream"
                value={prompt}
                onChange={(e) => setPrompt(e.target.value)}
              />
            </div>
            <div className="flex flex-wrap items-center gap-2">
              <button
                className={`${btnPrimary} inline-flex items-center gap-1.5`}
                onClick={sendDispatch}
                disabled={inFlight !== null || !selectedEndpoint || !prompt.trim()}
              >
                <IconSend className={`h-3.5 w-3.5 ${inFlight === selectedEndpoint ? 'animate-pulse' : ''}`} />
                {inFlight === selectedEndpoint ? 'Sending…' : `Send → ${selectedEndpoint || '?'}`}
              </button>
              <button
                className={`${btnSecondary} inline-flex items-center gap-1.5`}
                onClick={broadcastToAll}
                disabled={inFlight !== null || !prompt.trim() || endpoints.filter((e) => e.online).length === 0}
              >
                <IconBroadcast className="h-3.5 w-3.5" />
                Send to all online
              </button>
              <button
                className={`${btnSecondary} inline-flex items-center gap-1.5`}
                onClick={() => setHistory([])}
                disabled={history.length === 0}
              >
                <IconTrash className="h-3.5 w-3.5" />
                Clear history
              </button>
            </div>
          </div>
        </div>
      </section>

      {/* History */}
      <section>
        <h2 className="mb-2 flex items-center gap-2 font-display text-xs uppercase tracking-wider text-turd-cream-dim">
          <IconHistory className="h-4 w-4" />
          Recent dispatches ({history.length}/{HISTORY_LIMIT})
        </h2>
        {history.length === 0 ? (
          <div className="flex items-center gap-3 rounded border border-dashed border-turd-bronze/30 bg-turd-bg-soft/40 p-4 text-sm text-turd-cream-dim">
            <IconInbox className="h-5 w-5 text-turd-cream-dim/60" />
            <span>No dispatches yet. Send a prompt above to see traffic.</span>
          </div>
        ) : (
          <ul className="space-y-2">
            {history.map((entry, idx) => {
              const r = entry.result;
              const hasError = r.error !== null;
              return (
                <li
                  key={`${entry.ts}-${idx}`}
                  className={`rounded border ${
                    hasError ? 'border-turd-red/40' : 'border-turd-bronze/40'
                  } bg-turd-bg-soft p-3`}
                >
                  <div className="flex flex-wrap items-center gap-3 font-mono text-[11px]">
                    <span className="text-turd-cream-dim">
                      {new Date(entry.ts).toLocaleTimeString()}
                    </span>
                    <span className="font-display text-turd-mustard">
                      {entry.endpoint}
                    </span>
                    <span className="text-turd-cream-dim">{entry.model}</span>
                    {!hasError && (
                      <>
                        <span className="text-turd-green">
                          {formatLatency(r.totalDurationMs)}
                        </span>
                        {r.tokensPerSec !== null && (
                          <span className="text-turd-cream">
                            {r.tokensPerSec.toFixed(1)} tok/s
                          </span>
                        )}
                        {r.evalCount !== null && (
                          <span className="text-turd-cream-dim">
                            {r.evalCount} tok out
                          </span>
                        )}
                        {r.promptEvalCount !== null && (
                          <span className="text-turd-cream-dim">
                            {r.promptEvalCount} tok in
                          </span>
                        )}
                      </>
                    )}
                  </div>
                  <p className="mt-1 font-mono text-[11px] text-turd-cream-dim">
                    &gt; {entry.promptPreview}
                  </p>
                  {hasError ? (
                    <p className="mt-1 font-mono text-[11px] text-turd-red">
                      ERROR: {r.error}
                    </p>
                  ) : (
                    <pre className="mt-1 max-h-40 overflow-auto whitespace-pre-wrap rounded bg-turd-bg-mid p-2 font-mono text-[11px] text-turd-cream">
                      {r.response}
                    </pre>
                  )}
                </li>
              );
            })}
          </ul>
        )}
      </section>
    </div>
  );
}
