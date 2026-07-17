// Forensic Archive page — browse the append-only keys.jsonl history.
//
// Reads `C:/Development/Claude/scumdump/data/archive/keys.jsonl` via
// the dump_archive_entries Tauri command. Every key/offset/hash/sig
// we resolve gets recorded here, append-only, so when SCUM patches
// and rotates the AES key (or moves the pak validator) the OLD values
// stay banked forever.

import { useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import {
  type ArchiveEntry,
  type ArchiveListing,
  dumpArchiveEntries,
} from '../lib/tauri-dump';
import { formatTs } from '../lib/format-ts';

function truncate(s: string, n: number): string {
  if (s.length <= n) return s;
  return `${s.slice(0, n - 1)}…`;
}

export function DumpArchivePage() {
  const [keyTypeFilter, setKeyTypeFilter] = useState<string>('');
  const [buildFilter, setBuildFilter] = useState<string>('');

  const { data, isLoading } = useQuery<ArchiveListing>({
    queryKey: ['dump', 'archive', keyTypeFilter, buildFilter],
    queryFn: () =>
      dumpArchiveEntries({
        keyType: keyTypeFilter || undefined,
        build: buildFilter || undefined,
        limit: 500,
      }),
    refetchInterval: 10_000,
    staleTime: 5_000,
  });

  const grouped = useMemo(() => {
    if (!data?.entries) return new Map<string, ArchiveEntry[]>();
    const g = new Map<string, ArchiveEntry[]>();
    for (const e of data.entries) {
      const arr = g.get(e.keyType) ?? [];
      arr.push(e);
      g.set(e.keyType, arr);
    }
    return g;
  }, [data?.entries]);

  if (isLoading) {
    return (
      <p className="font-display text-xs uppercase tracking-widest text-turd-cream-dim/40">
        Loading archive…
      </p>
    );
  }

  if (!data || data.total === 0) {
    return (
      <div className="space-y-4">
        <header>
          <p className="font-display text-xs uppercase tracking-[0.4em] text-turd-mustard">
            Builder · Forensic Archive
          </p>
          <h1 className="mt-1 font-display text-3xl text-turd-cream md:text-4xl">
            Keys & Offsets History
          </h1>
        </header>
        <section className="glass rounded-xl p-5">
          <p className="text-sm text-turd-cream-dim">
            No entries archived yet. Each Phase B / Phase C run writes
            to{' '}
            <code className="rounded bg-turd-bg-deep/60 px-1.5 py-0.5">
              scumdump/data/archive/keys.jsonl
            </code>{' '}
            — run any phase to seed the archive.
          </p>
        </section>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <header>
        <p className="font-display text-xs uppercase tracking-[0.4em] text-turd-mustard">
          Builder · Forensic Archive
        </p>
        <h1 className="mt-1 font-display text-3xl text-turd-cream md:text-4xl">
          Keys & Offsets History
        </h1>
        <p className="mt-2 max-w-2xl text-sm text-turd-cream-dim">
          Append-only record of every AES key, RVA, hash, and signature
          we resolve. When SCUM updates and rotates a key, the old value
          stays banked here forever. {data.total} total entries across{' '}
          {data.keyTypes.length} key types and {data.builds.length} build
          versions.
        </p>
        {data.archivePath && (
          <p className="mt-2 font-mono text-xs text-turd-cream-dim/60">
            {data.archivePath}
          </p>
        )}
      </header>

      <section className="glass rounded-xl p-4">
        <div className="flex flex-wrap items-end gap-4">
          <div>
            <label
              htmlFor="kt-filter"
              className="block text-[10px] uppercase tracking-widest text-turd-mustard"
            >
              Key type
            </label>
            <select
              id="kt-filter"
              value={keyTypeFilter}
              onChange={(e) => setKeyTypeFilter(e.target.value)}
              className="mt-1 rounded border border-turd-bronze/40 bg-turd-bg-deep/60 px-3 py-1.5 text-sm text-turd-cream"
            >
              <option value="">All ({data.keyTypes.length})</option>
              {data.keyTypes.map((kt) => (
                <option key={kt} value={kt}>
                  {kt}
                </option>
              ))}
            </select>
          </div>
          <div>
            <label
              htmlFor="build-filter"
              className="block text-[10px] uppercase tracking-widest text-turd-mustard"
            >
              Build
            </label>
            <select
              id="build-filter"
              value={buildFilter}
              onChange={(e) => setBuildFilter(e.target.value)}
              className="mt-1 rounded border border-turd-bronze/40 bg-turd-bg-deep/60 px-3 py-1.5 text-sm text-turd-cream"
            >
              <option value="">All builds</option>
              {data.builds.map((b) => (
                <option key={b} value={b}>
                  v{b}
                </option>
              ))}
            </select>
          </div>
          <button
            onClick={() => {
              setKeyTypeFilter('');
              setBuildFilter('');
            }}
            className="rounded border border-turd-bronze/40 px-3 py-1.5 text-sm text-turd-cream-dim hover:text-turd-cream"
          >
            Clear filters
          </button>
          <span className="ml-auto text-sm text-turd-cream-dim">
            Showing {data.entries.length} of {data.total} entries
          </span>
        </div>
      </section>

      {[...grouped.entries()].map(([keyType, entries]) => (
        <section
          key={keyType}
          className="glass rounded-xl p-5"
        >
          <h2 className="font-display text-sm uppercase tracking-widest text-turd-mustard">
            {keyType}{' '}
            <span className="text-turd-cream-dim">({entries.length})</span>
          </h2>
          <div className="mt-3 overflow-auto rounded border border-turd-bronze/20">
            <table className="w-full text-left text-sm">
              <thead className="bg-turd-bg-deep/60">
                <tr className="border-b border-turd-bronze/30">
                  <th className="px-3 py-2 font-display text-[10px] uppercase tracking-widest text-turd-mustard">
                    When
                  </th>
                  <th className="px-3 py-2 font-display text-[10px] uppercase tracking-widest text-turd-mustard">
                    Build
                  </th>
                  <th className="px-3 py-2 font-display text-[10px] uppercase tracking-widest text-turd-mustard">
                    Target
                  </th>
                  <th className="px-3 py-2 font-display text-[10px] uppercase tracking-widest text-turd-mustard">
                    Value
                  </th>
                  <th className="px-3 py-2 font-display text-[10px] uppercase tracking-widest text-turd-mustard">
                    Source
                  </th>
                </tr>
              </thead>
              <tbody>
                {entries.map((e, i) => (
                  <tr
                    key={`${e.ts}-${i}`}
                    className="border-b border-turd-bronze/15 last:border-b-0 hover:bg-turd-bg-soft/30"
                    title={e.notes ?? undefined}
                  >
                    <td className="px-3 py-2 font-mono text-xs text-turd-cream-dim">
                      {formatTs(e.ts)}
                    </td>
                    <td className="px-3 py-2 font-mono text-xs text-turd-cream">
                      {e.scumBuild ? `v${e.scumBuild}` : '—'}
                    </td>
                    <td className="px-3 py-2 text-xs text-turd-cream-dim">
                      {e.target ?? '—'}
                    </td>
                    <td className="px-3 py-2 font-mono text-xs text-turd-cream">
                      {truncate(e.value, 80)}
                    </td>
                    <td className="px-3 py-2 text-xs text-turd-cream-dim">
                      {e.source ?? '—'}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>
      ))}
    </div>
  );
}

export default DumpArchivePage;
