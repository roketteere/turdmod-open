import { useEffect, useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import {
  engineRpc,
  type DescribeFunctionResponse,
  type FindFunctionsResponse,
  type FunctionEntry,
  type FunctionParam,
} from '../lib/tauri-engine';
import { ExportControls } from '../components/ExportControls';

// ---------------------------------------------------------------------------
// FunctionsPage — sibling to Schema. Browse SCUM's full UFunction surface
// (~12k functions). Server-side grep because client-side would mean shipping
// megabytes of JSON per keystroke.
// ---------------------------------------------------------------------------

// Preset emit caps. "All" picks a number well above SCUM's ~12.3k UFunctions
// — the bridge respects `limit` exactly, so this is effectively unlimited.
const LIMIT_OPTIONS: { label: string; value: number }[] = [
  { label: '500', value: 500 },
  { label: '2k', value: 2000 },
  { label: '5k', value: 5000 },
  { label: 'All', value: 50000 },
];
const DEFAULT_LIMIT = 500;

// Same color scheme as SchemaPage's property table for visual continuity.
function paramTypeStyle(type: string): string {
  if (type === 'ObjectProperty' || type === 'ClassProperty') {
    return 'text-turd-mustard-bright';
  }
  if (type === 'StructProperty') return 'text-turd-green';
  if (type === 'BoolProperty') return 'text-turd-cream-dim';
  if (type === 'FloatProperty' || type === 'IntProperty' || type === 'ByteProperty') {
    return 'text-turd-bronze-light';
  }
  if (type === 'StrProperty' || type === 'TextProperty' || type === 'NameProperty') {
    return 'text-turd-cream';
  }
  if (type === 'EnumProperty') return 'text-turd-mustard';
  if (type === 'ArrayProperty' || type === 'MapProperty' || type === 'SetProperty') {
    return 'text-turd-red';
  }
  if (type.endsWith('DelegateProperty')) return 'text-turd-cream-dim italic';
  return 'text-turd-cream-dim';
}

export function FunctionsPage() {
  // What the user typed; debounced into appliedGrep for the query.
  const [grepInput, setGrepInput] = useState('');
  const [appliedGrep, setAppliedGrep] = useState('');
  const [limit, setLimit] = useState<number>(DEFAULT_LIMIT);
  const [selected, setSelected] = useState<FunctionEntry | null>(null);

  // 300ms debounce on the grep input so we don't fire an RPC per keystroke.
  useEffect(() => {
    const t = setTimeout(() => setAppliedGrep(grepInput.trim()), 300);
    return () => clearTimeout(t);
  }, [grepInput]);

  const fnsQuery = useQuery<FindFunctionsResponse, Error>({
    queryKey: ['engine', 'findFunctions', appliedGrep, limit],
    queryFn: () =>
      engineRpc<FindFunctionsResponse>('findFunctions', {
        grep: appliedGrep,
        limit: String(limit),
      }),
    retry: false,
    staleTime: 60_000,
  });

  const describeQuery = useQuery<DescribeFunctionResponse, Error>({
    queryKey: ['engine', 'describeFunction', selected?.owner, selected?.name],
    queryFn: () =>
      engineRpc<DescribeFunctionResponse>('describeFunction', {
        owner: selected!.owner,
        name: selected!.name,
      }),
    enabled: selected !== null,
    retry: false,
    staleTime: 60_000,
  });

  const functions: FunctionEntry[] = fnsQuery.data?.functions ?? [];
  const params: FunctionParam[] = describeQuery.data?.params ?? [];

  const totalFunctions = fnsQuery.data?.totalFunctions ?? 0;
  const emitted = fnsQuery.data?.emitted ?? 0;

  // Pre-compute a stable selection key so React knows which row is active.
  const selectedKey = useMemo(
    () => (selected ? `${selected.owner}::${selected.name}` : null),
    [selected],
  );

  const inputCls =
    'rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-1.5 font-mono text-xs text-turd-cream placeholder:text-turd-cream-dim/60 focus:border-turd-mustard focus:outline-none';

  return (
    <div className="flex h-full flex-col gap-3">
      <header>
        <p className="font-display text-xs tracking-[0.3em] text-turd-mustard">
          TurdMOD
        </p>
        <h1 className="mt-1 font-display text-3xl text-turd-cream">Functions</h1>
        <p className="mt-1 text-xs text-turd-cream-dim">
          Every UFunction in SCUM (~12k). Server-side grep — type to filter.
          Click any function to inspect its parameter signature.
        </p>
      </header>

      {fnsQuery.isError && (
        <div className="rounded border border-turd-red/60 bg-turd-bg-deep p-3 font-mono text-xs text-turd-red">
          <div>Couldn't reach the engine. Engine must be running.</div>
          <div className="mt-2 break-all text-turd-cream-dim">
            {String(fnsQuery.error?.message ?? fnsQuery.error ?? '(no detail)')}
          </div>
        </div>
      )}

      <div className="grid flex-1 grid-cols-[340px_1fr] gap-3 overflow-hidden">
        {/* ── Left rail: function list ───────────────────────────────── */}
        <aside className="flex flex-col overflow-hidden glass rounded-xl">
          <div className="border-b border-turd-bronze/30 p-3">
            <input
              type="text"
              placeholder="Grep functions (server-side)…"
              value={grepInput}
              onChange={(e) => setGrepInput(e.target.value)}
              className={`${inputCls} w-full`}
              autoFocus
            />
            <div className="mt-2 flex items-center justify-between gap-2">
              <p className="font-mono text-[10px] text-turd-cream-dim">
                {fnsQuery.isLoading || fnsQuery.isFetching
                  ? 'loading…'
                  : appliedGrep
                    ? `${emitted} matching "${appliedGrep}" of ${totalFunctions} total`
                    : `${emitted} of ${totalFunctions} (cap ${limit})`}
              </p>
              <ExportControls
                all={functions}
                allLabel={
                  appliedGrep
                    ? `scum_functions_${appliedGrep}`
                    : 'scum_functions'
                }
              />
            </div>
            {/* Emit-cap selector — change how many rows the bridge returns. */}
            <div className="mt-2 flex items-center gap-1">
              <span className="font-mono text-[9px] uppercase tracking-wider text-turd-cream-dim">
                Limit
              </span>
              {LIMIT_OPTIONS.map((opt) => (
                <button
                  key={opt.value}
                  type="button"
                  onClick={() => setLimit(opt.value)}
                  className={`rounded border px-1.5 py-0.5 font-mono text-[10px] transition-colors ${
                    limit === opt.value
                      ? 'border-turd-mustard bg-turd-mustard/20 text-turd-mustard-bright'
                      : 'border-turd-bronze/60 text-turd-cream-dim hover:border-turd-cream hover:text-turd-cream'
                  }`}
                >
                  {opt.label}
                </button>
              ))}
            </div>
          </div>
          <div className="flex-1 overflow-auto">
            {functions.map((f) => {
              const key = `${f.owner}::${f.name}`;
              return (
                <button
                  key={key}
                  type="button"
                  onClick={() => setSelected(f)}
                  className={`block w-full px-3 py-1.5 text-left font-mono text-[11px] transition-colors ${
                    selectedKey === key
                      ? 'bg-turd-mustard/20 text-turd-mustard-bright'
                      : 'text-turd-cream hover:bg-white/5'
                  }`}
                >
                  <div className="flex items-baseline gap-2">
                    <span className="truncate">{f.name}</span>
                  </div>
                  <span className="text-[9px] text-turd-cream-dim/70">
                    {f.owner}
                  </span>
                </button>
              );
            })}
            {!fnsQuery.isLoading && functions.length === 0 && (
              <p className="px-3 py-4 text-center text-xs text-turd-cream-dim">
                {appliedGrep
                  ? `No UFunctions match "${appliedGrep}".`
                  : 'No functions returned.'}
              </p>
            )}
          </div>
        </aside>

        {/* ── Right pane: function details ────────────────────────────── */}
        <section className="flex flex-col overflow-hidden rounded-lg border border-turd-bronze/30 bg-turd-bg-deep">
          {!selected ? (
            <div className="flex h-full items-center justify-center p-10 text-center">
              <p className="font-display text-turd-cream-dim">
                Select a function to inspect.
              </p>
            </div>
          ) : describeQuery.isLoading ? (
            <div className="flex h-full items-center justify-center p-10 text-center">
              <p className="font-display text-turd-cream-dim">Loading…</p>
            </div>
          ) : describeQuery.isError ? (
            <div className="p-4 font-mono text-xs text-turd-red">
              {String(describeQuery.error?.message ?? describeQuery.error ?? '(no detail)')}
            </div>
          ) : !describeQuery.data?.found ? (
            <div className="p-4 text-xs text-turd-red">
              {describeQuery.data?.error ?? 'Function not found on the live engine.'}
            </div>
          ) : (
            <>
              <header className="border-b border-turd-bronze/30 p-4">
                <div className="flex items-baseline gap-3">
                  <h2 className="font-display text-xl text-turd-cream">
                    {selected.name}
                  </h2>
                  <div className="ml-auto">
                    <ExportControls
                      selected={describeQuery.data}
                      selectedLabel={`${selected.owner}_${selected.name}`}
                    />
                  </div>
                </div>
                <p className="mt-1 font-mono text-[11px] text-turd-cream-dim">
                  owner: {describeQuery.data.owner ?? selected.owner}
                </p>
                <p className="mt-3 font-mono text-[10px] text-turd-cream-dim">
                  {params.length} {params.length === 1 ? 'param' : 'params'}
                </p>
              </header>

              <div className="flex-1 overflow-auto">
                {params.length === 0 ? (
                  <p className="py-6 text-center font-mono text-xs text-turd-cream-dim">
                    No parameters.
                  </p>
                ) : (
                  <table className="w-full font-mono text-[11px]">
                    <thead className="sticky top-0 bg-turd-bg-deep">
                      <tr className="border-b border-turd-bronze/30 text-left text-turd-cream-dim">
                        <th className="px-4 py-2 text-right font-normal w-12">#</th>
                        <th className="px-4 py-2 font-normal">Name</th>
                        <th className="px-4 py-2 font-normal">Type</th>
                      </tr>
                    </thead>
                    <tbody>
                      {params.map((p, i) => (
                        <tr
                          key={`${p.name}-${i}`}
                          className="border-b border-turd-bronze/10 text-turd-cream hover:bg-white/5"
                        >
                          <td className="px-4 py-1.5 text-right text-turd-cream-dim">
                            {i}
                          </td>
                          <td className="px-4 py-1.5">{p.name}</td>
                          <td className={`px-4 py-1.5 ${paramTypeStyle(p.type)}`}>
                            {p.type}
                          </td>
                        </tr>
                      ))}
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
