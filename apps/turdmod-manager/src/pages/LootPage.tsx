// Loot manager — a real page (nav tab), not a map panel. Top bar runs the loot admin commands as an
// online admin (export defaults / live reload). Below, pick a loot file -> a searchable GRID of every
// item with its game icon front-and-center + an inline Rarity dropdown -> Save writes the Override
// copy -> Reload applies live (no restart). Loot tree = {Name,Rarity,Children[]}; leaves are items.
import { useState, useEffect, useRef } from 'react';
import { useEngineHost } from '../lib/engineHost';
import { engineRpc } from '../lib/tauri-engine';
import { remoteLootFiles, remoteLootFileGet, remoteLootFileSet } from '../lib/tauri-remote';
import { loadIconMap, iconForCode } from '../lib/item-icons';

const RARITIES = ['Abundant', 'Common', 'Uncommon', 'Rare', 'VeryRare', 'ExtremelyRare'];
const RARITY_COLOR: Record<string, string> = {
  Abundant: '#9aa0a6', Common: '#86efac', Uncommon: '#22c55e', Rare: '#60a5fa', VeryRare: '#a78bfa', ExtremelyRare: '#f59e0b',
};
const lsName = () => { try { return localStorage.getItem('loot.executor') || ''; } catch { return ''; } };

function collectLeaves(node: any, out: any[]) {
  if (!node || typeof node !== 'object') return;
  const kids = node.Children;
  if (Array.isArray(kids) && kids.length) { for (const k of kids) collectLeaves(k, out); }
  else if (node.Name) out.push(node);
}

export function LootPage() {
  const target = useEngineHost();
  const [exec, setExec] = useState(lsName);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [files, setFiles] = useState<string[]>([]);
  const [sel, setSel] = useState('');
  const [tree, setTree] = useState<any>(null);
  const [leaves, setLeaves] = useState<any[]>([]);
  const [filter, setFilter] = useState('');
  const [iconMap, setIconMap] = useState<Map<string, string> | null>(null);
  const [, force] = useState(0);
  const origRarity = useRef<Map<any, string>>(new Map()); // leaf ref -> rarity at load, for staged-change count
  const [countdown, setCountdown] = useState(30);

  useEffect(() => { loadIconMap().then(setIconMap); }, []);
  useEffect(() => {
    remoteLootFiles(target).then((r) => {
      if (r.ok && r.files) { setFiles(r.files); setSel((s) => s || r.files!.find((f) => f.includes('/Override/')) || r.files![0] || ''); }
      else setFiles([]);
    }).catch(() => setFiles([]));
  }, [target]);

  const flash = (m: string) => { setMsg(m); setTimeout(() => setMsg(null), 8000); };

  const run = async (label: string, commands: string[]) => {
    const name = exec.trim();
    if (!name) return flash('✗ enter your ONLINE admin character name (top-right) first');
    try { localStorage.setItem('loot.executor', name); } catch { /* ignore */ }
    setBusy(true);
    try { for (const c of commands) await engineRpc('runAdminCommand', { command: c, playerName: name }); flash(`✓ ${label} — sent as ${name}`); }
    catch (e) { flash('✗ ' + String(e)); }
    finally { setBusy(false); }
  };

  const loadFile = async (path: string) => {
    if (!path) return;
    setBusy(true); setMsg(null);
    try {
      const r = await remoteLootFileGet(target, path);
      if (!r.ok || r.contents == null) return flash('✗ ' + (r.error ?? 'load failed'));
      const t = JSON.parse(r.contents);
      const lv: any[] = []; collectLeaves(t, lv);
      setTree(t); setLeaves(lv);
      origRarity.current = new Map(lv.map((l) => [l, l.Rarity]));
      flash(`loaded ${lv.length} items from ${path}`);
    } catch (e) { flash('✗ ' + String(e)); }
    finally { setBusy(false); }
  };

  // Auto-load the selected server's table whenever the file (or server) changes — no manual Load.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => { if (sel) loadFile(sel); }, [sel, target]);

  const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

  // Write the staged tree to the Override copy (no reload). Returns ok.
  const writeOverride = async (): Promise<boolean> => {
    if (!tree) return false;
    const overridePath = sel.replace('/Default/', '/Override/');
    const r = await remoteLootFileSet(target, overridePath, JSON.stringify(tree, null, '\t'));
    if (r.ok) {
      if (!files.includes(overridePath)) setFiles((f) => [...f, overridePath].sort());
      origRarity.current = new Map(leaves.map((l) => [l, l.Rarity])); // staged -> committed
      return true;
    }
    flash('✗ ' + (r.error ?? 'save failed'));
    return false;
  };

  // Apply path 1: save Override, take effect on the NEXT restart (no mid-session spawner reset).
  const saveForRestart = async () => {
    setBusy(true); setMsg(null);
    try { if (await writeOverride()) { flash('✓ saved to Override — applies on next restart'); force((x) => x + 1); } }
    catch (e) { flash('✗ ' + String(e)); }
    finally { setBusy(false); }
  };

  // Apply path 2: announce a countdown to players, then write + live Reload.
  const announceAndApply = async () => {
    const name = exec.trim();
    if (!name) return flash('✗ enter your ONLINE admin char (top-right) to reload live');
    try { localStorage.setItem('loot.executor', name); } catch { /* ignore */ }
    setBusy(true); setMsg(null);
    try {
      if (countdown > 0) {
        const marks = [...new Set([countdown, Math.floor(countdown / 2), 5])].filter((m) => m > 0).sort((a, b) => b - a);
        await engineRpc('broadcastChat', { text: `[ScummyMap] ⚠ Loot tables updating in ${countdown}s…`, channel: '6' });
        let prev = countdown;
        for (const m of marks) {
          await sleep((prev - m) * 1000);
          if (m !== countdown) await engineRpc('broadcastChat', { text: `[ScummyMap] ⚠ Loot update in ${m}s…`, channel: '6' });
          prev = m;
        }
        await sleep(prev * 1000);
      }
      if (!(await writeOverride())) return;
      await engineRpc('runAdminCommand', { command: 'ReloadLootCustomizationsAndResetSpawners', playerName: name });
      await engineRpc('broadcastChat', { text: '[ScummyMap] ✓ Loot tables updated!', channel: '6' });
      flash('✓ announced, saved & reloaded live'); force((x) => x + 1);
    } catch (e) { flash('✗ ' + String(e)); }
    finally { setBusy(false); }
  };

  const shown = filter ? leaves.filter((l) => String(l.Name).toLowerCase().includes(filter.toLowerCase())) : leaves;
  const dirty = leaves.filter((l) => origRarity.current.get(l) !== l.Rarity).length;
  const inputCls = 'rounded border border-turd-bronze/50 bg-turd-bg-deep/70 px-2 py-1 text-sm text-turd-cream focus:border-turd-mustard focus:outline-none';

  return (
    <div className="flex h-full flex-col gap-4">
      <header className="flex items-start justify-between gap-4">
        <div>
          <p className="font-display text-xs tracking-[0.3em] text-turd-mustard">TurdMOD</p>
          <h1 className="mt-0.5 font-display text-2xl text-turd-cream">Loot Manager</h1>
          <p className="mt-1 text-xs text-turd-cream-dim">Edit drop rarities with real item icons, save to Override, reload live — no restart.</p>
        </div>
        <div className="flex flex-wrap items-center justify-end gap-2">
          <label className="flex items-center gap-1.5 text-xs text-turd-cream-dim">
            run as
            <input value={exec} onChange={(e) => setExec(e.target.value)} placeholder="online admin (e.g. Zilla)" className={`${inputCls} w-44`} />
          </label>
          <button disabled={busy} onClick={() => run('Export defaults', ['ExportDefaultLootTree', 'ExportDefaultItemSpawningParameters'])} className="rounded border border-turd-bronze/50 bg-turd-bg-deep/70 px-3 py-1.5 text-xs text-turd-cream-dim hover:text-turd-cream">Export defaults</button>
          <button disabled={busy} onClick={() => run('Reload loot', ['ReloadLootCustomizationsAndResetSpawners'])} className="rounded border border-turd-green/50 bg-turd-green/15 px-3 py-1.5 text-xs font-bold text-turd-green hover:bg-turd-green/25">⟳ Reload loot (live)</button>
        </div>
      </header>

      <div className="flex flex-wrap items-center gap-2 border-y border-turd-bronze/20 py-2">
        <span className="text-xs uppercase tracking-wider text-turd-cream-dim/60">File</span>
        <select value={sel} onChange={(e) => setSel(e.target.value)} className={`${inputCls} min-w-[260px]`}>
          {files.length === 0 && <option value="">(no loot files — Export defaults first)</option>}
          {files.map((f) => <option key={f} value={f}>{f}</option>)}
        </select>
        <button disabled={busy || !sel} onClick={() => loadFile(sel)} title="discard staged edits + re-fetch this file" className="rounded border border-turd-bronze/50 bg-turd-bg-deep/70 px-3 py-1.5 text-xs text-turd-cream-dim hover:text-turd-cream">↻ Refresh</button>
        {tree && (
          <div className="flex items-center gap-2">
            <span className={dirty ? 'rounded bg-turd-mustard/15 px-2 py-0.5 text-xs font-bold text-turd-mustard-bright' : 'text-xs text-turd-cream-dim/40'}>{dirty} staged</span>
            <button disabled={busy || !dirty} onClick={saveForRestart} title="write Override now; takes effect on the next server restart (no live spawner reset)" className="rounded border border-turd-bronze/50 bg-turd-bg-deep/70 px-3 py-1.5 text-xs text-turd-cream-dim hover:text-turd-cream disabled:opacity-40">Save for next restart</button>
            <label className="flex items-center gap-1 text-xs text-turd-cream-dim">warn
              <select value={countdown} onChange={(e) => setCountdown(Number(e.target.value))} className={inputCls}>
                <option value={0}>now</option><option value={15}>15s</option><option value={30}>30s</option><option value={60}>60s</option>
              </select>
            </label>
            <button disabled={busy || !dirty} onClick={announceAndApply} title="announce a countdown to players, then save + live reload" className="rounded border border-turd-green/50 bg-turd-green/15 px-3 py-1.5 text-xs font-bold text-turd-green hover:bg-turd-green/25 disabled:opacity-40">📣 Announce &amp; apply</button>
          </div>
        )}
        {leaves.length > 0 && <input value={filter} onChange={(e) => setFilter(e.target.value)} placeholder={`filter ${leaves.length} items…`} className={`${inputCls} ml-auto w-56`} />}
        {msg && <span className={`ml-2 text-xs ${msg.startsWith('✓') ? 'text-turd-green' : 'text-red-400'}`}>{msg}</span>}
      </div>

      <div className="min-h-0 flex-1 overflow-auto">
        {leaves.length === 0 ? (
          <div className="flex h-full items-center justify-center text-sm text-turd-cream-dim/60">
            {files.length === 0
              ? <span>No loot files on this server yet — click <span className="mx-1 font-bold text-turd-cream">Export defaults</span> (needs an admin online).</span>
              : busy ? 'Loading the server’s loot tables…' : 'Select a loot file above.'}
          </div>
        ) : (
          <div className="grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-2">
            {shown.map((leaf, i) => {
              const url = iconForCode(iconMap, String(leaf.Name));
              return (
                <div key={i} className="flex flex-col items-center gap-1 rounded border border-turd-bronze/25 bg-turd-bg-deep/50 p-2">
                  {url
                    ? <img src={url} alt="" className="h-12 w-12 object-contain" onError={(e) => { (e.currentTarget as HTMLImageElement).style.visibility = 'hidden'; }} />
                    : <div className="flex h-12 w-12 items-center justify-center text-2xl">📦</div>}
                  <span className="w-full truncate text-center text-[11px] text-turd-cream" title={String(leaf.Name)}>{String(leaf.Name).replace(/_/g, ' ')}</span>
                  <select
                    value={RARITIES.includes(leaf.Rarity) ? leaf.Rarity : ''}
                    onChange={(e) => { leaf.Rarity = e.target.value; force((x) => x + 1); }}
                    className="w-full rounded border border-turd-bronze/40 bg-turd-bg-deep px-1 py-0.5 text-[11px]"
                    style={{ color: RARITY_COLOR[leaf.Rarity] ?? '#ddd' }}
                  >
                    {!RARITIES.includes(leaf.Rarity) && <option value="">{leaf.Rarity ?? '—'}</option>}
                    {RARITIES.map((r) => <option key={r} value={r} style={{ color: RARITY_COLOR[r] }}>{r}</option>)}
                  </select>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
