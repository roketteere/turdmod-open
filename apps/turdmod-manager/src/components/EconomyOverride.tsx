// EconomyOverride.json editor — friendly form (toggles + inputs) over SCUM's trader economy file.
// Reads/writes via the admin-file endpoint (EconomyOverride.json must be in the allowlist). Preserves
// the per-trader `traders` block + any unknown keys on save. Boot-read -> applies on next restart.
// @dep tauri-remote.ts remoteAdminFileGet/Set; admin_files.rs allowlist.
import { useState, useEffect, useCallback, useRef } from 'react';
import { remoteAdminFileGet, remoteAdminFileSet, remoteServerRestart } from '../lib/tauri-remote';
import { loadIconMap, iconForCode } from '../lib/item-icons';
import DraggablePanel from './DraggablePanel';
import ItemBrowser from './ItemBrowser';
import TRADER_CATALOG from '../data/trader-catalog.json';

const hostLabel = (t: string) => (t === 'local' ? 'Local' : 'OVH');
const FILE = 'EconomyOverride.json';

type FieldType = 'bool' | 'num';
interface Field { group: string; key: string; label: string; type: FieldType; hint?: string }

// Known economy-override scalar fields, grouped for a neat layout. Unknown keys + the `traders`
// block are preserved untouched on save.
const FIELDS: Field[] = [
  { group: 'Traders', key: 'traders-unlimited-funds', label: 'Unlimited funds', type: 'bool' },
  { group: 'Traders', key: 'traders-unlimited-stock', label: 'Unlimited stock', type: 'bool' },
  { group: 'Traders', key: 'trader-funds-change-rate-per-hour-multiplier', label: 'Funds change rate / h ×', type: 'num' },
  { group: 'Tradeables', key: 'tradeable-rotation-enabled', label: 'Tradeable rotation', type: 'bool' },
  { group: 'Tradeables', key: 'fully-restock-tradeable-hours', label: 'Full restock (hrs)', type: 'num' },
  { group: 'Tradeables', key: 'tradeable-rotation-time-ingame-hours-min', label: 'Rotation min (in-game hrs)', type: 'num' },
  { group: 'Tradeables', key: 'tradeable-rotation-time-ingame-hours-max', label: 'Rotation max (in-game hrs)', type: 'num' },
  { group: 'Tradeables', key: 'tradeable-rotation-time-of-day-min', label: 'Rotation time-of-day min', type: 'num' },
  { group: 'Tradeables', key: 'tradeable-rotation-time-of-day-max', label: 'Rotation time-of-day max', type: 'num' },
  { group: 'Tradeables', key: 'global-only-after-player-sale-tradeable-availability-enabled', label: 'Available after player sale only', type: 'bool' },
  { group: 'Economy', key: 'economy-reset-time-hours', label: 'Economy reset (hrs, -1 = off)', type: 'num' },
  { group: 'Economy', key: 'prices-randomization-time-hours', label: 'Price randomization (hrs, -1 = off)', type: 'num' },
  { group: 'Economy', key: 'prices-subject-to-player-count', label: 'Prices scale w/ player count', type: 'bool' },
  { group: 'Economy', key: 'enable-fame-point-requirement', label: 'Fame-point requirement', type: 'bool' },
  { group: 'Economy', key: 'economy-logging', label: 'Economy logging', type: 'bool' },
  { group: 'Gold', key: 'gold-price-subject-to-global-multiplier', label: 'Gold uses global ×', type: 'bool' },
  { group: 'Gold', key: 'gold-base-price', label: 'Gold base price (-1 = default)', type: 'num' },
  { group: 'Gold', key: 'gold-sale-price-modifier', label: 'Gold sale modifier (-1 = default)', type: 'num' },
  { group: 'Gold', key: 'gold-price-change-percentage-step', label: 'Gold % step (-1 = default)', type: 'num' },
  { group: 'Gold', key: 'gold-price-change-per-step', label: 'Gold price / step (-1 = default)', type: 'num' },
];
const GROUPS = ['Traders', 'Tradeables', 'Economy', 'Gold'];

const selStyle = {
  background: '#26262b', color: '#ddd', border: '1px solid #3a3a40',
  borderRadius: 4, padding: '2px 5px', fontSize: 12,
} as const;

export default function EconomyOverride({ target, embedded }: { target: string; embedded?: boolean }) {
  const raw = useRef<any>(null); // full parsed JSON (preserves `traders` + unknown keys)
  const [vals, setVals] = useState<Record<string, string> | null>(null);
  const [unavailable, setUnavailable] = useState(false);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [showImport, setShowImport] = useState(false);
  const [importText, setImportText] = useState('');
  const [showTraders, setShowTraders] = useState(false);
  const [iconMap, setIconMap] = useState<Map<string, string> | null>(null);
  const origVals = useRef<Record<string, string>>({}); // values at load, for the staged-change count
  const [countdown, setCountdown] = useState(60);
  const [selTrader, setSelTrader] = useState('');
  const [selZone, setSelZone] = useState('');
  const [itemSearch, setItemSearch] = useState('');
  const [showBrowser, setShowBrowser] = useState(false);
  const [itemsDirty, setItemsDirty] = useState(false);
  const [, forceTick] = useState(0);
  const bump = () => { setItemsDirty(true); forceTick((t) => t + 1); };
  const tradersOf = () => { const eo = raw.current?.['economy-override']; if (eo && !eo.traders) eo.traders = {}; return (eo?.traders ?? {}) as Record<string, any[]>; };
  const NEW_ITEM = () => ({ 'tradeable-code': '', 'base-purchase-price': '-1', 'base-sell-price': '-1', 'delta-price': '-1.0', 'can-be-purchased': 'default', 'required-famepoints': '-1', 'available-after-sale-only': 'default' });
  const addItem = (code: string) => {
    const t = tradersOf(); const tn = selTrader || Object.keys(t)[0]; if (!tn) return;
    if (!Array.isArray(t[tn])) t[tn] = [];
    if (t[tn].some((x) => x['tradeable-code'] === code)) return;
    t[tn].push({ ...NEW_ITEM(), 'tradeable-code': code }); bump();
  };
  const CATALOG = TRADER_CATALOG as unknown as Record<string, Array<{ code: string; fame: number; cat: number; buyable: number; sellable: number; icon?: string }>>;
  // Override upsert by code (the merged view edits the EconomyOverride block; default-only items
  // become overrides on first edit; ↺ removes the override, reverting to the game default).
  const upsertOverride = (tn: string, code: string, field: string, val: string) => {
    const t = tradersOf(); if (!Array.isArray(t[tn])) t[tn] = [];
    let e = t[tn].find((x: any) => x['tradeable-code'] === code);
    if (!e) { e = { ...NEW_ITEM(), 'tradeable-code': code }; t[tn].push(e); }
    e[field] = val; bump();
  };
  const clearOverride = (tn: string, code: string) => { const t = tradersOf(); if (Array.isArray(t[tn])) { const i = t[tn].findIndex((x: any) => x['tradeable-code'] === code); if (i >= 0) { t[tn].splice(i, 1); bump(); } } };

  const refresh = useCallback(async () => {
    try {
      const r = await remoteAdminFileGet(target, FILE);
      const obj = JSON.parse(r.contents ?? '{}');
      raw.current = obj;
      const eo = obj['economy-override'] ?? {};
      const v: Record<string, string> = {};
      for (const f of FIELDS) v[f.key] = eo[f.key] != null ? String(eo[f.key]) : '';
      setVals(v);
      origVals.current = { ...v };
      const tnames = Object.keys(eo['traders'] ?? {});
      setSelTrader((s) => (s && tnames.includes(s) ? s : tnames[0] ?? ''));
      setItemsDirty(false);
      setUnavailable(false);
    } catch (e) {
      setUnavailable(true);
      setVals(null);
    }
  }, [target]);

  useEffect(() => { refresh(); }, [refresh]);
  useEffect(() => { loadIconMap().then(setIconMap); }, []); // game-asset icons (once)

  const set = (k: string, v: string) => setVals((p) => ({ ...(p ?? {}), [k]: v }));

  // Write the staged form values to EconomyOverride.json. Returns ok.
  const writeFile = async (): Promise<boolean> => {
    if (!vals || !raw.current) return false;
    const obj = raw.current;
    obj['economy-override'] = obj['economy-override'] ?? {};
    for (const f of FIELDS) {
      if (vals[f.key] !== '') obj['economy-override'][f.key] = vals[f.key]; // SCUM stores values as strings
    }
    const r = await remoteAdminFileSet(target, FILE, JSON.stringify(obj, null, 2));
    if (r.ok) { origVals.current = { ...vals }; setItemsDirty(false); return true; } // staged -> committed
    setMsg('✗ ' + (r.error ?? 'save failed'));
    return false;
  };

  // Apply path 1: save the file; takes effect on the next restart (EconomyOverride is boot-read).
  const saveForRestart = async () => {
    setBusy(true); setMsg(null);
    try { if (await writeFile()) setMsg('✓ saved — applies on next restart'); }
    catch (e) { setMsg('✗ ' + String(e)); }
    finally { setBusy(false); setTimeout(() => setMsg(null), 8000); }
  };

  // Apply path 2: save + a WARNED restart now (the service broadcasts the countdown banners then
  // reboots, which re-reads the new economy). Boot-read settings can't reload live like loot does.
  const applyNow = async () => {
    setBusy(true); setMsg(null);
    try {
      if (!(await writeFile())) return;
      await remoteServerRestart(target, countdown);
      setMsg(`✓ saved + ${countdown}s warned restart started — applies on reboot`);
    } catch (e) { setMsg('✗ ' + String(e)); }
    finally { setBusy(false); setTimeout(() => setMsg(null), 10000); }
  };

  // Import a full EconomyOverride.json (authored in Jubaroo / SCUM Trader Tool) — validate JSON,
  // confirm it's an economy file, then write the whole thing. Refreshes the form from the new file.
  const doImport = async () => {
    let parsed: any;
    try { parsed = JSON.parse(importText); } catch { setMsg('✗ invalid JSON'); setTimeout(() => setMsg(null), 7000); return; }
    if (!parsed || !parsed['economy-override']) { setMsg('✗ not an EconomyOverride file (missing "economy-override")'); setTimeout(() => setMsg(null), 7000); return; }
    setBusy(true); setMsg(null);
    try {
      const r = await remoteAdminFileSet(target, FILE, JSON.stringify(parsed, null, 2));
      if (r.ok) { raw.current = parsed; await refresh(); setImportText(''); setShowImport(false); setMsg('✓ imported — applies on next restart'); }
      else setMsg('✗ ' + (r.error ?? 'save failed'));
    } catch (e) { setMsg('✗ ' + String(e)); }
    finally { setBusy(false); setTimeout(() => setMsg(null), 7000); }
  };

  const host = hostLabel(target);
  const dirty = vals ? FIELDS.filter((f) => vals[f.key] !== (origVals.current[f.key] ?? '')).length : 0;
  const canSave = dirty > 0 || itemsDirty;

  const body = (
      <div style={{ padding: 12, color: '#ddd', fontFamily: 'system-ui, sans-serif', fontSize: 12 }}>
        {unavailable ? (
          <div style={{ color: '#d2992b' }}>
            EconomyOverride.json not reachable on {host} (service not updated there yet, or file missing).
          </div>
        ) : !vals ? (
          <div style={{ color: '#888' }}>Loading…</div>
        ) : (
          <>
            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(280px, 1fr))', gap: 14, alignContent: 'start' }}>
              {GROUPS.map((g) => (
                <div key={g} style={{ background: '#1e1e22', border: '1px solid #2a2a30', borderRadius: 8, padding: 14 }}>
                  <div style={{ color: '#e8c45a', fontWeight: 700, fontSize: 11, textTransform: 'uppercase', letterSpacing: 0.5, marginBottom: 4 }}>{g}</div>
                  {FIELDS.filter((f) => f.group === g).map((f) => (
                    <div key={f.key} style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 4 }}>
                      <span style={{ flex: 1, minWidth: 0, color: '#bbb' }}>{f.label}</span>
                      {f.type === 'bool' ? (
                        <select value={vals[f.key] === '1' ? '1' : '0'} disabled={busy} onChange={(e) => set(f.key, e.target.value)} style={{ ...selStyle, width: 70 }}>
                          <option value="0">Off</option>
                          <option value="1">On</option>
                        </select>
                      ) : (
                        <input value={vals[f.key]} disabled={busy} onChange={(e) => set(f.key, e.target.value)} style={{ ...selStyle, width: 70, textAlign: 'right' }} />
                      )}
                    </div>
                  ))}
                </div>
              ))}
            </div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginTop: 8, borderTop: '1px solid #2c2c30', paddingTop: 8, flexWrap: 'wrap' }}>
              <span style={{ fontSize: 11, color: dirty ? '#e8c45a' : '#777', fontWeight: dirty ? 700 : 400 }}>{dirty} staged</span>
              <button disabled={busy || !canSave} onClick={saveForRestart} title="write the file; applies on the next restart" style={{ background: '#26262b', color: '#ddd', border: '1px solid #3a3a40', borderRadius: 4, padding: '4px 10px', cursor: busy ? 'wait' : 'pointer', opacity: canSave ? 1 : 0.5 }}>Save for next restart</button>
              <label style={{ color: '#999', display: 'flex', alignItems: 'center', gap: 4, fontSize: 11 }}>warn
                <select value={countdown} onChange={(e) => setCountdown(Number(e.target.value))} style={selStyle}>
                  <option value={0}>now</option>
                  <option value={30}>30s</option>
                  <option value={60}>60s</option>
                  <option value={120}>2m</option>
                  <option value={300}>5m</option>
                </select>
              </label>
              <button disabled={busy || !canSave} onClick={applyNow} title="save + a warned server restart (EconomyOverride is boot-read, so apply = reboot)" style={{ background: '#238636', color: '#fff', border: '1px solid #2ea043', borderRadius: 4, padding: '4px 12px', fontWeight: 700, cursor: busy ? 'wait' : 'pointer', opacity: canSave ? 1 : 0.5 }}>↻ Apply now (warned restart)</button>
              <button disabled={busy} onClick={refresh} title="discard staged edits + reload from server" style={{ background: '#26262b', color: '#ddd', border: '1px solid #3a3a40', borderRadius: 4, padding: '4px 10px', cursor: 'pointer' }}>Reload</button>
            </div>
            <div style={{ marginTop: 8 }}>
              <button onClick={() => setShowImport((v) => !v)} style={{ background: 'none', border: 'none', color: '#9ab', cursor: 'pointer', fontSize: 12, padding: 0 }}>
                {showImport ? '▾' : '▸'} Import full file (from Jubaroo / SCUM Trader Tool)
              </button>
              {showImport && (
                <div style={{ marginTop: 6 }}>
                  <textarea
                    value={importText}
                    onChange={(e) => setImportText(e.target.value)}
                    placeholder="Paste a full EconomyOverride.json here…"
                    spellCheck={false}
                    style={{ width: '100%', height: 90, background: '#1c1c20', color: '#ddd', border: '1px solid #3a3a40', borderRadius: 4, fontFamily: 'monospace', fontSize: 11, padding: 6, boxSizing: 'border-box', resize: 'vertical' }}
                  />
                  <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginTop: 4 }}>
                    <button disabled={busy || !importText.trim()} onClick={doImport} style={{ background: '#1f6feb', color: '#fff', border: '1px solid #388bfd', borderRadius: 4, padding: '4px 12px', fontWeight: 700, cursor: busy ? 'wait' : 'pointer' }}>Import &amp; Save</button>
                    <span style={{ color: '#777', fontSize: 11 }}>Replaces the whole file (validated). Applies on next restart.</span>
                  </div>
                </div>
              )}
            </div>
            <div style={{ marginTop: 8 }}>
              <button onClick={() => setShowTraders((v) => !v)} style={{ background: 'none', border: 'none', color: '#9ab', cursor: 'pointer', fontSize: 12, padding: 0 }}>
                {showTraders ? '▾' : '▸'} Per-trader items (game-icon browser)
              </button>
              {showTraders && (() => {
                const traders = (raw.current?.['economy-override']?.['traders']) || {};
                const names = Object.keys(traders);
                if (!names.length) return <div style={{ color: '#888', marginTop: 6 }}>No traders block — use Export defaults / Import a file first.</div>;
                const parse = (k: string) => { const p = k.split('_'); return { zone: p.slice(0, 2).join('_'), type: p.slice(2).join('_') || k }; };
                const nice = (z: string) => z.replace(/_/g, '');
                const zones = Array.from(new Set(names.map((n) => parse(n).zone)));
                const curZone = selZone && zones.includes(selZone) ? selZone : zones[0];
                const TYPE_ORDER = ['Armory', 'Trader', 'BoatShop', 'Mechanic', 'Hospital', 'Saloon'];
                const zoneTraders = names.filter((n) => parse(n).zone === curZone).sort((a, b) => TYPE_ORDER.indexOf(parse(a).type) - TYPE_ORDER.indexOf(parse(b).type));
                const cur = selTrader && zoneTraders.includes(selTrader) ? selTrader : zoneTraders[0];
                const items: any[] = Array.isArray(traders[cur]) ? traders[cur] : [];
                const numIn = { width: 54, background: '#1c1c20', color: '#ddd', border: '1px solid #3a3a40', borderRadius: 3, padding: '3px 4px', fontSize: 11, textAlign: 'right' as const };
                return (
                  <div style={{ marginTop: 8 }}>
                    <div style={{ display: 'flex', gap: 10, marginBottom: 12 }}>
                      {zones.map((z) => (
                        <button key={z} onClick={() => { setSelZone(z); const zt = names.filter((n) => parse(n).zone === z); setSelTrader(zt[0] || ''); }}
                          style={{ minWidth: 70, padding: '12px 18px', borderRadius: 10, fontWeight: 800, fontSize: 18, cursor: 'pointer', border: z === curZone ? '2px solid #e8c45a' : '1px solid #3a3a40', background: z === curZone ? '#3a2f12' : '#1e1e22', color: z === curZone ? '#f5d36b' : '#bbb' }}>
                          {nice(z)}
                        </button>
                      ))}
                    </div>
                    <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginBottom: 12 }}>
                      {zoneTraders.map((tn) => { const ty = parse(tn).type; const n = (traders[tn] || []).length;
                        return (
                          <button key={tn} onClick={() => setSelTrader(tn)}
                            style={{ padding: '8px 14px', borderRadius: 7, fontSize: 13, cursor: 'pointer', border: tn === cur ? '1px solid #388bfd' : '1px solid #2a2a30', background: tn === cur ? '#16314f' : '#1a1a1e', color: tn === cur ? '#cfe3ff' : '#aaa', fontWeight: tn === cur ? 700 : 400 }}>
                            {ty}{n > 0 && <span style={{ marginLeft: 6, color: '#e8c45a', fontWeight: 700 }}>{n}</span>}
                          </button>
                        );
                      })}
                    </div>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 8, borderTop: '1px solid #2a2a30', paddingTop: 10 }}>
                      <span style={{ fontWeight: 800, color: '#f5d36b', fontSize: 15 }}>{nice(curZone)}</span>
                      <span style={{ color: '#888' }}>›</span>
                      <span style={{ fontWeight: 700, color: '#cfe3ff', fontSize: 14 }}>{parse(cur).type}</span>
                      <span style={{ color: '#777', fontSize: 12 }}>· {items.length} override{items.length === 1 ? '' : 's'}</span>
                      <button disabled={busy} onClick={() => setShowBrowser(true)} style={{ marginLeft: 'auto', background: '#1f6feb', color: '#fff', border: '1px solid #388bfd', borderRadius: 6, padding: '7px 16px', fontWeight: 700, cursor: 'pointer', fontSize: 13 }}>+ Add item</button>
                    </div>
                    {(() => {
                      const type = parse(cur).type;
                      const defaults = CATALOG[type] || [];
                      const ovArr: any[] = Array.isArray(traders[cur]) ? traders[cur] : [];
                      const ovBy: Record<string, any> = {}; ovArr.forEach((o) => { ovBy[String(o['tradeable-code'])] = o; });
                      const defBy: Record<string, { code: string; fame: number; cat: number; buyable: number; sellable: number; icon?: string }> = {}; defaults.forEach((d) => { defBy[d.code] = d; });
                      const codes = Array.from(new Set([...defaults.map((d) => d.code), ...ovArr.map((o) => String(o['tradeable-code']))]));
                      const q = itemSearch.trim().toLowerCase();
                      const shown = (q ? codes.filter((c) => c.toLowerCase().includes(q)) : codes).slice(0, 400);
                      return (
                        <div>
                          <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 8 }}>
                            <input value={itemSearch} onChange={(e) => setItemSearch(e.target.value)} placeholder={`search ${codes.length} items this trader sells…`} style={{ ...selStyle, flex: 1, padding: '5px 8px' }} />
                            <span style={{ color: '#888', fontSize: 11 }}>{ovArr.length} overridden</span>
                          </div>
                          {codes.length === 0 && <div style={{ color: '#777', fontSize: 12, padding: '12px 0' }}>No default catalog for this trader type. Use “Add item” to add tradeables.</div>}
                          {shown.map((code) => {
                            const ov = ovBy[code]; const def = defBy[code];
                            const url = (def && def.icon) ? '/item-icons/' + def.icon : (iconForCode(iconMap, code + '_C') || iconForCode(iconMap, code));
                            return (
                              <div key={code} style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '4px 0', borderBottom: '1px solid #242428', background: ov ? '#161d14' : 'transparent' }}>
                                {url
                                  ? <img src={url} alt="" width={28} height={28} style={{ objectFit: 'contain' }} onError={(e) => { (e.currentTarget as HTMLImageElement).style.visibility = 'hidden'; }} />
                                  : <span style={{ width: 28, textAlign: 'center', fontSize: 16 }}>📦</span>}
                                <span style={{ flex: 1, minWidth: 80, fontFamily: 'monospace', fontSize: 12, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }} title={code}>{code.replace(/_/g, ' ')}</span>
                                {def && def.fame > 0 && <span style={{ color: '#c9a227', fontSize: 9 }} title="fame required">★{def.fame}</span>}
                                <label style={{ color: '#7fb5ff', fontSize: 9, display: 'flex', flexDirection: 'column', gap: 1 }}>buy<input value={ov ? String(ov['base-purchase-price'] ?? '') : ''} placeholder="def" onChange={(e) => upsertOverride(cur, code, 'base-purchase-price', e.target.value)} style={numIn} /></label>
                                <label style={{ color: '#3fb950', fontSize: 9, display: 'flex', flexDirection: 'column', gap: 1 }}>sell<input value={ov ? String(ov['base-sell-price'] ?? '') : ''} placeholder="def" onChange={(e) => upsertOverride(cur, code, 'base-sell-price', e.target.value)} style={numIn} /></label>
                                <select value={ov ? String(ov['can-be-purchased'] ?? 'default') : 'default'} onChange={(e) => upsertOverride(cur, code, 'can-be-purchased', e.target.value)} style={{ ...selStyle, fontSize: 10 }} title="can be purchased">
                                  <option value="default">default</option><option value="true">buyable</option><option value="false">locked</option>
                                </select>
                                {ov ? <button onClick={() => clearOverride(cur, code)} title="reset to default" style={{ background: 'none', border: 'none', color: '#9a9', cursor: 'pointer', fontSize: 13 }}>↺</button> : <span style={{ width: 13 }} />}
                              </div>
                            );
                          })}
                          {shown.length === 400 && <div style={{ color: '#666', fontSize: 10, textAlign: 'center', padding: 4 }}>showing 400 of {codes.length} — refine search</div>}
                        </div>
                      );
                    })()}
                  </div>
                );
              })()}
              {showBrowser && (
                <div style={{ position: 'fixed', inset: 0, background: 'rgba(0,0,0,0.65)', zIndex: 60, display: 'flex', alignItems: 'center', justifyContent: 'center', padding: 24 }} onClick={() => setShowBrowser(false)}>
                  <div style={{ width: 'min(880px, 94vw)', height: 'min(660px, 88vh)' }} onClick={(e) => e.stopPropagation()}>
                    <ItemBrowser onPick={(code) => addItem(code)} onClose={() => setShowBrowser(false)} />
                  </div>
                </div>
              )}
            </div>
            {msg && <div style={{ marginTop: 6, color: msg.startsWith('✓') ? '#3fb950' : '#f85149' }}>{msg}</div>}
          </>
        )}
      </div>
  );
  if (embedded) return <div style={{ width: '100%' }}>{body}</div>;
  return (
    <DraggablePanel title={`Economy Override · ${host}`} defaultCorner="tr" defaultWidth={360} defaultHeight={420} minH={220}>
      {body}
    </DraggablePanel>
  );
}
