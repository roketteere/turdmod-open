// Final wizard step: detect Node, commit all settings, optionally start the
// companion, then close the wizard.

import { useEffect, useState, type MouseEvent } from 'react';
import { useWizard } from '../WizardContext';
import { managerDetectNode, settingsSetPartial } from '../../lib/tauri-settings';
import { companionStart } from '../../lib/tauri-companion';
import { setActiveTarget } from '../../lib/tauri-target';
import { setInstallPath } from '../../lib/tauri';

interface StepFinishProps {
  onDone: () => void;
}

export function StepFinish({ onDone }: StepFinishProps) {
  const { state, dispatch, prev } = useWizard();
  const {
    target,
    installPath,
    logsDir,
    skipRcon,
    rconHost,
    rconPort,
    rconPassword,
    nodeInfo,
    startCompanion,
    committing,
    commitError,
  } = state;

  useEffect(() => {
    if (nodeInfo !== null) return;
    managerDetectNode()
      .then((info) => dispatch({ type: 'SET_NODE_INFO', info }))
      .catch(() =>
        dispatch({ type: 'SET_NODE_INFO', info: { found: false, version: null } }),
      );
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const [companionLaunching, setCompanionLaunching] = useState(false);

  const handleDone = async () => {
    if (committing) return;
    dispatch({ type: 'SET_COMMITTING', committing: true });
    dispatch({ type: 'SET_COMMIT_ERROR', error: null });

    try {
      await setActiveTarget(target);

      // If the user manually browsed for an install path, persist it as the
      // global override so resolve_active_install picks it up.
      if (installPath) {
        await setInstallPath(installPath);
      }

      const port = skipRcon ? undefined : parseInt(rconPort, 10);
      await settingsSetPartial({
        scumServerLogsDir: logsDir.trim() || null,
        rconHost: skipRcon ? undefined : rconHost.trim() || null,
        rconPort: skipRcon
          ? undefined
          : !isNaN(port as number) && (port as number) > 0
            ? port
            : null,
        rconPassword: skipRcon ? undefined : rconPassword,
        wizardCompleted: true,
      });

      if (startCompanion) {
        setCompanionLaunching(true);
        try {
          await companionStart();
        } catch {
          // Non-fatal — companion failure doesn't block wizard completion.
        } finally {
          setCompanionLaunching(false);
        }
      }

      onDone();
    } catch (e) {
      dispatch({
        type: 'SET_COMMIT_ERROR',
        error: e instanceof Error ? e.message : String(e),
      });
    } finally {
      dispatch({ type: 'SET_COMMITTING', committing: false });
    }
  };

  const summaryItems: { label: string; value: string; warn?: boolean }[] = [
    {
      label: 'Target',
      value: target === 'server' ? 'Dedicated Server' : 'SCUM Client',
    },
    {
      label: 'Install path',
      value: installPath ?? '(not set — configure in Settings)',
      warn: !installPath,
    },
    {
      label: 'Logs directory',
      value: logsDir.trim() || '(not set)',
      warn: !logsDir.trim(),
    },
    {
      label: 'RCON',
      value: skipRcon
        ? 'Skipped'
        : rconHost
          ? `${rconHost}:${rconPort}`
          : '(not configured)',
    },
  ];

  return (
    <div className="flex h-full flex-col">
      <div className="flex-1 space-y-5">
        <div className="rounded-lg border border-turd-bronze/20 bg-turd-bg-soft/30 px-4 py-3">
          <p className="mb-3 font-display text-[10px] uppercase tracking-wider text-turd-mustard">
            Summary
          </p>
          <dl className="space-y-2">
            {summaryItems.map(({ label, value, warn }) => (
              <div key={label} className="flex justify-between gap-4">
                <dt className="shrink-0 font-mono text-[10px] uppercase tracking-wider text-turd-cream-dim">
                  {label}
                </dt>
                <dd
                  className={[
                    'break-all text-right font-mono text-xs',
                    warn ? 'text-turd-mustard' : 'text-turd-cream',
                  ].join(' ')}
                >
                  {value}
                </dd>
              </div>
            ))}
          </dl>
        </div>

        <div className="rounded-lg border border-turd-bronze/20 bg-turd-bg-soft/30 px-4 py-3">
          <p className="mb-2 font-display text-[10px] uppercase tracking-wider text-turd-mustard">
            Node.js (required for companion)
          </p>
          {nodeInfo === null ? (
            <p className="text-xs text-turd-cream-dim">Checking…</p>
          ) : nodeInfo.found ? (
            <p className="font-mono text-xs text-turd-green">
              Found — {nodeInfo.version ?? 'version unknown'}
            </p>
          ) : (
            <div className="space-y-1">
              <p className="text-xs text-turd-red">
                Node.js not found on PATH. The companion won't start without it.
              </p>
              <p className="text-xs text-turd-cream-dim">
                Install from <NodeJsLink /> then re-open TurdMOD Manager.
                {' '}(Phase 4 will bundle Node so this step disappears.)
              </p>
            </div>
          )}
        </div>

        {nodeInfo?.found && (
          <label className="flex cursor-pointer items-center gap-3">
            <input
              type="checkbox"
              checked={startCompanion}
              onChange={(e) =>
                dispatch({ type: 'SET_START_COMPANION', start: e.target.checked })
              }
              className="h-4 w-4 rounded border-turd-bronze/60 bg-turd-bg-soft accent-turd-mustard-bright"
            />
            <span className="text-sm text-turd-cream">
              Start companion now when I click Done
            </span>
          </label>
        )}

        {commitError && (
          <div className="rounded border border-turd-red/40 bg-turd-red/5 px-4 py-3">
            <p className="font-display text-[10px] uppercase tracking-wider text-turd-red">
              Save failed
            </p>
            <p className="mt-1 font-mono text-xs text-turd-red">{commitError}</p>
            <p className="mt-1 text-xs text-turd-cream-dim">
              Check that the app has write access to its data directory, then try again.
            </p>
          </div>
        )}
      </div>

      <div className="mt-6 flex justify-between">
        <button
          type="button"
          onClick={prev}
          disabled={committing}
          className="rounded border border-turd-bronze/40 px-5 py-2 font-display text-xs uppercase tracking-wider text-turd-cream-dim transition-colors hover:border-turd-cream hover:text-turd-cream disabled:opacity-40"
        >
          Back
        </button>
        <button
          type="button"
          onClick={handleDone}
          disabled={committing || companionLaunching}
          className="rounded border border-turd-mustard bg-turd-mustard/10 px-8 py-2.5 font-display text-xs uppercase tracking-wider text-turd-mustard-bright transition-colors hover:bg-turd-bronze/40 disabled:opacity-40 focus:outline-none focus:ring-2 focus:ring-turd-mustard/50"
        >
          {committing || companionLaunching ? 'Saving…' : 'Done — launch Manager'}
        </button>
      </div>
    </div>
  );
}

function NodeJsLink() {
  const handleClick = async (e: MouseEvent) => {
    e.preventDefault();
    try {
      const { open } = await import('@tauri-apps/plugin-shell');
      await open('https://nodejs.org/en/download/');
    } catch {
      // fall through
    }
  };
  return (
    <a
      href="https://nodejs.org/en/download/"
      onClick={handleClick}
      className="text-turd-mustard underline hover:text-turd-mustard-bright"
    >
      nodejs.org
    </a>
  );
}
