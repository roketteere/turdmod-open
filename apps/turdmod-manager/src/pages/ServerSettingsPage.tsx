import { useState, useCallback, useMemo, type ReactNode } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { detectScum } from '../lib/tauri';
import { parseIni, applyPatch, inferType } from '../lib/ini';
import type { ParsedIni, IniSection } from '../lib/ini';
import { KEY_META, humaniseKey } from '../schemas/serverSettings';
import { useEngineHost } from '../lib/engineHost';
import { HostToggle } from '../components/HostToggle';

// OVH's ServerSettings.ini lives at a fixed path; read read-only over SSH.
const OVH_INI_PATH = 'C:\\SCUMServer\\SCUM\\Saved\\Config\\WindowsServer\\ServerSettings.ini';

// Comprehensive editor for SCUM's ServerSettings.ini. Loads the live file
// from <server>/SCUM/Saved/Config/WindowsServer/ServerSettings.ini, renders
// every key with type-inferred inputs, tracks per-key dirty state, and
// patches only the modified lines back to disk on save.
//
// Schema enrichment is OPTIONAL — keys not present in KEY_META still
// render with a humanised label and the right input type. The page never
// silently hides a key.

type LoadState = 'loading' | 'no-install' | 'parse-error' | 'ready';

interface SettingsState {
  parsed: ParsedIni;
  rawText: string;
  iniPath: string;
  values: Map<string, string>;
  baseline: Map<string, string>;
}

function buildMaps(parsed: ParsedIni): Map<string, string> {
  const m = new Map<string, string>();
  for (const section of parsed.sections) {
    for (const { key, value } of section.keys) {
      m.set(`[${section.name}].${key}`, value);
    }
  }
  return m;
}

function compoundKey(sectionName: string, key: string): string {
  return `[${sectionName}].${key}`;
}

interface BoolToggleProps {
  value: string;
  onChange: (v: string) => void;
}

function BoolToggle({ value, onChange }: BoolToggleProps) {
  const isTrue = value === 'True';
  return (
    <div className="flex items-center">
      <button
        type="button"
        onClick={() => onChange('True')}
        className={`px-3 py-1 rounded-l text-sm font-semibold border transition-colors ${
          isTrue
            ? 'bg-turd-green text-turd-bg-deep border-turd-green'
            : 'bg-turd-bg-mid text-turd-cream-dim border-turd-bg-soft hover:border-turd-cream-dim'
        }`}
      >
        True
      </button>
      <button
        type="button"
        onClick={() => onChange('False')}
        className={`px-3 py-1 rounded-r text-sm font-semibold border transition-colors ${
          !isTrue
            ? 'bg-turd-red text-turd-bg-deep border-turd-red'
            : 'bg-turd-bg-mid text-turd-cream-dim border-turd-bg-soft hover:border-turd-cream-dim'
        }`}
      >
        False
      </button>
    </div>
  );
}

interface KeyRowProps {
  sectionName: string;
  iniKey: string;
  currentValue: string;
  baselineValue: string;
  onChange: (ck: string, v: string) => void;
  onReset: (ck: string) => void;
}

function KeyRow({
  sectionName,
  iniKey,
  currentValue,
  baselineValue,
  onChange,
  onReset,
}: KeyRowProps) {
  const ck = compoundKey(sectionName, iniKey);
  const meta = KEY_META[ck];
  const inferredType = inferType(baselineValue);
  const isDirty = currentValue !== baselineValue;
  const [infoOpen, setInfoOpen] = useState(false);

  const label = meta?.label ?? humaniseKey(iniKey);
  const description = meta?.description;
  const hasRichInfo = !!(meta?.howToUse || meta?.benefit || meta?.sideEffect);

  const handleChange = useCallback(
    (v: string) => onChange(ck, v),
    [ck, onChange],
  );

  let inputEl: ReactNode;

  if (meta?.options) {
    inputEl = (
      <select
        value={currentValue}
        onChange={(e) => handleChange(e.target.value)}
        className="bg-turd-bg-soft border border-turd-bg-soft focus:border-turd-bronze rounded px-2 py-1 text-sm text-turd-cream w-56 outline-none"
      >
        {meta.options.map((opt) => (
          <option key={opt.value} value={opt.value}>
            {opt.label}
          </option>
        ))}
      </select>
    );
  } else if (inferredType === 'bool') {
    inputEl = <BoolToggle value={currentValue} onChange={handleChange} />;
  } else if (inferredType === 'int') {
    inputEl = (
      <input
        type="number"
        step="1"
        value={currentValue}
        min={meta?.min}
        max={meta?.max}
        onChange={(e) => handleChange(e.target.value)}
        className="bg-turd-bg-soft border border-turd-bg-soft focus:border-turd-bronze rounded px-2 py-1 text-sm text-turd-cream w-32 outline-none font-mono"
      />
    );
  } else if (inferredType === 'float') {
    inputEl = (
      <input
        type="number"
        step="any"
        value={currentValue}
        min={meta?.min}
        max={meta?.max}
        onChange={(e) => handleChange(e.target.value)}
        className="bg-turd-bg-soft border border-turd-bg-soft focus:border-turd-bronze rounded px-2 py-1 text-sm text-turd-cream w-40 outline-none font-mono"
      />
    );
  } else if (inferredType === 'duration') {
    inputEl = (
      <input
        type="text"
        placeholder="HH:MM:SS"
        value={currentValue}
        onChange={(e) => handleChange(e.target.value)}
        className="bg-turd-bg-soft border border-turd-bg-soft focus:border-turd-bronze rounded px-2 py-1 text-sm text-turd-cream w-36 outline-none font-mono"
      />
    );
  } else {
    const isLong =
      currentValue.length > 60 ||
      iniKey.toLowerCase().includes('message') ||
      iniKey.toLowerCase().includes('description') ||
      iniKey.toLowerCase().includes('welcome') ||
      iniKey.toLowerCase().includes('url');
    if (isLong) {
      inputEl = (
        <textarea
          value={currentValue}
          rows={2}
          onChange={(e) => handleChange(e.target.value)}
          className="bg-turd-bg-soft border border-turd-bg-soft focus:border-turd-bronze rounded px-2 py-1 text-sm text-turd-cream w-72 outline-none resize-y"
        />
      );
    } else {
      inputEl = (
        <input
          type="text"
          value={currentValue}
          onChange={(e) => handleChange(e.target.value)}
          className="bg-turd-bg-soft border border-turd-bg-soft focus:border-turd-bronze rounded px-2 py-1 text-sm text-turd-cream w-56 outline-none"
        />
      );
    }
  }

  return (
    <div
      className={`border-b border-turd-bg-soft ${
        isDirty ? 'bg-turd-bg-mid' : ''
      }`}
    >
      <div className="flex items-start justify-between gap-4 py-3 px-4">
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <span
              className={`text-sm font-semibold ${
                isDirty ? 'text-turd-mustard-bright' : 'text-turd-cream'
              }`}
            >
              {label}
            </span>
            {isDirty && <span className="text-turd-mustard text-xs">●</span>}
            {hasRichInfo && (
              <button
                type="button"
                onClick={() => setInfoOpen((o) => !o)}
                title={infoOpen ? 'Hide info' : 'What is this? How do I use it?'}
                className={`flex items-center justify-center w-5 h-5 rounded-full border text-[10px] font-bold transition-colors ${
                  infoOpen
                    ? 'border-turd-mustard bg-turd-mustard/20 text-turd-mustard-bright'
                    : 'border-turd-bronze/50 text-turd-bronze hover:border-turd-mustard hover:text-turd-mustard-bright'
                }`}
              >
                ?
              </button>
            )}
          </div>
          {description && (
            <p className="text-xs text-turd-cream-dim mt-0.5">{description}</p>
          )}
          <span className="font-mono text-xs text-turd-cream-dim opacity-60">{iniKey}</span>
        </div>

        <div className="flex items-center gap-2 shrink-0">
          {inputEl}
          {isDirty && (
            <button
              type="button"
              title="Reset to original"
              onClick={() => onReset(ck)}
              className="text-turd-cream-dim hover:text-turd-cream transition-colors text-base leading-none"
            >
              ↶
            </button>
          )}
        </div>
      </div>

      {infoOpen && hasRichInfo && (
        <div className="px-4 pb-4 -mt-1">
          <div className="rounded border border-turd-bronze/40 bg-turd-bg-deep/60 p-3 space-y-2">
            {meta?.howToUse && (
              <div>
                <span className="font-display text-[10px] uppercase tracking-wider text-turd-mustard">
                  How to use
                </span>
                <p className="text-xs text-turd-cream mt-0.5">{meta.howToUse}</p>
              </div>
            )}
            {meta?.benefit && (
              <div>
                <span className="font-display text-[10px] uppercase tracking-wider text-turd-green">
                  Benefit
                </span>
                <p className="text-xs text-turd-cream mt-0.5">{meta.benefit}</p>
              </div>
            )}
            {meta?.sideEffect && (
              <div>
                <span className="font-display text-[10px] uppercase tracking-wider text-turd-red">
                  Side effect / caution
                </span>
                <p className="text-xs text-turd-cream mt-0.5">{meta.sideEffect}</p>
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

export function ServerSettingsPage() {
  const queryClient = useQueryClient();
  const host = useEngineHost();
  const readOnly = host === 'remote'; // OVH config is view-only from here.
  const [loadState, setLoadState] = useState<LoadState>('loading');
  const [settings, setSettings] = useState<SettingsState | null>(null);
  const [activeSection, setActiveSection] = useState<string>('');
  const [searchQuery, setSearchQuery] = useState('');
  const [error, setError] = useState<string | null>(null);

  const { refetch: reloadFile } = useQuery({
    queryKey: ['serverSettings', host],
    queryFn: async () => {
      setLoadState('loading');
      setError(null);

      // Fetch the raw .ini from the active host: OVH over SSH (read-only) or
      // the local install via the detected path.
      let iniPath = '';
      let rawText = '';
      if (host === 'remote') {
        iniPath = OVH_INI_PATH;
        try {
          const res = await invoke<{ content: string; existed: boolean }>(
            'read_remote_text_file',
            { path: iniPath },
          );
          if (!res.existed) {
            setError(`OVH ServerSettings.ini not readable (tunnel/SSH down?): ${iniPath}`);
            setLoadState('parse-error');
            return null;
          }
          rawText = res.content;
        } catch (e) {
          setError(`OVH read failed: ${String(e)}`);
          setLoadState('parse-error');
          return null;
        }
      } else {
        const result = await detectScum();
        if (!result.server) {
          setLoadState('no-install');
          return null;
        }
        iniPath = `${result.server.replace(/\\/g, '/')}/SCUM/Saved/Config/WindowsServer/ServerSettings.ini`;
        try {
          const res = await invoke<{ content: string; existed: boolean }>(
            'manager_read_text_file',
            { path: iniPath },
          );
          if (!res.existed) {
            setError(`File not found: ${iniPath}`);
            setLoadState('parse-error');
            return null;
          }
          rawText = res.content;
        } catch (e) {
          setError(String(e));
          setLoadState('parse-error');
          return null;
        }
      }

      let parsed: ParsedIni;
      try {
        parsed = parseIni(rawText);
      } catch (e) {
        setError(String(e));
        setLoadState('parse-error');
        return null;
      }

      const values = buildMaps(parsed);
      const baseline = new Map(values);

      setSettings({ parsed, rawText, iniPath, values, baseline });
      if (parsed.sections.length > 0) {
        setActiveSection((prev) => prev || parsed.sections[0].name);
      }
      setLoadState('ready');
      return parsed;
    },
    refetchOnWindowFocus: false,
    retry: false,
  });

  const dirtyKeys = useMemo<Map<string, string>>(() => {
    if (!settings) return new Map();
    const dirty = new Map<string, string>();
    for (const [ck, val] of settings.values) {
      if (settings.baseline.get(ck) !== val) dirty.set(ck, val);
    }
    return dirty;
  }, [settings]);

  const isDirty = dirtyKeys.size > 0;

  const handleChange = useCallback((ck: string, value: string) => {
    setSettings((prev) => {
      if (!prev) return prev;
      const next = new Map(prev.values);
      next.set(ck, value);
      return { ...prev, values: next };
    });
  }, []);

  const handleReset = useCallback((ck: string) => {
    setSettings((prev) => {
      if (!prev) return prev;
      const original = prev.baseline.get(ck);
      if (original === undefined) return prev;
      const next = new Map(prev.values);
      next.set(ck, original);
      return { ...prev, values: next };
    });
  }, []);

  const handleDiscard = useCallback(() => {
    setSettings((prev) => {
      if (!prev) return prev;
      return { ...prev, values: new Map(prev.baseline) };
    });
  }, []);

  const saveMutation = useMutation({
    mutationFn: async () => {
      if (!settings) throw new Error('No settings loaded');
      const patch = new Map<string, string>(dirtyKeys);
      const patched = applyPatch(settings.rawText, patch);
      await invoke('manager_write_text_file', {
        path: settings.iniPath,
        content: patched,
      });
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['serverSettings'] });
      reloadFile();
    },
    onError: (e) => {
      setError(String(e));
    },
  });

  const query = searchQuery.toLowerCase().trim();

  const filteredSections = useMemo(() => {
    if (!settings || !query) return [];
    return settings.parsed.sections
      .map((s) => ({
        name: s.name,
        keys: s.keys.filter((k) => k.key.toLowerCase().includes(query)),
      }))
      .filter((s) => s.keys.length > 0);
  }, [settings, query]);

  const activeKeys = useMemo<IniSection['keys']>(() => {
    if (!settings || query) return [];
    const sec = settings.parsed.sections.find((s) => s.name === activeSection);
    return sec?.keys ?? [];
  }, [settings, activeSection, query]);

  const renderKeyRow = (sectionName: string, k: { key: string; value: string }) => {
    if (!settings) return null;
    const ck = compoundKey(sectionName, k.key);
    const currentValue = settings.values.get(ck) ?? k.value;
    const baselineValue = settings.baseline.get(ck) ?? k.value;
    return (
      <KeyRow
        key={ck}
        sectionName={sectionName}
        iniKey={k.key}
        currentValue={currentValue}
        baselineValue={baselineValue}
        onChange={handleChange}
        onReset={handleReset}
      />
    );
  };

  if (loadState === 'loading') {
    return (
      <div className="flex items-center justify-center h-64 text-turd-cream-dim text-sm">
        Loading ServerSettings.ini…
      </div>
    );
  }

  if (loadState === 'no-install') {
    return (
      <div className="flex flex-col items-center justify-center h-64 gap-3">
        <span className="text-turd-red font-semibold">SCUM server not detected</span>
        <span className="text-turd-cream-dim text-sm">
          Could not locate ServerSettings.ini. Make sure the SCUM dedicated
          server is installed and detected in Settings.
        </span>
        <button
          type="button"
          onClick={() => reloadFile()}
          className="mt-2 px-4 py-2 bg-turd-bronze text-turd-bg-deep rounded text-sm font-semibold hover:bg-turd-mustard transition-colors"
        >
          Retry
        </button>
      </div>
    );
  }

  if (loadState === 'parse-error') {
    return (
      <div className="flex flex-col items-center justify-center h-64 gap-3">
        <span className="text-turd-red font-semibold">Failed to load ServerSettings.ini</span>
        {error && (
          <pre className="text-xs text-turd-cream-dim bg-turd-bg-mid px-4 py-2 rounded max-w-2xl overflow-auto whitespace-pre-wrap">
            {error}
          </pre>
        )}
        <button
          type="button"
          onClick={() => reloadFile()}
          className="mt-2 px-4 py-2 bg-turd-bronze text-turd-bg-deep rounded text-sm font-semibold hover:bg-turd-mustard transition-colors"
        >
          Retry
        </button>
      </div>
    );
  }

  if (!settings) return null;

  const sections = settings.parsed.sections;

  return (
    <div className="flex flex-col h-full min-h-0 bg-turd-bg-deep text-turd-cream">
      <div className="px-6 pt-6 pb-3 shrink-0">
        <div className="flex items-center justify-between gap-3">
          <h1 className="font-display text-2xl text-turd-cream">
            Server Settings
            {isDirty && !readOnly && (
              <span className="ml-2 text-base text-turd-mustard font-mono">
                • {dirtyKeys.size} unsaved
              </span>
            )}
          </h1>
          <HostToggle />
        </div>
        <p className="text-turd-cream-dim text-sm mt-1">
          {readOnly
            ? 'Viewing the OVH production ServerSettings.ini (read-only). To change OVH config use the controlled stop→edit→start deploy, not a live edit.'
            : "Edit your SCUM server's ServerSettings.ini. All keys, organized by section."}
        </p>
        <p className="font-mono text-xs text-turd-cream-dim opacity-60 mt-1 truncate">
          {settings.iniPath}
          {readOnly && <span className="ml-2 text-turd-bronze">· ☁ OVH · read-only</span>}
        </p>
      </div>

      <div className="px-6 py-2 flex items-center gap-3 shrink-0 flex-wrap">
        <div className="relative flex-1 max-w-xs">
          <input
            type="text"
            placeholder="Search keys…"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="w-full bg-turd-bg-soft border border-turd-bg-soft focus:border-turd-bronze rounded px-3 py-1.5 text-sm text-turd-cream outline-none pr-7"
          />
          {searchQuery && (
            <button
              type="button"
              onClick={() => setSearchQuery('')}
              className="absolute right-2 top-1/2 -translate-y-1/2 text-turd-cream-dim hover:text-turd-cream text-xs"
            >
              ✕
            </button>
          )}
        </div>

        <div className="flex items-center gap-2 ml-auto">
          {error && saveMutation.isError && (
            <span className="text-turd-red text-xs">{error}</span>
          )}
          <button
            type="button"
            onClick={() => reloadFile()}
            disabled={saveMutation.isPending}
            className="px-3 py-1.5 rounded text-sm border border-turd-bg-soft text-turd-cream-dim hover:text-turd-cream hover:border-turd-cream-dim transition-colors disabled:opacity-40"
          >
            Reload
          </button>
          <button
            type="button"
            onClick={handleDiscard}
            disabled={!isDirty || saveMutation.isPending}
            className="px-3 py-1.5 rounded text-sm border border-turd-bg-soft text-turd-cream-dim hover:text-turd-red hover:border-turd-red transition-colors disabled:opacity-40"
          >
            Discard
          </button>
          <button
            type="button"
            onClick={() => saveMutation.mutate()}
            disabled={!isDirty || saveMutation.isPending || readOnly}
            title={readOnly ? 'OVH config is read-only here — use the controlled deploy to change it' : undefined}
            className={`px-4 py-1.5 rounded text-sm font-semibold transition-colors disabled:opacity-40 ${
              isDirty && !readOnly
                ? 'bg-turd-bronze text-turd-bg-deep hover:bg-turd-mustard-bright'
                : 'bg-turd-bg-soft text-turd-cream-dim cursor-not-allowed'
            }`}
          >
            {readOnly ? 'Read-only' : saveMutation.isPending ? 'Saving…' : 'Save'}
          </button>
        </div>
      </div>

      {!query && (
        <div className="px-6 flex gap-1 overflow-x-auto shrink-0 border-b border-turd-bg-soft pb-0">
          {sections.map((sec) => {
            const sectionDirty = [...dirtyKeys.keys()].some((ck) =>
              ck.startsWith(`[${sec.name}].`),
            );
            return (
              <button
                key={sec.name}
                type="button"
                onClick={() => setActiveSection(sec.name)}
                className={`px-4 py-2 text-sm font-semibold whitespace-nowrap border-b-2 transition-colors ${
                  activeSection === sec.name
                    ? 'border-turd-bronze text-turd-cream'
                    : 'border-transparent text-turd-cream-dim hover:text-turd-cream'
                }`}
              >
                {sec.name}
                {sectionDirty && (
                  <span className="ml-1 text-turd-mustard text-xs">●</span>
                )}
              </button>
            );
          })}
        </div>
      )}

      <div className="flex-1 overflow-y-auto min-h-0">
        {query ? (
          filteredSections.length === 0 ? (
            <div className="flex items-center justify-center h-32 text-turd-cream-dim text-sm">
              No keys match "{searchQuery}"
            </div>
          ) : (
            filteredSections.map((sec) => (
              <div key={sec.name}>
                <div className="px-4 py-1.5 bg-turd-bg-mid border-b border-turd-bg-soft">
                  <span className="text-xs font-semibold text-turd-mustard uppercase tracking-wider">
                    {sec.name}
                  </span>
                </div>
                {sec.keys.map((k) => renderKeyRow(sec.name, k))}
              </div>
            ))
          )
        ) : activeKeys.length === 0 ? (
          <div className="flex items-center justify-center h-32 text-turd-cream-dim text-sm">
            No keys in this section.
          </div>
        ) : (
          activeKeys.map((k) => renderKeyRow(activeSection, k))
        )}
      </div>
    </div>
  );
}
