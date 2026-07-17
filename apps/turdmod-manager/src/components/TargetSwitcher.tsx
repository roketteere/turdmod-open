// Segmented toggle in the sidebar — switches between the SCUM client install
// and the SCUM Dedicated Server install. Disabled segments show "(not found)"
// with a tooltip pointing at Settings → install path.

import { useActiveTarget, useDetectedInstalls, useSetActiveTarget } from '../hooks/useTarget';
import type { InstallTarget } from '../lib/tauri-target';

const TARGETS: { value: InstallTarget; label: string }[] = [
  { value: 'game', label: 'Client' },
  { value: 'server', label: 'Server' },
];

export function TargetSwitcher() {
  const installsQuery = useDetectedInstalls();
  const targetQuery = useActiveTarget();
  const setTarget = useSetActiveTarget();

  const installs = installsQuery.data;
  const activeTarget = targetQuery.data;

  return (
    <div className="px-2 pb-3">
      <p className="mb-1.5 font-mono text-[9px] uppercase tracking-widest text-turd-cream-dim/60">
        Target
      </p>
      <div
        role="tablist"
        aria-label="Active install target"
        className="flex overflow-hidden rounded border border-turd-bronze/30"
      >
        {TARGETS.map(({ value, label }) => {
          const isActive = activeTarget === value;
          const isDetected = installs ? installs[value] !== null : false;
          const isLoading = installsQuery.isPending || targetQuery.isPending;
          const isDisabled = !isDetected || setTarget.isPending || isLoading;

          const tooltip = !isDetected
            ? `${label} install not detected — set path in Settings`
            : isActive
              ? `Currently active: ${label}`
              : `Switch to ${label} install`;

          return (
            <button
              key={value}
              role="tab"
              type="button"
              aria-selected={isActive}
              disabled={isDisabled}
              title={tooltip}
              onClick={() => {
                if (!isActive && isDetected) setTarget.mutate(value);
              }}
              className={[
                'flex flex-1 items-center justify-center gap-1.5 py-1.5 font-display text-[10px] uppercase tracking-wider transition-colors',
                'border-r border-turd-bronze/30 last:border-r-0',
                'disabled:cursor-not-allowed',
                isActive
                  ? 'bg-turd-mustard-bright text-turd-bg-deep font-semibold'
                  : isDetected
                    ? 'text-turd-cream-dim hover:bg-turd-bg-soft/40 hover:text-turd-cream'
                    : 'text-turd-cream-dim/30',
              ].join(' ')}
            >
              {/* Active-state dot — high-contrast visual cue that the
                  text-only highlight alone wasn't providing. */}
              <span
                className={[
                  'h-1.5 w-1.5 rounded-full',
                  isActive
                    ? 'bg-turd-bg-deep'
                    : isDetected
                      ? 'bg-turd-cream-dim/30'
                      : 'bg-turd-cream-dim/20',
                ].join(' ')}
              />
              {label}
              {!isDetected && (
                <span className="text-turd-cream-dim/30">*</span>
              )}
            </button>
          );
        })}
      </div>
      {setTarget.isError && (
        <p className="mt-1 text-[9px] text-turd-red">
          Switch failed: {setTarget.error?.message}
        </p>
      )}
    </div>
  );
}
