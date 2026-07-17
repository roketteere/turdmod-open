import { useEffect, useMemo, useState } from 'react';
import { ModCard } from '../components/ModCard';
import { useInstalledMods, useMods } from '../hooks/useMods';
import { MOD_CATEGORIES, type ModCategory } from '../lib/tauri';

type Filter = ModCategory | 'All';

const FILTERS: Filter[] = ['All', ...MOD_CATEGORIES];

function useDebounced<T>(value: T, delay: number): T {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const t = setTimeout(() => setDebounced(value), delay);
    return () => clearTimeout(t);
  }, [value, delay]);
  return debounced;
}

export function BrowsePage() {
  const [search, setSearch] = useState('');
  const [filter, setFilter] = useState<Filter>('All');
  const debouncedSearch = useDebounced(search, 250);

  const modsQuery = useMods({
    category: filter === 'All' ? null : filter,
    search: debouncedSearch.trim() || null,
  });
  const installedQuery = useInstalledMods();

  const installedBySlug = useMemo(() => {
    const map = new Map<string, NonNullable<typeof installedQuery.data>[number]>();
    for (const m of installedQuery.data ?? []) map.set(m.slug, m);
    return map;
  }, [installedQuery.data]);

  return (
    <div className="space-y-6">
      <header>
        <p className="font-display text-xs tracking-[0.3em] text-turd-mustard">
          Catalog
        </p>
        <h1 className="mt-1 font-display text-3xl text-turd-cream">Browse mods</h1>
        <p className="mt-1 text-sm text-turd-cream-dim">
          First-party and community mods, ready to install into your local SCUM.
        </p>
      </header>

      <div className="space-y-3">
        <input
          type="search"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder="Search by name or summary"
          className="w-full rounded border border-turd-bronze/40 bg-turd-bg-mid/40 px-3 py-2 text-sm text-turd-cream placeholder:text-turd-cream-dim/60 focus:border-turd-mustard focus:outline-none"
        />
        <div className="flex flex-wrap gap-2">
          {FILTERS.map((f) => {
            const active = f === filter;
            return (
              <button
                key={f}
                type="button"
                onClick={() => setFilter(f)}
                className={[
                  'rounded px-3 py-1 font-mono text-[11px] uppercase tracking-wider transition-colors',
                  active
                    ? 'bg-turd-mustard text-turd-bg-deep'
                    : 'border border-turd-bronze/40 text-turd-cream-dim hover:border-turd-mustard/60 hover:text-turd-cream',
                ].join(' ')}
              >
                {f}
              </button>
            );
          })}
        </div>
      </div>

      {modsQuery.isLoading && (
        <p className="py-12 text-center text-sm text-turd-cream-dim">Loading…</p>
      )}

      {modsQuery.isError && (
        <div className="rounded-lg border border-turd-red/40 bg-turd-bg-mid/40 p-6">
          <h2 className="font-display text-lg text-turd-red">
            Can't reach turdmod.com — check your connection.
          </h2>
          <p className="mt-2 text-sm text-turd-cream-dim">
            {modsQuery.error.message}
          </p>
          <button
            type="button"
            onClick={() => modsQuery.refetch()}
            className="mt-4 rounded border border-turd-bronze px-4 py-1.5 text-sm text-turd-cream transition-colors hover:border-turd-mustard-bright hover:text-turd-mustard-bright"
          >
            Retry
          </button>
        </div>
      )}

      {modsQuery.isSuccess && modsQuery.data.length === 0 && (
        <div className="glass rounded-xl p-8 text-center">
          <p className="font-display text-lg text-turd-cream">
            No mods match that filter.
          </p>
          <p className="mt-2 text-sm text-turd-cream-dim">
            Try a different category or clear the search box.
          </p>
        </div>
      )}

      {modsQuery.isSuccess && modsQuery.data.length > 0 && (
        <ul className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
          {modsQuery.data.map((mod) => (
            <ModCard
              key={mod.slug}
              mod={mod}
              installed={installedBySlug.get(mod.slug) ?? null}
            />
          ))}
        </ul>
      )}
    </div>
  );
}
