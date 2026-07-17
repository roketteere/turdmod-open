// Economy — a real page (nav tab). Auto-loads the selected server's EconomyOverride.json: trader
// funds/stock toggles, multipliers, gold pricing, per-trader item overrides (with game icons), and
// import. Applies on next restart. Reuses the EconomyOverride component in full-page (embedded) mode.
import { useEngineHost } from '../lib/engineHost';
import EconomyOverride from '../components/EconomyOverride';

export function EconomyPage() {
  const target = useEngineHost();
  return (
    <div className="flex h-full flex-col gap-4">
      <header>
        <p className="font-display text-xs tracking-[0.3em] text-turd-mustard">TurdMOD</p>
        <h1 className="mt-0.5 font-display text-2xl text-turd-cream">Economy</h1>
        <p className="mt-1 text-xs text-turd-cream-dim">
          Trader funds/stock, multipliers, gold pricing &amp; per-trader item overrides for{' '}
          <span className="text-turd-cream">{target === 'local' ? 'Local' : 'Remote'}</span>. Applies on next restart.
        </p>
      </header>
      <div className="min-h-0 flex-1 overflow-auto rounded border border-turd-bronze/20 bg-turd-bg-deep/30">
        <EconomyOverride target={target} embedded />
      </div>
    </div>
  );
}
