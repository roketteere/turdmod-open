// Rich player character sheet — vitals, attributes, skills, full inventory.
// Pulls /scumdb/player-card + /scumdb/player-inventory (host-aware) for the
// selected player. Works for online AND offline (it's DB data).

import { useQuery } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { useMemo, useState } from 'react';
import type { EngineHost } from '../lib/engineHost';
import { scumdbSetSkill, scumdbSetAttribute, type SkillEdit, type AttrEdit } from '../lib/tauri-remote';
import { ItemIcon } from './ItemIcon';

interface Skill { name: string; level: number | null; xp: number | null }
interface PlayerCard {
  name: string; bank: number; gold: number; fame: number | null; alive: boolean | null;
  playTime: number | null;
  attributes: Record<string, number>;
  vitals: Record<string, number>;
  skills: Skill[];
}
interface InvItem { entityId: number; class: string; owner: number }
interface Inventory { count: number; cashItems: number; items: InvItem[] }

// "LuisMoncada_Jacket_ES" -> "Luis Moncada Jacket"
function pretty(cls: string): string {
  return cls.replace(/_ES$/, '').replace(/_C$/, '').replace(/_/g, ' ')
    .replace(/([a-z])([A-Z])/g, '$1 $2').replace(/\s+/g, ' ').trim();
}
// Category glyph + bucket from the class keywords.
function categorize(cls: string): { glyph: string; bucket: string } {
  const c = cls.toLowerCase();
  if (/(jacket|pants|boots|shirt|sock|glove|hat|cap|vest|coat|kilt|underwear|boxer|brief|shorts|skirt|dress|shoe)/.test(c)) return { glyph: '👕', bucket: 'Clothing' };
  if (/(backpack|quiver|pouch|bag|container|crate|chest)/.test(c)) return { glyph: '🎒', bucket: 'Containers' };
  if (/(rifle|pistol|gun|axe|knife|machete|sword|bow|weapon|hacha|spear|bat|club|hammer)/.test(c)) return { glyph: '🔪', bucket: 'Weapons' };
  if (/(magazine|ammo|cal_|round|bullet|shell|pile)/.test(c)) return { glyph: '🔫', bucket: 'Ammo' };
  if (/(mre|stew|food|meat|bread|water|drink|can|fruit|veg|cheese|burger|berry|fish)/.test(c)) return { glyph: '🍖', bucket: 'Food/Drink' };
  if (/(lock|key|card)/.test(c)) return { glyph: '🔒', bucket: 'Keys/Locks' };
  if (/(medical|bandage|pill|syringe|antibiotic|painkiller|splint)/.test(c)) return { glyph: '💊', bucket: 'Medical' };
  return { glyph: '📦', bucket: 'Other' };
}

const VITAL_LABELS: Record<string, string> = {
  Stamina: 'Stamina', HeartRate: 'Heart rate', BodyTemperature: 'Body temp', Mass: 'Mass',
};

export function PlayerSheet({ steam, target, online }: {
  steam: string; target: EngineHost; online?: boolean;
}) {
  const [attrEdit, setAttrEdit] = useState<Record<string, string>>({});
  const [skillName, setSkillName] = useState('');
  const [skillLvl, setSkillLvl] = useState('');
  const [skillXp, setSkillXp] = useState('');
  const [savingSkill, setSavingSkill] = useState(false);
  const [skillMsg, setSkillMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [savingAttr, setSavingAttr] = useState(false);
  const [attrMsg, setAttrMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const card = useQuery<PlayerCard>({
    queryKey: ['player-card', target, steam],
    queryFn: () => invoke('scumdb_player_card', { target, steam }),
    retry: false, refetchInterval: 15000,
  });
  const inv = useQuery<Inventory>({
    queryKey: ['player-inventory', target, steam],
    queryFn: () => invoke('scumdb_player_inventory', { target, steam }),
    retry: false, refetchInterval: 30000,
  });

  // Group inventory by bucket → counted class lines.
  const grouped = useMemo(() => {
    const buckets = new Map<string, Map<string, number>>();
    for (const it of inv.data?.items ?? []) {
      const { bucket } = categorize(it.class);
      const m = buckets.get(bucket) ?? new Map();
      m.set(it.class, (m.get(it.class) ?? 0) + 1);
      buckets.set(bucket, m);
    }
    const order = ['Weapons', 'Ammo', 'Clothing', 'Containers', 'Food/Drink', 'Medical', 'Keys/Locks', 'Other'];
    return order.filter((b) => buckets.has(b)).map((b) => ({
      bucket: b,
      items: [...(buckets.get(b)!.entries())].map(([cls, n]) => ({ cls, n })).sort((a, b2) => a.cls.localeCompare(b2.cls)),
    }));
  }, [inv.data]);

  const c = card.data;
  const stamina = c?.vitals?.Stamina;

  const saveSkill = async () => {
    if (!skillName) return;
    setSavingSkill(true);
    setSkillMsg(null);
    try {
      const edit: SkillEdit = { name: skillName };
      if (skillLvl) edit.level = Number(skillLvl);
      if (skillXp) edit.xp = Number(skillXp);
      const r = await scumdbSetSkill(target, steam, [edit]);
      if (!r.ok) throw new Error(r.error || 'write failed');
      setSkillMsg({ ok: true, text: `Saved ${skillName}${r.restarted ? ' · server restarting…' : ''} (backup ${r.backup ?? '—'})` });
      setTimeout(() => void card.refetch(), r.restarted ? 9000 : 1200);
    } catch (e) {
      setSkillMsg({ ok: false, text: String(e) });
    } finally {
      setSavingSkill(false);
    }
  };

  const saveAttrs = async () => {
    const attrs: AttrEdit[] = ['Strength', 'Constitution', 'Dexterity', 'Intelligence']
      .filter((a) => attrEdit[a] != null && attrEdit[a] !== '')
      .map((a) => ({ name: a, value: Number(attrEdit[a]) }));
    if (attrs.length === 0) {
      setAttrMsg({ ok: false, text: 'Enter at least one value to set.' });
      return;
    }
    setSavingAttr(true);
    setAttrMsg(null);
    try {
      const r = await scumdbSetAttribute(target, steam, attrs);
      if (!r.ok) throw new Error(r.error || 'write failed');
      setAttrMsg({ ok: true, text: `Saved ${r.updated ?? 0} attr${r.restarted ? ' · server restarting…' : ''} (backup ${r.backup ?? '—'})` });
      setAttrEdit({});
      setTimeout(() => void card.refetch(), r.restarted ? 9000 : 1200);
    } catch (e) {
      setAttrMsg({ ok: false, text: String(e) });
    } finally {
      setSavingAttr(false);
    }
  };

  return (
    <div className="space-y-2.5 px-3 py-2 text-[11px]">
      {card.isError && <p className="text-[10px] text-turd-red">Card unavailable (server/DB unreachable).</p>}

      {/* Vitals */}
      {c && (Object.keys(c.vitals).length > 0 || Object.keys(c.attributes).length > 0) && (
        <div>
          <div className="mb-1 flex items-center gap-1.5 font-display text-[10px] uppercase tracking-wider text-turd-mustard">
            <img src="/game-icons/metabolism.png" alt="" className="h-3.5 w-3.5" onError={(e) => (e.currentTarget.style.display = 'none')} /> Vitals
          </div>
          {stamina != null && (
            <div className="mb-1 flex items-center gap-2 font-mono text-[10px] text-turd-cream-dim">
              <span className="w-16">Stamina</span>
              <div className="h-1.5 flex-1 rounded-full bg-turd-bg-soft">
                <div className="h-full rounded-full bg-turd-green" style={{ width: `${Math.max(0, Math.min(100, stamina))}%` }} />
              </div>
              <span className="w-8 text-right text-turd-cream">{Math.round(stamina)}</span>
            </div>
          )}
          <div className="grid grid-cols-2 gap-x-3 gap-y-0.5 font-mono text-[10px] text-turd-cream-dim">
            {Object.entries(c.vitals).filter(([k]) => k !== 'Stamina').map(([k, v]) => (
              <div key={k} className="flex justify-between"><span>{VITAL_LABELS[k] ?? k}</span><span className="text-turd-cream">{v.toFixed(1)}</span></div>
            ))}
          </div>
        </div>
      )}

      {/* Attributes */}
      {c && Object.keys(c.attributes).length > 0 && (
        <div>
          <div className="mb-1 font-display text-[10px] uppercase tracking-wider text-turd-mustard">Attributes</div>
          <div className="grid grid-cols-4 gap-1 text-center font-mono">
            {['Strength', 'Constitution', 'Dexterity', 'Intelligence'].map((a) => (
              <div key={a} className="rounded border border-turd-bronze/20 bg-turd-bg-deep/60 py-1">
                <div className="text-[8px] uppercase text-turd-cream-dim/70">{a.slice(0, 3)}</div>
                <div className="text-turd-mustard-bright">{c.attributes[a] != null ? c.attributes[a].toFixed(1) : '—'}</div>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Skills — level + raw XP (the same XP the in-game skills screen shows). */}
      {c && c.skills.length > 0 && (
        <div>
          <div className="mb-1 font-display text-[10px] uppercase tracking-wider text-turd-mustard">Skills · {c.skills.length}</div>
          <div className="max-h-44 space-y-0.5 overflow-auto font-mono text-[10px]">
            {[...c.skills].sort((a, b) => (b.xp ?? 0) - (a.xp ?? 0)).map((s) => (
              <div key={s.name} className="flex items-center gap-2">
                <span className="w-24 shrink-0 truncate text-turd-cream-dim" title={s.name}>{s.name}</span>
                <span className={`w-6 shrink-0 text-right ${s.level && s.level > 0 ? 'text-turd-green' : 'text-turd-cream-dim/40'}`}>L{s.level ?? 0}</span>
                <span className="flex-1 text-right text-turd-cream-dim/70">
                  {s.xp != null ? Math.round(s.xp).toLocaleString() : '—'}<span className="text-turd-cream-dim/40"> xp</span>
                </span>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Edit attributes — save-edit of the body_simulation blob (works online & offline). */}
      {c && Object.keys(c.attributes).length > 0 && (
        <div className="space-y-1 rounded border border-turd-bronze/20 bg-turd-bg-mid/20 p-1.5">
          <div className="font-display text-[9px] uppercase tracking-wider text-turd-mustard">Edit attributes · base STR/CON/DEX/INT</div>
          <div className="flex items-center gap-1">
            {['Strength', 'Constitution', 'Dexterity', 'Intelligence'].map((a) => (
              <input key={a} value={attrEdit[a] ?? ''} title={a}
                onChange={(e) => setAttrEdit((v) => ({ ...v, [a]: e.target.value.replace(/[^0-9.]/g, '') }))}
                placeholder={c.attributes[a] != null ? c.attributes[a].toFixed(1) : a.slice(0, 3)}
                className="w-full min-w-0 rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-1 py-0.5 text-center text-[9px] text-turd-cream" />
            ))}
            <button disabled={savingAttr} onClick={saveAttrs}
              className="shrink-0 rounded border border-turd-bronze/40 px-1.5 py-0.5 text-[9px] text-turd-cream hover:bg-white/5 disabled:opacity-30">{savingAttr ? '…' : 'Set'}</button>
          </div>
          <p className="text-[8px] leading-tight text-turd-cream-dim/50">
            Writes the save (backs up scum.db first; briefly restarts the server{online ? ' — this player is online and will be disconnected' : ''}).
          </p>
          {attrMsg && <p className={`text-[9px] ${attrMsg.ok ? 'text-turd-green' : 'text-turd-red'}`}>{attrMsg.text}</p>}
        </div>
      )}

      {/* Edit a skill — save-edit (works online & offline; backs up scum.db + briefly restarts). */}
      {c && c.skills.length > 0 && (
        <div className="space-y-1 rounded border border-turd-bronze/20 bg-turd-bg-mid/20 p-1.5">
          <div className="font-display text-[9px] uppercase tracking-wider text-turd-mustard">Edit skill · level &amp; XP</div>
          <div className="flex items-center gap-1">
            <select value={skillName}
              onChange={(e) => {
                const n = e.target.value; setSkillName(n);
                const sk = c.skills.find((s) => s.name === n);
                setSkillLvl(sk?.level != null ? String(sk.level) : '');
                setSkillXp(sk?.xp != null ? String(Math.round(sk.xp)) : '');
              }}
              className="min-w-0 flex-1 rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-1 py-0.5 text-[9px] text-turd-cream">
              <option value="">skill…</option>
              {[...c.skills].sort((a, b) => a.name.localeCompare(b.name)).map((s) => <option key={s.name} value={s.name}>{s.name}</option>)}
            </select>
            <input value={skillLvl} onChange={(e) => setSkillLvl(e.target.value.replace(/[^0-9]/g, ''))} placeholder="Lv" title="Level"
              className="w-8 rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-1 py-0.5 text-center text-[9px] text-turd-cream" />
            <input value={skillXp} onChange={(e) => setSkillXp(e.target.value.replace(/[^0-9]/g, ''))} placeholder="XP" title="XP"
              className="w-16 rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-1 py-0.5 text-center text-[9px] text-turd-cream" />
            <button disabled={!skillName || savingSkill || (!skillLvl && !skillXp)} onClick={saveSkill}
              className="shrink-0 rounded border border-turd-bronze/40 px-1.5 py-0.5 text-[9px] text-turd-cream hover:bg-white/5 disabled:opacity-30">{savingSkill ? '…' : 'Save'}</button>
          </div>
          <p className="text-[8px] leading-tight text-turd-cream-dim/50">Writes the save (backs up scum.db first; briefly restarts the server if it's running).</p>
          {skillMsg && <p className={`text-[9px] ${skillMsg.ok ? 'text-turd-green' : 'text-turd-red'}`}>{skillMsg.text}</p>}
        </div>
      )}

      {/* Inventory */}
      <div>
        <div className="mb-1 font-display text-[10px] uppercase tracking-wider text-turd-mustard">
          Inventory · {inv.data?.count ?? (inv.isPending ? '…' : 0)}
        </div>
        {inv.isError && <p className="text-[10px] text-turd-red">Inventory unavailable.</p>}
        <div className="max-h-48 space-y-1.5 overflow-auto">
          {grouped.map((g) => (
            <div key={g.bucket}>
              <div className="text-[9px] uppercase tracking-wider text-turd-cream-dim/60">{g.bucket}</div>
              {g.items.map(({ cls, n }) => {
                const { glyph } = categorize(cls);
                return (
                  <div key={cls} className="flex items-center gap-1.5 font-mono text-[10px] text-turd-cream">
                    <ItemIcon cls={cls} glyph={glyph} size={18} />
                    <span className="flex-1 truncate" title={cls}>{pretty(cls)}</span>
                    {n > 1 && <span className="text-turd-mustard-bright">×{n}</span>}
                  </div>
                );
              })}
            </div>
          ))}
          {!inv.isPending && (inv.data?.count ?? 0) === 0 && !inv.isError && (
            <p className="text-[10px] text-turd-cream-dim/50">Empty / no character.</p>
          )}
        </div>
      </div>
    </div>
  );
}
