import { useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { useEngineHost, setEngineHost } from '../lib/engineHost';

// SCUM.db viewer — a clean, website-style window into all 161 game tables
// (players, vehicles, squads, banks, quests, bunkers…). Reads the live DB via
// the service /scumdb/* endpoints (local or OVH) — read-only, WAL-safe.
// @dep tauri cmds: scumdb_tables / scumdb_rows (remote_commands.rs → service).

type Target = 'local' | 'remote';
const PAGE = 100;

interface TablesResp { tables: string[]; count: number }
interface RowsResp {
  table: string;
  columns: string[];
  rows: Record<string, unknown>[];
  total: number;
  limit: number;
  offset: number;
}

// Friendly label for a snake_case table name: prisoner_skill → "Prisoner Skill".
function pretty(name: string): string {
  return name.replace(/_/g, ' ').replace(/\b\w/g, (c) => c.toUpperCase());
}

function fmtCell(v: unknown): string {
  if (v === null || v === undefined) return '—';
  if (typeof v === 'number') return Number.isInteger(v) ? String(v) : v.toFixed(3);
  const s = String(v);
  return s.length > 120 ? s.slice(0, 117) + '…' : s;
}

export function ScumDbPage() {
  // Bound to the global server selector (sidebar) — one source of truth.
  const target = useEngineHost();
  const [selected, setSelected] = useState<string | null>(null);
  const [tableFilter, setTableFilter] = useState('');
  const [page, setPage] = useState(0);
  const [rowFilter, setRowFilter] = useState('');

  const tablesQuery = useQuery<TablesResp>({
    queryKey: ['scumdb-tables', target],
    queryFn: () => invoke('scumdb_tables', { target }),
    retry: false,
  });

  const rowsQuery = useQuery<RowsResp>({
    queryKey: ['scumdb-rows', target, selected, page],
    queryFn: () => invoke('scumdb_rows', { target, table: selected, limit: PAGE, offset: page * PAGE }),
    enabled: !!selected,
    retry: false,
  });

  const tables = useMemo(() => {
    const all = tablesQuery.data?.tables ?? [];
    const q = tableFilter.trim().toLowerCase();
    return q ? all.filter((t) => t.toLowerCase().includes(q)) : all;
  }, [tablesQuery.data, tableFilter]);

  const visibleRows = useMemo(() => {
    const rows = rowsQuery.data?.rows ?? [];
    const q = rowFilter.trim().toLowerCase();
    if (!q) return rows;
    return rows.filter((r) => Object.values(r).some((v) => String(v ?? '').toLowerCase().includes(q)));
  }, [rowsQuery.data, rowFilter]);

  const total = rowsQuery.data?.total ?? 0;
  const cols = rowsQuery.data?.columns ?? [];
  const pages = Math.max(1, Math.ceil(total / PAGE));

  const selectTable = (t: string) => { setSelected(t); setPage(0); setRowFilter(''); };

  return (
    <div className="flex h-full flex-col gap-3">
      <header className="flex items-end justify-between">
        <div>
          <p className="font-display text-xs tracking-[0.3em] text-turd-mustard">TurdMOD</p>
          <h1 className="mt-1 font-display text-3xl text-turd-cream">Game Database</h1>
          <p className="mt-1 text-xs text-turd-cream-dim">
            Live SCUM.db — {tablesQuery.data?.count ?? '…'} tables, read-only.
          </p>
        </div>
        {/* Server selector */}
        <div className="flex overflow-hidden rounded border border-turd-bronze/40">
          {(['local', 'remote'] as Target[]).map((t) => (
            <button
              key={t}
              onClick={() => { setEngineHost(t); setSelected(null); }}
              className={`px-4 py-1.5 text-xs font-medium transition-colors ${
                target === t ? 'bg-turd-mustard-bright text-turd-bg-deep' : 'bg-turd-bg-deep/60 text-turd-cream-dim hover:text-turd-cream'
              }`}
            >
              {t === 'local' ? '🏠 Local' : '☁ OVH'}
            </button>
          ))}
        </div>
      </header>

      <div className="grid min-h-0 flex-1 grid-cols-[260px_1fr] gap-3">
        {/* Table list */}
        <aside className="flex min-h-0 flex-col glass rounded-xl">
          <div className="border-b border-turd-bronze/30 p-2">
            <input
              value={tableFilter}
              onChange={(e) => setTableFilter(e.target.value)}
              placeholder={`Search ${tablesQuery.data?.count ?? ''} tables…`}
              className="w-full rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-2 py-1 font-mono text-xs text-turd-cream placeholder:text-turd-cream-dim/50 focus:border-turd-mustard-bright focus:outline-none"
            />
          </div>
          <div className="flex-1 overflow-auto p-1">
            {tablesQuery.isPending && <p className="p-3 text-xs text-turd-cream-dim">Loading…</p>}
            {tablesQuery.isError && (
              <p className="p-3 text-xs text-turd-red">
                {target === 'remote' ? 'OVH unreachable (start the tunnel).' : 'Local service not responding.'}
              </p>
            )}
            {tables.map((t) => (
              <button
                key={t}
                onClick={() => selectTable(t)}
                className={`block w-full truncate rounded px-2 py-1 text-left font-mono text-[11px] transition-colors ${
                  selected === t ? 'bg-turd-mustard/20 text-turd-mustard-bright' : 'text-turd-cream hover:bg-white/5'
                }`}
                title={t}
              >
                {pretty(t)}
              </button>
            ))}
          </div>
        </aside>

        {/* Rows */}
        <section className="flex min-h-0 flex-col glass rounded-xl">
          {!selected ? (
            <div className="flex flex-1 items-center justify-center p-8 text-center">
              <p className="text-sm text-turd-cream-dim">Pick a table on the left to browse its data.</p>
            </div>
          ) : (
            <>
              <div className="flex flex-wrap items-center gap-3 border-b border-turd-bronze/30 p-2">
                <h2 className="font-display text-sm text-turd-mustard-bright">{pretty(selected)}</h2>
                <span className="font-mono text-[10px] text-turd-cream-dim">
                  {total.toLocaleString()} rows · {cols.length} cols
                </span>
                <input
                  value={rowFilter}
                  onChange={(e) => setRowFilter(e.target.value)}
                  placeholder="Filter this page…"
                  className="ml-auto w-48 rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-2 py-1 font-mono text-[11px] text-turd-cream placeholder:text-turd-cream-dim/50 focus:border-turd-mustard-bright focus:outline-none"
                />
                <div className="flex items-center gap-1">
                  <button
                    onClick={() => setPage((p) => Math.max(0, p - 1))}
                    disabled={page === 0}
                    className="rounded border border-turd-bronze/40 px-2 py-1 font-mono text-[11px] text-turd-cream-dim hover:text-turd-cream disabled:opacity-30"
                  >‹</button>
                  <span className="font-mono text-[10px] text-turd-cream-dim">{page + 1}/{pages}</span>
                  <button
                    onClick={() => setPage((p) => Math.min(pages - 1, p + 1))}
                    disabled={page >= pages - 1}
                    className="rounded border border-turd-bronze/40 px-2 py-1 font-mono text-[11px] text-turd-cream-dim hover:text-turd-cream disabled:opacity-30"
                  >›</button>
                </div>
              </div>
              <div className="flex-1 overflow-auto">
                {rowsQuery.isPending && <p className="p-3 text-xs text-turd-cream-dim">Loading rows…</p>}
                {rowsQuery.isError && <p className="p-3 text-xs text-turd-red">Failed to load rows: {String((rowsQuery.error as Error)?.message ?? rowsQuery.error)}</p>}
                {rowsQuery.data && (rowsQuery.data.total ?? 0) === 0 && <p className="p-3 text-xs text-turd-cream-dim">This table is empty (0 rows).</p>}
                {rowsQuery.data && (
                  <table className="w-full border-collapse text-[11px]">
                    <thead className="sticky top-0 bg-turd-bg-deep">
                      <tr>
                        {cols.map((c) => (
                          <th key={c} className="border-b border-turd-bronze/30 px-2 py-1.5 text-left font-mono font-semibold text-turd-mustard">
                            {c}
                          </th>
                        ))}
                      </tr>
                    </thead>
                    <tbody>
                      {visibleRows.map((r, i) => (
                        <tr key={i} className="hover:bg-white/5">
                          {cols.map((c) => (
                            <td key={c} className="max-w-xs truncate border-b border-turd-bronze/10 px-2 py-1 font-mono text-turd-cream" title={String(r[c] ?? '')}>
                              {fmtCell(r[c])}
                            </td>
                          ))}
                        </tr>
                      ))}
                      {visibleRows.length === 0 && (
                        <tr><td colSpan={cols.length} className="p-4 text-center text-xs text-turd-cream-dim">No rows match.</td></tr>
                      )}
                    </tbody>
                  </table>
                )}
              </div>
            </>
          )}
        </section>
      </div>
    </div>
  );
}
