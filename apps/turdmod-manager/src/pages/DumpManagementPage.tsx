// Dump Management page — drives the sibling scumdump CLI from the GUI.
//
// Section layout:
//   1. SCUM Build Status card (Steam vs extracted, AES fingerprint)
//   2. Phase A / B / C cards (counts, last-run, individual Run button)
//   3. Composite actions (Run All, Re-extract AES, Open Dump Folder)
//   4. Streaming log pane
//
// Backend lives in apps/turdmod-manager/src-tauri/src/dump_commands.rs.

import { useEffect, useMemo, useRef, useState } from 'react';
import { useMutation } from '@tanstack/react-query';
import {
  useDumpActiveStatus,
  useDumpDiffSummary,
  useDumpListBuilds,
  useDumpExtractAes,
  useDumpLogBuffer,
  useDumpOpenFolder,
  useDumpRunAll,
  useDumpRunPhase,
  useDumpStatus,
  type BufferedLogLine,
} from '../hooks/useDumpStatus';
import {
  snapshotInstall,
  snapshotList,
  type SnapshotEntry,
  type SnapshotResult,
} from '../lib/tauri-snapshot';
import type { DiffNameSet } from '../lib/tauri-dump';
import { dumpInjectDumper7, type InjectResult } from '../lib/tauri-dump';
import {
  assistSummarizeDiff,
  loadAssistSettings,
} from '../lib/tauri-assist';
import { formatTs } from '../lib/format-ts';

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  return `${(n / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

// Local alias so existing call sites stay terse. The shared helper
// in lib/format-ts.ts returns `YYYY-MM-DD HH:MM:SS` consistently.
const formatDate = formatTs;

// ---------------------------------------------------------------------------
// Log line tone classifier — pick a Tailwind class based on content.
// Order matters: error > warn > success > meta > info. Status lines
// (heartbeats) get their own bright mustard treatment so they pop
// even when surrounded by neutral info lines.
// ---------------------------------------------------------------------------

type LineTone = 'error' | 'warn' | 'success' | 'meta' | 'info';

function classifyLineTone(text: string, stream: 'stdout' | 'stderr'): LineTone {
  if (stream === 'stderr') return 'error';
  // Errors — substring match because pnpm prints things like
  // " ELIFECYCLE  Command failed " with leading spaces + extra
  // spacing.
  if (/(error|err |failed|fatal|elifecycle|panicked|exception|not recognized|not found)/i.test(text)) {
    return 'error';
  }
  if (/(warning|warn:|deprecated|skipping|skipped|missing)/i.test(text)) {
    return 'warn';
  }
  if (/(complete|done|ok\b|ready|success|installed|registered|copied|stabilized|detected|mounted|connected|listening|✓)/i.test(text)) {
    return 'success';
  }
  // Meta — pnpm script preamble, banners, shell echoes.
  if (text.startsWith('>') || text.startsWith('$ ') || /^={3,}/.test(text)) {
    return 'meta';
  }
  return 'info';
}

const TONE_CLASS: Record<LineTone, string> = {
  error: 'text-red-400',
  warn: 'text-amber-300',
  success: 'text-emerald-400',
  meta: 'text-turd-cream-dim/60',
  info: 'text-turd-cream',
};

function Card({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <section className="glass rounded-xl p-5">
      <h2 className="font-display text-sm uppercase tracking-widest text-turd-mustard">
        {title}
      </h2>
      <div className="mt-3">{children}</div>
    </section>
  );
}

function Button({
  onClick,
  disabled,
  children,
  variant = 'default',
}: {
  onClick: () => void;
  disabled?: boolean;
  children: React.ReactNode;
  variant?: 'default' | 'primary';
}) {
  const base =
    'rounded border px-4 py-2 text-sm font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-40';
  const variants = {
    default:
      'border-turd-bronze/40 bg-turd-bg-soft/40 text-turd-cream hover:bg-turd-bg-soft/60',
    primary:
      'border-turd-mustard/40 bg-turd-mustard/20 text-turd-mustard-bright hover:bg-turd-mustard/30',
  };
  return (
    <button onClick={onClick} disabled={disabled} className={`${base} ${variants[variant]}`}>
      {children}
    </button>
  );
}

function DiffCounts({ d, label }: { d: DiffNameSet | null | undefined; label: string }) {
  const [open, setOpen] = useState(false);
  if (!d) {
    return (
      <div className="flex justify-between text-sm">
        <dt className="text-turd-cream-dim">{label}:</dt>
        <dd className="text-turd-cream-dim/60">—</dd>
      </div>
    );
  }
  const added = d.added.length;
  const removed = d.removed.length;
  const changed = d.changedCount;
  const anyChange = added > 0 || removed > 0 || changed > 0;
  const expandable = added + removed > 0;
  return (
    <div>
      <button
        type="button"
        disabled={!expandable}
        onClick={() => setOpen((o) => !o)}
        className="flex w-full items-baseline justify-between text-left text-sm hover:bg-turd-bg-deep/30 disabled:cursor-default disabled:hover:bg-transparent"
      >
        <dt className="text-turd-cream-dim">
          {expandable && (
            <span className="mr-1 text-turd-cream-dim/60">{open ? '▾' : '▸'}</span>
          )}
          {label}:
        </dt>
        <dd className={anyChange ? 'font-mono text-turd-cream' : 'font-mono text-turd-cream-dim'}>
          <span className="text-emerald-400">+{added}</span> /{' '}
          <span className="text-red-400">-{removed}</span> /{' '}
          <span className="text-turd-mustard">~{changed}</span>
        </dd>
      </button>
      {open && expandable && (
        <div className="mt-1 ml-3 grid grid-cols-1 gap-2 text-[11px] sm:grid-cols-2">
          {added > 0 && (
            <div>
              <p className="font-medium text-emerald-400">+ Added ({added})</p>
              <ul className="mt-0.5 max-h-40 overflow-y-auto font-mono text-turd-cream-dim">
                {d.added.slice(0, 200).map((n) => (
                  <li key={`a-${n}`} className="truncate">{n}</li>
                ))}
                {d.added.length > 200 && (
                  <li className="text-turd-cream-dim/50">… +{d.added.length - 200} more</li>
                )}
              </ul>
            </div>
          )}
          {removed > 0 && (
            <div>
              <p className="font-medium text-red-400">− Removed ({removed})</p>
              <ul className="mt-0.5 max-h-40 overflow-y-auto font-mono text-turd-cream-dim">
                {d.removed.slice(0, 200).map((n) => (
                  <li key={`r-${n}`} className="truncate">{n}</li>
                ))}
                {d.removed.length > 200 && (
                  <li className="text-turd-cream-dim/50">… +{d.removed.length - 200} more</li>
                )}
              </ul>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

/** Renders an active heartbeat as a dedicated card above the log
 *  pane: phase label, elapsed-style summary, a real or indeterminate
 *  progress bar, and an expandable per-phase details list.
 *
 *  Important: stays mounted across status transitions even when
 *  `status` is null — toggling between mounted/unmounted would reset
 *  the user's expand/collapse state every time a phase finishes.
 *  When there's nothing to show, we render a placeholder div with
 *  `display: none` so the component tree (and the `expanded` state
 *  inside) survives. */
function CurrentPhaseCard({ status }: { status: BufferedLogLine | null }) {
  const [expanded, setExpanded] = useState(false);
  const [injectResult, setInjectResult] = useState<InjectResult | null>(null);
  const injectMutation = useMutation({
    mutationFn: (target: 'server' | 'client') => dumpInjectDumper7(target),
    onSuccess: (r) => setInjectResult(r),
    onError: () => setInjectResult(null),
  });
  if (!status) return <div className="hidden" aria-hidden="true" />;
  const hasPercent = typeof status.percent === 'number';
  const pct = hasPercent ? Math.min(100, Math.max(0, status.percent!)) : null;

  // Show the inject button only during the inject-wait phase (scumdump
  // emits group IDs like phase-b-server-wait / phase-b-client-wait).
  const isInjectWait = !!status.statusGroup?.endsWith('-wait');
  const targetFromGroup: 'server' | 'client' | null = status.statusGroup
    ?.includes('phase-b-client-')
    ? 'client'
    : status.statusGroup?.includes('phase-b-server-')
    ? 'server'
    : null;
  return (
    <section className="rounded-lg border border-cyan-400/40 bg-turd-bg-mid/60 p-5">
      <div className="flex items-start justify-between gap-4">
        <div>
          <p className="font-display text-xs uppercase tracking-[0.4em] text-cyan-400">
            <span className="mr-2 inline-block animate-pulse">⟳</span>
            Current phase
          </p>
          <h2 className="mt-1 font-display text-lg text-turd-cream">
            {status.phaseLabel ?? status.phase}
          </h2>
          {status.line && (
            <p className="mt-1 text-sm text-turd-cream-dim">{status.line}</p>
          )}
        </div>
        <div className="text-right text-xs text-turd-cream-dim/70">
          last update {status.ts}
        </div>
      </div>

      <div className="mt-4">
        {hasPercent ? (
          <>
            <div className="flex items-center justify-between text-xs text-turd-cream-dim">
              <span>progress</span>
              <span className="font-mono text-cyan-400">{pct}%</span>
            </div>
            <div className="mt-1 h-2 w-full overflow-hidden rounded bg-turd-bg-deep/80">
              <div
                className="h-full bg-cyan-400 transition-all duration-300 ease-out"
                style={{ width: `${pct}%` }}
              />
            </div>
          </>
        ) : (
          // Indeterminate state — no % data available. Render an
          // animated stripe so the user knows it's still alive but
          // not falsely claiming progress.
          <>
            <div className="flex items-center justify-between text-xs text-turd-cream-dim">
              <span>progress</span>
              <span className="font-mono text-cyan-400/60">— indeterminate</span>
            </div>
            <div className="mt-1 h-2 w-full overflow-hidden rounded bg-turd-bg-deep/80">
              <div className="h-full w-1/3 animate-pulse bg-cyan-400/40" />
            </div>
          </>
        )}
      </div>

      {isInjectWait && targetFromGroup && (
        <div className="mt-4 flex flex-col gap-2">
          <div className="flex flex-wrap items-center gap-3">
            <button
              onClick={() => injectMutation.mutate(targetFromGroup)}
              disabled={injectMutation.isPending}
              className="rounded border border-cyan-400/60 bg-cyan-400/15 px-3 py-1.5 text-xs font-medium text-cyan-300 hover:bg-cyan-400/25 disabled:opacity-40"
            >
              {injectMutation.isPending
                ? `Injecting Dumper-7 into ${targetFromGroup}…`
                : `Inject Dumper-7.dll into ${targetFromGroup === 'server' ? 'GameServer.exe' : 'SCUM.exe'}`}
            </button>
            {injectResult && (
              <span
                className={
                  injectResult.exitCode === 0
                    ? 'text-xs text-emerald-400'
                    : 'text-xs text-amber-300'
                }
              >
                {injectResult.exitCode === 0 ? '✓' : '⚠'} {injectResult.message}{' '}
                <span className="text-turd-cream-dim">
                  (exit {injectResult.exitCode})
                </span>
              </span>
            )}
            {injectMutation.isError && (
              <span className="text-xs text-red-400">
                inject failed: {String(injectMutation.error)}
              </span>
            )}
          </div>
          {targetFromGroup === 'client' && (
            <p className="text-xs text-amber-200/80">
              ⏱ Wait <strong>60–90s</strong> after launching SCUM.exe before
              clicking inject — until its memory plateaus (~3.9 GB). Injecting
              too early kills the dump mid-walk (gets the master SDK.hpp but
              empty per-package SDK/ folder). Verified 2026-05-22.
            </p>
          )}
        </div>
      )}

      {status.details && (
        <details
          className="mt-4 text-xs"
          open={expanded}
          onToggle={(e) => setExpanded(e.currentTarget.open)}
        >
          <summary className="cursor-pointer text-turd-cream-dim hover:text-turd-cream">
            {expanded ? 'Hide details' : 'Show details'}
          </summary>
          <dl className="mt-2 grid grid-cols-[max-content_1fr] gap-x-6 gap-y-1">
            {Object.entries(status.details).map(([k, v]) => (
              <div key={k} className="contents">
                <dt className="text-turd-cream-dim">{k}:</dt>
                <dd className="font-mono text-turd-cream">{String(v)}</dd>
              </div>
            ))}
          </dl>
        </details>
      )}
    </section>
  );
}

// ---------------------------------------------------------------------------
// SnapshotsCard — versioned backups of SCUM Server + SCUM Client installs so
// we can roll forward/back between Steam builds without losing known-working
// state. Per IDEAS.md 2026-05-22 (Joel's "muahaha" request).
//
// MVP: snapshot now button per target. Restore is deferred (needs elevation;
// designing that carefully is its own session).
// ---------------------------------------------------------------------------
function SnapshotsCard() {
  const [list, setList] = useState<SnapshotEntry[]>([]);
  const [lastResult, setLastResult] = useState<SnapshotResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<'server' | 'client' | null>(null);

  const refresh = async () => {
    try {
      setList(await snapshotList());
    } catch (e) {
      setError(String(e));
    }
  };

  useEffect(() => {
    void refresh();
  }, []);

  const fire = async (target: 'server' | 'client') => {
    setBusy(target);
    setError(null);
    try {
      const result = await snapshotInstall(target);
      setLastResult(result);
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <Card title="Install snapshots (server + client)">
      <p className="mb-3 text-xs text-turd-cream-dim">
        Versioned full-copy backups of the SCUM install dirs. Use these
        to roll back when a Steam patch breaks mods, or to keep a clean
        baseline before testing. Stored at{' '}
        <code className="rounded bg-turd-bg-deep/60 px-1.5 py-0.5">
          C:/TurdMOD-snapshots/&lt;target&gt;/v&lt;build&gt;/
        </code>
        . Snapshot uses{' '}
        <code className="rounded bg-turd-bg-deep/60 px-1.5 py-0.5">
          robocopy /MIR
        </code>{' '}
        so repeated snapshots of the same install are near-instant. Restore
        is deferred to a follow-up (needs admin elevation).
      </p>

      <div className="mb-4 flex flex-wrap gap-3">
        <button
          type="button"
          onClick={() => fire('server')}
          disabled={busy !== null}
          className="rounded border border-emerald-400/40 bg-emerald-400/15 px-3 py-1.5 text-sm font-medium text-emerald-300 hover:bg-emerald-400/25 disabled:opacity-40"
        >
          {busy === 'server' ? 'Snapshotting server…' : '📸 Snapshot Server'}
        </button>
        <button
          type="button"
          onClick={() => fire('client')}
          disabled={busy !== null}
          className="rounded border border-cyan-400/40 bg-cyan-400/15 px-3 py-1.5 text-sm font-medium text-cyan-300 hover:bg-cyan-400/25 disabled:opacity-40"
        >
          {busy === 'client' ? 'Snapshotting client…' : '📸 Snapshot Client'}
        </button>
        <button
          type="button"
          onClick={() => void refresh()}
          disabled={busy !== null}
          className="rounded border border-turd-bronze/40 bg-turd-bg-soft px-3 py-1.5 text-sm text-turd-cream hover:border-turd-bronze disabled:opacity-40"
        >
          Refresh list
        </button>
      </div>

      {error && (
        <p className="mb-3 text-xs text-red-400">{error}</p>
      )}

      {lastResult && (
        <div className="mb-3 rounded border border-turd-bronze/30 bg-turd-bg-deep/60 p-3 text-xs">
          <p className="text-turd-cream">
            <span className={lastResult.ok ? 'text-emerald-400' : 'text-red-400'}>
              {lastResult.ok ? '✓' : '✗'}
            </span>{' '}
            {lastResult.target} v{lastResult.scumBuild} —{' '}
            {lastResult.summary}{' '}
            <span className="text-turd-cream-dim">
              ({(lastResult.durationMs / 1000).toFixed(1)}s, robocopy exit{' '}
              {lastResult.robocopyExitCode})
            </span>
          </p>
          <p className="mt-1 text-turd-cream-dim/70">{lastResult.destination}</p>
        </div>
      )}

      {list.length === 0 ? (
        <p className="text-xs text-turd-cream-dim/60">
          No snapshots yet. Click a Snapshot button above to capture the
          current install.
        </p>
      ) : (
        <div className="space-y-1">
          <p className="text-xs uppercase tracking-wider text-turd-cream-dim/60">
            {list.length} snapshot{list.length === 1 ? '' : 's'}
          </p>
          <table className="w-full text-xs">
            <thead className="text-turd-cream-dim/60">
              <tr>
                <th className="px-2 py-1 text-left">Target</th>
                <th className="px-2 py-1 text-left">Build</th>
                <th className="px-2 py-1 text-right">Size</th>
                <th className="px-2 py-1 text-left">Created</th>
                <th className="px-2 py-1 text-left">Path</th>
              </tr>
            </thead>
            <tbody className="text-turd-cream">
              {list.map((e) => (
                <tr key={`${e.target}-${e.scumBuild}`} className="border-t border-turd-bronze/15">
                  <td className="px-2 py-1">
                    <span
                      className={
                        e.target === 'server'
                          ? 'text-emerald-400'
                          : 'text-cyan-300'
                      }
                    >
                      {e.target}
                    </span>
                  </td>
                  <td className="px-2 py-1 font-mono">v{e.scumBuild}</td>
                  <td className="px-2 py-1 text-right font-mono">
                    {(e.sizeBytes / (1024 * 1024 * 1024)).toFixed(1)} GB
                  </td>
                  <td className="px-2 py-1 text-turd-cream-dim">
                    {e.createdAtIso.replace('T', ' ').replace('Z', '')}
                  </td>
                  <td className="px-2 py-1 font-mono text-turd-cream-dim/70 text-[10px]">
                    {e.path}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </Card>
  );
}

export function DumpManagementPage() {
  const status = useDumpStatus();
  const runPhase = useDumpRunPhase();
  const runAll = useDumpRunAll();
  const extractAes = useDumpExtractAes();
  const openFolder = useDumpOpenFolder();
  const [selectedDiffBuild, setSelectedDiffBuild] = useState<string | undefined>(undefined);
  const diff = useDumpDiffSummary(selectedDiffBuild);
  const builds = useDumpListBuilds();
  const { lines, clear } = useDumpLogBuffer();
  const activeStatus = useDumpActiveStatus(lines);

  // AI Assistant integration — read settings from localStorage every
  // render so toggling on the AI Assistant page reflects immediately
  // without an event bus. Cheap (single localStorage read).
  const assistSettings = loadAssistSettings();
  const [diffExplanation, setDiffExplanation] = useState<string>('');
  const explainDiff = useMutation({
    mutationFn: async () => {
      if (!diff.data) throw new Error('no diff loaded');
      return assistSummarizeDiff(
        assistSettings.model,
        JSON.stringify(diff.data, null, 2),
      );
    },
    onSuccess: (out) => setDiffExplanation(out),
    onError: (e) => setDiffExplanation(`Error: ${String(e)}`),
  });

  const anyRunning =
    runPhase.isPending || runAll.isPending || extractAes.isPending;

  const logRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    // Auto-scroll log to bottom on new lines.
    if (logRef.current) {
      logRef.current.scrollTop = logRef.current.scrollHeight;
    }
  }, [lines.length]);

  const s = status.data;

  const updateBanner = useMemo(() => {
    if (!s?.updateAvailable) return null;
    return (
      <div className="rounded border border-turd-mustard/40 bg-turd-mustard/15 p-4 text-sm text-turd-mustard-bright">
        ⚠ SCUM updated to build <strong>{s.steamBuild ?? '?'}</strong> — last
        extracted is <strong>v{s.extractedBuild ?? '?'}</strong>. Click "Run
        All Phases" to refresh.
      </div>
    );
  }, [s?.updateAvailable, s?.steamBuild, s?.extractedBuild]);

  if (status.isLoading) {
    return (
      <p className="font-display text-xs uppercase tracking-widest text-turd-cream-dim/40">
        Loading dump status…
      </p>
    );
  }

  if (s && !s.scumdumpPresent) {
    return (
      <div className="space-y-4">
        <header>
          <h1 className="font-display text-3xl text-turd-cream">Dump Management</h1>
        </header>
        <Card title="scumdump not installed">
          <p className="text-sm text-turd-cream-dim">
            The sibling extraction pipeline isn't present at
            <code className="mx-1 rounded bg-turd-bg-deep/60 px-1.5 py-0.5">
              C:/Development/Claude/scumdump/
            </code>
            (or your <code>$SCUMDUMP_ROOT</code> override). Clone it from
            the turdmod org and re-launch the Manager.
          </p>
        </Card>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <header>
        <p className="font-display text-xs uppercase tracking-[0.4em] text-turd-mustard">
          Builder · SCUM Game Database
        </p>
        <h1 className="mt-1 font-display text-3xl text-turd-cream md:text-4xl">
          Dump Management
        </h1>
        <p className="mt-2 max-w-2xl text-sm text-turd-cream-dim">
          Run the three-phase SCUM data extraction pipeline (live UE4SS
          reflection, Dumper-7 SDK headers, CUE4Parse pak content) and
          inspect the resulting on-disk database under{' '}
          <code className="rounded bg-turd-bg-deep/60 px-1.5 py-0.5">
            scumdump/data/extracted/
          </code>
          .
        </p>
      </header>

      {updateBanner}

      <Card title="SCUM Server Build Status">
        <dl className="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-1.5 text-sm">
          <dt className="text-turd-cream-dim">Server Steam build:</dt>
          <dd className="text-turd-cream">
            {s?.steamBuild ?? '—'}{' '}
            <span className="text-turd-cream-dim/60">
              (Steam app 3792580)
            </span>
          </dd>
          <dt className="text-turd-cream-dim">Client Steam build:</dt>
          <dd className="text-turd-cream">
            {s?.steamClientBuild ?? '—'}{' '}
            <span className="text-turd-cream-dim/60">
              (Steam app 513710)
            </span>
          </dd>
          <dt className="text-turd-cream-dim">Latest extracted:</dt>
          <dd className="text-turd-cream">
            {s?.extractedBuild ? `v${s.extractedBuild}` : '—'}
          </dd>
          <dt className="text-turd-cream-dim">Status:</dt>
          <dd
            className={
              s?.updateAvailable
                ? 'text-turd-mustard-bright'
                : 'text-emerald-400'
            }
          >
            {s?.updateAvailable ? '⚠ Update available' : '✓ Up to date'}
          </dd>
          <dt className="text-turd-cream-dim">AES key fingerprint:</dt>
          <dd className="font-mono text-turd-cream">
            {s?.aesFingerprint ?? '—'}
          </dd>
          <dt className="text-turd-cream-dim">Build dir:</dt>
          <dd className="font-mono text-xs text-turd-cream-dim/80">
            {s?.latestBuildDir ?? '—'}
          </dd>
        </dl>
      </Card>

      <CurrentPhaseCard status={activeStatus} />

      <div className="grid gap-6 md:grid-cols-2 xl:grid-cols-4">
        <Card title="Phase A — Live Reflection">
          <dl className="space-y-1 text-sm">
            <div className="flex justify-between">
              <dt className="text-turd-cream-dim">Last run:</dt>
              <dd className="text-turd-cream">
                {formatDate(s?.phaseADumpedAt)}
              </dd>
            </div>
            <div className="flex justify-between">
              <dt className="text-turd-cream-dim">Classes:</dt>
              <dd className="font-mono text-turd-cream">
                {s?.phaseA?.classes?.count?.toLocaleString() ?? '—'}
              </dd>
            </div>
            <div className="flex justify-between">
              <dt className="text-turd-cream-dim">Enums:</dt>
              <dd className="font-mono text-turd-cream">
                {s?.phaseA?.enums?.count?.toLocaleString() ?? '—'}
              </dd>
            </div>
            <div className="flex justify-between">
              <dt className="text-turd-cream-dim">Structs:</dt>
              <dd className="font-mono text-turd-cream">
                {s?.phaseA?.structs?.count?.toLocaleString() ?? '—'}
              </dd>
            </div>
          </dl>
          <p className="mt-3 text-xs text-turd-cream-dim/80">
            Requires engine running (bridge RPCs).
          </p>
          <div className="mt-3">
            <Button
              onClick={() => runPhase.mutate('phase-a')}
              disabled={anyRunning}
            >
              Run Phase A
            </Button>
          </div>
        </Card>

        <Card title="Phase B — SDK Headers (Server)">
          <dl className="space-y-1 text-sm">
            <div className="flex justify-between">
              <dt className="text-turd-cream-dim">Last run:</dt>
              <dd className="text-turd-cream">
                {formatDate(s?.phaseB?.dumpedAt)}
              </dd>
            </div>
            <div className="flex justify-between">
              <dt className="text-turd-cream-dim">Headers:</dt>
              <dd className="font-mono text-turd-cream">
                {s?.phaseB?.fileCount?.toLocaleString() ?? '—'}
              </dd>
            </div>
            <div className="flex justify-between">
              <dt className="text-turd-cream-dim">Total size:</dt>
              <dd className="font-mono text-turd-cream">
                {s?.phaseB?.byteCount != null ? formatBytes(s.phaseB.byteCount) : '—'}
              </dd>
            </div>
          </dl>
          <p className="mt-3 text-xs text-turd-cream-dim/80">
            Dumper-7 inject; target GameServer.exe.
          </p>
          <div className="mt-3">
            <Button
              onClick={() => runPhase.mutate('phase-b')}
              disabled={anyRunning}
            >
              Run Phase B
            </Button>
          </div>
        </Card>

        <Card title="Phase B — SDK Headers (Client)">
          <dl className="space-y-1 text-sm">
            <div className="flex justify-between">
              <dt className="text-turd-cream-dim">Last run:</dt>
              <dd className="text-turd-cream">
                {formatDate(s?.phaseBClient?.dumpedAt)}
              </dd>
            </div>
            <div className="flex justify-between">
              <dt className="text-turd-cream-dim">Headers:</dt>
              <dd className="font-mono text-turd-cream">
                {s?.phaseBClient?.fileCount?.toLocaleString() ?? '—'}
              </dd>
            </div>
            <div className="flex justify-between">
              <dt className="text-turd-cream-dim">Total size:</dt>
              <dd className="font-mono text-turd-cream">
                {s?.phaseBClient?.byteCount != null
                  ? formatBytes(s.phaseBClient.byteCount)
                  : '—'}
              </dd>
            </div>
          </dl>
          <p className="mt-3 text-xs text-turd-cream-dim/80">
            Dumper-7 inject; target SCUM.exe (client). Render/UI/input classes.
          </p>
          <div className="mt-3">
            <Button
              onClick={() => runPhase.mutate('phase-b-client')}
              disabled={anyRunning}
            >
              Run Phase B (Client)
            </Button>
          </div>
        </Card>

        <Card title="Phase C — Pak Content">
          <dl className="space-y-1 text-sm">
            <div className="flex justify-between">
              <dt className="text-turd-cream-dim">Last run:</dt>
              <dd className="text-turd-cream">
                {formatDate(s?.phaseC?.dumpedAt)}
              </dd>
            </div>
            <div className="flex justify-between">
              <dt className="text-turd-cream-dim">Widgets:</dt>
              <dd className="font-mono text-turd-cream">
                {s?.phaseC?.widgets?.count?.toLocaleString() ?? '—'}
              </dd>
            </div>
            <div className="flex justify-between">
              <dt className="text-turd-cream-dim">DataTables:</dt>
              <dd className="font-mono text-turd-cream">
                {s?.phaseC?.datatables?.count?.toLocaleString() ?? '—'}
              </dd>
            </div>
            <div className="flex justify-between">
              <dt className="text-turd-cream-dim">Strings:</dt>
              <dd className="font-mono text-turd-cream">
                {s?.phaseC?.strings?.count?.toLocaleString() ?? '—'}{' '}
                <span className="text-turd-cream-dim">
                  ({s?.phaseC?.strings?.bytes != null ? formatBytes(s.phaseC.strings.bytes) : '—'})
                </span>
              </dd>
            </div>
          </dl>
          <p className="mt-3 text-xs text-turd-cream-dim/80">
            Uses AES key. Engine not required.
          </p>
          <div className="mt-3">
            <Button
              onClick={() => runPhase.mutate('phase-c')}
              disabled={anyRunning}
            >
              Run Phase C
            </Button>
          </div>
        </Card>
      </div>

      <Card title="Diff vs previous build">
        {builds.data && builds.data.length > 1 && (
          <div className="mb-3 flex items-center gap-2 text-xs text-turd-cream-dim">
            <label htmlFor="diff-build">Show diff for build:</label>
            <select
              id="diff-build"
              value={selectedDiffBuild ?? ''}
              onChange={(e) =>
                setSelectedDiffBuild(e.target.value || undefined)
              }
              className="rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-2 py-1 text-xs text-turd-cream focus:border-turd-mustard-bright focus:outline-none"
            >
              <option value="">latest (v{builds.data[0]})</option>
              {builds.data.map((b) => (
                <option key={b} value={b}>v{b}</option>
              ))}
            </select>
          </div>
        )}
        {diff.data ? (
          <div className="space-y-3">
            <p className="text-sm text-turd-cream-dim">
              v{diff.data.previousBuild} → v{diff.data.currentBuild} · computed{' '}
              {formatDate(diff.data.computedAt)}
            </p>
            <dl className="space-y-1">
              <DiffCounts d={diff.data.phaseA?.classes} label="Classes (server reflection)" />
              <DiffCounts d={diff.data.phaseA?.enums} label="Enums" />
              <DiffCounts d={diff.data.phaseA?.structs} label="Structs" />
              <DiffCounts d={diff.data.phaseB} label="SDK files (server)" />
              <DiffCounts d={diff.data.phaseBClient} label="SDK files (client)" />
              <DiffCounts d={diff.data.phaseC?.widgets} label="Widgets" />
              <DiffCounts d={diff.data.phaseC?.datatables} label="DataTables" />
              <DiffCounts d={diff.data.phaseC?.strings} label="Strings (locres files)" />
            </dl>
            {diff.data.notes.length > 0 && (
              <details className="mt-2 text-xs text-turd-cream-dim/80">
                <summary className="cursor-pointer">
                  Notes ({diff.data.notes.length})
                </summary>
                <ul className="mt-1 list-disc pl-5">
                  {diff.data.notes.map((n, i) => (
                    <li key={i}>{n}</li>
                  ))}
                </ul>
              </details>
            )}
            {assistSettings.enabled && (
              <div className="mt-3">
                <button
                  onClick={() => explainDiff.mutate()}
                  disabled={explainDiff.isPending}
                  className="rounded border border-turd-mustard/40 bg-turd-mustard/20 px-3 py-1.5 text-xs font-medium text-turd-mustard-bright hover:bg-turd-mustard/30 disabled:opacity-40"
                >
                  {explainDiff.isPending
                    ? `Asking ${assistSettings.model}…`
                    : `🧠 Explain this diff (via ${assistSettings.model})`}
                </button>
                {diffExplanation && (
                  <pre className="mt-3 max-h-72 overflow-auto whitespace-pre-wrap rounded border border-turd-bronze/30 bg-turd-bg-deep/60 p-3 text-xs text-turd-cream">
                    {diffExplanation}
                  </pre>
                )}
              </div>
            )}
          </div>
        ) : (
          <p className="text-sm text-turd-cream-dim">
            No diff computed yet. Run{' '}
            <code className="rounded bg-turd-bg-deep/60 px-1.5 py-0.5">
              scumdump diff &lt;prev&gt; &lt;curr&gt;
            </code>{' '}
            (or "Run All Phases" — auto-diffs against the immediately
            prior build). The diff lands at{' '}
            <code className="rounded bg-turd-bg-deep/60 px-1.5 py-0.5">
              {`<build>/_diff.json`}
            </code>
            .
          </p>
        )}
      </Card>

      <SnapshotsCard />

      <Card title="Composite actions">
        <div className="flex flex-wrap gap-3">
          <Button
            variant="primary"
            onClick={() => runAll.mutate()}
            disabled={anyRunning}
          >
            Run All Phases (A → B → C)
          </Button>
          <Button onClick={() => extractAes.mutate()} disabled={anyRunning}>
            Re-extract AES key
          </Button>
          <Button
            onClick={() => openFolder.mutate()}
            disabled={!s?.latestBuildDir}
          >
            Open Dump Folder
          </Button>
        </div>
        {runPhase.isError && (
          <p className="mt-3 text-xs text-red-400">
            Phase failed: {String(runPhase.error)}
          </p>
        )}
        {runAll.isError && (
          <p className="mt-3 text-xs text-red-400">
            Run All failed: {String(runAll.error)}
          </p>
        )}
      </Card>

      <Card title="Log">
        <div className="mb-2 flex items-center justify-between">
          <span className="text-xs text-turd-cream-dim">
            {lines.length} lines · streaming from `dump://log`
          </span>
          <button
            onClick={clear}
            className="text-xs text-turd-cream-dim hover:text-turd-cream"
          >
            Clear
          </button>
        </div>
        <div
          ref={logRef}
          className="h-72 overflow-auto rounded border border-turd-bronze/30 bg-turd-bg-deep/60 p-3 font-mono text-xs leading-relaxed"
        >
          {lines.length === 0 ? (
            <span className="text-turd-cream-dim/60">
              No output yet. Click a phase button to start.
            </span>
          ) : (
            lines.map((l, i) => {
              const isStatus = !!l.statusGroup;
              // Status lines always render in the live "this is updating"
              // mustard tone; other lines get tone by content classification.
              const toneCls = isStatus
                ? 'text-turd-mustard/90 italic'
                : TONE_CLASS[classifyLineTone(l.line, l.stream)];
              return (
                <div
                  key={isStatus ? `status:${l.statusGroup}` : i}
                  className={toneCls}
                >
                  <span className="text-turd-cream-dim/70">[{l.ts}]</span>{' '}
                  <span className="text-turd-mustard/60">[{l.phase}]</span>{' '}
                  {isStatus && (
                    <span className="animate-pulse text-cyan-400">⟳</span>
                  )}{' '}
                  {l.line}
                </div>
              );
            })
          )}
        </div>
      </Card>
    </div>
  );
}

export default DumpManagementPage;
