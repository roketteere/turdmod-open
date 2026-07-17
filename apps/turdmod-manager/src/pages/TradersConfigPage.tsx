// Traders — dedicated Server Config tab. LIVE control panel (not a generic file builder):
//   • Funds: real per-trader available_funds from economy_traders (save DB) + one-click refill
//     to 99,999 (live, via the traderFunds bridge scan→set). Auto-refilled hourly by trader_refill.
//   • Economy + per-trader item overrides: the full EconomyOverride.json surface (unlimited
//     stock/funds, rotation, restock, gold, per-trader tradeables with in-game icons), embedded.
// Replaces the old floating map economy panel (TradersPanel) — trader config now lives HERE.
// @dep scumdb_rows('economy_traders'); engineRpc('traderFunds'); EconomyOverride (embedded).
import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useEngineHost } from '../lib/engineHost';
import { engineRpc } from '../lib/tauri-engine';
import { HostToggle } from '../components/HostToggle';
import EconomyOverride from '../components/EconomyOverride';

const TARGET_FUNDS = 99999;

interface TraderRow { id: number; available_funds: number; trader_runtime_id: string }
interface RowsResp { rows: TraderRow[] }
interface ScanResp { ok: boolean; traders: number; candidates: { offset: number; value: number }[] }

type Section = 'funds' | 'economy';

export function TradersConfigPage() {
  const target = useEngineHost();
  const host = target === 'local' ? 'Local' : 'Remote';
  const [section, setSection] = useState<Section>('funds');
  const [rows, setRows] = useState<TraderRow[]>([]);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [amount, setAmount] = useState(TARGET_FUNDS);

  const refresh = useCallback(async () => {
    try {
      const r = await invoke<RowsResp>('scumdb_rows', { target, table: 'economy_traders', limit: 100, offset: 0 });
      setRows((r.rows ?? []).slice().sort((a, b) => a.id - b.id));
      setErr(null);
    } catch (e) { setRows([]); setErr(String(e)); }
  }, [target]);

  useEffect(() => { refresh(); const id = setInterval(refresh, 15000); return () => clearInterval(id); }, [refresh]);

  const flash = (m: string, ms = 7000) => { setMsg(m); setTimeout(() => setMsg(null), ms); };

  const refillAll = async () => {
    if (!window.confirm(`Set ALL traders' funds to ${amount.toLocaleString()} on ${host} now? (live)`)) return;
    setBusy(true);
    try {
      const scan = await engineRpc<ScanResp>('traderFunds', { mode: 'scan' });
      const cands = scan.candidates ?? [];
      if (cands.length !== 1) {
        flash(`✗ scan found ${cands.length} fund-offset candidates (need exactly 1) — can't safely auto-set.`, 12000);
        return;
      }
      const res = await engineRpc<{ ok: boolean; traders: number }>('traderFunds', {
        mode: 'set', offset: String(cands[0].offset), value: String(amount),
      });
      flash(`✓ set ${res.traders ?? 0} traders to ${amount.toLocaleString()} (offset ${cands[0].offset})`);
      setTimeout(refresh, 1500);
    } catch (e) { flash(`✗ refill failed: ${String(e)}`, 12000); }
    finally { setBusy(false); }
  };

  const funded = rows.filter((t) => t.available_funds >= TARGET_FUNDS).length;
  const tabCls = (s: Section) =>
    `px-4 py-2 text-sm font-semibold border-b-2 transition-colors ${section === s ? 'border-turd-bronze text-turd-cream' : 'border-transparent text-turd-cream-dim hover:text-turd-cream'}`;
  const inputCls = 'rounded border border-turd-bronze/50 bg-turd-bg-deep/70 px-2 py-1 text-sm text-turd-cream focus:border-turd-mustard focus:outline-none';

  return (
    <div className="flex h-full min-h-0 flex-col bg-turd-bg-deep text-turd-cream">
      <header className="shrink-0 px-6 pt-6 pb-3">
        <div className="flex items-center justify-between gap-3">
          <div>
            <p className="font-display text-xs tracking-[0.3em] text-turd-mustard">TurdMOD · Server Config</p>
            <h1 className="mt-0.5 font-display text-2xl text-turd-cream">Traders</h1>
            <p className="mt-1 text-xs text-turd-cream-dim">
              Live trader funds, economy rules &amp; per-trader item overrides for{' '}
              <span className="text-turd-cream">{host}</span>. Settings apply on next restart; funds refill is live.
            </p>
          </div>
          <HostToggle />
        </div>
        <div className="mt-3 flex gap-1 border-b border-turd-bg-soft">
          <button className={tabCls('funds')} onClick={() => setSection('funds')}>
            Funds &amp; Roster
            <span className="ml-1.5 rounded bg-turd-bg-soft px-1.5 py-0.5 text-[10px] text-turd-cream-dim">{rows.length}</span>
          </button>
          <button className={tabCls('economy')} onClick={() => setSection('economy')}>Economy &amp; Items</button>
        </div>
      </header>

      <div className="min-h-0 flex-1 overflow-auto px-6 pb-6">
        {section === 'funds' ? (
          <div className="flex flex-col gap-4">
            <div className="flex flex-wrap items-center gap-3 rounded border border-turd-bronze/20 bg-turd-bg-mid/40 px-4 py-3">
              <div className="flex-1">
                <p className="text-sm font-semibold text-turd-cream">Trader funds</p>
                <p className="text-xs text-turd-cream-dim">Cash each trader can spend buying from players. Auto-refills hourly; <span className="text-turd-green">{funded}</span>/{rows.length} at target.</p>
              </div>
              <label className="flex items-center gap-1.5 text-xs text-turd-cream-dim">refill all to
                <input type="number" min={0} max={100000000} step={1000} value={amount} disabled={busy}
                  onChange={(e) => setAmount(Math.max(0, Math.min(100000000, Number(e.target.value) || 0)))}
                  className={`${inputCls} w-28 text-right font-mono`} />
              </label>
              <button disabled={busy} onClick={refillAll}
                className="rounded border border-turd-mustard/50 bg-turd-mustard/15 px-3 py-1.5 text-xs font-bold text-turd-mustard-bright hover:bg-turd-mustard/25 disabled:opacity-40">
                {busy ? '…' : '💰 Refill now (live)'}
              </button>
              <button disabled={busy} onClick={refresh}
                className="rounded border border-turd-bronze/50 bg-turd-bg-deep/70 px-3 py-1.5 text-xs text-turd-cream-dim hover:text-turd-cream">↻ Refresh</button>
            </div>

            {err && <div className="rounded border border-turd-bronze/40 bg-turd-bg-mid px-3 py-2 text-xs text-red-400">⚠ Can’t read traders on {host}: {err}</div>}
            {msg && <div className={`text-xs ${msg.startsWith('✓') ? 'text-turd-green' : 'text-red-400'}`}>{msg}</div>}

            <div className="overflow-hidden rounded border border-turd-bronze/20">
              <table className="w-full border-collapse text-sm">
                <thead>
                  <tr className="bg-turd-bg-mid/60 text-left text-xs uppercase tracking-wider text-turd-cream-dim">
                    <th className="px-3 py-2">#</th>
                    <th className="px-3 py-2">Trader (runtime id)</th>
                    <th className="px-3 py-2 text-right">Available funds</th>
                  </tr>
                </thead>
                <tbody>
                  {rows.map((t) => (
                    <tr key={t.id} className="border-t border-turd-bg-soft/40">
                      <td className="px-3 py-1.5 text-turd-cream-dim">{t.id}</td>
                      <td className="px-3 py-1.5 font-mono text-xs text-turd-cream-dim">{(t.trader_runtime_id ?? '').slice(0, 12)}</td>
                      <td className={`px-3 py-1.5 text-right font-mono ${t.available_funds >= TARGET_FUNDS ? 'text-turd-green' : 'text-turd-mustard-bright'}`}>
                        {Number(t.available_funds ?? 0).toLocaleString()}
                      </td>
                    </tr>
                  ))}
                  {rows.length === 0 && !err && <tr><td colSpan={3} className="px-3 py-4 text-center text-xs text-turd-cream-dim">No traders loaded — server reachable?</td></tr>}
                </tbody>
              </table>
            </div>
            <p className="text-[11px] text-turd-cream-dim/60">Funds shown are from the save DB (refresh after a server save). Per-trader funds are set all-at-once by the bridge; individual amounts are a future refinement.</p>
          </div>
        ) : (
          <EconomyOverride target={target} embedded />
        )}
      </div>
    </div>
  );
}
