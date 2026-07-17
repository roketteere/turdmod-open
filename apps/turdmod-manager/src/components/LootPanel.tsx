// Loot customization control + Override editor.
//  - Commands (run as an ONLINE ADMIN char; verified the no-executor path fakes ok:true):
//    Export defaults (bootstrap Loot files) + Reload loot (applies Override edits LIVE, no restart).
//  - Override editor: pick a loot file, Load -> flat list of leaf items (game icons + editable
//    Rarity), Save writes to the Override copy; then Reload applies it. The loot tree is
//    { Name, Rarity, Children[] }; leaf nodes (no Children) are the items.
// @dep tauri-engine engineRpc; tauri-remote loot file wrappers; item-icons resolver.
import { useState, useEffect } from 'react';
import { engineRpc } from '../lib/tauri-engine';
import { remoteLootFiles, remoteLootFileGet, remoteLootFileSet } from '../lib/tauri-remote';
import { loadIconMap, iconForCode } from '../lib/item-icons';
import DraggablePanel from './DraggablePanel';

const RARITIES = ['Abundant', 'Common', 'Uncommon', 'Rare', 'VeryRare', 'ExtremelyRare'];
const lsName = () => { try { return localStorage.getItem('loot.executor') || ''; } catch { return ''; } };
const inp = { background: '#26262b', color: '#ddd', border: '1px solid #3a3a40', borderRadius: 4, padding: '3px 6px', fontSize: 12 } as const;

function collectLeaves(node: any, out: any[]) {
  if (!node || typeof node !== 'object') return;
  const kids = node.Children;
  if (Array.isArray(kids) && kids.length) { for (const k of kids) collectLeaves(k, out); }
  else if (node.Name) out.push(node);
}

export default function LootPanel({ target }: { target: string }) {
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

  useEffect(() => { loadIconMap().then(setIconMap); }, []);
  useEffect(() => {
    remoteLootFiles(target).then((r) => {
      if (r.ok && r.files) { setFiles(r.files); if (r.files.length) setSel((s) => s || r.files!.find((f) => f.includes('/Override/')) || r.files![0]); }
    }).catch(() => setFiles([]));
  }, [target]);

  const run = async (label: string, commands: string[]) => {
    const name = exec.trim();
    if (!name) { setMsg('✗ enter your ONLINE admin character name first'); setTimeout(() => setMsg(null), 7000); return; }
    try { localStorage.setItem('loot.executor', name); } catch { /* ignore */ }
    setBusy(true); setMsg(null);
    try { for (const c of commands) await engineRpc('runAdminCommand', { command: c, playerName: name }); setMsg(`✓ ${label} — sent as ${name}`); }
    catch (e) { setMsg('✗ ' + String(e)); }
    finally { setBusy(false); setTimeout(() => setMsg(null), 8000); }
  };

  const load = async () => {
    if (!sel) return;
    setBusy(true); setMsg(null);
    try {
      const r = await remoteLootFileGet(target, sel);
      if (!r.ok || r.contents == null) { setMsg('✗ ' + (r.error ?? 'load failed')); return; }
      const t = JSON.parse(r.contents);
      const lv: any[] = []; collectLeaves(t, lv);
      setTree(t); setLeaves(lv); setMsg(`loaded ${lv.length} items`);
    } catch (e) { setMsg('✗ ' + String(e)); }
    finally { setBusy(false); }
  };

  const save = async () => {
    if (!tree) return;
    const overridePath = sel.replace('/Default/', '/Override/');
    setBusy(true); setMsg(null);
    try {
      const r = await remoteLootFileSet(target, overridePath, JSON.stringify(tree, null, '\t'));
      if (r.ok) { setMsg(`✓ saved ${overridePath} — hit Reload to apply`); if (!files.includes(overridePath)) setFiles((f) => [...f, overridePath].sort()); }
      else setMsg('✗ ' + (r.error ?? 'save failed'));
    } catch (e) { setMsg('✗ ' + String(e)); }
    finally { setBusy(false); setTimeout(() => setMsg(null), 9000); }
  };

  const host = target === 'local' ? 'Local' : 'Remote';
  const shown = filter ? leaves.filter((l) => String(l.Name).toLowerCase().includes(filter.toLowerCase())) : leaves;

  return (
    <DraggablePanel title={`Loot · ${host}`} defaultCorner="bl" defaultWidth={360} defaultHeight={470} minH={200}>
      <div style={{ padding: 12, color: '#ddd', fontFamily: 'system-ui, sans-serif', fontSize: 12 }}>
        <div style={{ display: 'flex', gap: 6, alignItems: 'center', marginBottom: 8 }}>
          <span style={{ color: '#999', whiteSpace: 'nowrap' }}>Run as:</span>
          <input value={exec} onChange={(e) => setExec(e.target.value)} placeholder="online admin char (e.g. Zilla)" style={{ ...inp, flex: 1, minWidth: 0 }} />
        </div>
        <div style={{ display: 'flex', gap: 6 }}>
          <button disabled={busy} onClick={() => run('Export defaults', ['ExportDefaultLootTree', 'ExportDefaultItemSpawningParameters'])} style={{ flex: 1, background: '#26262b', color: '#ddd', border: '1px solid #3a3a40', borderRadius: 4, padding: '5px', cursor: busy ? 'wait' : 'pointer' }}>Export defaults</button>
          <button disabled={busy} onClick={() => run('Reload loot', ['ReloadLootCustomizationsAndResetSpawners'])} style={{ flex: 1, background: '#238636', color: '#fff', border: '1px solid #2ea043', borderRadius: 4, padding: '5px', fontWeight: 700, cursor: busy ? 'wait' : 'pointer' }}>⟳ Reload (live)</button>
        </div>

        <div style={{ marginTop: 10, borderTop: '1px solid #2c2c30', paddingTop: 8 }}>
          <div style={{ color: '#e8c45a', fontWeight: 700, fontSize: 11, textTransform: 'uppercase', letterSpacing: 0.5, marginBottom: 5 }}>Override editor</div>
          <div style={{ display: 'flex', gap: 6 }}>
            <select value={sel} onChange={(e) => setSel(e.target.value)} style={{ ...inp, flex: 1, minWidth: 0 }}>
              {files.length === 0 && <option value="">(no loot files — Export defaults first)</option>}
              {files.map((f) => <option key={f} value={f}>{f}</option>)}
            </select>
            <button disabled={busy || !sel} onClick={load} style={{ background: '#1f6feb', color: '#fff', border: '1px solid #388bfd', borderRadius: 4, padding: '3px 12px', fontWeight: 700, cursor: 'pointer' }}>Load</button>
          </div>
          {leaves.length > 0 && (
            <>
              <input value={filter} onChange={(e) => setFilter(e.target.value)} placeholder={`filter ${leaves.length} items…`} style={{ ...inp, width: '100%', boxSizing: 'border-box', marginTop: 6 }} />
              <div style={{ maxHeight: 190, overflowY: 'auto', marginTop: 6 }}>
                {shown.map((leaf, i) => {
                  const url = iconForCode(iconMap, String(leaf.Name));
                  return (
                    <div key={i} style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 3 }}>
                      {url ? <img src={url} alt="" width={20} height={20} style={{ objectFit: 'contain' }} onError={(e) => { (e.currentTarget as HTMLImageElement).style.visibility = 'hidden'; }} /> : <span style={{ width: 20, textAlign: 'center' }}>📦</span>}
                      <span style={{ flex: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis' }}>{leaf.Name}</span>
                      <select value={leaf.Rarity ?? ''} onChange={(e) => { leaf.Rarity = e.target.value; force((x) => x + 1); }} style={{ ...inp, fontSize: 11 }}>
                        {!RARITIES.includes(leaf.Rarity) && <option value={leaf.Rarity ?? ''}>{leaf.Rarity ?? '—'}</option>}
                        {RARITIES.map((r) => <option key={r} value={r}>{r}</option>)}
                      </select>
                    </div>
                  );
                })}
              </div>
              <button disabled={busy} onClick={save} style={{ marginTop: 6, background: '#238636', color: '#fff', border: '1px solid #2ea043', borderRadius: 4, padding: '4px 12px', fontWeight: 700, cursor: busy ? 'wait' : 'pointer' }}>Save to Override</button>
            </>
          )}
        </div>

        <div style={{ marginTop: 8, color: '#777', fontSize: 11, lineHeight: 1.4 }}>
          Edit rarities → <b>Save to Override</b> → <b>Reload</b> applies live (no restart). Higher rarity tier = rarer.
        </div>
        {msg && <div style={{ marginTop: 6, color: msg.startsWith('✓') ? '#3fb950' : '#f85149' }}>{msg}</div>}
      </div>
    </DraggablePanel>
  );
}
