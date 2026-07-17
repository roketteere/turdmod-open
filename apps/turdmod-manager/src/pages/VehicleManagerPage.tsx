import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useEngineHost } from '../lib/engineHost';
import { remoteDataFileGet, remoteDataFileSet, remoteVehiclesDetail, remoteVehiclesRepo, type ServerVehicle } from '../lib/tauri-remote';
import { SlideTabs, FadeSwap, type TabSpec } from '../lib/motion';

// Vehicle Manager — modded vehicle systems. Data loads ONCE per (host,dataset)
// and is cached at module scope, so switching sub-tabs reuses the last pull and
// re-entering the page shows cached data — NEVER auto-polls. Use Refresh to
// re-read. @inv no setInterval / no self-retriggering effects anywhere.

type TabKey = 'vehicles' | 'durability' | 'registrations' | 'history';
const TABS: TabSpec<TabKey>[] = [
  { key: 'vehicles', label: 'Server Vehicles' },
  { key: 'durability', label: 'Durability' },
  { key: 'registrations', label: 'Registrations' },
  { key: 'history', label: 'Repo History' },
];

const shortClass = (c: string) => c.replace(/^BPC_/, '').replace(/_C$/, '').replace(/_/g, ' ').trim();
const shortPart = (c: string) => c.replace(/^BPC?_/, '').replace(/_C$/, '');
// stock 0.2 absorbs 20% (takes 80%); %less = 1 - (1-V)/0.8
const pctLess = (v: number) => Math.round((1 - (1 - v) / 0.8) * 100);

// ── load-once + module cache (survives sub-tab unmount; manual Refresh only) ──
const dataCache = new Map<string, unknown>();
function useCachedLoad<T>(key: string, loader: () => Promise<T>, initial: T) {
  const loaderRef = useRef(loader);
  loaderRef.current = loader; // keep latest closure without re-triggering the effect
  const [data, setData] = useState<T>(() => (dataCache.has(key) ? (dataCache.get(key) as T) : initial));
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true); setErr(null);
    try {
      const r = await loaderRef.current();
      dataCache.set(key, r);
      setData(r);
    } catch (e) { setErr(String((e as Error)?.message ?? e)); }
    finally { setLoading(false); }
  }, [key]);

  // First time we see this key → one fetch. Cached afterwards (incl. across
  // remounts), so re-entering a tab reuses the last pull with NO request.
  useEffect(() => {
    if (dataCache.has(key)) { setData(dataCache.get(key) as T); return; }
    void refresh();
  }, [key, refresh]);

  return { data, loading, err, refresh };
}

// ── click-to-sort ────────────────────────────────────────────────────────────
type SortDir = 'asc' | 'desc';
function useSort<K extends string>(initial: K | null = null) {
  const [key, setKey] = useState<K | null>(initial);
  const [dir, setDir] = useState<SortDir>('asc');
  const onClick = (k: K) => {
    if (k === key) setDir((d) => (d === 'asc' ? 'desc' : 'asc'));
    else { setKey(k); setDir('asc'); }
  };
  return { key, dir, onClick };
}
// numeric when both sides are numbers, else locale string compare
const cmp = (a: unknown, b: unknown): number => {
  const as = String(a ?? '').trim();
  const bs = String(b ?? '').trim();
  const an = Number(as);
  const bn = Number(bs);
  if (as !== '' && bs !== '' && !Number.isNaN(an) && !Number.isNaN(bn)) return an - bn;
  return as.localeCompare(bs);
};
function SortTh<K extends string>({ k, sort, children, className }: { k: K; sort: ReturnType<typeof useSort<K>>; children: React.ReactNode; className?: string }) {
  const active = sort.key === k;
  return (
    <th
      onClick={() => sort.onClick(k)}
      className={`cursor-pointer select-none px-2 py-2 font-normal hover:text-turd-cream ${active ? 'text-turd-mustard' : ''} ${className ?? ''}`}
    >
      {children}{active ? (sort.dir === 'asc' ? ' ▲' : ' ▼') : ''}
    </th>
  );
}

export function VehicleManagerPage() {
  const target = useEngineHost();
  const [tab, setTab] = useState<TabKey>('vehicles');
  return (
    <div className="flex h-full min-h-0 flex-col gap-4">
      <header>
        <p className="font-display text-[11px] font-semibold tracking-[0.32em] text-turd-mustard/90">TURDMOD</p>
        <h1 className="mt-1 font-display text-[2rem] font-bold leading-none text-turd-cream">Vehicle Manager</h1>
        <p className="mt-1.5 text-xs text-turd-cream-dim">Durability tuning, registrations, and repossession history. Loads once per visit — click Refresh to re-read. Target: <span className="text-turd-mustard">{target}</span>.</p>
      </header>
      <SlideTabs tabs={TABS} value={tab} onChange={setTab} layoutId="vehicle-manager-tab" />
      <div className="min-h-0 flex-1">
        <FadeSwap swapKey={tab} className="h-full min-h-0">
          {tab === 'vehicles' && <ServerVehiclesTab target={target} />}
          {tab === 'durability' && <DurabilityTab target={target} />}
          {tab === 'registrations' && <RegistrationsTab target={target} />}
          {tab === 'history' && <HistoryTab target={target} />}
        </FadeSwap>
      </div>
    </div>
  );
}

// ── Server Vehicles (view / sort / select / repossess) ──────────────────────
type VSortKey = 'id' | 'class' | 'sector' | 'owner' | 'locked' | 'parts' | 'missing';

function ServerVehiclesTab({ target }: { target: string }) {
  const { data: vehicles, loading, err, refresh } = useCachedLoad<ServerVehicle[]>(
    `${target}:vehicles`,
    async () => (await remoteVehiclesDetail(target)).vehicles ?? [],
    [],
  );
  const [sel, setSel] = useState<Set<number>>(new Set());
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const sort = useSort<VSortKey>('id');

  // Expected parts per exact class = union of all parts seen for that class (the
  // fullest same-type vehicle defines "complete"). Missing = expected - present.
  const expectedByClass = useMemo(() => {
    const m = new Map<string, Set<string>>();
    for (const v of vehicles) {
      if (!m.has(v.class)) m.set(v.class, new Set());
      const s = m.get(v.class)!;
      for (const p of v.parts) s.add(p);
    }
    return m;
  }, [vehicles]);
  const missingOf = useCallback((v: ServerVehicle) => {
    const exp = expectedByClass.get(v.class);
    if (!exp) return [] as string[];
    const have = new Set(v.parts);
    return [...exp].filter((p) => !have.has(p));
  }, [expectedByClass]);

  const sorted = useMemo(() => {
    if (!sort.key) return vehicles;
    const k = sort.key;
    const val = (v: ServerVehicle): unknown => {
      switch (k) {
        case 'id': return v.id;
        case 'class': return shortClass(v.class);
        case 'sector': return v.sector;
        case 'owner': return v.owner ?? '';
        case 'locked': return v.locked ? 1 : 0;
        case 'parts': return v.parts.length;
        case 'missing': return missingOf(v).length;
        default: return '';
      }
    };
    const arr = [...vehicles].sort((a, b) => cmp(val(a), val(b)));
    if (sort.dir === 'desc') arr.reverse();
    return arr;
  }, [vehicles, sort.key, sort.dir, missingOf]);

  const toggle = (id: number) => setSel((s) => { const n = new Set(s); if (n.has(id)) n.delete(id); else n.add(id); return n; });
  const allSel = vehicles.length > 0 && sel.size === vehicles.length;
  const toggleAll = () => setSel(allSel ? new Set() : new Set(vehicles.map((v) => v.id)));

  const repo = async () => {
    if (sel.size === 0) return;
    if (!window.confirm(`Repossess ${sel.size} vehicle(s)? This destroys them in-world and announces it.`)) return;
    setBusy(true); setMsg(null);
    try {
      const picked = vehicles.filter((v) => sel.has(v.id)).map((v) => ({ id: v.id, vehicle: shortClass(v.class), owner: v.owner ?? undefined }));
      const r = await remoteVehiclesRepo(target, picked);
      setMsg({ ok: true, text: `Repossessed ${r.destroyed}${r.failed ? `, ${r.failed} failed (need a player online)` : ''}.` });
      setSel(new Set());
      await refresh();
    } catch (e) { setMsg({ ok: false, text: String((e as Error)?.message ?? e) }); }
    finally { setBusy(false); }
  };

  return (
    <div className="flex h-full min-h-0 flex-col gap-3">
      <div className="flex items-center justify-between">
        <p className="text-[11px] text-turd-cream-dim">{vehicles.length} vehicles on the server. Click a column to sort. Select + repossess (destroys live + announces). “Missing” = parts absent vs the fullest same-type vehicle. Hover Parts/Missing for the list.</p>
        <div className="flex items-center gap-2">
          <button type="button" onClick={() => void refresh()} disabled={loading} className="rounded-lg border border-turd-bronze/50 px-3 py-1.5 font-display text-xs uppercase tracking-wider text-turd-cream-dim hover:text-turd-cream disabled:opacity-40">{loading ? 'Refreshing…' : 'Refresh'}</button>
          <button type="button" onClick={() => void repo()} disabled={busy || sel.size === 0} className="rounded-lg border border-turd-red/60 bg-turd-red/15 px-4 py-1.5 font-display text-xs uppercase tracking-wider text-turd-red hover:bg-turd-red/25 disabled:opacity-30">{busy ? 'Repossessing…' : `Repossess${sel.size ? ` (${sel.size})` : ''}`}</button>
        </div>
      </div>
      {msg && <p className={`font-mono text-[11px] ${msg.ok ? 'text-turd-green' : 'text-turd-red'}`}>{msg.text}</p>}
      <section className="glass min-h-0 flex-1 overflow-auto rounded-xl">
        {err && <p className="px-4 py-2 font-mono text-[11px] text-turd-red">{err}</p>}
        {vehicles.length === 0 && !loading ? (
          <p className="px-2 py-8 text-center text-xs text-turd-cream-dim">No vehicles found.</p>
        ) : (
          <table className="w-full font-mono text-[11px]">
            <thead className="sticky top-0 bg-turd-bg-mid/95">
              <tr className="border-b border-turd-bronze/30 text-left text-turd-cream-dim">
                <th className="px-2 py-2"><input type="checkbox" checked={allSel} onChange={toggleAll} className="accent-turd-mustard" /></th>
                <SortTh k="id" sort={sort}>ID</SortTh>
                <SortTh k="class" sort={sort}>Type</SortTh>
                <SortTh k="sector" sort={sort}>Sector</SortTh>
                <th className="px-2 py-2 font-normal">Coords</th>
                <SortTh k="owner" sort={sort}>Owner</SortTh>
                <SortTh k="locked" sort={sort}>Lock</SortTh>
                <SortTh k="parts" sort={sort}>Parts</SortTh>
                <SortTh k="missing" sort={sort}>Missing</SortTh>
              </tr>
            </thead>
            <tbody>
              {sorted.map((v) => {
                const miss = missingOf(v);
                return (
                  <tr key={v.id} className={`border-b border-turd-bronze/10 ${sel.has(v.id) ? 'bg-turd-mustard/10' : ''} text-turd-cream`}>
                    <td className="px-2 py-1.5"><input type="checkbox" checked={sel.has(v.id)} onChange={() => toggle(v.id)} className="accent-turd-mustard" /></td>
                    <td className="px-2 py-1.5 text-turd-cream-dim">{v.id}</td>
                    <td className="px-2 py-1.5">{shortClass(v.class)}</td>
                    <td className="px-2 py-1.5 text-turd-mustard">{v.sector}</td>
                    <td className="px-2 py-1.5 text-turd-cream-dim/70">{v.x != null ? `${Math.round(v.x)}, ${Math.round(v.y ?? 0)}` : '—'}</td>
                    <td className="px-2 py-1.5">{v.owner ?? <span className="text-turd-cream-dim/60">unowned</span>}</td>
                    <td className="px-2 py-1.5">{v.locked ? '🔒' : ''}</td>
                    <td className="px-2 py-1.5 text-turd-cream-dim" title={v.parts.map(shortPart).join(', ')}>{v.parts.length}</td>
                    <td className="px-2 py-1.5" title={miss.map(shortPart).join(', ')}>{miss.length > 0 ? <span className="text-turd-red">{miss.length}</span> : <span className="text-turd-green">0</span>}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </section>
    </div>
  );
}

// ── Durability ─────────────────────────────────────────────────────────────
function DurabilityTab({ target }: { target: string }) {
  const { data: loaded, loading, err, refresh } = useCachedLoad<Record<string, number>>(
    `${target}:durability`,
    async () => {
      const r = await remoteDataFileGet(target, 'vehicle_durability.json');
      if (!r.ok) throw new Error(r.error ?? 'read failed');
      return r.contents?.trim() ? JSON.parse(r.contents) : {};
    },
    {},
  );
  const [cfg, setCfg] = useState<Record<string, number>>(loaded);
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);

  // Sync the editable copy whenever a fresh pull lands (first load or Refresh).
  useEffect(() => { setCfg(loaded); setDirty(false); }, [loaded]);

  const save = async () => {
    setSaving(true); setMsg(null);
    try {
      const r = await remoteDataFileSet(target, 'vehicle_durability.json', JSON.stringify(cfg, null, 2));
      if (!r.ok) throw new Error(r.error ?? 'write failed');
      dataCache.set(`${target}:durability`, cfg); // keep cache consistent with the save
      setDirty(false);
      setMsg({ ok: true, text: 'Saved — re-applies live on the next durability tick (≤90s).' });
      setTimeout(() => setMsg(null), 4000);
    } catch (e) { setMsg({ ok: false, text: String((e as Error)?.message ?? e) }); }
    finally { setSaving(false); }
  };

  const families = Object.keys(cfg).sort();
  const setVal = (f: string, v: number) => { setCfg((c) => ({ ...c, [f]: v })); setDirty(true); };
  const setAll = (v: number) => { setCfg((c) => Object.fromEntries(Object.keys(c).map((k) => [k, v]))); setDirty(true); };

  return (
    <div className="flex h-full min-h-0 flex-col gap-3">
      <div className="flex items-center justify-between">
        <p className="text-[11px] text-turd-cream-dim">Per-family <code className="text-turd-mustard">_linearEnergyAbsorption</code> (stock 0.2). Higher = tougher. The mod applies edited values live.</p>
        <div className="flex items-center gap-2">
          <button type="button" onClick={() => setAll(0.2)} className="rounded border border-turd-bronze/50 px-2 py-1 text-[10px] text-turd-cream-dim hover:text-turd-cream">All → stock</button>
          <button type="button" onClick={() => setAll(0.92)} className="rounded border border-turd-bronze/50 px-2 py-1 text-[10px] text-turd-cream-dim hover:text-turd-cream">All → tough (0.92)</button>
          <button type="button" onClick={() => void refresh()} disabled={loading} className="rounded-lg border border-turd-bronze/50 px-3 py-1.5 font-display text-xs uppercase tracking-wider text-turd-cream-dim hover:text-turd-cream disabled:opacity-40">{loading ? 'Refreshing…' : 'Refresh'}</button>
          <button type="button" onClick={() => void save()} disabled={saving || !dirty} className="rounded-lg border border-turd-mustard/60 bg-turd-mustard/20 px-4 py-1.5 font-display text-xs uppercase tracking-wider text-turd-mustard-bright hover:bg-turd-mustard/30 disabled:opacity-30">{saving ? 'Saving…' : dirty ? 'Save' : 'Saved'}</button>
        </div>
      </div>
      {(msg || err) && <p className={`font-mono text-[11px] ${msg ? (msg.ok ? 'text-turd-green' : 'text-turd-red') : 'text-turd-red'}`}>{msg?.text ?? err}</p>}
      <section className="glass min-h-0 flex-1 overflow-auto rounded-xl p-3">
        {families.length === 0 ? (
          <p className="px-2 py-8 text-center text-xs text-turd-cream-dim">{loading ? 'Loading…' : 'No durability config yet — the mod seeds it on its next tick with a player near a vehicle. Refresh then.'}</p>
        ) : (
          <div className="grid grid-cols-2 gap-2">
            {families.map((f) => (
              <div key={f} className="glass-soft flex items-center justify-between rounded-lg px-3 py-2">
                <div>
                  <p className="font-mono text-xs text-turd-cream">{f.replace(/^BPC_|_$/g, '')}</p>
                  <p className="font-mono text-[9px] text-turd-cream-dim/70">~{pctLess(cfg[f])}% less damage</p>
                </div>
                <input type="number" step="0.01" min="0" max="1" value={cfg[f]} onChange={(e) => setVal(f, Number(e.target.value))} className="w-20 rounded border border-turd-bronze/60 bg-turd-bg-soft px-2 py-1 font-mono text-xs text-turd-cream focus:border-turd-mustard focus:outline-none" />
              </div>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}

const relTime = (secs?: number) => {
  if (!secs) return '';
  const d = Math.floor(Date.now() / 1000) - secs;
  if (d < 0) return `in ${Math.ceil(-d / 3600)}h`;
  if (d < 3600) return `${Math.floor(d / 60)}m ago`;
  if (d < 86400) return `${Math.floor(d / 3600)}h ago`;
  return `${Math.floor(d / 86400)}d ago`;
};

function RegistrationsTab({ target }: { target: string }) {
  const { data: rows, loading, err, refresh } = useCachedLoad<any[]>(
    `${target}:registrations`,
    async () => {
      const r = await remoteDataFileGet(target, 'vehicle_ownership.json');
      if (!r.ok) throw new Error(r.error ?? 'read failed');
      const p = r.contents?.trim() ? JSON.parse(r.contents) : {};
      return Array.isArray(p?.vehicles) ? p.vehicles : [];
    },
    [],
  );
  return (
    <Panel title={`Registrations · ${rows.length}`} loading={loading} err={err} refresh={refresh}>
      <Table head={['Vehicle', 'Owner', 'Entity', 'Type', 'Expires']} rows={rows.map((v) => [
        v.vehicle ?? '?', v.owner ?? '?', String(v.entity_id ?? '—'),
        v.temp ? 'temp' : 'permanent',
        v.temp ? relTime(Number(v.expires_at)) : '—',
      ])} empty="No registered vehicles." />
    </Panel>
  );
}

function HistoryTab({ target }: { target: string }) {
  const { data: rows, loading, err, refresh } = useCachedLoad<any[]>(
    `${target}:history`,
    async () => {
      const r = await remoteDataFileGet(target, 'repo_history.json');
      if (!r.ok) throw new Error(r.error ?? 'read failed');
      const p = r.contents?.trim() ? JSON.parse(r.contents) : [];
      return Array.isArray(p) ? [...p].reverse() : [];
    },
    [],
  );
  return (
    <Panel title={`Repossession History · ${rows.length}`} loading={loading} err={err} refresh={refresh}>
      <Table head={['Vehicle', 'Owner', 'Entity', 'When']} rows={rows.map((h) => [
        h.vehicle ?? '?', h.owner ?? '?', String(h.entity_id ?? '—'), relTime(Number(h.at)),
      ])} empty="No repossessions yet." />
    </Panel>
  );
}

function Panel({ title, loading, err, refresh, children }: { title: string; loading: boolean; err: string | null; refresh: () => void; children: React.ReactNode }) {
  return (
    <section className="glass flex h-full min-h-0 flex-col overflow-hidden rounded-xl">
      <div className="flex items-center justify-between border-b border-turd-bronze/30 px-4 py-2">
        <h2 className="font-display text-sm uppercase tracking-widest text-turd-mustard">{title}</h2>
        <button type="button" onClick={refresh} disabled={loading} className="text-[11px] text-turd-cream-dim hover:text-turd-cream disabled:opacity-40">{loading ? 'Refreshing…' : 'Refresh'}</button>
      </div>
      {err && <p className="px-4 py-2 font-mono text-[11px] text-turd-red">{err}</p>}
      <div className="min-h-0 flex-1 overflow-auto p-3">{children}</div>
    </section>
  );
}

// Generic table with click-to-sort column headers.
function Table({ head, rows, empty }: { head: string[]; rows: (string | number)[][]; empty: string }) {
  const [sortCol, setSortCol] = useState<number | null>(null);
  const [dir, setDir] = useState<SortDir>('asc');
  const onSort = (i: number) => {
    if (i === sortCol) setDir((d) => (d === 'asc' ? 'desc' : 'asc'));
    else { setSortCol(i); setDir('asc'); }
  };
  const sorted = useMemo(() => {
    if (sortCol == null) return rows;
    const arr = [...rows].sort((a, b) => cmp(a[sortCol], b[sortCol]));
    if (dir === 'desc') arr.reverse();
    return arr;
  }, [rows, sortCol, dir]);

  if (rows.length === 0) return <p className="px-2 py-8 text-center text-xs text-turd-cream-dim">{empty}</p>;
  return (
    <table className="w-full font-mono text-[11px]">
      <thead className="sticky top-0 bg-turd-bg-mid/95">
        <tr className="border-b border-turd-bronze/30 text-left text-turd-cream-dim">
          {head.map((h, i) => (
            <th key={h} onClick={() => onSort(i)} className={`cursor-pointer select-none px-3 py-2 font-normal hover:text-turd-cream ${sortCol === i ? 'text-turd-mustard' : ''}`}>
              {h}{sortCol === i ? (dir === 'asc' ? ' ▲' : ' ▼') : ''}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {sorted.map((r, i) => (
          <tr key={i} className="border-b border-turd-bronze/10 text-turd-cream">
            {r.map((c, j) => <td key={j} className="px-3 py-1.5">{c}</td>)}
          </tr>
        ))}
      </tbody>
    </table>
  );
}
