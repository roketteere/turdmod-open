import { useState } from 'react';
import { useWizard } from '../WizardContext';
import { settingsTestRcon } from '../../lib/tauri-settings';

export function StepRcon() {
  const { state, dispatch, next, prev } = useWizard();
  const { skipRcon, rconHost, rconPort, rconPassword, rconTestResult } = state;

  const [testing, setTesting] = useState(false);

  const handleTest = async () => {
    const port = parseInt(rconPort, 10);
    if (!rconHost.trim() || isNaN(port)) return;
    setTesting(true);
    dispatch({ type: 'SET_RCON_TEST', result: null });
    try {
      const result = await settingsTestRcon(rconHost.trim(), port, rconPassword);
      dispatch({ type: 'SET_RCON_TEST', result });
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
          RCON lets the companion send admin commands to your server — ban
          players, send messages, run events. You can skip this and add it
          later.
        </p>

        <div className="flex items-center gap-3">
          <button
            type="button"
            role="switch"
            aria-checked={skipRcon}
            onClick={() => dispatch({ type: 'SET_SKIP_RCON', skip: !skipRcon })}
            className={[
              'relative inline-flex h-6 w-11 shrink-0 items-center rounded-full border-2 transition-colors',
              skipRcon
                ? 'border-turd-bronze/40 bg-turd-bronze/20'
                : 'border-turd-mustard/60 bg-turd-mustard/20',
            ].join(' ')}
          >
            <span
              className={[
                'inline-block h-4 w-4 rounded-full bg-turd-cream transition-transform',
                skipRcon ? 'translate-x-1' : 'translate-x-5',
              ].join(' ')}
            />
          </button>
          <span className="text-sm text-turd-cream-dim">
            {skipRcon ? 'Skipping RCON — set it up in Settings later' : 'Configure RCON now'}
          </span>
        </div>

        {!skipRcon && (
          <div className="space-y-3">
            <div className="grid grid-cols-2 gap-3">
              <label className="flex flex-col gap-1">
                <span className="font-display text-[10px] uppercase tracking-wider text-turd-cream-dim">
                  Host
                </span>
                <input
                  type="text"
                  value={rconHost}
                  onChange={(e) => dispatch({ type: 'SET_RCON_HOST', host: e.target.value })}
                  placeholder="127.0.0.1"
                  className={inputCls}
                />
              </label>
              <label className="flex flex-col gap-1">
                <span className="font-display text-[10px] uppercase tracking-wider text-turd-cream-dim">
                  Port
                </span>
                <input
                  type="number"
                  value={rconPort}
                  onChange={(e) => dispatch({ type: 'SET_RCON_PORT', port: e.target.value })}
                  min={1}
                  max={65535}
                  className={inputCls}
                />
              </label>
              <label className="col-span-2 flex flex-col gap-1">
                <span className="font-display text-[10px] uppercase tracking-wider text-turd-cream-dim">
                  Password{' '}
                  <span className="ml-1 normal-case tracking-normal text-turd-cream-dim/60">
                    (stored in OS keychain, never on disk)
                  </span>
                </span>
                <input
                  type="password"
                  value={rconPassword}
                  onChange={(e) => dispatch({ type: 'SET_RCON_PASSWORD', password: e.target.value })}
                  placeholder="RCON password"
                  className={inputCls}
                />
              </label>
            </div>

            <div className="flex items-center gap-3">
              <button
                type="button"
                onClick={handleTest}
                disabled={testing || !rconHost.trim()}
                className="rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-1.5 font-display text-xs uppercase tracking-wider text-turd-mustard-bright transition-colors hover:border-turd-mustard disabled:opacity-40"
              >
                {testing ? 'Testing…' : 'Test connection'}
              </button>
              {rconTestResult && (
                <span className="font-mono text-xs">
                  {rconTestResult.ok ? (
                    <span className="text-turd-green">Connection OK</span>
                  ) : (
                    <span className="text-turd-red">{rconTestResult.error ?? 'Failed'}</span>
                  )}
                </span>
              )}
            </div>

            {rconTestResult && !rconTestResult.ok && (
              <p className="text-xs text-turd-cream-dim">
                Test failed — but that's OK. You can still continue and fix
                this in Settings once the server is running.
              </p>
            )}
          </div>
        )}
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
