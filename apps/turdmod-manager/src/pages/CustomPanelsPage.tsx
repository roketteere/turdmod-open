import { useMemo, useState } from 'react';
import { useEnqueuePanel, useLoaderStatus } from '../hooks/useLoaderStatus';

// Custom Panels — composer for in-game ImGui HUD overlays drawn by the
// turdmod-loader (apps/turdmod-loader/src/hooks.rs). The panel JSON is
// passed straight through `enqueuePanel` → loader pipe → render queue.
//
// V1 ships two surfaces side-by-side:
//   1. Quick-send form for the common case (title + body + duration).
//   2. Raw JSON editor for full control. Presets seed the editor with
//      working payload shapes lifted from hooks.rs.
//
// Local state lives in localStorage so reloads don't wipe in-progress
// work. Recent sends accumulate a small history so the admin can replay.

const STORAGE_KEY = 'turdmod.customPanels.v1';
const HISTORY_KEY = 'turdmod.customPanels.history.v1';
const MAX_HISTORY = 12;

interface StoredState {
  title: string;
  body: string;
  durationSecs: number;
  rawJson: string;
  mode: 'quick' | 'raw';
}

interface HistoryEntry {
  tsMs: number;
  payload: Record<string, unknown>;
  // Pre-rendered single-line preview for the history list.
  preview: string;
  outcome: 'ok' | 'error';
  errorText?: string;
}

function loadState(): StoredState {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return defaults();
    const parsed = JSON.parse(raw) as Partial<StoredState>;
    return {
      title: parsed.title ?? 'Welcome',
      body: parsed.body ?? '',
      durationSecs: parsed.durationSecs ?? 5,
      rawJson: parsed.rawJson ?? '',
      mode: parsed.mode === 'raw' ? 'raw' : 'quick',
    };
  } catch {
    return defaults();
  }
}

function defaults(): StoredState {
  return {
    title: 'Welcome',
    body: 'Welcome to the server!',
    durationSecs: 5,
    rawJson: '',
    mode: 'quick',
  };
}

function saveState(state: StoredState) {
  try { localStorage.setItem(STORAGE_KEY, JSON.stringify(state)); } catch { /* ignore */ }
}

function loadHistory(): HistoryEntry[] {
  try {
    const raw = localStorage.getItem(HISTORY_KEY);
    if (!raw) return [];
    return JSON.parse(raw) as HistoryEntry[];
  } catch { return []; }
}

function saveHistory(h: HistoryEntry[]) {
  try { localStorage.setItem(HISTORY_KEY, JSON.stringify(h.slice(0, MAX_HISTORY))); } catch { /* ignore */ }
}

// Starter payloads cribbed from hooks.rs::enqueue_loading_splash and
// enqueue_default_welcome_panel — these are the only shapes the render
// loop is guaranteed to recognize today.
const PRESETS: Record<string, Record<string, unknown>> = {
  'Loading splash': {
    kind: 'loading_splash',
    server_name: 'TurdMOD',
    tagline: 'Loading modding engine...',
    auto_close_after_secs: 3.0,
    actions: [],
    sections: [],
  },
  'Welcome panel': {
    server_name: 'TurdMOD',
    tagline: 'Welcome to the server.',
    greeting: 'Have fun out there. Press X (top-right) or Dismiss to close.',
    sections: [
      {
        kind: 'rules',
        title: 'House rules',
        items: [
          'No griefing within 200m of spawn',
          'Squad up in #general before raiding',
          'Report bugs to the admin via Discord',
        ],
      },
    ],
    actions: [{ kind: 'dismiss', label: 'Dismiss' }],
  },
  'Quick announce': {
    server_name: 'Server message',
    tagline: 'Restart in 5 minutes — save your loot.',
    auto_close_after_secs: 8.0,
    sections: [],
    actions: [],
  },
};

export function CustomPanelsPage() {
  const initial = useMemo(loadState, []);
  const [title, setTitle] = useState(initial.title);
  const [body, setBody] = useState(initial.body);
  const [durationSecs, setDurationSecs] = useState(initial.durationSecs);
  const [rawJson, setRawJson] = useState(initial.rawJson);
  const [mode, setMode] = useState<'quick' | 'raw'>(initial.mode);
  const [history, setHistory] = useState<HistoryEntry[]>(loadHistory);
  const [parseError, setParseError] = useState<string | null>(null);

  const status = useLoaderStatus();
  const enqueue = useEnqueuePanel();

  const persist = (next: Partial<StoredState>) => {
    const merged: StoredState = {
      title, body, durationSecs, rawJson, mode,
      ...next,
    };
    saveState(merged);
  };

  // Build the JSON payload from current state. Quick mode produces a
  // welcome-panel shape with a single rules section if body is non-empty.
  const buildQuickPayload = (): Record<string, unknown> => ({
    server_name: title || 'Server message',
    tagline: body,
    ...(durationSecs > 0 ? { auto_close_after_secs: durationSecs } : {}),
    sections: [],
    actions: durationSecs > 0 ? [] : [{ kind: 'dismiss', label: 'Dismiss' }],
  });

  const send = () => {
    setParseError(null);
    let payload: Record<string, unknown>;
    if (mode === 'quick') {
      payload = buildQuickPayload();
    } else {
      try {
        const parsed = JSON.parse(rawJson);
        if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
          setParseError('Payload must be a JSON object.');
          return;
        }
        payload = parsed as Record<string, unknown>;
      } catch (e) {
        setParseError((e as Error).message);
        return;
      }
    }

    enqueue.mutate(payload, {
      onSuccess: () => {
        const entry: HistoryEntry = {
          tsMs: Date.now(),
          payload,
          preview: previewLine(payload),
          outcome: 'ok',
        };
        const next = [entry, ...history].slice(0, MAX_HISTORY);
        setHistory(next);
        saveHistory(next);
      },
      onError: (err) => {
        const entry: HistoryEntry = {
          tsMs: Date.now(),
          payload,
          preview: previewLine(payload),
          outcome: 'error',
          errorText: err.message,
        };
        const next = [entry, ...history].slice(0, MAX_HISTORY);
        setHistory(next);
        saveHistory(next);
      },
    });
  };

  const applyPreset = (name: string) => {
    const payload = PRESETS[name];
    if (!payload) return;
    const json = JSON.stringify(payload, null, 2);
    setRawJson(json);
    setMode('raw');
    persist({ rawJson: json, mode: 'raw' });
  };

  const replayFromHistory = (entry: HistoryEntry) => {
    const json = JSON.stringify(entry.payload, null, 2);
    setRawJson(json);
    setMode('raw');
    persist({ rawJson: json, mode: 'raw' });
  };

  return (
    <div className="flex h-full flex-col gap-3">
      <header className="flex items-start justify-between gap-4">
        <div>
          <p className="font-display text-xs tracking-[0.3em] text-turd-mustard">
            TurdMOD
          </p>
          <h1 className="mt-1 font-display text-3xl text-turd-cream">
            Custom Panels
          </h1>
          <p className="mt-1 text-xs text-turd-cream-dim">
            Compose an in-game HUD panel and push it into the SCUM client
            via the turdmod-loader's ImGui render queue.
          </p>
        </div>
        <LoaderStatusBadge
          loading={status.isLoading}
          ok={status.isSuccess}
          version={status.data?.version}
          pid={status.data?.pid}
          error={status.error?.message}
        />
      </header>

      <div className="grid min-h-0 flex-1 grid-cols-[1fr_320px] gap-3 overflow-hidden">
        {/* Left: composer */}
        <section className="flex min-h-0 flex-col overflow-hidden glass rounded-xl">
          <div className="flex items-center justify-between border-b border-turd-bronze/30 px-4 py-2">
            <div className="flex items-center gap-2">
              <ModeTab active={mode === 'quick'} onClick={() => { setMode('quick'); persist({ mode: 'quick' }); }} label="Quick" />
              <ModeTab active={mode === 'raw'} onClick={() => { setMode('raw'); persist({ mode: 'raw' }); }} label="Raw JSON" />
            </div>
            <div className="flex items-center gap-2">
              <span className="text-[10px] text-turd-cream-dim">Preset:</span>
              {Object.keys(PRESETS).map((name) => (
                <button
                  key={name}
                  type="button"
                  onClick={() => applyPreset(name)}
                  className="rounded border border-turd-bronze/30 px-2 py-0.5 text-[10px] text-turd-cream-dim hover:bg-turd-bg-soft/40 hover:text-turd-cream"
                >
                  {name}
                </button>
              ))}
            </div>
          </div>

          <div className="flex-1 overflow-auto p-4">
            {mode === 'quick' ? (
              <div className="space-y-3">
                <Field label="Title (server_name)">
                  <input
                    type="text"
                    value={title}
                    onChange={(e) => { setTitle(e.target.value); persist({ title: e.target.value }); }}
                    className="w-full rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-2 py-1 font-mono text-xs text-turd-cream focus:border-turd-mustard-bright focus:outline-none"
                  />
                </Field>
                <Field label="Body (tagline)">
                  <textarea
                    value={body}
                    onChange={(e) => { setBody(e.target.value); persist({ body: e.target.value }); }}
                    rows={4}
                    className="w-full resize-y rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-2 py-1 font-mono text-xs text-turd-cream focus:border-turd-mustard-bright focus:outline-none"
                  />
                </Field>
                <Field label={`Auto-close after (seconds; 0 = persistent with Dismiss button)`}>
                  <input
                    type="number"
                    min={0}
                    step={0.5}
                    value={durationSecs}
                    onChange={(e) => {
                      const v = Math.max(0, Number(e.target.value) || 0);
                      setDurationSecs(v);
                      persist({ durationSecs: v });
                    }}
                    className="w-32 rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-2 py-1 font-mono text-xs text-turd-cream focus:border-turd-mustard-bright focus:outline-none"
                  />
                </Field>
                <details className="rounded border border-turd-bronze/20 bg-turd-bg-deep/30 px-3 py-2 text-[10px] text-turd-cream-dim">
                  <summary className="cursor-pointer text-turd-cream-dim/80">Preview JSON</summary>
                  <pre className="mt-2 overflow-auto font-mono text-[10px] text-turd-cream-dim">
                    {JSON.stringify(buildQuickPayload(), null, 2)}
                  </pre>
                </details>
              </div>
            ) : (
              <div className="flex h-full flex-col gap-2">
                <textarea
                  value={rawJson}
                  onChange={(e) => { setRawJson(e.target.value); persist({ rawJson: e.target.value }); setParseError(null); }}
                  spellCheck={false}
                  placeholder='{ "server_name": "TurdMOD", "tagline": "Hello!", "sections": [], "actions": [{ "kind": "dismiss", "label": "Dismiss" }] }'
                  className="min-h-[260px] flex-1 resize-none rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-3 py-2 font-mono text-[11px] text-turd-cream focus:border-turd-mustard-bright focus:outline-none"
                />
                {parseError && (
                  <p className="font-mono text-[10px] text-red-300">{parseError}</p>
                )}
              </div>
            )}
          </div>

          <div className="flex items-center justify-between border-t border-turd-bronze/30 px-4 py-2">
            <p className="text-[10px] text-turd-cream-dim">
              {status.isSuccess
                ? `Loader online · v${status.data.version} · pid ${status.data.pid}`
                : status.error
                  ? <span className="text-red-300">Loader offline — {status.error.message}</span>
                  : 'Checking loader…'}
            </p>
            <div className="flex items-center gap-2">
              {enqueue.isPending && (
                <span className="text-[10px] text-turd-cream-dim">Sending…</span>
              )}
              {enqueue.isError && !enqueue.isPending && (
                <span className="text-[10px] text-red-300">
                  {enqueue.error.message}
                </span>
              )}
              {enqueue.isSuccess && !enqueue.isPending && (
                <span className="text-[10px] text-turd-mustard-bright">Queued.</span>
              )}
              <button
                type="button"
                onClick={send}
                disabled={enqueue.isPending || !status.isSuccess}
                className="rounded border border-turd-mustard/40 bg-turd-mustard/15 px-3 py-1 text-xs font-medium text-turd-mustard-bright hover:bg-turd-mustard/25 disabled:cursor-not-allowed disabled:opacity-40"
              >
                Send to game
              </button>
            </div>
          </div>
        </section>

        {/* Right: history */}
        <section className="flex min-h-0 flex-col overflow-hidden glass rounded-xl">
          <div className="flex items-center justify-between border-b border-turd-bronze/30 px-4 py-2">
            <h2 className="font-display text-sm uppercase tracking-widest text-turd-mustard">
              Recent sends
            </h2>
            {history.length > 0 && (
              <button
                type="button"
                onClick={() => { setHistory([]); saveHistory([]); }}
                className="text-[10px] text-turd-cream-dim hover:text-turd-cream"
              >
                Clear
              </button>
            )}
          </div>
          <div className="flex-1 overflow-auto p-2">
            {history.length === 0 ? (
              <p className="px-2 py-1 text-xs text-turd-cream-dim">
                Nothing sent yet. Press <strong>Send to game</strong> after picking a preset.
              </p>
            ) : (
              <ul className="space-y-1">
                {history.map((entry) => (
                  <li key={entry.tsMs}>
                    <button
                      type="button"
                      onClick={() => replayFromHistory(entry)}
                      className="w-full rounded border border-transparent px-2 py-1 text-left hover:border-turd-bronze/30 hover:bg-turd-bg-soft/40"
                      title="Click to replay (loads into Raw JSON editor)"
                    >
                      <div className="flex items-baseline gap-2">
                        <span className={`text-[9px] ${entry.outcome === 'ok' ? 'text-turd-mustard-bright' : 'text-red-300'}`}>
                          {entry.outcome === 'ok' ? '●' : '✕'}
                        </span>
                        <span className="font-mono text-[10px] text-turd-cream-dim">
                          {new Date(entry.tsMs).toLocaleTimeString()}
                        </span>
                        <span className="truncate text-[11px] text-turd-cream">
                          {entry.preview}
                        </span>
                      </div>
                      {entry.errorText && (
                        <p className="mt-0.5 truncate pl-4 font-mono text-[9px] text-red-300/80">
                          {entry.errorText}
                        </p>
                      )}
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </section>
      </div>
    </div>
  );
}

function previewLine(payload: Record<string, unknown>): string {
  const title = (payload.server_name as string) || (payload.title as string) || '(no title)';
  const tagline = (payload.tagline as string) || (payload.body as string) || '';
  return tagline ? `${title} — ${tagline}` : title;
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="block">
      <span className="block text-[10px] uppercase tracking-wider text-turd-cream-dim">
        {label}
      </span>
      <div className="mt-1">{children}</div>
    </label>
  );
}

function ModeTab({ active, onClick, label }: { active: boolean; onClick: () => void; label: string }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={[
        'rounded px-2 py-0.5 text-[11px]',
        active
          ? 'bg-turd-bg-soft text-turd-mustard-bright'
          : 'text-turd-cream-dim hover:bg-turd-bg-soft/40 hover:text-turd-cream',
      ].join(' ')}
    >
      {label}
    </button>
  );
}

function LoaderStatusBadge({
  loading, ok, version, pid, error,
}: { loading: boolean; ok: boolean; version?: string; pid?: number; error?: string }) {
  let color = 'text-turd-cream-dim';
  let dot = 'bg-turd-cream-dim/40';
  let label = 'Checking…';
  if (ok) {
    color = 'text-turd-mustard-bright';
    dot = 'bg-turd-mustard-bright';
    label = `Loader online · v${version} · pid ${pid}`;
  } else if (!loading && error) {
    color = 'text-red-300';
    dot = 'bg-red-400';
    label = 'Loader offline';
  }
  return (
    <div
      className={`flex items-center gap-2 rounded border border-turd-bronze/30 bg-turd-bg-deep/40 px-3 py-1 text-[10px] ${color}`}
      title={error ?? ''}
    >
      <span className={`inline-block h-1.5 w-1.5 rounded-full ${dot}`} />
      <span>{label}</span>
    </div>
  );
}
