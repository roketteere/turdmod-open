import { useCallback, useEffect, useState } from 'react';
import {
  remoteScumpilotStatus,
  remoteScumpilotPause,
  remoteScumpilotResume,
  type ScumpilotStatus,
} from '../lib/tauri-remote';

// Pilot — control surface for the scumpilot AI admin (Phase 4 integration).
// Talks to the service's /scumpilot/* proxy via the remote_* Tauri commands.
// Pause freezes the brain's executors (it keeps perceiving/reasoning but takes
// no actions); resume re-enables. Polls status every 5s.
export function PilotPage() {
  const [status, setStatus] = useState<ScumpilotStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setStatus(await remoteScumpilotStatus());
      setError(null);
    } catch (e) {
      setError((e as Error).message ?? String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
    const t = setInterval(() => void refresh(), 5000);
    return () => clearInterval(t);
  }, [refresh]);

  const act = useCallback(
    async (fn: () => Promise<unknown>) => {
      setBusy(true);
      try {
        await fn();
        await refresh();
      } catch (e) {
        setError((e as Error).message ?? String(e));
      } finally {
        setBusy(false);
      }
    },
    [refresh],
  );

  const agents = status ? Object.entries(status.agents) : [];

  return (
    <div className="mx-auto flex max-w-2xl flex-col gap-6">
      <header>
        <p className="font-display text-xs tracking-[0.3em] text-turd-mustard">AI ADMIN</p>
        <h1 className="mt-1 font-display text-3xl text-turd-cream">Pilot</h1>
        <p className="mt-1 text-sm text-turd-cream-dim">
          Live control for the scumpilot AI admin. Pause to freeze its actions
          instantly (it keeps watching + reasoning, but acts on nothing); resume
          to re-enable. In-game you can also use <code>#pilot pause</code>.
        </p>
      </header>

      {error ? (
        <div className="rounded-lg border border-red-500/40 bg-red-950/30 p-3 text-sm text-red-300">
          {error}
          <p className="mt-1 text-[11px] text-turd-cream-dim">
            Requires the remote service (Settings → Remote) and scumpilot running
            with SCUMPILOT_HTTP_PORT/TOKEN set on the server.
          </p>
        </div>
      ) : null}

      <section className="glass rounded-xl p-4">
        <h2 className="font-display text-sm uppercase tracking-widest text-turd-mustard">Status</h2>
        {!status ? (
          <p className="mt-2 text-sm text-turd-cream-dim">Loading…</p>
        ) : (
          <div className="mt-3 flex flex-col gap-2 text-sm text-turd-cream">
            <div className="flex items-center gap-2">
              <span className="text-turd-cream-dim">Brain:</span>
              <span
                className={
                  status.allPaused
                    ? 'rounded bg-amber-900/50 px-2 py-0.5 text-xs text-amber-300'
                    : 'rounded bg-emerald-900/50 px-2 py-0.5 text-xs text-emerald-300'
                }
              >
                {status.allPaused ? 'PAUSED' : 'ACTIVE'}
              </span>
            </div>
            <div className="flex items-center gap-2">
              <span className="text-turd-cream-dim">Owner online:</span>
              <span>{status.ownerOnline ? 'yes' : 'no'}</span>
            </div>
            <div>
              <span className="text-turd-cream-dim">Agents:</span>
              {agents.length === 0 ? (
                <span className="ml-2 text-turd-cream-dim">(none registered)</span>
              ) : (
                <ul className="mt-1 flex flex-col gap-1">
                  {agents.map(([name, paused]) => (
                    <li key={name} className="flex items-center gap-2 text-[13px]">
                      <span className={paused ? 'text-amber-300' : 'text-emerald-300'}>
                        {paused ? '⏸' : '▶'}
                      </span>
                      <span>{name}</span>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </div>
        )}

        <div className="mt-4 flex gap-2">
          <button
            type="button"
            disabled={busy}
            onClick={() => void act(remoteScumpilotPause)}
            className="rounded border border-amber-500/40 bg-amber-900/30 px-3 py-1.5 text-sm text-amber-200 hover:bg-amber-900/50 disabled:opacity-50"
          >
            Pause all
          </button>
          <button
            type="button"
            disabled={busy}
            onClick={() => void act(remoteScumpilotResume)}
            className="rounded border border-emerald-500/40 bg-emerald-900/30 px-3 py-1.5 text-sm text-emerald-200 hover:bg-emerald-900/50 disabled:opacity-50"
          >
            Resume all
          </button>
          <button
            type="button"
            disabled={busy}
            onClick={() => void refresh()}
            className="rounded border border-turd-bronze/40 px-3 py-1.5 text-sm text-turd-cream-dim hover:bg-turd-bg-mid/60 disabled:opacity-50"
          >
            Refresh
          </button>
        </div>
      </section>
    </div>
  );
}
