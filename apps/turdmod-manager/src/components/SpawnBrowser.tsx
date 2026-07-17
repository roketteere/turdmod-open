// Clickable spawn browser — category tabs + search + icon grid. Built from the
// real OVH item classes (spawn-catalog.json: 894 verified _ES_C classes with
// matched icons). Click an item to spawn it on the target via the parent.

import { useEffect, useMemo, useState } from 'react';

interface CatItem { cls: string; name: string; icon: string | null; veh?: boolean }
type Catalog = Record<string, CatItem[]>;

export function SpawnBrowser({ targetLabel, onClose, onSpawn }: {
  targetLabel: string;
  onClose: () => void;
  onSpawn: (cls: string, count: number, veh: boolean) => void;
}) {
  const [cat, setCat] = useState<Catalog>({});
  const [tab, setTab] = useState('');
  const [q, setQ] = useState('');
  const [count, setCount] = useState('1');

  useEffect(() => {
    fetch('/spawn-catalog.json').then((r) => r.json()).then((c: Catalog) => {
      setCat(c); setTab(Object.keys(c)[0] ?? '');
    }).catch(() => {});
  }, []);

  const items = useMemo(() => {
    const ql = q.trim().toLowerCase();
    if (ql) return Object.values(cat).flat().filter((i) => i.name.toLowerCase().includes(ql) || i.cls.toLowerCase().includes(ql)).slice(0, 400);
    return cat[tab] ?? [];
  }, [cat, tab, q]);

  const n = Math.max(1, Number(count) || 1);
  const tabCls = (active: boolean) =>
    `rounded px-2 py-0.5 text-[11px] transition-colors ${active ? 'bg-turd-mustard-bright text-turd-bg-deep' : 'text-turd-cream-dim hover:bg-white/5 hover:text-turd-cream'}`;

  return (
    <div className="absolute inset-0 z-[60] flex items-center justify-center bg-black/50 p-4" onClick={onClose}>
      <div onClick={(e) => e.stopPropagation()} className="flex max-h-full w-[660px] max-w-full flex-col rounded-lg border border-turd-bronze/50 bg-turd-bg-deep shadow-2xl">
        <div className="flex flex-wrap items-center gap-2 border-b border-turd-bronze/30 px-4 py-2">
          <span className="font-display text-sm text-turd-mustard-bright">🎁 Spawn item</span>
          <span className="rounded bg-turd-bg-soft px-2 py-0.5 text-[10px] text-turd-green">→ {targetLabel}</span>
          <input
            value={q} onChange={(e) => setQ(e.target.value)} placeholder="Search all items…" autoFocus
            className="ml-auto w-44 rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-2 py-1 text-xs text-turd-cream focus:border-turd-mustard-bright focus:outline-none"
          />
          <label className="flex items-center gap-1 text-[11px] text-turd-cream-dim">qty
            <input value={count} onChange={(e) => setCount(e.target.value.replace(/[^0-9]/g, ''))}
              className="w-12 rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-1 py-1 text-center text-xs text-turd-cream" />
          </label>
          <button onClick={onClose} className="text-sm text-turd-cream-dim hover:text-turd-cream">✕</button>
        </div>

        {!q && (
          <div className="flex flex-wrap gap-1 border-b border-turd-bronze/20 px-3 py-1.5">
            {Object.keys(cat).map((k) => (
              <button key={k} onClick={() => setTab(k)} className={tabCls(tab === k)}>{k} <span className="opacity-50">{cat[k].length}</span></button>
            ))}
          </div>
        )}

        <div className="grid min-h-0 flex-1 grid-cols-[repeat(auto-fill,minmax(96px,1fr))] content-start gap-1.5 overflow-auto p-3">
          {items.length === 0 && <p className="col-span-full p-4 text-center text-xs text-turd-cream-dim">{Object.keys(cat).length ? 'No matches.' : 'Loading catalog…'}</p>}
          {items.map((i) => (
            <button
              key={i.cls} title={i.cls}
              onClick={() => onSpawn(i.cls, n, !!i.veh)}
              className="flex flex-col items-center gap-1 rounded border border-turd-bronze/20 bg-turd-bg-mid/30 p-2 transition-colors hover:border-turd-mustard/50 hover:bg-turd-mustard/10"
            >
              {i.icon
                ? <img src={i.icon} alt="" className="h-10 w-10 object-contain" onError={(e) => { e.currentTarget.style.visibility = 'hidden'; }} />
                : <span className="flex h-10 w-10 items-center justify-center text-2xl">{i.veh ? '🚗' : '📦'}</span>}
              <span className="line-clamp-2 text-center text-[9px] leading-tight text-turd-cream">{i.name}</span>
            </button>
          ))}
        </div>
        <p className="border-t border-turd-bronze/20 px-4 py-1.5 text-[9px] text-turd-cream-dim/60">
          {Object.values(cat).reduce((s, a) => s + a.length, 0)} items · click to spawn ×{n} on {targetLabel}
        </p>
      </div>
    </div>
  );
}
