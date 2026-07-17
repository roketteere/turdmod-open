import { useCallback, useEffect, useMemo, useState } from 'react';
import { useEngineHost } from '../lib/engineHost';
import {
  remotePermissionsGet,
  remotePermissionsSet,
  type PermissionsState,
  type PlayerPerms,
  type ModConfig,
} from '../lib/tauri-remote';
import { SlideTabs, FadeSwap, type TabSpec } from '../lib/motion';

// Permissions — per-player tier ladder + per-command overrides. Mirrors the
// in-game !perm state exactly; writes apply live without server restart.
// @dep tauri-remote.ts:remotePermissionsGet/Set, PlayerPerms, ModConfig, PermissionsState

type TabKey = 'players' | 'commands';
const TABS: TabSpec<TabKey>[] = [
  { key: 'players', label: 'Players' },
  { key: 'commands', label: 'Commands' },
];

// Tri-state for per-player per-command override.
type OverrideState = 'inherit' | 'allow' | 'deny';

function overrideOf(map: Record<string, boolean>, cmd: string): OverrideState {
  if (!(cmd in map)) return 'inherit';
  return map[cmd] ? 'allow' : 'deny';
}

function applyOverride(map: Record<string, boolean>, cmd: string, state: OverrideState): Record<string, boolean> {
  const next = { ...map };
  if (state === 'inherit') {
    delete next[cmd];
  } else {
    next[cmd] = state === 'allow';
  }
  return next;
}

export function PermissionsPage() {
  const target = useEngineHost();
  const [tab, setTab] = useState<TabKey>('players');
  const [perms, setPerms] = useState<PermissionsState | null>(null);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);

  const load = useCallback(async () => {
    setLoading(true); setMsg(null);
    try {
      const r = await remotePermissionsGet(target);
      setPerms(r);
      setDirty(false);
    } catch (e) {
      setMsg({ ok: false, text: String((e as Error)?.message ?? e) });
    } finally {
      setLoading(false);
    }
  }, [target]);

  useEffect(() => { void load(); }, [load]);

  const save = async () => {
    if (!perms) return;
    setSaving(true); setMsg(null);
    try {
      const r = await remotePermissionsSet(target, { players: perms.players, mods: perms.mods });
      if (!r.ok) throw new Error('save failed');
      setDirty(false);
      setMsg({ ok: true, text: 'Saved — permissions apply live.' });
      setTimeout(() => setMsg(null), 4000);
    } catch (e) {
      setMsg({ ok: false, text: String((e as Error)?.message ?? e) });
    } finally {
      setSaving(false);
    }
  };

  const patchPlayers = (players: Record<string, PlayerPerms>) => {
    setPerms((p) => p ? { ...p, players } : p);
    setDirty(true);
  };

  const patchMods = (mods: Record<string, ModConfig>) => {
    setPerms((p) => p ? { ...p, mods } : p);
    setDirty(true);
  };

  return (
    <div className="flex h-full min-h-0 flex-col gap-4">
      <header className="flex items-end justify-between">
        <div>
          <p className="font-display text-[11px] font-semibold tracking-[0.32em] text-turd-mustard/90">TURDMOD</p>
          <h1 className="mt-1 font-display text-[2rem] font-bold leading-none text-turd-cream">Permissions</h1>
          <p className="mt-1.5 text-xs text-turd-cream-dim">
            Per-player tiers + per-command overrides — mirrors in-game !perm, applies live. Target: <span className="text-turd-mustard">{target}</span>.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={() => void load()}
            disabled={loading}
            className="rounded-lg border border-turd-bronze/50 px-3 py-1.5 font-display text-xs uppercase tracking-wider text-turd-cream-dim hover:text-turd-cream disabled:opacity-40"
          >
            {loading ? 'Loading…' : 'Reload'}
          </button>
          <button
            type="button"
            onClick={() => void save()}
            disabled={saving || !dirty || !perms}
            className="rounded-lg border border-turd-mustard/60 bg-turd-mustard/20 px-4 py-1.5 font-display text-xs uppercase tracking-wider text-turd-mustard-bright hover:bg-turd-mustard/30 disabled:cursor-not-allowed disabled:opacity-30"
          >
            {saving ? 'Saving…' : dirty ? 'Save' : 'Saved'}
          </button>
        </div>
      </header>

      {msg && <p className={`font-mono text-[11px] ${msg.ok ? 'text-turd-green' : 'text-turd-red'}`}>{msg.text}</p>}

      <SlideTabs tabs={TABS} value={tab} onChange={setTab} layoutId="permissions-tab" />

      <div className="min-h-0 flex-1">
        <FadeSwap swapKey={tab} className="h-full min-h-0">
          {tab === 'players' && (
            <PlayersTab
              perms={perms}
              loading={loading}
              onPlayersChange={patchPlayers}
            />
          )}
          {tab === 'commands' && (
            <CommandsTab
              perms={perms}
              loading={loading}
              onModsChange={patchMods}
            />
          )}
        </FadeSwap>
      </div>
    </div>
  );
}

// ── Players tab ────────────────────────────────────────────────────────────────

function PlayersTab({
  perms,
  loading,
  onPlayersChange,
}: {
  perms: PermissionsState | null;
  loading: boolean;
  onPlayersChange: (players: Record<string, PlayerPerms>) => void;
}) {
  const [search, setSearch] = useState('');
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [cmdSearch, setCmdSearch] = useState('');
  const [addInput, setAddInput] = useState('');

  const tiers = perms?.tiers ?? [];
  const mods = perms?.mods ?? {};

  const allPlayers = useMemo(() => {
    if (!perms) return [];
    return Object.entries(perms.players).map(([id, p]) => ({ id, ...p }));
  }, [perms]);

  const filtered = useMemo(() => {
    const q = search.toLowerCase();
    if (!q) return allPlayers;
    return allPlayers.filter((p) =>
      p.name.toLowerCase().includes(q) || p.id.toLowerCase().includes(q),
    );
  }, [allPlayers, search]);

  const sortedCmds = useMemo(() =>
    Object.keys(mods).sort((a, b) => a.localeCompare(b)),
    [mods],
  );

  const filteredCmds = useMemo(() => {
    const q = cmdSearch.toLowerCase();
    if (!q) return sortedCmds;
    return sortedCmds.filter((c) => c.toLowerCase().includes(q));
  }, [sortedCmds, cmdSearch]);

  const setPlayerTier = (id: string, tier: string) => {
    if (!perms) return;
    onPlayersChange({ ...perms.players, [id]: { ...perms.players[id], tier } });
  };

  const setOverride = (id: string, cmd: string, state: OverrideState) => {
    if (!perms) return;
    const player = perms.players[id];
    const next = applyOverride(player.mod_overrides, cmd, state);
    onPlayersChange({ ...perms.players, [id]: { ...player, mod_overrides: next } });
  };

  const bulkAllow = (id: string) => {
    if (!perms) return;
    const next = Object.fromEntries(sortedCmds.map((c) => [c, true]));
    onPlayersChange({ ...perms.players, [id]: { ...perms.players[id], mod_overrides: next } });
  };

  const bulkDeny = (id: string) => {
    if (!perms) return;
    const next = Object.fromEntries(sortedCmds.map((c) => [c, false]));
    onPlayersChange({ ...perms.players, [id]: { ...perms.players[id], mod_overrides: next } });
  };

  const clearOverrides = (id: string) => {
    if (!perms) return;
    onPlayersChange({ ...perms.players, [id]: { ...perms.players[id], mod_overrides: {} } });
  };

  const addPlayer = () => {
    if (!perms || !addInput.trim()) return;
    const v = addInput.trim();
    const isSteamId = /^\d{17}$/.test(v);
    const key = isSteamId ? v : `pending_${v.toLowerCase()}`;
    if (perms.players[key]) { setAddInput(''); return; }
    onPlayersChange({
      ...perms.players,
      [key]: { name: isSteamId ? key : v, tier: tiers[0] ?? '', mod_overrides: {} },
    });
    setAddInput('');
  };

  const isPending = (id: string) => id.startsWith('pending_');

  if (loading && !perms) {
    return <p className="py-8 text-center text-xs text-turd-cream-dim">Loading…</p>;
  }

  return (
    <div className="flex h-full min-h-0 flex-col gap-3">
      <div className="flex items-center gap-2">
        <input
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder="Search name or Steam ID…"
          className="flex-1 rounded-lg border border-turd-bronze/50 bg-turd-bg-soft px-3 py-1.5 font-mono text-xs text-turd-cream placeholder-turd-cream-dim/50 focus:border-turd-mustard focus:outline-none"
        />
        <div className="flex items-center gap-1.5">
          <input
            value={addInput}
            onChange={(e) => setAddInput(e.target.value)}
            onKeyDown={(e) => { if (e.key === 'Enter') addPlayer(); }}
            placeholder="SteamID64 or name…"
            className="w-48 rounded-lg border border-turd-bronze/50 bg-turd-bg-soft px-3 py-1.5 font-mono text-xs text-turd-cream placeholder-turd-cream-dim/50 focus:border-turd-mustard focus:outline-none"
          />
          <button
            type="button"
            onClick={addPlayer}
            disabled={!addInput.trim() || !perms}
            className="rounded-lg border border-turd-mustard/50 bg-turd-mustard/10 px-3 py-1.5 font-display text-[11px] uppercase tracking-wider text-turd-mustard-bright hover:bg-turd-mustard/20 disabled:opacity-30"
          >
            + Add
          </button>
        </div>
      </div>

      <section className="glass min-h-0 flex-1 overflow-auto rounded-xl">
        {filtered.length === 0 ? (
          <p className="px-2 py-8 text-center text-xs text-turd-cream-dim">
            {perms ? 'No players match.' : 'Failed to load.'}
          </p>
        ) : (
          <div className="divide-y divide-turd-bronze/10">
            {filtered.map(({ id, name, tier, mod_overrides }) => {
              const expanded = expandedId === id;
              const overrideCount = Object.keys(mod_overrides).length;
              return (
                <div key={id}>
                  {/* Player row */}
                  <div
                    className={`flex cursor-pointer items-center gap-3 px-4 py-2.5 hover:bg-turd-bg-mid/20 ${expanded ? 'bg-turd-mustard/5' : ''}`}
                    onClick={() => setExpandedId(expanded ? null : id)}
                  >
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-2">
                        <span className="font-mono text-xs text-turd-cream">{name || id}</span>
                        {isPending(id) && (
                          <span className="rounded-full border border-turd-mustard/40 bg-turd-mustard/10 px-1.5 py-0 font-display text-[9px] uppercase tracking-wider text-turd-mustard">
                            pending
                          </span>
                        )}
                        {overrideCount > 0 && (
                          <span className="rounded-full border border-turd-bronze/40 px-1.5 py-0 font-mono text-[9px] text-turd-cream-dim">
                            {overrideCount} override{overrideCount !== 1 ? 's' : ''}
                          </span>
                        )}
                      </div>
                      <p className="font-mono text-[10px] text-turd-cream-dim/60">{id}</p>
                    </div>
                    <select
                      value={tier}
                      onChange={(e) => { e.stopPropagation(); setPlayerTier(id, e.target.value); }}
                      onClick={(e) => e.stopPropagation()}
                      className="rounded border border-turd-bronze/60 bg-turd-bg-soft px-2 py-1 font-mono text-xs text-turd-cream focus:border-turd-mustard focus:outline-none"
                    >
                      {tiers.map((t) => (
                        <option key={t} value={t}>{t}</option>
                      ))}
                    </select>
                    <span className="font-mono text-[10px] text-turd-cream-dim/50">{expanded ? '▲' : '▼'}</span>
                  </div>

                  {/* Expanded override editor */}
                  {expanded && (
                    <div className="border-t border-turd-bronze/15 bg-turd-bg-mid/20 px-4 pb-3 pt-2">
                      <div className="mb-2 flex items-center gap-2">
                        <span className="font-display text-[10px] uppercase tracking-wider text-turd-cream-dim">Per-command overrides</span>
                        <div className="ml-auto flex items-center gap-1.5">
                          <button
                            type="button"
                            onClick={() => bulkAllow(id)}
                            className="rounded border border-turd-green/40 px-2 py-0.5 font-display text-[10px] uppercase tracking-wider text-turd-green hover:bg-turd-green/10"
                          >
                            Allow all
                          </button>
                          <button
                            type="button"
                            onClick={() => bulkDeny(id)}
                            className="rounded border border-turd-red/40 px-2 py-0.5 font-display text-[10px] uppercase tracking-wider text-turd-red hover:bg-turd-red/10"
                          >
                            Deny all
                          </button>
                          <button
                            type="button"
                            onClick={() => clearOverrides(id)}
                            className="rounded border border-turd-bronze/50 px-2 py-0.5 font-display text-[10px] uppercase tracking-wider text-turd-cream-dim hover:text-turd-cream"
                          >
                            Clear
                          </button>
                        </div>
                      </div>
                      <input
                        value={cmdSearch}
                        onChange={(e) => setCmdSearch(e.target.value)}
                        placeholder="Filter commands…"
                        className="mb-2 w-full rounded border border-turd-bronze/40 bg-turd-bg-soft px-2 py-1 font-mono text-[11px] text-turd-cream placeholder-turd-cream-dim/40 focus:border-turd-mustard focus:outline-none"
                        onClick={(e) => e.stopPropagation()}
                      />
                      <div className="max-h-56 overflow-y-auto">
                        <table className="w-full font-mono text-[11px]">
                          <thead className="sticky top-0 bg-turd-bg-mid/95">
                            <tr className="border-b border-turd-bronze/30 text-left text-turd-cream-dim">
                              <th className="px-2 py-1 font-normal">Command</th>
                              <th className="px-2 py-1 font-normal">Min tier</th>
                              <th className="px-2 py-1 font-normal">Override</th>
                            </tr>
                          </thead>
                          <tbody>
                            {filteredCmds.map((cmd) => {
                              const cfg = mods[cmd];
                              const ov = overrideOf(mod_overrides, cmd);
                              return (
                                <tr key={cmd} className="border-b border-turd-bronze/10 text-turd-cream">
                                  <td className="px-2 py-1">{cmd}</td>
                                  <td className="px-2 py-1 text-turd-mustard">{cfg?.required_tier ?? '—'}</td>
                                  <td className="px-2 py-1">
                                    <TriState
                                      value={ov}
                                      onChange={(s) => setOverride(id, cmd, s)}
                                    />
                                  </td>
                                </tr>
                              );
                            })}
                          </tbody>
                        </table>
                      </div>
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </section>
    </div>
  );
}

// Tri-state toggle: Inherit | Allow | Deny
function TriState({ value, onChange }: { value: OverrideState; onChange: (s: OverrideState) => void }) {
  const STATES: OverrideState[] = ['inherit', 'allow', 'deny'];
  const next = () => onChange(STATES[(STATES.indexOf(value) + 1) % STATES.length]);
  const label = value === 'inherit' ? 'Inherit' : value === 'allow' ? 'Allow' : 'Deny';
  const cls =
    value === 'allow'
      ? 'border-turd-green/50 bg-turd-green/15 text-turd-green'
      : value === 'deny'
      ? 'border-turd-red/50 bg-turd-red/15 text-turd-red'
      : 'border-turd-bronze/50 bg-transparent text-turd-cream-dim';
  return (
    <button
      type="button"
      onClick={(e) => { e.stopPropagation(); next(); }}
      className={`min-w-[4.5rem] rounded border px-2 py-0.5 font-display text-[10px] uppercase tracking-wider transition-colors ${cls} hover:opacity-80`}
    >
      {label}
    </button>
  );
}

// ── Commands tab ───────────────────────────────────────────────────────────────

function CommandsTab({
  perms,
  loading,
  onModsChange,
}: {
  perms: PermissionsState | null;
  loading: boolean;
  onModsChange: (mods: Record<string, ModConfig>) => void;
}) {
  const [search, setSearch] = useState('');

  const tiers = perms?.tiers ?? [];

  const sorted = useMemo(() =>
    Object.keys(perms?.mods ?? {}).sort((a, b) => a.localeCompare(b)),
    [perms],
  );

  const filtered = useMemo(() => {
    const q = search.toLowerCase();
    if (!q) return sorted;
    return sorted.filter((c) => c.toLowerCase().includes(q));
  }, [sorted, search]);

  const setEnabled = (cmd: string, enabled: boolean) => {
    if (!perms) return;
    onModsChange({ ...perms.mods, [cmd]: { ...perms.mods[cmd], enabled } });
  };

  const setRequiredTier = (cmd: string, required_tier: string) => {
    if (!perms) return;
    onModsChange({ ...perms.mods, [cmd]: { ...perms.mods[cmd], required_tier } });
  };

  const topTier = tiers[tiers.length - 1] ?? '';

  if (loading && !perms) {
    return <p className="py-8 text-center text-xs text-turd-cream-dim">Loading…</p>;
  }

  return (
    <div className="flex h-full min-h-0 flex-col gap-3">
      <div className="flex items-center gap-2">
        <input
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder="Filter commands…"
          className="flex-1 rounded-lg border border-turd-bronze/50 bg-turd-bg-soft px-3 py-1.5 font-mono text-xs text-turd-cream placeholder-turd-cream-dim/50 focus:border-turd-mustard focus:outline-none"
        />
        <span className="font-mono text-[11px] text-turd-cream-dim">{filtered.length} / {sorted.length}</span>
      </div>

      <section className="glass min-h-0 flex-1 overflow-auto rounded-xl">
        {filtered.length === 0 ? (
          <p className="px-2 py-8 text-center text-xs text-turd-cream-dim">
            {perms ? 'No commands match.' : 'Failed to load.'}
          </p>
        ) : (
          <table className="w-full font-mono text-[11px]">
            <thead className="sticky top-0 bg-turd-bg-mid/95">
              <tr className="border-b border-turd-bronze/30 text-left text-turd-cream-dim">
                <th className="px-3 py-2 font-normal">Command / Feature</th>
                <th className="px-3 py-2 font-normal">Enabled</th>
                <th className="px-3 py-2 font-normal">Min Tier</th>
                <th className="px-3 py-2 font-normal">Tag</th>
              </tr>
            </thead>
            <tbody>
              {filtered.map((cmd) => {
                const cfg = perms!.mods[cmd];
                const isAdmin = cfg.required_tier === topTier;
                return (
                  <tr key={cmd} className="border-b border-turd-bronze/10 text-turd-cream">
                    <td className="px-3 py-1.5">{cmd}</td>
                    <td className="px-3 py-1.5">
                      <input
                        type="checkbox"
                        checked={cfg.enabled}
                        onChange={(e) => setEnabled(cmd, e.target.checked)}
                        className="accent-turd-mustard"
                      />
                    </td>
                    <td className="px-3 py-1.5">
                      <select
                        value={cfg.required_tier}
                        onChange={(e) => setRequiredTier(cmd, e.target.value)}
                        className="rounded border border-turd-bronze/60 bg-turd-bg-soft px-2 py-0.5 text-xs text-turd-cream focus:border-turd-mustard focus:outline-none"
                      >
                        {tiers.map((t) => (
                          <option key={t} value={t}>{t}</option>
                        ))}
                      </select>
                    </td>
                    <td className="px-3 py-1.5">
                      {isAdmin && (
                        <span className="rounded-full border border-turd-mustard/40 bg-turd-mustard/10 px-1.5 py-0 font-display text-[9px] uppercase tracking-wider text-turd-mustard">
                          admin
                        </span>
                      )}
                    </td>
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
