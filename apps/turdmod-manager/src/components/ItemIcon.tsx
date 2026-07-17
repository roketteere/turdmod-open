// Best-effort SCUM item icon for an entity.class. The icon set (resized to 64px)
// lives in /item-icons; _index.json lists every file. We match class tokens to
// icon basenames (the class→icon relation isn't 1:1 in the data), with a glyph
// fallback. Results are cached per class.

import { useEffect, useState } from 'react';

let FILES: string[] | null = null;
let loading: Promise<void> | null = null;

async function load(): Promise<void> {
  if (FILES) return;
  if (!loading) {
    loading = (async () => {
      try {
        const idx: { file: string }[] = await fetch('/item-icons/_index.json').then((r) => r.json());
        FILES = idx.map((e) => e.file.split('/').pop() || '').filter((f) => f.toLowerCase().startsWith('ico_'));
      } catch { FILES = []; }
    })();
  }
  await loading;
}

const STOP = new Set(['es', 'inventory', 'vicinity', 'the', 'of', 'item', 'small', 'large', 'left', 'right', 'new', 'empty', 'full', 'closed', 'open']);
function tokens(cls: string): string[] {
  return cls.replace(/_ES$|_C$/i, '').toLowerCase().split(/[_\s]+/).filter((t) => t.length >= 3 && !STOP.has(t) && !/^\d+$/.test(t));
}

const cache = new Map<string, string | null>();
async function resolve(cls: string): Promise<string | null> {
  if (cache.has(cls)) return cache.get(cls)!;
  await load();
  const ts = tokens(cls);
  let best: string | null = null;
  let bestScore = 0;
  for (const f of FILES!) {
    const base = f.replace(/\.png$/i, '').replace(/^ico_/, '');
    let score = 0;
    for (const t of ts) if (base.includes(t)) score += t.length;
    if (score > bestScore) { bestScore = score; best = f; }
  }
  const result = bestScore >= 4 ? `/item-icons/${best}` : null;
  cache.set(cls, result);
  return result;
}

export function ItemIcon({ cls, glyph, size = 16 }: { cls: string; glyph: string; size?: number }) {
  const [src, setSrc] = useState<string | null>(cache.get(cls) ?? null);
  const [done, setDone] = useState(cache.has(cls));
  useEffect(() => {
    let alive = true;
    resolve(cls).then((s) => { if (alive) { setSrc(s); setDone(true); } });
    return () => { alive = false; };
  }, [cls]);
  if (src) return <img src={src} alt="" style={{ width: size, height: size }} className="shrink-0 object-contain" />;
  return <span style={{ width: size, fontSize: size - 2 }} className={`inline-block shrink-0 text-center ${done ? '' : 'opacity-50'}`}>{glyph}</span>;
}
