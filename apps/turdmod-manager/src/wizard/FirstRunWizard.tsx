// Full-screen wizard overlay. Replaces the normal layout while
// wizard_completed === false. Esc prompts to skip.

import { useCallback, useEffect, useState } from 'react';
import { WizardProvider, useWizard } from './WizardContext';
import { StepWelcome } from './steps/StepWelcome';
import { StepDetect } from './steps/StepDetect';
import { StepLogsDir } from './steps/StepLogsDir';
import { StepRcon } from './steps/StepRcon';
import { StepFinish } from './steps/StepFinish';
import { settingsSetPartial } from '../lib/tauri-settings';

interface FirstRunWizardProps {
  onDone: () => void;
}

// Persist wizardCompleted=true so the skipped wizard doesn't reappear on
// every reload. Best-effort: if the write fails we still close the wizard
// (the user explicitly chose to skip; trapping them on a save error is
// worse than the wizard re-appearing once).
async function skipWizard(onDone: () => void) {
  try {
    await settingsSetPartial({ wizardCompleted: true });
  } catch {
    // ignore — close anyway
  }
  onDone();
}

const STEP_META = [
  { label: 'Welcome' },
  { label: 'Find SCUM' },
  { label: 'Logs Directory' },
  { label: 'RCON (optional)' },
  { label: 'Finish' },
];

function WizardShell({ onDone }: FirstRunWizardProps) {
  const { state, stepCount } = useWizard();
  const { step } = state;

  const [escPrompt, setEscPrompt] = useState(false);

  const handleKeyDown = useCallback((e: KeyboardEvent) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      setEscPrompt(true);
    }
  }, []);

  useEffect(() => {
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [handleKeyDown]);

  const steps = [
    <StepWelcome key="welcome" />,
    <StepDetect key="detect" />,
    <StepLogsDir key="logs" />,
    <StepRcon key="rcon" />,
    <StepFinish key="finish" onDone={onDone} />,
  ];

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-turd-bg-deep">
      {escPrompt && (
        <div className="fixed inset-0 z-[60] flex items-center justify-center bg-black/60">
          <div className="rounded-xl border border-turd-bronze/40 bg-turd-bg-mid p-8 shadow-2xl">
            <p className="mb-2 font-display text-lg text-turd-cream">
              Skip first-run setup?
            </p>
            <p className="mb-6 text-sm text-turd-cream-dim">
              You can re-run this wizard any time from Settings.
            </p>
            <div className="flex gap-3">
              <button
                type="button"
                autoFocus
                onClick={() => setEscPrompt(false)}
                className="flex-1 rounded border border-turd-bronze/60 px-4 py-2 font-display text-xs uppercase tracking-wider text-turd-cream transition-colors hover:border-turd-cream"
              >
                Continue setup
              </button>
              <button
                type="button"
                onClick={() => skipWizard(onDone)}
                className="flex-1 rounded bg-turd-bronze/40 px-4 py-2 font-display text-xs uppercase tracking-wider text-turd-cream-dim transition-colors hover:text-turd-cream"
              >
                Skip for now
              </button>
            </div>
          </div>
        </div>
      )}

      <div className="flex w-full max-w-lg flex-col overflow-hidden rounded-2xl border border-turd-bronze/30 bg-turd-bg-mid shadow-2xl">
        <div className="flex items-center justify-between border-b border-turd-bronze/20 px-8 py-5">
          <div>
            <p className="font-display text-xs tracking-[0.3em] text-turd-mustard">
              TurdMOD Manager
            </p>
            <p className="mt-0.5 font-display text-xl text-turd-cream">
              {STEP_META[step]?.label ?? ''}
            </p>
          </div>
          <div className="flex flex-col items-end gap-2">
            <button
              type="button"
              onClick={() => setEscPrompt(true)}
              aria-label="Close first-run setup"
              title="Skip setup (you can re-run from Settings later)"
              className="font-display text-[10px] uppercase tracking-wider text-turd-cream-dim/60 transition-colors hover:text-turd-cream"
            >
              Skip ×
            </button>
            <div className="flex items-center gap-2" aria-label={`Step ${step + 1} of ${stepCount}`}>
              {Array.from({ length: stepCount }).map((_, i) => (
                <span
                  key={i}
                  aria-hidden
                  className={[
                    'h-2 w-2 rounded-full transition-colors',
                    i < step
                      ? 'bg-turd-mustard/60'
                      : i === step
                        ? 'bg-turd-mustard-bright'
                        : 'bg-turd-bronze/30',
                  ].join(' ')}
                />
              ))}
            </div>
          </div>
        </div>

        <div className="min-h-[340px] px-8 py-7">{steps[step]}</div>
      </div>
    </div>
  );
}

export function FirstRunWizard({ onDone }: FirstRunWizardProps) {
  return (
    <WizardProvider>
      <WizardShell onDone={onDone} />
    </WizardProvider>
  );
}
