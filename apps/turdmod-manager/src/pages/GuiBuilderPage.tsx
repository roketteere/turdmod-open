import { useMemo, useState, useRef, useEffect, type ReactElement } from 'react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { save } from '@tauri-apps/plugin-dialog';
import {
  engineRpc,
  type ApplyRecipeResponse,
  type DescribeWidgetResponse,
  type DumpWidgetsResponse,
  type ReadClassValuesResponse,
  type ShowPanelResponse,
  type WidgetEntry,
  type WidgetProperty,
  type PropertyValue,
} from '../lib/tauri-engine';

const FETCH_LIMIT = 1000;

const EDITABLE_KINDS = new Set(['bool', 'int', 'float', 'byte', 'name', 'string']);

// ---------------------------------------------------------------------------
// Helpers copied from SchemaPage to keep visual parity without a shared module
// ---------------------------------------------------------------------------

function propertyTypeStyle(type: string): string {
  if (type === 'ObjectProperty' || type === 'ClassProperty') return 'text-turd-mustard-bright';
  if (type === 'StructProperty') return 'text-turd-green';
  if (type === 'BoolProperty') return 'text-turd-cream-dim';
  if (type === 'FloatProperty' || type === 'IntProperty' || type === 'ByteProperty')
    return 'text-turd-bronze-light';
  if (type === 'StrProperty' || type === 'TextProperty' || type === 'NameProperty')
    return 'text-turd-cream';
  if (type === 'EnumProperty') return 'text-turd-mustard';
  if (type === 'ArrayProperty' || type === 'MapProperty' || type === 'SetProperty')
    return 'text-turd-red';
  if (type.endsWith('DelegateProperty')) return 'text-turd-cream-dim italic';
  return 'text-turd-cream-dim';
}

function renderPropertyValue(v: PropertyValue | undefined, loading: boolean): ReactElement {
  if (!v) return <span className="text-turd-cream-dim/40">{loading ? '…' : '—'}</span>;
  switch (v.valueKind) {
    case 'bool':
      return v.value
        ? <span className="text-turd-green">true</span>
        : <span className="text-turd-cream-dim">false</span>;
    case 'int':
    case 'byte':
    case 'float':
      return <span className="text-turd-bronze-light">{String(v.value)}</span>;
    case 'name':
    case 'string': {
      const s = String(v.value ?? '');
      const truncated = s.length > 60 ? s.slice(0, 60) + '…' : s;
      return <span className="text-turd-cream" title={s.length > 60 ? s : undefined}>"{truncated}"</span>;
    }
    default:
      return <span className="text-turd-cream-dim/40">—</span>;
  }
}

function slugify(s: string): string {
  return s.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');
}

interface Override {
  property: string;
  type: string;
  valueKind: string;
  value: boolean | number | string;
}

// Inline editor for an override row — always in edit mode, local state only.
// Simpler than SchemaPage's EditableValueCell because there's no mutation
// lifecycle: edits go to React state, not to the live CDO.
interface OverrideValueInputProps {
  override: Override;
  onChange: (value: boolean | number | string) => void;
}

function OverrideValueInput({ override, onChange }: OverrideValueInputProps) {
  const inputBase =
    'rounded border border-turd-bronze/60 bg-turd-bg-soft px-1.5 py-0.5 font-mono text-[11px] text-turd-cream focus:border-turd-mustard focus:outline-none';

  if (override.valueKind === 'bool') {
    return (
      <input
        type="checkbox"
        checked={override.value as boolean}
        onChange={(e) => onChange(e.target.checked)}
        className="accent-turd-mustard"
      />
    );
  }
  if (override.valueKind === 'name' || override.valueKind === 'string') {
    return (
      <input
        type="text"
        value={override.value as string}
        onChange={(e) => onChange(e.target.value)}
        className={`${inputBase} w-[28ch]`}
      />
    );
  }
  return (
    <input
      type="number"
      step={override.valueKind === 'float' ? 'any' : '1'}
      value={override.value as number}
      onChange={(e) =>
        onChange(
          override.valueKind === 'float'
            ? parseFloat(e.target.value) || 0
            : parseInt(e.target.value, 10) || 0,
        )
      }
      className={`${inputBase} w-24`}
    />
  );
}

interface AddOverrideDropdownProps {
  properties: WidgetProperty[];
  valuesByKey: Map<string, PropertyValue>;
  existingOverrides: Override[];
  onAdd: (prop: WidgetProperty, seedValue: PropertyValue | undefined) => void;
  onClose: () => void;
}

function AddOverrideDropdown({
  properties,
  valuesByKey,
  existingOverrides,
  onAdd,
  onClose,
}: AddOverrideDropdownProps) {
  const [search, setSearch] = useState('');
  const inputRef = useRef<HTMLInputElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        onClose();
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [onClose]);

  const existingNames = new Set(existingOverrides.map((o) => o.property));
  const searchLow = search.toLowerCase();

  const allEditable = properties.filter((p) => {
    const v = valuesByKey.get(`${p.name}@${p.offset}`);
    return v && EDITABLE_KINDS.has(v.valueKind);
  });

  const candidates = allEditable.filter((p) => {
    if (existingNames.has(p.name)) return false;
    if (searchLow && !p.name.toLowerCase().includes(searchLow)) return false;
    return true;
  });

  return (
    <div
      ref={containerRef}
      className="absolute left-0 top-full z-50 mt-1 flex w-80 flex-col rounded-lg border border-turd-bronze/60 bg-turd-bg-deep shadow-xl"
    >
      <div className="border-b border-turd-bronze/30 p-2">
        <input
          ref={inputRef}
          type="text"
          placeholder="Search properties…"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Escape') onClose(); }}
          className="w-full rounded border border-turd-bronze/60 bg-turd-bg-soft px-2 py-1 font-mono text-[11px] text-turd-cream placeholder:text-turd-cream-dim/60 focus:border-turd-mustard focus:outline-none"
        />
      </div>
      <div className="max-h-64 overflow-auto">
        {candidates.length === 0 ? (
          <p className="px-3 py-4 text-center font-mono text-[11px] text-turd-cream-dim">
            {existingNames.size > 0 && existingNames.size === allEditable.length
              ? 'All editable properties already added.'
              : 'No matches.'}
          </p>
        ) : (
          candidates.map((p) => {
            const v = valuesByKey.get(`${p.name}@${p.offset}`);
            return (
              <button
                key={`${p.name}@${p.offset}`}
                type="button"
                onClick={() => { onAdd(p, v); onClose(); }}
                className="flex w-full items-center gap-3 px-3 py-1.5 text-left transition-colors hover:bg-white/5"
              >
                <span className="flex-1 truncate font-mono text-[11px] text-turd-cream">
                  {p.name}
                </span>
                <span className={`font-mono text-[10px] ${propertyTypeStyle(p.type)}`}>
                  {p.type}
                </span>
                <span className="font-mono text-[10px] text-turd-cream-dim/60">
                  {renderPropertyValue(v, false)}
                </span>
              </button>
            );
          })
        )}
      </div>
    </div>
  );
}

type SaveState = 'idle' | 'saving' | 'saved' | 'error';
type ApplyState = 'idle' | 'applying' | 'applied' | 'partial' | 'error';

export function GuiBuilderPage() {
  const [widgetFilter, setWidgetFilter] = useState('');
  const [selectedWidget, setSelectedWidget] = useState<string | null>(null);
  const [overrides, setOverrides] = useState<Override[]>([]);
  const [recipeName, setRecipeName] = useState('');
  const [recipeDesc, setRecipeDesc] = useState('');
  const [showAddDropdown, setShowAddDropdown] = useState(false);
  const [saveState, setSaveState] = useState<SaveState>('idle');
  const [saveError, setSaveError] = useState<string | null>(null);
  const [applyState, setApplyState] = useState<ApplyState>('idle');
  const [applyMessage, setApplyMessage] = useState<string | null>(null);
  const [applyDetails, setApplyDetails] = useState<string | null>(null);
  const [pushState, setPushState] = useState<'idle' | 'pushing' | 'pushed' | 'error'>('idle');
  const [pushMessage, setPushMessage] = useState<string | null>(null);

  const widgetsQuery = useQuery<DumpWidgetsResponse, Error>({
    queryKey: ['engine', 'dumpWidgets'],
    queryFn: () => engineRpc<DumpWidgetsResponse>('dumpWidgets', { limit: String(FETCH_LIMIT) }),
    retry: false,
    staleTime: 5 * 60_000,
  });

  const describeQuery = useQuery<DescribeWidgetResponse, Error>({
    queryKey: ['engine', 'describeWidget', selectedWidget, 'true'],
    queryFn: () =>
      engineRpc<DescribeWidgetResponse>('describeWidget', {
        name: selectedWidget!,
        includeInherited: 'true',
        limit: '500',
      }),
    enabled: selectedWidget !== null,
    retry: false,
    staleTime: 60_000,
  });

  const valuesQuery = useQuery<ReadClassValuesResponse, Error>({
    queryKey: ['engine', 'readClassValues', selectedWidget, 'true'],
    queryFn: () =>
      engineRpc<ReadClassValuesResponse>('readClassValues', {
        name: selectedWidget!,
        includeInherited: 'true',
      }),
    enabled: selectedWidget !== null,
    retry: false,
    staleTime: 60_000,
  });

  const valuesByKey = useMemo(() => {
    const m = new Map<string, PropertyValue>();
    for (const v of valuesQuery.data?.values ?? []) {
      m.set(`${v.name}@${v.offset}`, v);
    }
    return m;
  }, [valuesQuery.data]);

  const applyMutation = useMutation<ApplyRecipeResponse, Error, void>({
    mutationFn: async () => {
      const payload = {
        name: selectedWidget!,
        overrides: overrides.map((o) => ({
          property: o.property,
          value: String(o.value),
          valueKind: o.valueKind,
        })),
      };
      return engineRpc<ApplyRecipeResponse>('applyRecipe', payload);
    },
    onMutate: () => {
      setApplyState('applying');
      setApplyMessage(null);
      setApplyDetails(null);
    },
    onSuccess: (data) => {
      if (!data.ok && data.applied === 0) {
        setApplyState('error');
        setApplyMessage(data.error ?? 'All overrides failed');
        if (data.results) {
          const failedNames = data.results.filter((r) => !r.ok).map((r) => `${r.property}: ${r.error ?? 'unknown'}`);
          setApplyDetails(failedNames.join('\n'));
        }
      } else if (data.failed > 0) {
        setApplyState('partial');
        setApplyMessage(`Applied ${data.applied}/${data.applied + data.failed} — ${data.failed} failed`);
        if (data.results) {
          const failedNames = data.results.filter((r) => !r.ok).map((r) => `${r.property}: ${r.error ?? 'unknown'}`);
          setApplyDetails(failedNames.join('\n'));
        }
      } else {
        setApplyState('applied');
        setApplyMessage(`Applied ${data.applied}/${data.applied} overrides`);
      }
      setTimeout(() => { setApplyState('idle'); setApplyMessage(null); setApplyDetails(null); }, 5000);
    },
    onError: (err) => {
      setApplyState('error');
      setApplyMessage(String(err.message ?? err));
      setTimeout(() => { setApplyState('idle'); setApplyMessage(null); setApplyDetails(null); }, 5000);
    },
  });

  const pushMutation = useMutation<ShowPanelResponse, Error, void>({
    mutationFn: async () => {
      const payload = JSON.stringify({
        overrides: overrides.map((o) => ({
          property: o.property,
          value: String(o.value),
          valueKind: o.valueKind,
        })),
      });
      return engineRpc<ShowPanelResponse>('showPanel', {
        command: 'Show',
        widgetClass: `/TurdMODContent/Widgets/${selectedWidget}`,
        payload,
        target: 'all',
      });
    },
    onMutate: () => { setPushState('pushing'); setPushMessage(null); },
    onSuccess: (data) => {
      if (data.ok) {
        setPushState('pushed');
        setPushMessage('Pushed to all players');
      } else {
        setPushState('error');
        setPushMessage(data.error ?? 'Push failed');
      }
      setTimeout(() => { setPushState('idle'); setPushMessage(null); }, 5000);
    },
    onError: (err) => {
      setPushState('error');
      setPushMessage(String(err.message ?? err));
      setTimeout(() => { setPushState('idle'); setPushMessage(null); }, 5000);
    },
  });

  const canApply = selectedWidget !== null && overrides.length > 0;

  const widgets: WidgetEntry[] = widgetsQuery.data?.widgets ?? [];
  const filterLow = widgetFilter.toLowerCase();
  const visibleWidgets = useMemo(
    () => widgets.filter((w) => !filterLow || w.name.toLowerCase().includes(filterLow)),
    [widgets, filterLow],
  );

  const properties: WidgetProperty[] = describeQuery.data?.properties ?? [];

  const handleSelectWidget = (name: string) => {
    if (name === selectedWidget) return;
    setSelectedWidget(name);
    setOverrides([]);
    setShowAddDropdown(false);
  };

  const handleAddOverride = (prop: WidgetProperty, seed: PropertyValue | undefined) => {
    if (!seed || !EDITABLE_KINDS.has(seed.valueKind)) return;

    let seedValue: boolean | number | string;
    if (seed.valueKind === 'bool') seedValue = Boolean(seed.value);
    else if (seed.valueKind === 'string' || seed.valueKind === 'name')
      seedValue = String(seed.value ?? '');
    else seedValue = Number(seed.value ?? 0);

    setOverrides((prev) => [
      ...prev,
      { property: prop.name, type: prop.type, valueKind: seed.valueKind, value: seedValue },
    ]);
  };

  const handleRemoveOverride = (index: number) => {
    setOverrides((prev) => prev.filter((_, i) => i !== index));
  };

  const handleChangeOverrideValue = (index: number, value: boolean | number | string) => {
    setOverrides((prev) => prev.map((o, i) => (i === index ? { ...o, value } : o)));
  };

  const canSave = recipeName.trim().length > 0 && selectedWidget !== null;

  const handleSave = async () => {
    if (!canSave) return;
    setSaveState('saving');
    setSaveError(null);

    const recipe = {
      schemaVersion: 1,
      name: recipeName.trim(),
      description: recipeDesc.trim(),
      baseWidget: selectedWidget,
      overrides,
      createdAt: new Date().toISOString(),
    };

    try {
      const defaultName = `${slugify(recipeName.trim())}.recipe.json`;
      const path = await save({
        defaultPath: defaultName,
        filters: [{ name: 'JSON', extensions: ['json'] }],
      });
      if (!path) {
        setSaveState('idle');
        return;
      }
      await invoke('manager_write_text_file', {
        path,
        content: JSON.stringify(recipe, null, 2),
      });
      setSaveState('saved');
      setTimeout(() => setSaveState('idle'), 3000);
    } catch (e) {
      const msg = String((e as Error)?.message ?? e);
      setSaveError(msg);
      setSaveState('error');
      setTimeout(() => { setSaveState('idle'); setSaveError(null); }, 5000);
    }
  };

  const inputCls =
    'rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-1.5 font-mono text-xs text-turd-cream placeholder:text-turd-cream-dim/60 focus:border-turd-mustard focus:outline-none';

  return (
    <div className="flex h-full flex-col gap-3">
      <header>
        <p className="font-display text-xs tracking-[0.3em] text-turd-mustard">
          TurdMOD
        </p>
        <h1 className="mt-1 font-display text-3xl text-turd-cream">GUI Builder</h1>
        <p className="mt-1 text-xs text-turd-cream-dim">
          Compose custom UI from SCUM's existing widgets. Pick a base widget, override
          property values, save as a recipe (JSON).
        </p>
      </header>

      <section className="glass rounded-xl p-4">
        <div className="flex flex-wrap items-end gap-3">
          <div className="flex-1 min-w-48">
            <p className="font-mono text-[10px] uppercase tracking-wider text-turd-cream-dim">
              Recipe Name
            </p>
            <input
              type="text"
              placeholder="e.g. Welcome Screen"
              value={recipeName}
              onChange={(e) => setRecipeName(e.target.value)}
              className={`${inputCls} mt-1 w-full`}
            />
          </div>
          <div className="flex-[2] min-w-60">
            <p className="font-mono text-[10px] uppercase tracking-wider text-turd-cream-dim">
              Description
            </p>
            <input
              type="text"
              placeholder="Optional — what does this recipe do?"
              value={recipeDesc}
              onChange={(e) => setRecipeDesc(e.target.value)}
              className={`${inputCls} mt-1 w-full`}
            />
          </div>
          <div className="flex flex-col items-end gap-1">
            <div className="flex gap-2">
              <button
                type="button"
                onClick={handleSave}
                disabled={!canSave || saveState === 'saving'}
                className="rounded border border-turd-mustard/60 bg-turd-mustard/20 px-4 py-1.5 font-display text-xs uppercase tracking-wider text-turd-mustard-bright transition-colors hover:bg-turd-mustard/30 disabled:cursor-not-allowed disabled:opacity-30"
              >
                {saveState === 'saving' ? 'Saving…' : 'Save Recipe…'}
              </button>
              <button
                type="button"
                onClick={() => applyMutation.mutate()}
                disabled={!canApply || applyState === 'applying'}
                className="rounded border border-turd-green/60 bg-turd-green/20 px-4 py-1.5 font-display text-xs uppercase tracking-wider text-turd-green transition-colors hover:bg-turd-green/30 disabled:cursor-not-allowed disabled:opacity-30"
                title={!canApply ? 'Select a widget and add overrides first' : `Apply ${overrides.length} override(s) to the live CDO`}
              >
                {applyState === 'applying' ? 'Applying…' : 'Apply to Live CDO'}
              </button>
              <button
                type="button"
                onClick={() => pushMutation.mutate()}
                disabled={!canApply || pushState === 'pushing'}
                className="rounded border border-turd-red/60 bg-turd-red/20 px-4 py-1.5 font-display text-xs uppercase tracking-wider text-turd-red transition-colors hover:bg-turd-red/30 disabled:cursor-not-allowed disabled:opacity-30"
                title={!canApply ? 'Select a widget and add overrides first' : 'Push this widget to all connected players'}
              >
                {pushState === 'pushing' ? 'Pushing…' : 'Push to Players'}
              </button>
            </div>
            {saveState === 'saved' && (
              <span className="font-mono text-[10px] text-turd-green">saved</span>
            )}
            {saveState === 'error' && (
              <span
                className="font-mono text-[10px] text-turd-red"
                title={saveError ?? undefined}
              >
                failed{saveError ? `: ${saveError.slice(0, 50)}${saveError.length > 50 ? '…' : ''}` : ''}
              </span>
            )}
            {!canSave && saveState === 'idle' && (
              <span className="font-mono text-[10px] text-turd-cream-dim/50">
                {!recipeName.trim() && !selectedWidget
                  ? 'name + widget required'
                  : !recipeName.trim()
                  ? 'name required'
                  : 'select a base widget'}
              </span>
            )}
            {applyState === 'applied' && (
              <span className="font-mono text-[10px] text-turd-green">{applyMessage}</span>
            )}
            {applyState === 'partial' && (
              <span className="font-mono text-[10px] text-turd-mustard" title={applyDetails ?? undefined}>
                {applyMessage}
              </span>
            )}
            {applyState === 'error' && (
              <span
                className="font-mono text-[10px] text-turd-red"
                title={applyDetails ?? applyMessage ?? undefined}
              >
                failed{applyMessage ? `: ${applyMessage.slice(0, 60)}${applyMessage.length > 60 ? '…' : ''}` : ''}
              </span>
            )}
            {pushState === 'pushed' && (
              <span className="font-mono text-[10px] text-turd-green">{pushMessage}</span>
            )}
            {pushState === 'error' && (
              <span className="font-mono text-[10px] text-turd-red" title={pushMessage ?? undefined}>
                push failed{pushMessage ? `: ${pushMessage.slice(0, 60)}` : ''}
              </span>
            )}
          </div>
        </div>
      </section>

      {widgetsQuery.isError && (
        <div className="rounded border border-turd-red/60 bg-turd-bg-deep p-3 font-mono text-xs text-turd-red">
          <div>Couldn't reach the engine. Engine must be running.</div>
          <div className="mt-1 break-all text-turd-cream-dim">
            {String(widgetsQuery.error?.message ?? widgetsQuery.error ?? '(no detail)')}
          </div>
        </div>
      )}

      <div className="grid flex-1 grid-cols-[280px_1fr] gap-3 overflow-hidden">
        <aside className="flex flex-col overflow-hidden glass rounded-xl">
          <div className="border-b border-turd-bronze/30 p-3">
            <p className="mb-2 font-mono text-[10px] uppercase tracking-wider text-turd-mustard">
              Base Widget
            </p>
            <input
              type="text"
              placeholder="Filter widgets…"
              value={widgetFilter}
              onChange={(e) => setWidgetFilter(e.target.value)}
              className={`${inputCls} w-full`}
            />
            <p className="mt-2 font-mono text-[10px] text-turd-cream-dim">
              {widgetsQuery.isLoading
                ? 'loading…'
                : `${visibleWidgets.length}/${widgets.length} widgets`}
            </p>
          </div>
          <div className="flex-1 overflow-auto">
            {visibleWidgets.map((w) => (
              <button
                key={w.name}
                type="button"
                onClick={() => handleSelectWidget(w.name)}
                className={`block w-full px-3 py-1.5 text-left font-mono text-[11px] transition-colors ${
                  selectedWidget === w.name
                    ? 'bg-turd-mustard/20 text-turd-mustard-bright'
                    : 'text-turd-cream hover:bg-white/5'
                }`}
              >
                <span className="mr-2 inline-block w-6 text-[9px] uppercase text-turd-cream-dim/70">
                  {w.kind}
                </span>
                {w.name}
                {selectedWidget === w.name &&
                  describeQuery.data?.properties &&
                  describeQuery.data.properties.length > 0 && (
                    <span className="ml-2 text-[9px] text-turd-cream-dim/60">
                      · {describeQuery.data.properties.length} props
                    </span>
                  )}
              </button>
            ))}
          </div>
        </aside>

        <section className="flex flex-col overflow-hidden rounded-lg border border-turd-bronze/30 bg-turd-bg-deep">
          {!selectedWidget ? (
            <div className="flex h-full items-center justify-center p-10 text-center">
              <p className="font-display text-turd-cream-dim">
                Select a base widget to start composing.
              </p>
            </div>
          ) : (
            <>
              <header className="border-b border-turd-bronze/30 p-4">
                <div className="flex items-baseline gap-3">
                  <h2 className="font-display text-xl text-turd-cream">{selectedWidget}</h2>
                  {describeQuery.data?.kind && (
                    <span className="font-mono text-[10px] uppercase text-turd-cream-dim">
                      {describeQuery.data.kind}
                    </span>
                  )}
                </div>
                {describeQuery.data?.inheritance && (
                  <p className="mt-1 font-mono text-[10px] text-turd-cream-dim">
                    {describeQuery.data.inheritance.join('  ←  ')}
                  </p>
                )}
                <div className="mt-3 flex items-center gap-3">
                  <div className="relative">
                    <button
                      type="button"
                      onClick={() => setShowAddDropdown((v) => !v)}
                      disabled={describeQuery.isLoading || !describeQuery.data?.found}
                      className="rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-1.5 font-mono text-[11px] text-turd-cream transition-colors hover:border-turd-mustard hover:text-turd-mustard-bright disabled:cursor-not-allowed disabled:opacity-30"
                    >
                      + Add override
                    </button>
                    {showAddDropdown && (
                      <AddOverrideDropdown
                        properties={properties}
                        valuesByKey={valuesByKey}
                        existingOverrides={overrides}
                        onAdd={handleAddOverride}
                        onClose={() => setShowAddDropdown(false)}
                      />
                    )}
                  </div>
                  <span className="font-mono text-[10px] text-turd-cream-dim">
                    {overrides.length} override{overrides.length !== 1 ? 's' : ''}
                  </span>
                  {(describeQuery.isLoading || valuesQuery.isLoading) && (
                    <span className="font-mono text-[10px] text-turd-cream-dim/60">
                      loading properties…
                    </span>
                  )}
                  {describeQuery.isError && (
                    <span className="font-mono text-[10px] text-turd-red">
                      {String(describeQuery.error?.message ?? '(error)')}
                    </span>
                  )}
                </div>
              </header>

              <div className="flex-1 overflow-auto">
                {overrides.length === 0 ? (
                  <div className="flex h-full items-center justify-center p-10 text-center">
                    <p className="font-mono text-xs text-turd-cream-dim">
                      No overrides yet. Click "+ Add override" to pick a property.
                      <br />
                      <span className="text-[10px] text-turd-cream-dim/50">
                        Only editable properties (bool / int / float / byte / name / string) appear.
                      </span>
                    </p>
                  </div>
                ) : (
                  <table className="w-full font-mono text-[11px]">
                    <thead className="sticky top-0 bg-turd-bg-deep">
                      <tr className="border-b border-turd-bronze/30 text-left text-turd-cream-dim">
                        <th className="px-4 py-2 font-normal">Property</th>
                        <th className="px-4 py-2 font-normal">Type</th>
                        <th className="px-4 py-2 font-normal">Value</th>
                        <th className="px-4 py-2 font-normal"></th>
                      </tr>
                    </thead>
                    <tbody>
                      {overrides.map((o, i) => (
                        <tr
                          key={`${o.property}-${i}`}
                          className="border-b border-turd-bronze/10 text-turd-cream hover:bg-white/[0.03]"
                        >
                          <td className="px-4 py-2">{o.property}</td>
                          <td className={`px-4 py-2 ${propertyTypeStyle(o.type)}`}>
                            {o.type}
                          </td>
                          <td className="px-4 py-2">
                            <OverrideValueInput
                              override={o}
                              onChange={(v) => handleChangeOverrideValue(i, v)}
                            />
                          </td>
                          <td className="px-4 py-2 text-right">
                            <button
                              type="button"
                              onClick={() => handleRemoveOverride(i)}
                              className="rounded px-1.5 py-0.5 font-mono text-[10px] text-turd-cream-dim/60 transition-colors hover:bg-turd-red/20 hover:text-turd-red"
                              title="Remove override"
                            >
                              ✕
                            </button>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                )}
              </div>

              {properties.length > 0 && (
                <details className="border-t border-turd-bronze/30 bg-turd-bg-deep/40">
                  <summary className="cursor-pointer px-4 py-2 font-display text-[11px] uppercase tracking-wider text-turd-mustard hover:bg-turd-bg-soft/30">
                    Preview · all properties ({properties.length})
                  </summary>
                  <div className="max-h-96 overflow-auto px-4 py-2">
                    <table className="w-full font-mono text-[10px]">
                      <thead className="sticky top-0 bg-turd-bg-deep">
                        <tr className="border-b border-turd-bronze/20 text-left text-turd-cream-dim">
                          <th className="px-2 py-1 font-normal">Name</th>
                          <th className="px-2 py-1 font-normal">Type</th>
                          <th className="px-2 py-1 font-normal">Value</th>
                          <th className="px-2 py-1 font-normal">Offset</th>
                        </tr>
                      </thead>
                      <tbody>
                        {properties.map((p) => {
                          const v = valuesByKey.get(`${p.name}@${p.offset}`);
                          const isEditable = EDITABLE_KINDS.has(p.type);
                          return (
                            <tr
                              key={`${p.name}@${p.offset}`}
                              className="border-b border-turd-bronze/10 hover:bg-white/[0.03]"
                            >
                              <td
                                className={`px-2 py-1 ${
                                  isEditable ? 'text-turd-cream' : 'text-turd-cream-dim'
                                }`}
                              >
                                {p.name}
                              </td>
                              <td className={`px-2 py-1 ${propertyTypeStyle(p.type)}`}>
                                {p.type}
                              </td>
                              <td className="px-2 py-1 text-turd-bronze-light">
                                {v
                                  ? String(v.value ?? '—')
                                  : isEditable
                                    ? '…'
                                    : '—'}
                              </td>
                              <td className="px-2 py-1 text-turd-cream-dim/50">
                                +{p.offset}
                              </td>
                            </tr>
                          );
                        })}
                      </tbody>
                    </table>
                  </div>
                </details>
              )}

              {overrides.length > 0 && (
                <footer className="border-t border-turd-bronze/20 p-3">
                  <p className="font-mono text-[10px] text-turd-cream-dim/60">
                    Recipe will write{' '}
                    <span className="text-turd-cream-dim">{overrides.length} override{overrides.length !== 1 ? 's' : ''}</span>
                    {' '}onto{' '}
                    <span className="text-turd-mustard">{selectedWidget}</span>
                    {recipeName.trim()
                      ? <> as <span className="text-turd-cream">"{recipeName.trim()}"</span></>
                      : <span className="text-turd-cream-dim/40"> (name the recipe above)</span>}
                    .
                  </p>
                </footer>
              )}
            </>
          )}
        </section>
      </div>
    </div>
  );
}
