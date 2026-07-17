import { useState, useEffect } from 'react';
import { Link } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { useCompanionRestart, useCompanionStart, useCompanionStop, useCompanionStatus } from '../hooks/useCompanion';
import { useEngineInstall, useEngineStart, useEngineStatus, useEngineStop } from '../hooks/useEngine';
import { useMinPending } from '../hooks/useMinPending';
import { useSettings } from '../hooks/useSettings';
import { useDetectedInstalls } from '../hooks/useTarget';
import type { CompanionStatus } from '../lib/tauri-companion';
import type { EngineStatus, InstallReport } from '../lib/tauri-engine';
import {
  remoteGetConfig, remoteSetConfig, remoteHealth, remoteStatus,
  remoteServerStart, remoteServerStop, remoteServerRestart, remoteServerLogs,
} from '../lib/tauri-remote';

// ---------------------------------------------------------------------------
// Uptime helper
// ---------------------------------------------------------------------------

function formatUptime(startedAtIso: string): string {
  const started = new Date(startedAtIso).getTime();
  const now = Date.now();
  const diffSec = Math.max(0, Math.floor((now - started) / 1000));
  const h = Math.floor(diffSec / 3600);
  const m = Math.floor((diffSec % 3600) / 60);
  const s = diffSec % 60;
  if (h > 0) return `${h}h ${m}m ${s}s`;
  if (m > 0) return `${m}m ${s}s`;
  return `${s}s`;
}

// ---------------------------------------------------------------------------
// Status pills (one each for engine + companion)
// ---------------------------------------------------------------------------

type AnyStatus = CompanionStatus | EngineStatus;

function StatusPill({ status, idleLabel }: { status: AnyStatus | undefined; idleLabel: string }) {
  if (!status || status.kind === 'stopped') {
    return (
      <div className="inline-flex items-center gap-2 rounded-full border border-turd-bronze/30 bg-turd-bg-soft px-4 py-2">
        <span className="h-3 w-3 rounded-full bg-turd-cream-dim/30" />
        <span className="font-display text-sm text-turd-cream-dim">{idleLabel}</span>
      </div>
    );
  }
  if (status.kind === 'starting') {
    return (
      <div className="inline-flex items-center gap-2 rounded-full border border-turd-mustard/40 bg-turd-bg-soft px-4 py-2">
        <span className="h-3 w-3 animate-pulse rounded-full bg-turd-mustard" />
        <span className="font-display text-sm text-turd-mustard">Starting…</span>
      </div>
    );
  }
  if (status.kind === 'running') {
    return (
      <div className="inline-flex items-center gap-2 rounded-full border border-turd-green/30 bg-turd-bg-soft px-4 py-2">
        <span className="h-3 w-3 rounded-full bg-turd-green" />
        <span className="font-display text-sm text-turd-green">
          Running — Up {formatUptime(status.startedAtIso)} · PID {status.pid}
        </span>
      </div>
    );
  }
  return (
    <div className="inline-flex items-center gap-2 rounded-full border border-turd-red/40 bg-turd-bg-soft px-4 py-2">
      <span className="h-3 w-3 rounded-full bg-turd-red" />
      <span className="font-display text-sm text-turd-red">
        Crashed{status.exitCode != null ? ` — exit code ${status.exitCode}` : ''} · check Console for details
      </span>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Button style helpers
// ---------------------------------------------------------------------------

const btnBase =
  'rounded border px-4 py-2 font-display text-xs uppercase tracking-wider transition-colors disabled:opacity-40';
const btnPrimary =
  `${btnBase} border-turd-mustard bg-turd-bg-soft text-turd-mustard-bright hover:bg-turd-bronze/30`;
const btnSecondary =
  `${btnBase} border-turd-bronze/60 bg-turd-bg-soft text-turd-cream-dim hover:border-turd-cream hover:text-turd-cream`;
const btnDanger =
  `${btnBase} border-turd-red/50 bg-turd-bg-soft text-turd-red hover:border-turd-red`;

// Inline SVG spinner — pure CSS animation, no asset, no library.
// Sized to fit inside a button row.
function Spinner() {
  return (
    <svg
      className="h-3.5 w-3.5 animate-spin"
      viewBox="0 0 24 24"
      fill="none"
      aria-hidden="true"
    >
      <circle
        className="opacity-25"
        cx="12"
        cy="12"
        r="10"
        stroke="currentColor"
        strokeWidth="4"
      />
      <path
        className="opacity-75"
        fill="currentColor"
        d="M4 12a8 8 0 018-8v4a4 4 0 00-4 4H4z"
      />
    </svg>
  );
}

// ---------------------------------------------------------------------------
// Engine section — the DLL trio (UE4SS + bridge + loader) against SCUMServer
// ---------------------------------------------------------------------------

function EngineSection() {
  const statusQuery = useEngineStatus();
  const detected = useDetectedInstalls();
  const start = useEngineStart();
  const stop = useEngineStop();
  const install = useEngineInstall();

  const detectedServer = detected.data?.server ?? '';
  const [serverInstall, setServerInstall] = useState<string>('');

  // Once the detection fires, pre-fill the input.
  const effectiveInstall = serverInstall || detectedServer;

  const status = statusQuery.data;
  const isRunning = status?.kind === 'running';

  // useMinPending keeps the visual pending state on for at least 600ms
  // so fast operations (Install DLLs ≈ 50ms) still flash a visible
  // spinner. Without this the animation is invisible.
  const installBusy = useMinPending(install.isPending, 700);
  const startBusy = useMinPending(start.isPending, 700);
  const stopBusy = useMinPending(stop.isPending, 700);

  const isBusy = startBusy || stopBusy || installBusy;
  const canAct = effectiveInstall.length > 0;

  const [lastInstall, setLastInstall] = useState<{ report: InstallReport; ts: Date } | null>(null);
  // HH:MM:SS.mmm — same shape as the UE4SS log timestamps so the two
  // can be eyeballed side by side. Each install bumps the timestamp so
  // a fresh click is visibly distinct from a stale display.
  const fmtTs = (d: Date) => {
    const p = (n: number, w = 2) => String(n).padStart(w, '0');
    return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}.${p(d.getMilliseconds(), 3)}`;
  };

  return (
    <section className="relative overflow-hidden glass rounded-xl p-5">
      {/* Indeterminate progress bar — flush to the top of the section
          while any install/start/stop is in flight. Way more visible
          than just the button spinner. */}
      {isBusy && (
        <div className="absolute inset-x-0 top-0 h-0.5 overflow-hidden bg-turd-bronze/20">
          <div className="h-full w-1/3 animate-[indeterminate_1.2s_ease-in-out_infinite] bg-turd-mustard-bright" />
        </div>
      )}
      <div className="flex items-center justify-between">
        <h2 className="font-display text-sm uppercase tracking-widest text-turd-mustard">
          Engine — UE4SS + Bridge + Loader
        </h2>
        <StatusPill status={status} idleLabel="Stopped" />
      </div>

      <div className="mt-5 space-y-3">
        <label className="block">
          <span className="font-display text-[10px] uppercase tracking-wider text-turd-cream-dim">
            SCUMServer install
          </span>
          <input
            type="text"
            value={effectiveInstall}
            onChange={(e) => setServerInstall(e.target.value)}
            placeholder={
              detected.isLoading
                ? 'Detecting…'
                : detected.data?.server
                  ? detected.data.server
                  : 'Path to SCUMServer install (e.g. D:\\SteamLibrary\\steamapps\\common\\SCUM Server)'
            }
            className="mt-1 w-full rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-1.5 font-mono text-xs text-turd-cream placeholder:text-turd-cream-dim/60 focus:border-turd-mustard focus:outline-none"
          />
        </label>

        <div className="flex flex-wrap gap-2">
          <button
            type="button"
            disabled={!canAct || isBusy}
            onClick={() => install.mutate({ serverInstall: effectiveInstall }, {
              onSuccess: (report) => setLastInstall({ report, ts: new Date() }),
            })}
            className={btnSecondary}
          >
            {installBusy ? (
              <span className="inline-flex items-center gap-2">
                <Spinner /> INSTALLING ENGINE…
              </span>
            ) : (
              'INSTALL ENGINE'
            )}
          </button>
          <button
            type="button"
            disabled={!canAct || isRunning || isBusy}
            onClick={() => start.mutate({ serverInstall: effectiveInstall })}
            className={btnPrimary}
          >
            {startBusy ? (
              <span className="inline-flex items-center gap-2">
                <Spinner /> Starting…
              </span>
            ) : (
              'Start Engine'
            )}
          </button>
          <button
            type="button"
            disabled={!isRunning || isBusy}
            onClick={() => stop.mutate()}
            className={btnDanger}
          >
            {stopBusy ? (
              <span className="inline-flex items-center gap-2">
                <Spinner /> Stopping…
              </span>
            ) : (
              'Stop Engine'
            )}
          </button>
        </div>

        {install.isError && (
          <p className="text-xs text-turd-red">Install failed: {install.error?.message}</p>
        )}
        {start.isError && (
          <p className="text-xs text-turd-red">Start failed: {start.error?.message}</p>
        )}
        {stop.isError && (
          <p className="text-xs text-turd-red">Stop failed: {stop.error?.message}</p>
        )}

        {lastInstall && (
          <div className="rounded border border-turd-bronze/30 bg-turd-bg-deep p-3 text-[11px] font-mono text-turd-cream-dim">
            <div className="text-turd-green">[{fmtTs(lastInstall.ts)}] Install OK</div>
            <div>[{fmtTs(lastInstall.ts)}] UE4SS  → {lastInstall.report.ue4ssDllPath}</div>
            <div>[{fmtTs(lastInstall.ts)}] Bridge → {lastInstall.report.bridgeDllPath}</div>
            <div>
              [{fmtTs(lastInstall.ts)}]{' '}
              {lastInstall.report.wroteIni ? 'wrote UE4SS-settings.ini' : 'UE4SS-settings.ini already present'} ·{' '}
              {lastInstall.report.wroteModsLine ? 'enabled in mods.txt' : 'already enabled in mods.txt'}
            </div>
          </div>
        )}
      </div>
    </section>
  );
}

// ---------------------------------------------------------------------------
// Companion section — TS process that talks to the loader's named pipe
// ---------------------------------------------------------------------------

function CompanionSection() {
  const statusQuery = useCompanionStatus();
  const settingsQuery = useSettings();
  const start = useCompanionStart();
  const stop = useCompanionStop();
  const restart = useCompanionRestart();

  const status = statusQuery.data;
  const settings = settingsQuery.data;

  const isRunning = status?.kind === 'running';
  const isStopped = !status || status.kind === 'stopped' || status.kind === 'crashed';
  const isBusy = start.isPending || stop.isPending || restart.isPending;

  return (
    <section className="glass rounded-xl p-5">
      <div className="flex items-center justify-between">
        <h2 className="font-display text-sm uppercase tracking-widest text-turd-mustard">
          Companion
        </h2>
        <StatusPill status={status} idleLabel="Stopped" />
      </div>

      <div className="mt-5 flex gap-2">
        <button
          type="button"
          disabled={isRunning || isBusy}
          onClick={() => start.mutate()}
          className={btnPrimary}
        >
          {start.isPending ? 'Starting...' : 'Start'}
        </button>
        <button
          type="button"
          disabled={isStopped || isBusy}
          onClick={() => stop.mutate()}
          className={btnDanger}
        >
          {stop.isPending ? 'Stopping...' : 'Stop'}
        </button>
        <button
          type="button"
          disabled={isStopped || isBusy}
          onClick={() => restart.mutate()}
          className={btnSecondary}
        >
          {restart.isPending ? 'Restarting...' : 'Restart'}
        </button>
      </div>

      {start.isError && (
        <p className="mt-3 text-xs text-turd-red">Start failed: {start.error?.message}</p>
      )}
      {stop.isError && (
        <p className="mt-3 text-xs text-turd-red">Stop failed: {stop.error?.message}</p>
      )}

      <dl className="mt-5 grid gap-3">
        <SettingsRow
          label="Logs path"
          value={
            settings?.scumServerLogsDir ?? (
              <span className="text-turd-red">
                Not set —{' '}
                <Link to="/settings" className="underline hover:text-turd-mustard-bright">
                  open Settings
                </Link>
              </span>
            )
          }
        />
        <SettingsRow
          label="RCON"
          value={
            settings?.rconHost ? (
              <>
                {settings.rconHost}:{settings.rconPort ?? 27015}
                {settings.hasRconPassword ? ' (password set)' : ' (no password)'}
              </>
            ) : (
              <span className="text-turd-cream-dim">
                Not configured —{' '}
                <Link to="/settings" className="underline hover:text-turd-mustard-bright">
                  open Settings
                </Link>
              </span>
            )
          }
        />
        <SettingsRow
          label="Discord webhook"
          value={
            settings?.discordWebhook ? (
              <span className="text-turd-green">Configured</span>
            ) : (
              <span className="text-turd-cream-dim">
                Not set —{' '}
                <Link to="/settings" className="underline hover:text-turd-mustard-bright">
                  open Settings
                </Link>
              </span>
            )
          }
        />
      </dl>
    </section>
  );
}

// ---------------------------------------------------------------------------
// Remote section — OVH service API control
// ---------------------------------------------------------------------------

function RemoteSection() {
  const qc = useQueryClient();
  const cfgQuery = useQuery({ queryKey: ['remote-config'], queryFn: remoteGetConfig });
  const statusQuery = useQuery({
    queryKey: ['remote-status'],
    queryFn: remoteStatus,
    enabled: !!cfgQuery.data?.enabled,
    refetchInterval: 5000,
  });
  const healthQuery = useQuery({
    queryKey: ['remote-health'],
    queryFn: remoteHealth,
    enabled: !!cfgQuery.data?.enabled,
    refetchInterval: 30000,
  });
  const logsQuery = useQuery({
    queryKey: ['remote-logs'],
    queryFn: () => remoteServerLogs(30),
    enabled: !!cfgQuery.data?.enabled && !!statusQuery.data?.server_running,
    refetchInterval: 10000,
  });

  // EnginePage drives the CONFIGURED remote (RemoteConfig) — target "remote" routes
  // there via for_target's else branch (preserves the pre-target-param behavior).
  const startMut = useMutation({
    mutationFn: () => remoteServerStart('remote'),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['remote-status'] }),
  });
  const stopMut = useMutation({
    mutationFn: () => remoteServerStop('remote'),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['remote-status'] }),
  });
  const restartMut = useMutation({
    mutationFn: () => remoteServerRestart('remote'),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['remote-status'] }),
  });

  const [editing, setEditing] = useState(false);
  const [host, setHost] = useState('');
  const [port, setPort] = useState('9090');
  const [token, setToken] = useState('');

  useEffect(() => {
    if (cfgQuery.data) {
      setHost(cfgQuery.data.host);
      setPort(String(cfgQuery.data.port));
      setToken(cfgQuery.data.token);
    }
  }, [cfgQuery.data]);

  const saveCfg = async () => {
    await remoteSetConfig({ host, port: Number(port) || 9090, token, enabled: true });
    qc.invalidateQueries({ queryKey: ['remote-config'] });
    setEditing(false);
  };

  const status = statusQuery.data;
  const isRunning = status?.server_running ?? false;
  const isBusy = startMut.isPending || stopMut.isPending || restartMut.isPending;

  if (!cfgQuery.data?.enabled && !editing) {
    return (
      <section className="glass rounded-xl p-5">
        <div className="flex items-center justify-between">
          <h2 className="font-display text-sm uppercase tracking-widest text-turd-mustard">
            Remote — OVH Service
          </h2>
          <button type="button" onClick={() => setEditing(true)} className={btnSecondary}>
            Configure
          </button>
        </div>
        <p className="mt-3 text-xs text-turd-cream-dim">
          Connect to turdmod-service running on your OVH VPS for remote server management.
        </p>
      </section>
    );
  }

  return (
    <section className="glass rounded-xl p-5">
      <div className="flex items-center justify-between">
        <h2 className="font-display text-sm uppercase tracking-widest text-turd-mustard">
          Remote — OVH Service
        </h2>
        {healthQuery.data && (
          <div className="inline-flex items-center gap-2 rounded-full border border-turd-green/30 bg-turd-bg-soft px-4 py-2">
            <span className="h-3 w-3 rounded-full bg-turd-green" />
            <span className="font-display text-sm text-turd-green">
              {healthQuery.data.service} v{healthQuery.data.version}
            </span>
          </div>
        )}
        {healthQuery.isError && (
          <div className="inline-flex items-center gap-2 rounded-full border border-turd-red/40 bg-turd-bg-soft px-4 py-2">
            <span className="h-3 w-3 rounded-full bg-turd-red" />
            <span className="font-display text-sm text-turd-red">Offline</span>
          </div>
        )}
      </div>

      {editing ? (
        <div className="mt-4 space-y-3">
          <div className="grid grid-cols-3 gap-3">
            <label className="block">
              <span className="font-display text-[10px] uppercase tracking-wider text-turd-cream-dim">Host</span>
              <input type="text" value={host} onChange={(e) => setHost(e.target.value)}
                className="mt-1 w-full rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-1.5 font-mono text-xs text-turd-cream focus:border-turd-mustard focus:outline-none"
                placeholder="YOUR_SERVER_IP" />
            </label>
            <label className="block">
              <span className="font-display text-[10px] uppercase tracking-wider text-turd-cream-dim">Port</span>
              <input type="text" value={port} onChange={(e) => setPort(e.target.value)}
                className="mt-1 w-full rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-1.5 font-mono text-xs text-turd-cream focus:border-turd-mustard focus:outline-none"
                placeholder="9090" />
            </label>
            <label className="block">
              <span className="font-display text-[10px] uppercase tracking-wider text-turd-cream-dim">Token</span>
              <input type="password" value={token} onChange={(e) => setToken(e.target.value)}
                className="mt-1 w-full rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-1.5 font-mono text-xs text-turd-cream focus:border-turd-mustard focus:outline-none"
                placeholder="Bearer token" />
            </label>
          </div>
          <div className="flex gap-2">
            <button type="button" onClick={saveCfg} className={btnPrimary}>Save & Connect</button>
            <button type="button" onClick={() => setEditing(false)} className={btnSecondary}>Cancel</button>
          </div>
        </div>
      ) : (
        <>
          <div className="mt-4 flex items-center gap-3">
            <span className="font-mono text-xs text-turd-cream-dim">{host}:{port}</span>
            <button type="button" onClick={() => setEditing(true)} className="font-display text-[10px] uppercase tracking-wider text-turd-mustard hover:text-turd-mustard-bright">
              Edit
            </button>
          </div>

          {status && (
            <div className="mt-4 grid grid-cols-4 gap-4 rounded border border-turd-bronze/20 bg-turd-bg-deep p-3">
              <div>
                <div className="font-display text-[10px] uppercase tracking-wider text-turd-cream-dim">Status</div>
                <div className={`mt-1 text-xs font-bold ${isRunning ? 'text-turd-green' : 'text-turd-red'}`}>
                  {isRunning ? 'RUNNING' : 'STOPPED'}
                </div>
              </div>
              {status.pid && (
                <div>
                  <div className="font-display text-[10px] uppercase tracking-wider text-turd-cream-dim">PID</div>
                  <div className="mt-1 font-mono text-xs text-turd-cream">{status.pid}</div>
                </div>
              )}
              <div>
                <div className="font-display text-[10px] uppercase tracking-wider text-turd-cream-dim">Uptime</div>
                <div className="mt-1 font-mono text-xs text-turd-cream">
                  {status.uptime_secs > 0 ? `${Math.floor(status.uptime_secs / 3600)}h ${Math.floor((status.uptime_secs % 3600) / 60)}m` : '—'}
                </div>
              </div>
              <div>
                <div className="font-display text-[10px] uppercase tracking-wider text-turd-cream-dim">Restarts</div>
                <div className="mt-1 font-mono text-xs text-turd-cream">{status.restarts}</div>
              </div>
            </div>
          )}

          <div className="mt-4 flex gap-2">
            <button type="button" disabled={isRunning || isBusy}
              onClick={() => startMut.mutate()} className={btnPrimary}>
              {startMut.isPending ? 'Starting…' : 'Start Server'}
            </button>
            <button type="button" disabled={!isRunning || isBusy}
              onClick={() => stopMut.mutate()} className={btnDanger}>
              {stopMut.isPending ? 'Stopping…' : 'Stop Server'}
            </button>
            <button type="button" disabled={!isRunning || isBusy}
              onClick={() => restartMut.mutate()} className={btnSecondary}>
              {restartMut.isPending ? 'Restarting…' : 'Restart'}
            </button>
          </div>

          {startMut.isError && <p className="mt-2 text-xs text-turd-red">Start: {String(startMut.error)}</p>}
          {stopMut.isError && <p className="mt-2 text-xs text-turd-red">Stop: {String(stopMut.error)}</p>}

          {logsQuery.data?.lines && logsQuery.data.lines.length > 0 && (
            <div className="mt-4 max-h-40 overflow-y-auto rounded border border-turd-bronze/20 bg-turd-bg-deep p-3">
              <div className="font-display text-[10px] uppercase tracking-wider text-turd-cream-dim mb-2">
                Recent Logs
              </div>
              {logsQuery.data.lines.map((line, i) => (
                <div key={i} className="font-mono text-[10px] text-turd-cream-dim leading-relaxed">
                  {line}
                </div>
              ))}
            </div>
          )}
        </>
      )}
    </section>
  );
}

// ---------------------------------------------------------------------------
// Page
// ---------------------------------------------------------------------------

export function EnginePage() {
  return (
    <div className="space-y-6">
      <header>
        <p className="font-display text-xs tracking-[0.3em] text-turd-mustard">
          TurdMOD
        </p>
        <h1 className="mt-1 font-display text-3xl text-turd-cream">Engine</h1>
        <p className="mt-2 max-w-2xl text-sm text-turd-cream-dim">
          The Engine section installs and runs the in-process DLLs (UE4SS + bridge + loader) on
          your SCUMServer. The Companion section runs the log-tail / event dispatcher process that
          delivers gameplay events to mods. Remote mode connects to OVH for production control.
        </p>
      </header>

      <RemoteSection />
      <EngineSection />
      <CompanionSection />

      <div className="text-right">
        <Link
          to="/console"
          className="font-display text-xs uppercase tracking-widest text-turd-mustard-bright transition-opacity hover:opacity-80"
        >
          View live output in Console &rarr;
        </Link>
      </div>
    </div>
  );
}

function SettingsRow({
  label,
  value,
}: {
  label: string;
  value: React.ReactNode;
}) {
  return (
    <div className="flex items-start justify-between gap-4 border-b border-turd-bronze/10 pb-2 last:border-0 last:pb-0">
      <dt className="shrink-0 font-mono text-[10px] uppercase tracking-wider text-turd-cream-dim">
        {label}
      </dt>
      <dd className="text-right font-mono text-xs text-turd-cream">{value}</dd>
    </div>
  );
}
