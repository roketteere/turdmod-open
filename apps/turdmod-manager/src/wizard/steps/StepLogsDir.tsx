import { useEffect, useState } from 'react';
import { useWizard } from '../WizardContext';
import { settingsTestLogsDir } from '../../lib/tauri-settings';

// SCUM server logs live at <install>/SCUM/Saved/SaveFiles/Logs/
function deriveLogsDir(installPath: string | null): string {
  if (!installPath) return '';
  const base = installPath.replace(/\\/g, '/').replace(/\/$/, '');
  return `${base}/SCUM/Saved/SaveFiles/Logs`.replace(/\//g, '\\');
}

export function StepLogsDir() {
  const { state, dispatch, next, prev } = useWizard();
  const { installPath, logsDir, logsDirTestResult } = state;

  const [testing, setTesting] = useState(false);

  useEffect(() => {
    if (!logsDir && installPath) {
      dispatch({ type: 'SET_LOGS_DIR', dir: deriveLogsDir(installPath) });
    }
  }, [installPath]); // eslint-disable-line react-hooks/exhaustive-deps

  const handleBrowse = async () => {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const picked = await open({
        directory: true,
        multiple: false,
        title: 'Select SCUM server logs directory',
      });
      if (typeof picked === 'string') {
        dispatch({ type: 'SET_LOGS_DIR', dir: picked });
      }
    } catch {
      // user cancelled
    }
  };

  const handleTest = async () => {
    if (!logsDir.trim()) return;
    setTesting(true);
    dispatch({ type: 'SET_LOGS_DIR_TEST', result: null });
    try {
      const result = await settingsTestLogsDir(logsDir.trim());
      dispatch({ type: 'SET_LOGS_DIR_TEST', result });
    } finally {
      setTesting(false);
    }
  };

  const inputCls =
    'rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-2 font-mono text-sm text-turd-cream placeholder:text-turd-cream-dim/60 focus:border-turd-mustard focus:outline-none w-full';

  return (
    <div className="flex h-full flex-col">
      <div className="flex-1 space-y-4">
        <p className="text-sm text-turd-cream-dim">
          The companion watches your SCUM server's log files to detect
          in-game events. Point it at the Logs folder.
        </p>

        {logsDir && !logsDirTestResult && installPath && (
          <div className="rounded border border-turd-mustard/30 bg-turd-mustard/5 px-4 py-3">
            <p className="text-xs text-turd-mustard">
              Auto-suggested from your install path. Hit "Test" to verify, or
              browse to a different location.
            </p>
          </div>
        )}

        <div className="flex gap-2">
          <input
            type="text"
            value={logsDir}
            onChange={(e) => dispatch({ type: 'SET_LOGS_DIR', dir: e.target.value })}
            placeholder="C:\scum-server\SCUM\Saved\SaveFiles\Logs"
            className={inputCls}
            aria-label="Logs directory path"
          />
          <button
            type="button"
            onClick={handleBrowse}
            className="shrink-0 rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-2 font-display text-xs uppercase tracking-wider text-turd-cream-dim transition-colors hover:border-turd-cream hover:text-turd-cream"
          >
            Browse…
          </button>
        </div>

        <div className="flex items-center gap-3">
          <button
            type="button"
            onClick={handleTest}
            disabled={testing || !logsDir.trim()}
            className="rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-1.5 font-display text-xs uppercase tracking-wider text-turd-mustard-bright transition-colors hover:border-turd-mustard disabled:opacity-40"
          >
            {testing ? 'Testing…' : 'Test path'}
          </button>
          {logsDirTestResult && (
            <span className="font-mono text-xs">
              {!logsDirTestResult.exists ? (
                <span className="text-turd-red">Path doesn't exist</span>
              ) : !logsDirTestResult.hasLogFiles ? (
                <span className="text-turd-mustard">
                  Folder exists — no .log files yet (that's OK on a fresh server)
                </span>
              ) : (
                <span className="text-turd-green">Found .log files — looks right</span>
              )}
            </span>
          )}
        </div>

        <p className="text-xs text-turd-cream-dim">
          You can leave this blank for now and set it in Settings once your
          server is running.
        </p>
      </div>

      <div className="mt-6 flex justify-between">
        <button
          type="button"
          onClick={prev}
          className="rounded border border-turd-bronze/40 px-5 py-2 font-display text-xs uppercase tracking-wider text-turd-cream-dim transition-colors hover:border-turd-cream hover:text-turd-cream"
        >
          Back
        </button>
        <button
          type="button"
          onClick={next}
          className="rounded border border-turd-mustard bg-turd-bg-soft px-8 py-2.5 font-display text-xs uppercase tracking-wider text-turd-mustard-bright transition-colors hover:bg-turd-bronze/40 focus:outline-none focus:ring-2 focus:ring-turd-mustard/50"
        >
          Next
        </button>
      </div>
    </div>
  );
}
