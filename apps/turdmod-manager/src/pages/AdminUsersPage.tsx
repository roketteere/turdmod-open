// Admin Users editor — view/edit AdminUsers.ini + ServerSettingsAdminUsers.ini on
// the selected host (OVH / local) with per-permission checkboxes. Reads/writes via
// the service's /admin/file endpoint (RemoteClient). SCUM reads these at boot, so
// changes need a server restart to apply — the page offers "Save & Restart".
//
// AdminUsers.ini line format: `<steamid64>[Token1,Token2,...]`
// ServerSettingsAdminUsers.ini: bare `<steamid64>` per line (settings-admin tier).
// @dep tauri-remote.ts:remoteAdminFileGet/Set, admin-permissions.ts

import { useCallback, useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useEngineHost } from '../lib/engineHost';
import {
  remoteAdminFileGet,
  remoteAdminFileSet,
  remoteServerRestart,
} from '../lib/tauri-remote';
import { PERMISSION_GROUPS, ALL_PERMISSIONS } from '../data/admin-permissions';

const AU = 'AdminUsers.ini';
const SS = 'ServerSettingsAdminUsers.ini';
const STEAM_RE = /^7656119\d{10}$/;

interface AdminRow {
  steamId: string;
  name?: string;
  perms: string[]; // AdminUsers.ini permission tokens
  superAdmin: boolean; // present in ServerSettingsAdminUsers.ini
}

function parseAdminUsers(raw: string): { steamId: string; perms: string[] }[] {
  const out: { steamId: string; perms: string[] }[] = [];
  for (const line of raw.split(/\r?\n/)) {
    const t = line.trim();
    if (!t || t.startsWith('#') || t.startsWith(';')) continue;
    const m = t.match(/^(\d{17})\s*\[(.*)\]\s*$/);
    if (m) {
      out.push({ steamId: m[1], perms: m[2].split(',').map((s) => s.trim()).filter(Boolean) });
    } else if (/^\d{17}$/.test(t)) {
      out.push({ steamId: t, perms: [] });
    }
  }
  return out;
}

function parseIdList(raw: string): string[] {
  return raw
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter((l) => /^\d{17}$/.test(l));
}

function serializeAdminUsers(rows: AdminRow[]): string {
  return rows.map((r) => `${r.steamId}[${r.perms.join(',')}]`).join('\r\n') + '\r\n';
}

function serializeSettingsAdmins(rows: AdminRow[]): string {
  const ids = rows.filter((r) => r.superAdmin).map((r) => r.steamId);
  return ids.length ? ids.join('\r\n') + '\r\n' : '';
}

export function AdminUsersPage() {
  const target = useEngineHost(); // 'local' | 'remote'(OVH)
  const [rows, setRows] = useState<AdminRow[]>([]);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [msg, setMsg] = useState<{ kind: 'ok' | 'err' | 'info'; text: string } | null>(null);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [newId, setNewId] = useState('');

  const load = useCallback(async () => {
    setLoading(true);
    setMsg(null);
    try {
      const [au, ss] = await Promise.all([
        remoteAdminFileGet(target, AU),
        remoteAdminFileGet(target, SS),
      ]);
      const admins = parseAdminUsers(au.contents ?? '');
      const superIds = new Set(parseIdList(ss.contents ?? ''));
      // merge: every id in either file becomes a row
      const byId = new Map<string, AdminRow>();
      for (const a of admins) byId.set(a.steamId, { steamId: a.steamId, perms: a.perms, superAdmin: superIds.has(a.steamId) });
      for (const id of superIds) {
        if (!byId.has(id)) byId.set(id, { steamId: id, perms: [], superAdmin: true });
      }
      const merged = [...byId.values()];
      // best-effort name resolution from the live roster
      try {
        const roster = await invoke<{ players?: { steam: string; name: string }[] }>('monitor_players', { target });
        const nameBy = new Map((roster.players ?? []).map((p) => [p.steam, p.name]));
        for (const r of merged) r.name = nameBy.get(r.steamId);
      } catch {
        /* roster optional — server may be offline */
      }
      setRows(merged);
      setDirty(false);
    } catch (e) {
      setMsg({ kind: 'err', text: `Load failed: ${String(e)}` });
    } finally {
      setLoading(false);
    }
  }, [target]);

  useEffect(() => {
    load();
  }, [load]);

  const mutate = (fn: (draft: AdminRow[]) => AdminRow[]) => {
    setRows((prev) => fn(prev.map((r) => ({ ...r, perms: [...r.perms] }))));
    setDirty(true);
  };

  const togglePerm = (steamId: string, perm: string) =>
    mutate((draft) =>
      draft.map((r) =>
        r.steamId === steamId
          ? { ...r, perms: r.perms.includes(perm) ? r.perms.filter((p) => p !== perm) : [...r.perms, perm] }
          : r,
      ),
    );

  const setPerms = (steamId: string, perms: string[]) =>
    mutate((draft) => draft.map((r) => (r.steamId === steamId ? { ...r, perms } : r)));

  const toggleSuper = (steamId: string) =>
    mutate((draft) => draft.map((r) => (r.steamId === steamId ? { ...r, superAdmin: !r.superAdmin } : r)));

  const removeAdmin = (steamId: string) => mutate((draft) => draft.filter((r) => r.steamId !== steamId));

  const addAdmin = () => {
    const id = newId.trim();
    if (!STEAM_RE.test(id)) {
      setMsg({ kind: 'err', text: 'Enter a valid SteamID64 (17 digits, starts 7656119).' });
      return;
    }
    if (rows.some((r) => r.steamId === id)) {
      setMsg({ kind: 'err', text: 'That SteamID is already in the list.' });
      return;
    }
    // default a new admin to the full owner set + settings-admin, matching how
    // you/Zilla are configured (the common case for granting a trusted admin).
    mutate((draft) => [...draft, { steamId: id, perms: [...ALL_PERMISSIONS], superAdmin: true }]);
    setNewId('');
    setExpanded(id);
  };

  const save = useCallback(
    async (thenRestart: boolean) => {
      setSaving(true);
      setMsg(null);
      try {
        const au = serializeAdminUsers(rows);
        const ss = serializeSettingsAdmins(rows);
        const r1 = await remoteAdminFileSet(target, AU, au);
        if (!r1.ok) throw new Error(r1.error || 'AdminUsers.ini write failed');
        const r2 = await remoteAdminFileSet(target, SS, ss);
        if (!r2.ok) throw new Error(r2.error || 'ServerSettingsAdminUsers.ini write failed');
        setDirty(false);
        if (thenRestart) {
          setMsg({ kind: 'info', text: 'Saved. Restarting server to apply…' });
          await remoteServerRestart(target, 0);
          setMsg({ kind: 'ok', text: 'Saved and server restarting — permissions apply on boot.' });
        } else {
          setMsg({ kind: 'ok', text: 'Saved. Restart the server (or use Save & Restart) for SCUM to apply the changes.' });
        }
      } catch (e) {
        setMsg({ kind: 'err', text: `Save failed: ${String(e)}` });
      } finally {
        setSaving(false);
      }
    },
    [rows, target],
  );

  const hostLabel = target === 'local' ? '🏠 Local' : '☁ OVH';

  return (
    <div className="mx-auto max-w-5xl text-turd-cream">
      <div className="mb-4 flex items-center justify-between gap-3">
        <div>
          <h1 className="font-display text-xl font-black tracking-wide text-turd-cream">Admin Users</h1>
          <p className="text-xs text-turd-cream-dim">
            Editing <span className="text-turd-mustard-bright">{hostLabel}</span> — AdminUsers.ini + ServerSettingsAdminUsers.ini
          </p>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={load}
            disabled={loading || saving}
            className="rounded border border-turd-bronze/40 px-3 py-1.5 text-xs text-turd-cream-dim hover:bg-turd-bg-soft/40 disabled:opacity-40"
          >
            {loading ? 'Loading…' : 'Reload'}
          </button>
          <button
            onClick={() => save(false)}
            disabled={!dirty || saving || loading}
            className="rounded border border-turd-bronze/40 bg-turd-bg-soft px-3 py-1.5 text-xs text-turd-cream hover:bg-turd-bg-soft/70 disabled:opacity-40"
          >
            {saving ? 'Saving…' : 'Save'}
          </button>
          <button
            onClick={() => save(true)}
            disabled={!dirty || saving || loading}
            className="rounded border border-turd-mustard-bright/50 bg-turd-mustard-bright/20 px-3 py-1.5 text-xs font-semibold text-turd-mustard-bright hover:bg-turd-mustard-bright/30 disabled:opacity-40"
          >
            Save &amp; Restart
          </button>
        </div>
      </div>

      {msg && (
        <div
          className={[
            'mb-3 rounded border px-3 py-2 text-xs',
            msg.kind === 'ok'
              ? 'border-green-500/40 bg-green-500/10 text-green-300'
              : msg.kind === 'err'
                ? 'border-red-500/40 bg-red-500/10 text-red-300'
                : 'border-turd-bronze/40 bg-turd-bg-soft/40 text-turd-cream-dim',
          ].join(' ')}
        >
          {msg.text}
        </div>
      )}

      {dirty && (
        <div className="mb-3 text-[11px] text-turd-mustard-bright">● Unsaved changes — SCUM applies admin files on the next server restart.</div>
      )}

      {/* Add admin */}
      <div className="mb-4 flex items-center gap-2 rounded border border-turd-bronze/30 bg-turd-bg-mid/30 p-3">
        <input
          value={newId}
          onChange={(e) => setNewId(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && addAdmin()}
          placeholder="SteamID64 (e.g. 76561198xxxxxxxxx)"
          className="flex-1 rounded border border-turd-bronze/30 bg-turd-bg-deep px-3 py-1.5 text-sm text-turd-cream placeholder:text-turd-cream-dim/40 focus:border-turd-mustard-bright/50 focus:outline-none"
        />
        <button
          onClick={addAdmin}
          className="rounded border border-turd-bronze/40 bg-turd-bg-soft px-3 py-1.5 text-xs text-turd-cream hover:bg-turd-bg-soft/70"
        >
          + Add admin (full rights)
        </button>
      </div>

      {rows.length === 0 && !loading && (
        <p className="text-sm text-turd-cream-dim">No admins configured on {hostLabel}.</p>
      )}

      <div className="space-y-2">
        {rows.map((r) => (
          <AdminCard
            key={r.steamId}
            row={r}
            others={rows.filter((o) => o.steamId !== r.steamId)}
            open={expanded === r.steamId}
            onToggleOpen={() => setExpanded(expanded === r.steamId ? null : r.steamId)}
            onTogglePerm={(p) => togglePerm(r.steamId, p)}
            onSetPerms={(p) => setPerms(r.steamId, p)}
            onToggleSuper={() => toggleSuper(r.steamId)}
            onRemove={() => removeAdmin(r.steamId)}
          />
        ))}
      </div>
    </div>
  );
}

interface AdminCardProps {
  row: AdminRow;
  others: AdminRow[];
  open: boolean;
  onToggleOpen: () => void;
  onTogglePerm: (perm: string) => void;
  onSetPerms: (perms: string[]) => void;
  onToggleSuper: () => void;
  onRemove: () => void;
}

function AdminCard({ row, others, open, onToggleOpen, onTogglePerm, onSetPerms, onToggleSuper, onRemove }: AdminCardProps) {
  const permSet = useMemo(() => new Set(row.perms), [row.perms]);
  // tokens present in the file that we don't recognize (keep them, show them)
  const unknown = row.perms.filter((p) => !ALL_PERMISSIONS.includes(p));

  return (
    <div className="rounded border border-turd-bronze/30 bg-turd-bg-mid/20">
      <div className="flex items-center justify-between gap-3 px-3 py-2">
        <button onClick={onToggleOpen} className="flex flex-1 items-center gap-3 text-left">
          <span className="text-turd-cream-dim">{open ? '▾' : '▸'}</span>
          <div>
            <div className="text-sm font-semibold text-turd-cream">
              {row.name ?? <span className="text-turd-cream-dim">unknown name</span>}
              {row.superAdmin && (
                <span className="ml-2 rounded bg-turd-mustard-bright/20 px-1.5 py-0.5 text-[9px] uppercase tracking-wide text-turd-mustard-bright">
                  settings admin
                </span>
              )}
            </div>
            <div className="font-mono text-[11px] text-turd-cream-dim">{row.steamId}</div>
          </div>
        </button>
        <div className="flex items-center gap-3">
          <span className="text-[11px] text-turd-cream-dim">
            {row.perms.length}/{ALL_PERMISSIONS.length}
          </span>
          <button
            onClick={onRemove}
            className="rounded border border-red-500/30 px-2 py-1 text-[11px] text-red-300/80 hover:bg-red-500/10"
          >
            Remove
          </button>
        </div>
      </div>

      {open && (
        <div className="border-t border-turd-bronze/20 px-3 py-3">
          <div className="mb-3 flex flex-wrap items-center gap-2 text-[11px]">
            <label className="flex items-center gap-1.5 text-turd-cream-dim">
              <input type="checkbox" checked={row.superAdmin} onChange={onToggleSuper} />
              Settings admin (ServerSettingsAdminUsers.ini)
            </label>
            <span className="text-turd-cream-dim/40">|</span>
            <button onClick={() => onSetPerms([...ALL_PERMISSIONS])} className="rounded border border-turd-bronze/40 px-2 py-0.5 hover:bg-turd-bg-soft/40">
              Full set
            </button>
            <button onClick={() => onSetPerms([])} className="rounded border border-turd-bronze/40 px-2 py-0.5 hover:bg-turd-bg-soft/40">
              Clear
            </button>
            <CopyFrom others={others} onCopy={(perms) => onSetPerms(perms)} />
          </div>

          <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
            {PERMISSION_GROUPS.map((g) => {
              const allOn = g.perms.every((p) => permSet.has(p));
              return (
                <div key={g.label} className="rounded border border-turd-bronze/20 bg-turd-bg-deep/40 p-2">
                  <div className="mb-1.5 flex items-center justify-between">
                    <span className="text-[11px] font-semibold uppercase tracking-wide text-turd-mustard-bright/80">{g.label}</span>
                    <button
                      onClick={() => {
                        const next = allOn
                          ? row.perms.filter((p) => !g.perms.includes(p))
                          : [...new Set([...row.perms, ...g.perms])];
                        onSetPerms(next);
                      }}
                      className="text-[10px] text-turd-cream-dim hover:text-turd-cream"
                    >
                      {allOn ? 'none' : 'all'}
                    </button>
                  </div>
                  <div className="grid grid-cols-1 gap-0.5">
                    {g.perms.map((p) => (
                      <label key={p} className="flex items-center gap-1.5 text-[11px] text-turd-cream-dim hover:text-turd-cream">
                        <input type="checkbox" checked={permSet.has(p)} onChange={() => onTogglePerm(p)} />
                        {p}
                      </label>
                    ))}
                  </div>
                </div>
              );
            })}
          </div>

          {unknown.length > 0 && (
            <div className="mt-2 text-[10px] text-turd-cream-dim/60">
              Kept (unrecognized) tokens: {unknown.join(', ')}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function CopyFrom({ others, onCopy }: { others: AdminRow[]; onCopy: (perms: string[]) => void }) {
  if (others.length === 0) return null;
  return (
    <select
      defaultValue=""
      onChange={(e) => {
        const src = others.find((o) => o.steamId === e.target.value);
        if (src) onCopy([...src.perms]);
        e.currentTarget.value = '';
      }}
      className="rounded border border-turd-bronze/40 bg-turd-bg-deep px-2 py-0.5 text-[11px] text-turd-cream-dim"
    >
      <option value="">Copy from…</option>
      {others.map((o) => (
        <option key={o.steamId} value={o.steamId}>
          {o.name ?? o.steamId} ({o.perms.length})
        </option>
      ))}
    </select>
  );
}
