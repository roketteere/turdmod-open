// AI Assistant settings page — toggle the Ollama-powered mode, pick
// the active model, browse / pull local models, see GPU fit info.
//
// Backend: apps/turdmod-manager/src-tauri/src/assist.rs.
// Wires used: assist_gpu_info, assist_list_models, assist_pull_model,
// assist_chat. Settings persisted to localStorage (v1) — see
// tauri-assist.ts for the storage shape.

import { useEffect, useMemo, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  type AssistSettings,
  type LocalModel,
  type ModelFit,
  assistChat,
  assistGpuInfo,
  assistListModels,
  assistPullModel,
  classifyModelFit,
  loadAssistSettings,
  onAssistProgress,
  saveAssistSettings,
} from '../lib/tauri-assist';

function formatMib(mib: number | null | undefined): string {
  if (mib == null) return '—';
  if (mib < 1024) return `${mib} MiB`;
  return `${(mib / 1024).toFixed(1)} GiB`;
}

function FitBadge({ fit }: { fit: ModelFit }) {
  const map = {
    fits: { label: '✓ fits', cls: 'text-emerald-400' },
    tight: { label: '~ tight', cls: 'text-turd-mustard' },
    over: { label: '✗ over', cls: 'text-red-400' },
    unknown: { label: '? unknown', cls: 'text-turd-cream-dim' },
  } as const;
  const m = map[fit];
  return (
    <span className={`font-mono text-xs ${m.cls}`}>{m.label}</span>
  );
}

export function AiAssistantPage() {
  const qc = useQueryClient();
  const [settings, setSettings] = useState<AssistSettings>(() =>
    loadAssistSettings(),
  );
  const [pullName, setPullName] = useState('');
  const [pullLog, setPullLog] = useState<string[]>([]);
  const [testPrompt, setTestPrompt] = useState(
    'In one sentence: what is your role here?',
  );
  const [testResponse, setTestResponse] = useState<string>('');

  useEffect(() => {
    saveAssistSettings(settings);
  }, [settings]);

  // Subscribe to pull-progress events for the lifetime of the page.
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    onAssistProgress((p) => {
      const pct =
        p.total > 0
          ? ` ${Math.round((p.completed / p.total) * 100)}%`
          : '';
      setPullLog((prev) =>
        prev.concat(`${p.status}${pct}`).slice(-40),
      );
    }).then((u) => {
      unlisten = u;
    });
    return () => {
      unlisten?.();
    };
  }, []);

  const gpu = useQuery({
    queryKey: ['assist', 'gpu'],
    queryFn: assistGpuInfo,
    staleTime: Infinity, // GPU doesn't hot-swap
  });

  const models = useQuery({
    queryKey: ['assist', 'models'],
    queryFn: assistListModels,
    refetchInterval: 15_000,
    staleTime: 10_000,
  });

  const pullMutation = useMutation({
    mutationFn: (name: string) => assistPullModel(name),
    onSettled: () => {
      qc.invalidateQueries({ queryKey: ['assist', 'models'] });
    },
  });

  const testMutation = useMutation({
    mutationFn: () => assistChat(settings.model, testPrompt),
    onSuccess: (resp) => setTestResponse(resp),
    onError: (e) => setTestResponse(`Error: ${String(e)}`),
  });

  const decoratedModels = useMemo(() => {
    const m: LocalModel[] = models.data ?? [];
    return m.map((entry) => ({
      ...entry,
      fit: classifyModelFit(entry.estimatedVramMib, gpu.data?.vramMib),
    }));
  }, [models.data, gpu.data?.vramMib]);

  return (
    <div className="space-y-6">
      <header>
        <p className="font-display text-xs uppercase tracking-[0.4em] text-turd-mustard">
          Tools · AI Assistant
        </p>
        <h1 className="mt-1 font-display text-3xl text-turd-cream md:text-4xl">
          Ollama-powered Assistant
        </h1>
        <p className="mt-2 max-w-2xl text-sm text-turd-cream-dim">
          Optional layer: a local LLM running in Ollama interprets dump
          diffs, explains phase logs, and answers questions about the
          forensic archive — so you can keep digging without paging in
          a Claude session for every "what changed?" question.
        </p>
      </header>

      <section className="glass rounded-xl p-5">
        <h2 className="font-display text-sm uppercase tracking-widest text-turd-mustard">
          Settings
        </h2>
        <div className="mt-3 space-y-4">
          <label className="flex items-center gap-3 text-sm">
            <input
              type="checkbox"
              checked={settings.enabled}
              onChange={(e) =>
                setSettings((s) => ({ ...s, enabled: e.target.checked }))
              }
            />
            <span className="text-turd-cream">
              Use Ollama for diff summaries + log explanations
            </span>
          </label>
          <div>
            <label
              htmlFor="model-select"
              className="block text-[10px] uppercase tracking-widest text-turd-mustard"
            >
              Active model
            </label>
            <select
              id="model-select"
              value={settings.model}
              onChange={(e) =>
                setSettings((s) => ({ ...s, model: e.target.value }))
              }
              className="mt-1 w-full max-w-md rounded border border-turd-bronze/40 bg-turd-bg-deep/60 px-3 py-1.5 text-sm text-turd-cream"
            >
              {decoratedModels.length === 0 && (
                <option value={settings.model}>{settings.model}</option>
              )}
              {decoratedModels.map((m) => (
                <option key={m.name} value={m.name}>
                  {m.name} · {formatMib(m.estimatedVramMib)} · {m.fit}
                </option>
              ))}
            </select>
            <p className="mt-1 text-xs text-turd-cream-dim">
              Endpoint: <code>http://127.0.0.1:11434</code>. Start
              Ollama before testing.
            </p>
          </div>
          <div>
            <label
              htmlFor="test-prompt"
              className="block text-[10px] uppercase tracking-widest text-turd-mustard"
            >
              Test prompt
            </label>
            <div className="mt-1 flex gap-2">
              <input
                id="test-prompt"
                value={testPrompt}
                onChange={(e) => setTestPrompt(e.target.value)}
                className="flex-1 rounded border border-turd-bronze/40 bg-turd-bg-deep/60 px-3 py-1.5 text-sm text-turd-cream"
              />
              <button
                onClick={() => testMutation.mutate()}
                disabled={testMutation.isPending || !settings.model}
                className="rounded border border-turd-mustard/40 bg-turd-mustard/20 px-3 py-1.5 text-sm font-medium text-turd-mustard-bright hover:bg-turd-mustard/30 disabled:opacity-40"
              >
                {testMutation.isPending ? 'Asking…' : 'Test'}
              </button>
            </div>
            {testResponse && (
              <pre className="mt-3 max-h-60 overflow-auto rounded border border-turd-bronze/30 bg-turd-bg-deep/60 p-3 text-xs text-turd-cream">
                {testResponse}
              </pre>
            )}
          </div>
        </div>
      </section>

      <section className="glass rounded-xl p-5">
        <h2 className="font-display text-sm uppercase tracking-widest text-turd-mustard">
          GPU
        </h2>
        <dl className="mt-3 grid grid-cols-[max-content_1fr] gap-x-6 gap-y-1.5 text-sm">
          <dt className="text-turd-cream-dim">Detected:</dt>
          <dd className="text-turd-cream">{gpu.data?.name ?? '—'}</dd>
          <dt className="text-turd-cream-dim">VRAM:</dt>
          <dd className="font-mono text-turd-cream">
            {formatMib(gpu.data?.vramMib)}
          </dd>
          <dt className="text-turd-cream-dim">Source:</dt>
          <dd className="text-turd-cream-dim">{gpu.data?.source ?? '—'}</dd>
        </dl>
        {gpu.data?.note && (
          <p className="mt-2 text-xs text-turd-cream-dim/80">
            {gpu.data.note}
          </p>
        )}
        <p className="mt-3 text-xs text-turd-cream-dim/80">
          The model dropdown shows a fit badge per model:{' '}
          <FitBadge fit="fits" /> = clears 85% of usable VRAM,{' '}
          <FitBadge fit="tight" /> = clears 100% with no headroom,{' '}
          <FitBadge fit="over" /> = does not fit (CPU fallback only).
          Headroom = total VRAM − 1 GiB reserved for OS/overlay/browser.
          4 GiB GPUs and up are supported — pick a small model
          (qwen2.5-coder:1.5b, ~1 GiB) if VRAM is tight.
        </p>
      </section>

      <section className="glass rounded-xl p-5">
        <h2 className="font-display text-sm uppercase tracking-widest text-turd-mustard">
          Local model registry
        </h2>
        <div className="mt-3 overflow-auto rounded border border-turd-bronze/20">
          <table className="w-full text-left text-sm">
            <thead className="bg-turd-bg-deep/60">
              <tr className="border-b border-turd-bronze/30">
                <th className="px-3 py-2 font-display text-[10px] uppercase tracking-widest text-turd-mustard">
                  Model
                </th>
                <th className="px-3 py-2 font-display text-[10px] uppercase tracking-widest text-turd-mustard">
                  Size
                </th>
                <th className="px-3 py-2 font-display text-[10px] uppercase tracking-widest text-turd-mustard">
                  Est VRAM
                </th>
                <th className="px-3 py-2 font-display text-[10px] uppercase tracking-widest text-turd-mustard">
                  Fit
                </th>
              </tr>
            </thead>
            <tbody>
              {decoratedModels.length === 0 && (
                <tr>
                  <td
                    className="px-3 py-3 text-center text-xs text-turd-cream-dim"
                    colSpan={4}
                  >
                    {models.isError
                      ? 'Ollama not reachable at http://127.0.0.1:11434 — start it and refresh.'
                      : 'No models installed yet. Pull one below.'}
                  </td>
                </tr>
              )}
              {decoratedModels.map((m) => (
                <tr
                  key={m.name}
                  className="border-b border-turd-bronze/15 last:border-b-0"
                >
                  <td className="px-3 py-2 font-mono text-xs text-turd-cream">
                    {m.name}
                  </td>
                  <td className="px-3 py-2 font-mono text-xs text-turd-cream-dim">
                    {formatMib(Math.round(m.size / (1024 * 1024)))}
                  </td>
                  <td className="px-3 py-2 font-mono text-xs text-turd-cream-dim">
                    {formatMib(m.estimatedVramMib)}
                  </td>
                  <td className="px-3 py-2">
                    <FitBadge fit={m.fit} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>

        <div className="mt-4">
          <label
            htmlFor="pull-name"
            className="block text-[10px] uppercase tracking-widest text-turd-mustard"
          >
            Pull a new model
          </label>
          <div className="mt-1 flex gap-2">
            <input
              id="pull-name"
              value={pullName}
              onChange={(e) => setPullName(e.target.value)}
              placeholder="qwen2.5-coder:7b"
              className="flex-1 rounded border border-turd-bronze/40 bg-turd-bg-deep/60 px-3 py-1.5 text-sm text-turd-cream"
            />
            <button
              onClick={() => {
                if (pullName.trim()) {
                  setPullLog([]);
                  pullMutation.mutate(pullName.trim());
                }
              }}
              disabled={pullMutation.isPending || !pullName.trim()}
              className="rounded border border-turd-bronze/40 bg-turd-bg-soft/40 px-3 py-1.5 text-sm text-turd-cream hover:bg-turd-bg-soft/60 disabled:opacity-40"
            >
              {pullMutation.isPending ? 'Pulling…' : 'Pull'}
            </button>
          </div>
          {pullLog.length > 0 && (
            <pre className="mt-3 max-h-48 overflow-auto rounded border border-turd-bronze/30 bg-turd-bg-deep/60 p-3 text-xs text-turd-cream-dim">
              {pullLog.join('\n')}
            </pre>
          )}
        </div>
      </section>
    </div>
  );
}

export default AiAssistantPage;
