// ItemBrowser — searchable, category-filtered grid of every in-game item with its icon.
// Source: /item-icons/_index.json (4106 entries: exportName=ICO_<item>, assetPath, file).
// Category is derived from the asset path; the picked `code` is the exportName with the ICO_
// prefix stripped (editable downstream — SCUM tradeable-codes mostly match but admin can correct).
// Used by TraderItemsEditor to add tradeables to a trader. @dep public/item-icons/_index.json.
import { useState, useEffect, useMemo } from 'react';

interface IconEntry { exportName: string; assetPath: string; file: string }
interface Item { code: string; label: string; category: string; icon: string }

// "SCUM/Content/.../Items/First_aid/ICO_Scalpel" -> "First_aid"; falls back to a sane bucket.
function categoryOf(assetPath: string): string {
  const parts = assetPath.split('/');
  const i = parts.findIndex((p) => p === 'Items' || p === 'Item');
  const seg = i >= 0 && parts[i + 1] ? parts[i + 1] : parts[parts.length - 2] || 'Other';
  return seg.replace(/_/g, ' ');
}

const stripIco = (s: string) => s.replace(/^ICO_/i, '');

export default function ItemBrowser({ onPick, onClose }: { onPick: (code: string, label: string) => void; onClose?: () => void }) {
  const [items, setItems] = useState<Item[]>([]);
  const [q, setQ] = useState('');
  const [cat, setCat] = useState('All');
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    fetch('/item-icons/_index.json')
      .then((r) => r.json())
      .then((arr: IconEntry[]) => {
        const seen = new Set<string>();
        const out: Item[] = [];
        for (const e of arr) {
          if (!e.exportName || !e.file) continue;
          if (/^ICO_trader_/i.test(e.exportName)) continue;
          const code = stripIco(e.exportName);
          if (seen.has(code.toLowerCase())) continue;
          seen.add(code.toLowerCase());
          out.push({ code, label: code.replace(/_/g, ' '), category: categoryOf(e.assetPath), icon: `/item-icons/${e.file}` });
        }
        out.sort((a, b) => a.label.localeCompare(b.label));
        setItems(out);
      })
      .catch((e) => setErr(String(e)));
  }, []);

  const categories = useMemo(() => ['All', ...Array.from(new Set(items.map((i) => i.category))).sort()], [items]);
  const shown = useMemo(() => {
    const needle = q.trim().toLowerCase();
    return items.filter((i) => (cat === 'All' || i.category === cat) && (!needle || i.label.toLowerCase().includes(needle) || i.code.toLowerCase().includes(needle))).slice(0, 600);
  }, [items, q, cat]);

  const inputCls = 'rounded border border-turd-bronze/50 bg-turd-bg-deep/70 px-2 py-1 text-sm text-turd-cream focus:border-turd-mustard focus:outline-none';

  return (
    <div className="flex h-full min-h-0 flex-col rounded-lg border border-turd-bronze/40 bg-turd-bg-deep/95">
      <div className="flex shrink-0 items-center gap-2 border-b border-turd-bronze/20 p-3">
        <span className="font-display text-sm text-turd-cream">Add item</span>
        <input autoFocus value={q} onChange={(e) => setQ(e.target.value)} placeholder={`search ${items.length} items…`} className={`${inputCls} flex-1`} />
        <select value={cat} onChange={(e) => setCat(e.target.value)} className={`${inputCls} max-w-[180px]`}>
          {categories.map((c) => <option key={c} value={c}>{c}</option>)}
        </select>
        {onClose && <button onClick={onClose} className="rounded border border-turd-bronze/50 px-2 py-1 text-xs text-turd-cream-dim hover:text-turd-cream">✕</button>}
      </div>
      <div className="min-h-0 flex-1 overflow-auto p-2">
        {err && <div className="p-3 text-xs text-red-400">⚠ {err}</div>}
        <div className="grid grid-cols-[repeat(auto-fill,minmax(120px,1fr))] gap-1.5">
          {shown.map((it) => (
            <button key={it.code} onClick={() => onPick(it.code, it.label)} title={it.code}
              className="flex flex-col items-center gap-1 rounded border border-turd-bronze/20 bg-turd-bg-mid/40 p-2 text-center hover:border-turd-mustard/60 hover:bg-turd-bg-soft">
              <img src={it.icon} alt="" className="h-10 w-10 object-contain" onError={(e) => { (e.currentTarget as HTMLImageElement).style.visibility = 'hidden'; }} />
              <span className="w-full truncate text-[10px] text-turd-cream">{it.label}</span>
            </button>
          ))}
        </div>
        {shown.length === 0 && !err && <div className="p-4 text-center text-xs text-turd-cream-dim">No items match.</div>}
        {shown.length === 600 && <div className="p-2 text-center text-[10px] text-turd-cream-dim/60">Showing first 600 — refine the search.</div>}
      </div>
    </div>
  );
}
