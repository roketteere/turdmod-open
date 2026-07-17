import { useCallback, useEffect, useState } from 'react';
import { useEngineHost } from '../lib/engineHost';
import { remoteDataFileGet, remoteDataFileSet } from '../lib/tauri-remote';

// Safe Zones editor — PvP-disabled areas (center coords + radius). Reads/writes
// C:\TurdMOD\data\safe_zones.json via the service /data/file endpoint. The
// safe_zones mod reads this file live (5s tick), so saves apply without restart.
// Schema: { "zones": [{ name, x, y, z, radius }] }

interface Zone { name: string; x: number; y: number; z: number; radius: number }
const FILE = 'safe_zones.json';
const BLANK: Zone = { name: '', x: 0, y: 0, z: 0, radius: 5000 };

export function SafeZonesPage() {
  const target = useEngineHost();
  const [zones, setZones] = useState<Zone[]>([]);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);

  const load = useCallback(async () => {
    setLoading(true); setMsg(null);
    try {
      const r = await remoteDataFileGet(target, FILE);
      if (!r.ok) throw new Error(r.error ?? 'read failed');
      const parsed = r.contents?.trim() ? JSON.parse(r.contents) : { zones: [] };
      setZones(Array.isArray(parsed.zones) ? parsed.zones : []);
      setDirty(false);
    } catch (e) {
      setMsg({ ok: false, text: String((e as Error)?.message ?? e) });
    } finally {
      setLoading(false);
    }
  }, [target]);

  useEffect(() => { void load(); }, [load]);

  const save = async () => {
    setSaving(true); setMsg(null);
    try {
      const bad = zones.find((z) => !z.name.trim());
      if (bad) throw new Error('every zone needs a name');
      const r = await remoteDataFileSet(target, FILE, JSON.stringify({ zones }, null, 2));
      if (!r.ok) throw new Error(r.error ?? 'write failed');
      setDirty(false);
      setMsg({ ok: true, text: `Saved ${zones.length} zone(s) — applies live within ~5s.` });
      setTimeout(() => setMsg(null), 4000);
    } catch (e) {
      setMsg({ ok: false, text: String((e as Error)?.message ?? e) });
    } finally {
      setSaving(false);
    }
  };

  const update = (i: number, patch: Partial<Zone>) => {
    setZones((zs) => zs.map((z, idx) => (idx === i ? { ...z, ...patch } : z)));
    setDirty(true);
  };
  const add = () => { setZones((zs) => [...zs, { ...BLANK }]); setDirty(true); };
  const remove = (i: number) => { setZones((zs) => zs.filter((_, idx) => idx !== i)); setDirty(true); };

  const num = 'w-24 rounded border border-turd-bronze/60 bg-turd-bg-soft px-2 py-1 font-mono text-xs text-turd-cream focus:border-turd-mustard focus:outline-none';

  return (
    <div className="flex h-full min-h-0 flex-col gap-4">
      <header className="flex items-end justify-between">
        <div>
          <p className="font-display text-[11px] font-semibold tracking-[0.32em] text-turd-mustard/90">TURDMOD</p>
          <h1 className="mt-1 font-display text-[2rem] font-bold leading-none text-turd-cream">Safe Zones</h1>
          <p className="mt-1.5 text-xs text-turd-cream-dim">
            PvP-disabled areas (center + radius). Saved zones apply live (~5s) — no restart. Target: <span className="text-turd-mustard">{target}</span>.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <button type="button" onClick={() => void load()} disabled={loading} className="rounded-lg border border-turd-bronze/50 px-3 py-1.5 font-display text-xs uppercase tracking-wider text-turd-cream-dim hover:text-turd-cream disabled:opacity-40">{loading ? 'Loading…' : 'Reload'}</button>
          <button type="button" onClick={() => void save()} disabled={saving || !dirty} className="rounded-lg border border-turd-mustard/60 bg-turd-mustard/20 px-4 py-1.5 font-display text-xs uppercase tracking-wider text-turd-mustard-bright hover:bg-turd-mustard/30 disabled:cursor-not-allowed disabled:opacity-30">{saving ? 'Saving…' : dirty ? 'Save changes' : 'Saved'}</button>
        </div>
      </header>

      {msg && <p className={`font-mono text-[11px] ${msg.ok ? 'text-turd-green' : 'text-turd-red'}`}>{msg.text}</p>}

      <section className="glass flex min-h-0 flex-1 flex-col overflow-hidden rounded-xl">
        <div className="flex items-center justify-between border-b border-turd-bronze/30 px-4 py-2">
          <h2 className="font-display text-sm uppercase tracking-widest text-turd-mustard">Zones · {zones.length}</h2>
          <button type="button" onClick={add} className="rounded border border-turd-mustard/50 bg-turd-mustard/10 px-2.5 py-1 font-display text-[11px] uppercase tracking-wider text-turd-mustard-bright hover:bg-turd-mustard/20">+ Add zone</button>
        </div>
        <div className="flex-1 overflow-auto p-3">
          {zones.length === 0 ? (
            <p className="px-2 py-8 text-center text-xs text-turd-cream-dim">No safe zones. Add one — set its center (X/Y) and radius (SCUM world units; ~5000 ≈ a small base).</p>
          ) : (
            <table className="w-full font-mono text-[11px]">
              <thead className="sticky top-0 bg-turd-bg-mid/95">
                <tr className="border-b border-turd-bronze/30 text-left text-turd-cream-dim">
                  <th className="px-2 py-2 font-normal">Name</th>
                  <th className="px-2 py-2 font-normal">X</th>
                  <th className="px-2 py-2 font-normal">Y</th>
                  <th className="px-2 py-2 font-normal">Z</th>
                  <th className="px-2 py-2 font-normal">Radius</th>
                  <th className="px-2 py-2 font-normal"></th>
                </tr>
              </thead>
              <tbody>
                {zones.map((z, i) => (
                  <tr key={i} className="border-b border-turd-bronze/10">
                    <td className="px-2 py-1.5"><input value={z.name} onChange={(e) => update(i, { name: e.target.value })} placeholder="Zone name" className="w-40 rounded border border-turd-bronze/60 bg-turd-bg-soft px-2 py-1 text-xs text-turd-cream focus:border-turd-mustard focus:outline-none" /></td>
                    <td className="px-2 py-1.5"><input type="number" value={z.x} onChange={(e) => update(i, { x: Number(e.target.value) })} className={num} /></td>
                    <td className="px-2 py-1.5"><input type="number" value={z.y} onChange={(e) => update(i, { y: Number(e.target.value) })} className={num} /></td>
                    <td className="px-2 py-1.5"><input type="number" value={z.z} onChange={(e) => update(i, { z: Number(e.target.value) })} className={num} /></td>
                    <td className="px-2 py-1.5"><input type="number" value={z.radius} onChange={(e) => update(i, { radius: Number(e.target.value) })} className={num} /></td>
                    <td className="px-2 py-1.5"><button type="button" onClick={() => remove(i)} className="rounded border border-turd-red/40 px-2 py-0.5 text-[10px] text-turd-red hover:bg-turd-red/10">remove</button></td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      </section>
    </div>
  );
}
