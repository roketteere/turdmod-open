// Traders tab for the Admin Map — view per-trader available_funds and refill them.
// Funds come from economy_traders (the save DB; updates when SCUM saves). "Refill"
// sets the LIVE in-memory value on all 32 traders via the bridge `traderFunds`
// handler (scan offset -> set), which SCUM then persists. Auto-refills hourly via
// the service trader_refill mod. @dep engineRpc('traderFunds'), invoke('scumdb_rows').
// @ctx 2026-06-17. Per-trader individual edit = future (bridge set is all-at-once).
import { useState, useEffect, useCallback, type CSSProperties } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { engineRpc } from '../lib/tauri-engine';
import DraggablePanel from './DraggablePanel';

const TARGET_FUNDS = 99999;

interface TraderRow { id: number; available_funds: number; trader_runtime_id: string }
interface RowsResp { columns: string[]; rows: TraderRow[]; total: number }
interface ScanResp { ok: boolean; traders: number; candidates: { offset: number; value: number; agree: number }[] }

const setBtn = (disabled?: boolean): CSSProperties => ({
  background: disabled ? '#3a3a3a' : '#9e6a03', color: disabled ? '#888' : '#fff',
  border: 'none', borderRadius: 5, padding: '4px 10px', fontSize: 12,
  fontWeight: 600, cursor: disabled ? 'not-allowed' : 'pointer',
});

export default function TradersPanel({ target, onClose }: { target: string; onClose?: () => void }) {
  const host = target === 'local' ? 'Local' : 'Remote';
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
    } catch (e) {
      setRows([]);
      setErr(String(e));
    }
  }, [target]);

  useEffect(() => {
    refresh();
    const id = setInterval(refresh, 15000);
    return () => clearInterval(id);
  }, [refresh]);

  const flash = (m: string, ms = 7000) => { setMsg(m); setTimeout(() => setMsg(null), ms); };

  const refillAll = async () => {
    if (!window.confirm(`Set ALL traders' funds to ${amount.toLocaleString()} on ${host} now? (live)`)) return;
    setBusy(true);
    try {
      // Resolve the in-memory funds offset (scan), then set it on all traders.
      const scan = await engineRpc<ScanResp>('traderFunds', { mode: 'scan' });
      const cands = scan.candidates ?? [];
      if (cands.length !== 1) {
        flash(`✗ scan found ${cands.length} fund-offset candidates (need exactly 1) — can't safely auto-set. ${cands.map((c) => `@${c.offset}=${c.value}`).join(', ')}`, 12000);
        return;
      }
      const off = cands[0].offset;
      const res = await engineRpc<{ ok: boolean; traders: number }>('traderFunds', {
        mode: 'set', offset: String(off), value: String(amount),
      });
      flash(`✓ set ${res.traders ?? 0} traders to ${amount.toLocaleString()} (offset ${off})`);
      setTimeout(refresh, 1500); // DB reflects after SCUM's next save
    } catch (e) {
      flash(`✗ refill failed: ${String(e)}`, 12000);
    } finally {
      setBusy(false);
    }
  };

  return (
    <DraggablePanel
      title={`Traders · ${host}`}
      icon="💰"
      defaultCorner="tr"
      defaultWidth={300}
      defaultHeight={440}
      minH={220}
      onClose={onClose}
    >
      <div style={{ padding: 10, color: '#ddd', fontFamily: 'system-ui, sans-serif' }}>
        <div style={{ fontSize: 10, color: '#9e6a03', marginBottom: 8, lineHeight: 1.35 }}>
          Funds each trader can spend buying from players. Auto-refills <b>hourly</b>.
          <span style={{ color: '#777' }}> Values below are the save DB (refresh after a save).</span>
        </div>

        <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 8,
          borderBottom: '1px solid #3a3a40', paddingBottom: 8 }}>
          <span style={{ fontSize: 12, color: '#bbb', flex: 1 }}>Refill all to</span>
          <input
            type="number" min={0} max={100000000} step={1000} value={amount}
            disabled={busy}
            onChange={(e) => setAmount(Math.max(0, Math.min(100000000, Number(e.target.value) || 0)))}
            style={{ width: 86, background: '#26262b', color: '#ddd', border: '1px solid #3a3a40',
              borderRadius: 4, padding: '2px 4px', fontSize: 12, textAlign: 'right' }}
          />
          <button disabled={busy} style={setBtn(busy)} onClick={refillAll}>
            {busy ? '…' : 'Refill now'}
          </button>
        </div>

        {err && (
          <div style={{ marginBottom: 8, fontSize: 11, color: '#f0883e', background: '#3a2a14',
            border: '1px solid #6a4a1a', borderRadius: 6, padding: '6px 8px', wordBreak: 'break-word' }}>
            ⚠ Can’t read traders on {host}: {err}
          </div>
        )}

        <div style={{ maxHeight: 280, overflowY: 'auto' }}>
          <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: 11 }}>
            <thead>
              <tr style={{ color: '#888', textAlign: 'left' }}>
                <th style={{ padding: '2px 4px' }}>#</th>
                <th style={{ padding: '2px 4px' }}>trader</th>
                <th style={{ padding: '2px 4px', textAlign: 'right' }}>funds</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((t) => (
                <tr key={t.id} style={{ borderTop: '1px solid #2a2a30' }}>
                  <td style={{ padding: '2px 4px', color: '#888' }}>{t.id}</td>
                  <td style={{ padding: '2px 4px', color: '#bbb', fontFamily: 'monospace' }}>
                    {(t.trader_runtime_id ?? '').slice(0, 8)}
                  </td>
                  <td style={{ padding: '2px 4px', textAlign: 'right',
                    color: t.available_funds >= TARGET_FUNDS ? '#3fb950' : '#e0c060' }}>
                    {Number(t.available_funds ?? 0).toLocaleString()}
                  </td>
                </tr>
              ))}
              {rows.length === 0 && !err && (
                <tr><td colSpan={3} style={{ padding: '8px 4px', color: '#777' }}>No traders loaded.</td></tr>
              )}
            </tbody>
          </table>
        </div>

        {msg && (
          <div style={{ marginTop: 9, fontSize: 12, wordBreak: 'break-word',
            color: msg.startsWith('✓') ? '#3fb950' : '#f85149' }}>
            {msg}
          </div>
        )}
      </div>
    </DraggablePanel>
  );
}
