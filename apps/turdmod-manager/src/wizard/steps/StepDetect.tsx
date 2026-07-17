import { useEffect, useState } from 'react';
import { useWizard } from '../WizardContext';
import { listDetectedInstalls, type InstallTarget } from '../../lib/tauri-target';

export function StepDetect() {
  const { state, dispatch, next, prev } = useWizard();
  const { installs, target, installPath } = state;

  const [loading, setLoading] = useState(installs === null);
  const [pickError, setPickError] = useState<string | null>(null);

  useEffect(() => {
    if (installs !== null) return;
    setLoading(true);
    listDetectedInstalls()
      .then((found) => {
        dispatch({ type: 'SET_INSTALLS', installs: found });
        const defaultTarget: InstallTarget = found.server
          ? 'server'
          : found.game
            ? 'game'
            : 'server';
        dispatch({ type: 'SET_TARGET', target: defaultTarget });
        dispatch({ type: 'SET_INSTALL_PATH', path: found[defaultTarget] ?? null });
      })
      .catch(() => {
        dispatch({ type: 'SET_INSTALLS', installs: { game: null, server: null } });
      })
      .finally(() => setLoading(false));
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const handlePickManual = async () => {
    setPickError(null);
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const picked = await open({
        directory: true,
        multiple: false,
        title: 'Select your SCUM install folder',
      });
      if (typeof picked === 'string') {
        dispatch({ type: 'SET_INSTALL_PATH', path: picked });
      }
    } catch (e) {
      setPickError(e instanceof Error ? e.message : String(e));
    }
  };

  const handleTargetSelect = (t: InstallTarget) => {
    dispatch({ type: 'SET_TARGET', target: t });
    dispatch({ type: 'SET_INSTALL_PATH', path: installs?.[t] ?? null });
  };

  const nothingDetected = installs ? !installs.game && !installs.server : false;

  return (
    <div className="flex h-full flex-col">
      <div className="flex-1 space-y-5">
        {loading && (
          <p className="text-sm text-turd-cream-dim">Scanning for SCUM installs…</p>
        )}

        {!loading && installs && (
          <>
            {installs.game && installs.server && (
              <div className="space-y-3">
                <p className="text-sm text-turd-cream-dim">
                  Found both a SCUM client and a dedicated server installed
                  locally. Which one should the companion watch? (For remote
                  hosts like g-portal, skip this and use the Servers page.)
                </p>
                <div className="flex gap-3">
                  {(['server', 'game'] as InstallTarget[]).map((t) => (
                    <button
                      key={t}
                      type="button"
                      onClick={() => handleTargetSelect(t)}
                      className={[
                        'flex-1 rounded border px-4 py-3 font-display text-xs uppercase tracking-wider transition-colors',
                        target === t
                          ? 'border-turd-mustard bg-turd-bronze/30 text-turd-mustard-bright'
                          : 'border-turd-bronze/40 text-turd-cream-dim hover:border-turd-cream hover:text-turd-cream',
                      ].join(' ')}
                    >
                      {t === 'server' ? 'Dedicated Server' : 'SCUM Client'}
                    </button>
                  ))}
                </div>
              </div>
            )}

            {(installs.server || installs.game) && !(installs.server && installs.game) && (
              <div className="flex items-start gap-3 rounded-lg border border-turd-green/30 bg-turd-green/5 px-4 py-3">
                <span className="mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-turd-green/20 font-mono text-xs text-turd-green">
                  ✓
                </span>
                <div>
                  <p className="text-sm text-turd-cream">
                    {installs.server ? 'Dedicated server' : 'SCUM client'} detected
                  </p>
                  <p className="mt-0.5 break-all font-mono text-xs text-turd-cream-dim">
                    {installs.server ?? installs.game}
                  </p>
                </div>
              </div>
            )}

            {nothingDetected && (
              <div className="space-y-3">
                <div className="rounded-lg border border-turd-bronze/30 bg-turd-bg-soft/40 px-4 py-3">
                  <p className="text-sm text-turd-cream">
                    Couldn't find SCUM automatically.
                  </p>
                  <p className="mt-1 text-xs text-turd-cream-dim">
                    If SCUM is installed, pick the folder manually below. You
                    can also skip and set this in Settings later.
                  </p>
                </div>
                <button
                  type="button"
                  onClick={handlePickManual}
                  className="rounded border border-turd-bronze/60 bg-turd-bg-soft px-4 py-2 font-display text-xs uppercase tracking-wider text-turd-cream transition-colors hover:border-turd-cream hover:text-turd-cream"
                >
                  Browse for install folder…
                </button>
              </div>
            )}

            {installPath && nothingDetected && (
              <div className="flex items-start gap-3 rounded-lg border border-turd-green/30 bg-turd-green/5 px-4 py-3">
                <span className="mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-turd-green/20 font-mono text-xs text-turd-green">
                  ✓
                </span>
                <div>
                  <p className="text-sm text-turd-cream">Install folder set</p>
                  <p className="mt-0.5 break-all font-mono text-xs text-turd-cream-dim">{installPath}</p>
                  <button
                    type="button"
                    onClick={handlePickManual}
                    className="mt-1.5 text-xs text-turd-mustard hover:text-turd-mustard-bright"
                  >
                    Change…
                  </button>
                </div>
              </div>
            )}

            {pickError && <p className="text-xs text-turd-red">{pickError}</p>}
          </>
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
          disabled={loading}
          className="rounded border border-turd-mustard bg-turd-bg-soft px-8 py-2.5 font-display text-xs uppercase tracking-wider text-turd-mustard-bright transition-colors hover:bg-turd-bronze/40 disabled:opacity-40 focus:outline-none focus:ring-2 focus:ring-turd-mustard/50"
        >
          {installPath || nothingDetected ? 'Next' : 'Skip'}
        </button>
      </div>
    </div>
  );
}
