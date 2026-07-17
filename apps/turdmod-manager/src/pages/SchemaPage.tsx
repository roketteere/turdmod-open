import {
  useMemo,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactElement,
} from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  engineRpc,
  type DescribeWidgetResponse,
  type DumpWidgetsResponse,
  type PropertyValue,
  type ReadClassValuesResponse,
  type WidgetEntry,
  type WidgetProperty,
  type WriteClassDefaultResponse,
} from '../lib/tauri-engine';
import { ExportControls } from '../components/ExportControls';

// ---------------------------------------------------------------------------
// SchemaPage — V0 of the UI/UX Maker: browse SCUM's UI surface.
//
// Left rail: searchable list of every UUserWidget-derived UClass that
// dumpWidgets returns (currently 496 from a live SCUM dedicated server).
// Click a widget → right pane: inheritance chain + sortable / filterable
// property table from describeWidget.
//
// Reads run on demand via the engine_rpc Tauri command, which JSON-RPCs
// the bridge cppmod over the named pipe. No editing yet — schema only.
// ---------------------------------------------------------------------------

const FETCH_LIMIT = 1000;

// Color-code FProperty types so the table is scannable by category.
function propertyTypeStyle(type: string): string {
  if (type === 'ObjectProperty' || type === 'ClassProperty') {
    return 'text-turd-mustard-bright';
  }
  if (type === 'StructProperty') {
    return 'text-turd-green';
  }
  if (type === 'BoolProperty') {
    return 'text-turd-cream-dim';
  }
  if (type === 'FloatProperty' || type === 'IntProperty' || type === 'ByteProperty') {
    return 'text-turd-bronze-light';
  }
  if (type === 'StrProperty' || type === 'TextProperty' || type === 'NameProperty') {
    return 'text-turd-cream';
  }
  if (type === 'EnumProperty') {
    return 'text-turd-mustard';
  }
  if (type === 'ArrayProperty' || type === 'MapProperty' || type === 'SetProperty') {
    return 'text-turd-red';
  }
  if (type.endsWith('DelegateProperty')) {
    return 'text-turd-cream-dim italic';
  }
  return 'text-turd-cream-dim';
}

// Property kinds whose values can be edited inline. The bridge's
// writeClassDefault handler only supports these four scalars in V1;
// name / string / object / unsupported fall through to read-only render.
const EDITABLE_KINDS = new Set(['bool', 'int', 'float', 'byte', 'name', 'string']);

interface EditableValueCellProps {
  className: string;
  propertyName: string;
  value: PropertyValue | undefined;
  loading: boolean;
}

function EditableValueCell({
  className,
  propertyName,
  value,
  loading,
}: EditableValueCellProps) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState<string | boolean>('');
  const queryClient = useQueryClient();

  const mutation = useMutation<WriteClassDefaultResponse, Error, void>({
    mutationFn: () =>
      engineRpc<WriteClassDefaultResponse>('writeClassDefault', {
        name: className,
        propertyName,
        valueKind: value!.valueKind,
        value: String(draft),
      }),
    onSuccess: (res) => {
      if (res.ok) {
        // Prefix-match invalidation covers both includeInherited true/false
        // — a CDO change is visible from any inheritance view.
        queryClient.invalidateQueries({
          queryKey: ['engine', 'readClassValues', className],
        });
        setEditing(false);
      }
    },
  });

  if (!value || !EDITABLE_KINDS.has(value.valueKind)) {
    return renderPropertyValue(value, loading);
  }

  const enterEdit = () => {
    setDraft(value.valueKind === 'bool' ? Boolean(value.value) : String(value.value ?? ''));
    mutation.reset();
    setEditing(true);
  };

  if (!editing) {
    return (
      <span
        role="button"
        tabIndex={0}
        onClick={enterEdit}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ' ') enterEdit();
        }}
        className="cursor-pointer border-b border-dashed border-turd-bronze/60 transition-colors hover:border-turd-mustard/60"
      >
        {renderPropertyValue(value, loading)}
      </span>
    );
  }

  const apply = () => mutation.mutate();
  const cancel = () => {
    setEditing(false);
    mutation.reset();
  };

  const onKeyDown = (e: ReactKeyboardEvent) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      apply();
    }
    if (e.key === 'Escape') {
      e.preventDefault();
      cancel();
    }
  };

  const inputBase =
    'rounded border border-turd-bronze/60 bg-turd-bg-soft px-1.5 py-0.5 font-mono text-[11px] text-turd-cream focus:border-turd-mustard focus:outline-none';
  const actionBtn =
    'rounded border px-1.5 py-0.5 font-mono text-[10px] transition-colors disabled:cursor-not-allowed disabled:opacity-30';

  return (
    <span className="inline-flex flex-wrap items-center gap-1.5">
      {value.valueKind === 'bool' ? (
        <input
          type="checkbox"
          checked={draft as boolean}
          onChange={(e) => setDraft(e.target.checked)}
          onKeyDown={onKeyDown}
          className="accent-turd-mustard"
          autoFocus
        />
      ) : value.valueKind === 'name' || value.valueKind === 'string' ? (
        <input
          type="text"
          value={draft as string}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={onKeyDown}
          className={`${inputBase} w-[32ch]`}
          autoFocus
        />
      ) : (
        <input
          type="number"
          step={value.valueKind === 'float' ? 'any' : '1'}
          value={draft as string}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={onKeyDown}
          className={`${inputBase} w-24`}
          autoFocus
        />
      )}
      <button
        type="button"
        onClick={apply}
        disabled={mutation.isPending}
        className={`${actionBtn} border-turd-green/60 bg-turd-green/10 text-turd-green hover:bg-turd-green/20`}
      >
        {mutation.isPending ? '…' : '✓'}
      </button>
      <button
        type="button"
        onClick={cancel}
        disabled={mutation.isPending}
        className={`${actionBtn} border-turd-bronze/60 bg-turd-bg-soft text-turd-cream-dim hover:bg-white/5`}
      >
        ✕
      </button>
      {mutation.isSuccess && mutation.data && !mutation.data.ok && (
        <span className="font-mono text-[10px] text-turd-red">
          {mutation.data.error ?? 'write failed'}
        </span>
      )}
      {mutation.isError && (
        <span className="font-mono text-[10px] text-turd-red">
          {String(mutation.error?.message ?? mutation.error)}
        </span>
      )}
    </span>
  );
}

// Render the CDO value for a property, color-coded by kind. Unsupported
// types (Struct/Array/etc.) show "—"; loading shows a placeholder.
function renderPropertyValue(
  v: PropertyValue | undefined,
  loading: boolean,
): ReactElement {
  if (!v) {
    return (
      <span className="text-turd-cream-dim/40">{loading ? '…' : '—'}</span>
    );
  }
  switch (v.valueKind) {
    case 'bool':
      return v.value ? (
        <span className="text-turd-green">true</span>
      ) : (
        <span className="text-turd-cream-dim">false</span>
      );
    case 'int':
    case 'byte':
      return <span className="text-turd-bronze-light">{String(v.value)}</span>;
    case 'float':
      return <span className="text-turd-bronze-light">{String(v.value)}</span>;
    case 'name':
    case 'string': {
      const s = String(v.value ?? '');
      const truncated = s.length > 80 ? s.slice(0, 80) + '…' : s;
      return (
        <span className="text-turd-cream" title={s.length > 80 ? s : undefined}>
          "{truncated}"
        </span>
      );
    }
    case 'object':
      return v.value === null ? (
        <span className="text-turd-cream-dim italic">null</span>
      ) : (
        <span className="text-turd-mustard-bright">→ {String(v.value)}</span>
      );
    case 'unsupported':
    default:
      return <span className="text-turd-cream-dim/40">—</span>;
  }
}

export function SchemaPage() {
  const [filter, setFilter] = useState('');
  const [selected, setSelected] = useState<string | null>(null);
  const [includeInherited, setIncludeInherited] = useState(false);
  const [propFilter, setPropFilter] = useState('');

  const widgetsQuery = useQuery<DumpWidgetsResponse, Error>({
    queryKey: ['engine', 'dumpWidgets'],
    queryFn: () => engineRpc<DumpWidgetsResponse>('dumpWidgets', { limit: String(FETCH_LIMIT) }),
    retry: false,
    staleTime: 5 * 60_000,
  });

  const describeQuery = useQuery<DescribeWidgetResponse, Error>({
    queryKey: ['engine', 'describeWidget', selected, includeInherited],
    queryFn: () =>
      engineRpc<DescribeWidgetResponse>('describeWidget', {
        name: selected!,
        includeInherited: includeInherited ? 'true' : 'false',
        limit: '500',
      }),
    enabled: selected !== null,
    retry: false,
    staleTime: 60_000,
  });

  // Parallel query: current CDO values for the same class. Schema gives
  // layout (name/type/offset); this gives the live default value at that
  // offset. Merged client-side by property name so they render together.
  const valuesQuery = useQuery<ReadClassValuesResponse, Error>({
    queryKey: ['engine', 'readClassValues', selected, includeInherited],
    queryFn: () =>
      engineRpc<ReadClassValuesResponse>('readClassValues', {
        name: selected!,
        includeInherited: includeInherited ? 'true' : 'false',
      }),
    enabled: selected !== null,
    retry: false,
    staleTime: 60_000,
  });

  // Index the CDO value table by `${name}@${offset}` so we don't accidentally
  // match two different properties that happen to share a name across the
  // SuperStruct chain.
  const valuesByKey = useMemo(() => {
    const m = new Map<string, PropertyValue>();
    for (const v of valuesQuery.data?.values ?? []) {
      m.set(`${v.name}@${v.offset}`, v);
    }
    return m;
  }, [valuesQuery.data]);

  const widgets: WidgetEntry[] = widgetsQuery.data?.widgets ?? [];
  const filterLow = filter.toLowerCase();
  const visibleWidgets = useMemo(
    () =>
      widgets.filter((w) =>
        !filterLow ? true : w.name.toLowerCase().includes(filterLow),
      ),
    [widgets, filterLow],
  );

  const props: WidgetProperty[] = describeQuery.data?.properties ?? [];
  const propFilterLow = propFilter.toLowerCase();
  const visibleProps = useMemo(
    () =>
      props.filter((p) =>
        !propFilterLow
          ? true
          : p.name.toLowerCase().includes(propFilterLow) ||
            p.type.toLowerCase().includes(propFilterLow) ||
            (p.ownerClass ?? '').toLowerCase().includes(propFilterLow),
      ),
    [props, propFilterLow],
  );

  const inputCls =
    'rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-1.5 font-mono text-xs text-turd-cream placeholder:text-turd-cream-dim/60 focus:border-turd-mustard focus:outline-none';

  return (
    <div className="flex h-full flex-col gap-3">
      <header>
        <p className="font-display text-xs tracking-[0.3em] text-turd-mustard">
          TurdMOD
        </p>
        <h1 className="mt-1 font-display text-3xl text-turd-cream">Schema</h1>
        <p className="mt-1 text-xs text-turd-cream-dim">
          SCUM's UI surface — every UUserWidget-derived UClass on the running
          engine, drillable down to per-property byte offsets. Foundation
          for the UI/UX Maker.
        </p>
      </header>

      {widgetsQuery.isError && (
        <div className="rounded border border-turd-red/60 bg-turd-bg-deep p-3 font-mono text-xs text-turd-red">
          <div>Couldn't reach the engine. Engine must be running.</div>
          <div className="mt-2 break-all text-turd-cream-dim">
            {String(widgetsQuery.error?.message ?? widgetsQuery.error ?? '(no detail)')}
          </div>
        </div>
      )}

      <div className="grid flex-1 grid-cols-[280px_1fr] gap-3 overflow-hidden">
        {/* ── Left rail: widget list ─────────────────────────────────── */}
        <aside className="flex flex-col overflow-hidden glass rounded-xl">
          <div className="border-b border-turd-bronze/30 p-3">
            <input
              type="text"
              placeholder="Filter widgets…"
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
              className={`${inputCls} w-full`}
            />
            <div className="mt-2 flex items-center justify-between gap-2">
              <p className="font-mono text-[10px] text-turd-cream-dim">
                {widgetsQuery.isLoading
                  ? 'loading…'
                  : `${visibleWidgets.length}/${widgets.length} widgets`}
              </p>
              <ExportControls
                all={visibleWidgets}
                allLabel="scum_widgets"
              />
            </div>
          </div>
          <div className="flex-1 overflow-auto">
            {visibleWidgets.map((w) => (
              <button
                key={w.name}
                type="button"
                onClick={() => setSelected(w.name)}
                className={`block w-full px-3 py-1.5 text-left font-mono text-[11px] transition-colors ${
                  selected === w.name
                    ? 'bg-turd-mustard/20 text-turd-mustard-bright'
                    : 'text-turd-cream hover:bg-white/5'
                }`}
              >
                <span className="mr-2 inline-block w-6 text-[9px] uppercase text-turd-cream-dim/70">
                  {w.kind}
                </span>
                {w.name}
              </button>
            ))}
          </div>
        </aside>

        {/* ── Right pane: properties ─────────────────────────────────── */}
        <section className="flex flex-col overflow-hidden rounded-lg border border-turd-bronze/30 bg-turd-bg-deep">
          {!selected ? (
            <div className="flex h-full items-center justify-center p-10 text-center">
              <p className="font-display text-turd-cream-dim">
                Select a widget to inspect.
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
              {describeQuery.data?.error ?? 'Widget not found.'}
            </div>
          ) : (
            <>
              {/* Header row */}
              <header className="border-b border-turd-bronze/30 p-4">
                <div className="flex items-baseline gap-3">
                  <h2 className="font-display text-xl text-turd-cream">
                    {describeQuery.data.name}
                  </h2>
                  <span className="font-mono text-[10px] uppercase text-turd-cream-dim">
                    {describeQuery.data.kind}
                  </span>
                </div>
                {describeQuery.data.inheritance && (
                  <p className="mt-2 font-mono text-[10px] text-turd-cream-dim">
                    {describeQuery.data.inheritance.join('  ←  ')}
                  </p>
                )}
                <div className="mt-3 flex items-center gap-3">
                  <input
                    type="text"
                    placeholder="Filter properties (name / type / owner)…"
                    value={propFilter}
                    onChange={(e) => setPropFilter(e.target.value)}
                    className={`${inputCls} w-72`}
                  />
                  <label className="flex cursor-pointer items-center gap-1.5">
                    <input
                      type="checkbox"
                      checked={includeInherited}
                      onChange={(e) => setIncludeInherited(e.target.checked)}
                      className="accent-turd-mustard"
                    />
                    <span className="font-mono text-[10px] uppercase tracking-wider text-turd-cream-dim">
                      Inherited
                    </span>
                  </label>
                  <span className="font-mono text-[10px] text-turd-cream-dim">
                    {visibleProps.length}/{describeQuery.data.totalProperties ?? 0} props
                  </span>
                  <div className="ml-auto">
                    <ExportControls
                      selected={describeQuery.data}
                      selectedLabel={describeQuery.data.name ?? 'widget'}
                    />
                  </div>
                </div>
              </header>

              {/* Property table */}
              <div className="flex-1 overflow-auto">
                <table className="w-full font-mono text-[11px]">
                  <thead className="sticky top-0 bg-turd-bg-deep">
                    <tr className="border-b border-turd-bronze/30 text-left text-turd-cream-dim">
                      <th className="px-4 py-2 font-normal">Name</th>
                      <th className="px-4 py-2 font-normal">Type</th>
                      <th className="px-4 py-2 font-normal">Value</th>
                      <th className="px-4 py-2 text-right font-normal">Offset</th>
                      {includeInherited && (
                        <th className="px-4 py-2 font-normal">Owner</th>
                      )}
                    </tr>
                  </thead>
                  <tbody>
                    {visibleProps.map((p, i) => {
                      const v = valuesByKey.get(`${p.name}@${p.offset}`);
                      return (
                        <tr
                          key={`${p.name}-${p.offset}-${i}`}
                          className="border-b border-turd-bronze/10 text-turd-cream hover:bg-white/5"
                        >
                          <td className="px-4 py-1.5">{p.name}</td>
                          <td className={`px-4 py-1.5 ${propertyTypeStyle(p.type)}`}>
                            {p.type}
                          </td>
                          <td className="px-4 py-1.5">
                            <EditableValueCell
                              className={selected!}
                              propertyName={p.name}
                              value={v}
                              loading={valuesQuery.isLoading}
                            />
                          </td>
                          <td className="px-4 py-1.5 text-right text-turd-cream-dim">
                            0x{p.offset.toString(16).padStart(3, '0')}
                            <span className="ml-2 text-turd-cream-dim/60">
                              ({p.offset})
                            </span>
                          </td>
                          {includeInherited && (
                            <td className="px-4 py-1.5 text-turd-cream-dim">
                              {p.ownerClass ?? ''}
                            </td>
                          )}
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
                {visibleProps.length === 0 && (
                  <p className="py-6 text-center text-xs text-turd-cream-dim">
                    No properties match the filter.
                  </p>
                )}
              </div>
            </>
          )}
        </section>
      </div>
    </div>
  );
}
