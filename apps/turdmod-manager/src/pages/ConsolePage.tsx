import {
  useEffect,
  useRef,
  useState,
  useCallback,
  useMemo,
} from 'react';
import { Link } from 'react-router-dom';
import { invoke } from '@tauri-apps/api/core';
import {
  clearConsole,
  getConsoleLines,
  subscribeConsole,
  type LogLevel,
  type LogSource,
  type UnifiedLine,
} from '../lib/console-store';
import { useCompanionStatus } from '../hooks/useCompanion';
import { useEngineStatus } from '../hooks/useEngine';

// ─── Filter model ──────────────────────────────────────────────────────────
//
// A Filter is a structured pattern with three attributes:
//   - text: the pattern source (substring or regex source, no surrounding /)
//   - kind: 'include' means a matching line PASSES; 'exclude' means it FAILS
//   - regex: when true, `text` is compiled as a JavaScript regex (case-
//            insensitive); when false, `text` is matched as a case-insensitive
//            substring.
//
// Filter logic:
//   1. Levels + sources gate first (as before).
//   2. Active include filters: a line must match AT LEAST ONE (OR) — unless
//      no includes are active, in which case pass-through.
//   3. Active exclude filters: a line must match NONE — any exclude match
//      kills the line.
//
// This gives the "CLI-style" power: positive filtering + noise suppression,
// with regex when substring isn't expressive enough.

type Filter = {
  text: string;
  kind: 'include' | 'exclude';
  regex: boolean;
};

// Standard export path — fixed so Claude can poll it and Joel can grep with
// find-in-text tools. Future iteration could make this user-configurable.
const DEFAULT_EXPORT_PATH =
  'C:\\Users\\YOUR_USER\\AppData\\Local\\TurdMOD\\console-filter-export.jsonl';

// ---------------------------------------------------------------------------
// Source badge — colors per source
// ---------------------------------------------------------------------------

const SOURCE_STYLE: Record<LogSource, string> = {
  companion: 'text-turd-mustard-bright',
  launcher:  'text-turd-cream',
  ue4ss:     'text-turd-green',
  loader:    'text-turd-mustard',
  scum:      'text-turd-bronze-light',
};

function SourceBadge({ source }: { source: LogSource }) {
  return (
    <span className={`inline-block w-16 shrink-0 font-mono text-[10px] uppercase ${SOURCE_STYLE[source]}`}>
      {source}
    </span>
  );
}

function LevelBadge({ level }: { level: LogLevel }) {
  const cls =
    level === 'error'
      ? 'text-turd-red'
      : level === 'warn'
      ? 'text-turd-mustard'
      : 'text-turd-cream-dim';
  return (
    <span className={`inline-block w-10 shrink-0 font-mono text-[10px] uppercase ${cls}`}>
      {level}
    </span>
  );
}

function shortTs(iso: string): string {
  try {
    const d = new Date(iso);
    const hh = String(d.getUTCHours()).padStart(2, '0');
    const mm = String(d.getUTCMinutes()).padStart(2, '0');
    const ss = String(d.getUTCSeconds()).padStart(2, '0');
    return `${hh}:${mm}:${ss}`;
  } catch {
    return '??:??:??';
  }
}

// ---------------------------------------------------------------------------
// Page
// ---------------------------------------------------------------------------

const ALL_SOURCES: LogSource[] = ['companion', 'launcher', 'ue4ss', 'loader', 'scum'];

export function ConsolePage() {
  const [lines, setLines] = useState<UnifiedLine[]>(() => getConsoleLines());

  // ─── Multi-filter state + history ────────────────────────────────────────
  // Filters are structured (text/kind/regex) so the Console supports CLI-
  // grade power: include + exclude + regex. localStorage schema bumped to
  // v2; v1 (plain string array) is migrated forward on first load so users
  // don't lose history.
  const LS_FILTERS_V2 = 'console-filters-v2';
  const LS_HISTORY_V2 = 'console-filter-history-v2';
  const LS_FILTERS_V1 = 'console-filters-v1';
  const LS_HISTORY_V1 = 'console-filter-history-v1';
  const LS_EXPORT_ENABLED = 'console-export-enabled-v1';

  const loadFilters = (v2Key: string, v1Key: string): Filter[] => {
    try {
      const v2 = localStorage.getItem(v2Key);
      if (v2) {
        const parsed = JSON.parse(v2);
        if (Array.isArray(parsed)) {
          return parsed
            .filter(
              (f): f is Filter =>
                f &&
                typeof f === 'object' &&
                typeof f.text === 'string' &&
                (f.kind === 'include' || f.kind === 'exclude') &&
                typeof f.regex === 'boolean',
            );
        }
      }
      // v1 migration: strings → include/substring filters.
      const v1 = localStorage.getItem(v1Key);
      if (v1) {
        const parsed = JSON.parse(v1);
        if (Array.isArray(parsed)) {
          return parsed
            .filter((s) => typeof s === 'string')
            .map((text: string) => ({ text, kind: 'include' as const, regex: false }));
        }
      }
    } catch {
      // fall through to empty
    }
    return [];
  };

  const [filterInput, setFilterInput] = useState('');
  const [filterInputRegex, setFilterInputRegex] = useState(false);
  const [filters, setFilters] = useState<Filter[]>(() => loadFilters(LS_FILTERS_V2, LS_FILTERS_V1));
  const [filterHistory, setFilterHistory] = useState<Filter[]>(() =>
    loadFilters(LS_HISTORY_V2, LS_HISTORY_V1),
  );

  // File-export toggle — when on, each visible line is appended to a
  // local JSONL file (DEFAULT_EXPORT_PATH) via the Rust-side
  // manager_append_text_file Tauri command. This is what lets Claude poll
  // a stable file for live filter results AND lets Joel grep history
  // with any text editor's find-in-files feature.
  const [exportEnabled, setExportEnabled] = useState<boolean>(() => {
    try {
      return localStorage.getItem(LS_EXPORT_ENABLED) === 'true';
    } catch {
      return false;
    }
  });
  const [exportHint, setExportHint] = useState<string | null>(null);
  // Default: companion + launcher + scum on (the day-to-day signal).
  // ue4ss + loader off — they're engine-startup diagnostics, mostly
  // noise during normal play. Tick the checkbox to re-enable when
  // troubleshooting bridge issues.
  //
  // Both `levels` and `sources` persist to localStorage so they survive
  // tab switches and app restarts. The defaults merge in on every read
  // so adding a new source/level later doesn't strand users with a
  // partial saved object.
  const LS_LEVELS = 'console-levels-v1';
  const LS_SOURCES = 'console-sources-v1';
  const LEVEL_DEFAULTS = { info: true, warn: true, error: true };
  const SOURCE_DEFAULTS: Record<LogSource, boolean> = {
    companion: true, launcher: true, ue4ss: false, loader: false, scum: true,
  };
  const [levels, setLevels] = useState<typeof LEVEL_DEFAULTS>(() => {
    try {
      const raw = localStorage.getItem(LS_LEVELS);
      if (!raw) return LEVEL_DEFAULTS;
      const parsed = JSON.parse(raw);
      if (parsed && typeof parsed === 'object') {
        return { ...LEVEL_DEFAULTS, ...parsed };
      }
      return LEVEL_DEFAULTS;
    } catch {
      return LEVEL_DEFAULTS;
    }
  });
  const [sources, setSources] = useState<Record<LogSource, boolean>>(() => {
    try {
      const raw = localStorage.getItem(LS_SOURCES);
      if (!raw) return SOURCE_DEFAULTS;
      const parsed = JSON.parse(raw);
      if (parsed && typeof parsed === 'object') {
        return { ...SOURCE_DEFAULTS, ...parsed };
      }
      return SOURCE_DEFAULTS;
    } catch {
      return SOURCE_DEFAULTS;
    }
  });
  const [autoscroll, setAutoscroll] = useState(true);
  const scrollRef = useRef<HTMLDivElement>(null);
  const autoscrollRef = useRef(autoscroll);
  autoscrollRef.current = autoscroll;

  // True while we're programmatically scrolling — handleScroll uses this
  // to ignore the scroll event our own scrollTop assignment triggers.
  // Without this, autoscroll silently disables itself the moment a new
  // log line lands between the effect running and the scroll event
  // being processed: scrollHeight has grown but scrollTop hasn't, so
  // the atBottom math reports false even though we ARE at the bottom.
  const programmaticScrollRef = useRef(false);

  // Row-selection model — tracks line keys (not indices, since the
  // visible array shrinks as MAX_LINES rolls over). Click replaces
  // selection, Shift+click extends range, Ctrl/Cmd+click toggles
  // individual. "Copy selected" + "Copy all visible" copy as tab-
  // friendly plain text.
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [anchorKey, setAnchorKey] = useState<string | null>(null);
  const [copyHint, setCopyHint] = useState<string | null>(null);

  const companionStatus = useCompanionStatus();
  const engineStatus = useEngineStatus();
  const anythingRunning =
    companionStatus.data?.kind === 'running' || engineStatus.data?.kind === 'running';

  // Subscribe to the module-level store. The actual Tauri event
  // subscriptions are started in App.tsx so the buffer fills regardless
  // of which page is mounted.
  useEffect(() => {
    return subscribeConsole(setLines);
  }, []);

  // Persist active filters + history + level/source toggles + export
  // toggle to localStorage on change.
  useEffect(() => {
    localStorage.setItem(LS_FILTERS_V2, JSON.stringify(filters));
  }, [filters]);
  useEffect(() => {
    localStorage.setItem(LS_HISTORY_V2, JSON.stringify(filterHistory));
  }, [filterHistory]);
  useEffect(() => {
    localStorage.setItem(LS_LEVELS, JSON.stringify(levels));
  }, [levels]);
  useEffect(() => {
    localStorage.setItem(LS_SOURCES, JSON.stringify(sources));
  }, [sources]);
  useEffect(() => {
    localStorage.setItem(LS_EXPORT_ENABLED, exportEnabled ? 'true' : 'false');
  }, [exportEnabled]);

  // ─── Filter management callbacks ─────────────────────────────────────────
  //
  // Two equal-key filters (same text + same kind + same regex flag) are
  // considered identical for dedup purposes. History pushes newest-first
  // and caps at 50 — enough for a deep working session, small enough to
  // browse.
  const filterKey = (f: Filter) => `${f.kind}|${f.regex ? 'r' : 's'}|${f.text}`;
  const sameFilter = (a: Filter, b: Filter) => filterKey(a) === filterKey(b);

  const addFilter = useCallback(
    (kind: 'include' | 'exclude') => {
      const trimmed = filterInput.trim();
      if (!trimmed) return;
      const next: Filter = { text: trimmed, kind, regex: filterInputRegex };
      if (filters.some((f) => sameFilter(f, next))) {
        setFilterInput('');
        return;
      }
      setFilters((prev) => [...prev, next]);
      setFilterHistory((prev) => {
        const without = prev.filter((f) => !sameFilter(f, next));
        return [next, ...without].slice(0, 50);
      });
      setFilterInput('');
    },
    [filterInput, filterInputRegex, filters],
  );

  const removeFilter = useCallback((target: Filter) => {
    setFilters((prev) => prev.filter((f) => !sameFilter(f, target)));
  }, []);

  const clearAllFilters = useCallback(() => {
    setFilters([]);
  }, []);

  const reuseFromHistory = useCallback((target: Filter) => {
    setFilters((prev) => (prev.some((f) => sameFilter(f, target)) ? prev : [...prev, target]));
  }, []);

  const deleteFromHistory = useCallback((target: Filter) => {
    setFilterHistory((prev) => prev.filter((f) => !sameFilter(f, target)));
  }, []);

  // Clear the export file. Useful at the start of a new test session so
  // grep doesn't drown in stale matches.
  const clearExportFile = useCallback(async () => {
    try {
      await invoke('manager_write_text_file', {
        path: DEFAULT_EXPORT_PATH,
        content: '',
      });
      setExportHint('Export file cleared');
    } catch (e) {
      setExportHint(`Clear failed: ${e}`);
    }
    setTimeout(() => setExportHint(null), 2500);
  }, []);

  // Autoscroll on new lines. Mark the scroll as programmatic so the
  // onScroll listener doesn't treat it as a user "scroll away" event.
  useEffect(() => {
    if (autoscrollRef.current && scrollRef.current) {
      programmaticScrollRef.current = true;
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
      // Release the flag after the browser has had a chance to fire the
      // scroll event. requestAnimationFrame is enough — onScroll for a
      // programmatic scrollTop write fires on the same task or the next
      // microtask, before rAF callbacks.
      requestAnimationFrame(() => {
        programmaticScrollRef.current = false;
      });
    }
  }, [lines]);

  const handleScroll = useCallback(() => {
    if (programmaticScrollRef.current) return; // ignore our own writes
    const el = scrollRef.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 50;
    if (!atBottom && autoscrollRef.current) {
      setAutoscroll(false);
    }
  }, []);

  const resumeAutoscroll = () => {
    setAutoscroll(true);
    if (scrollRef.current) {
      programmaticScrollRef.current = true;
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
      requestAnimationFrame(() => {
        programmaticScrollRef.current = false;
      });
    }
  };

  // ─── Filter logic (include/exclude/regex) ────────────────────────────────
  //
  // Compile every active filter into a matcher function once per render
  // (cheap; substrings just lowercase, regexes compile once). Invalid
  // regexes fall back to substring against the literal source so a bad
  // pattern doesn't break the whole filter.
  const { includeMatchers, excludeMatchers } = useMemo(() => {
    const compile = (f: Filter): ((raw: string) => boolean) => {
      if (f.regex) {
        try {
          const re = new RegExp(f.text, 'i');
          return (raw: string) => re.test(raw);
        } catch {
          // Bad regex — fall back to substring on the literal source.
          const low = f.text.toLowerCase();
          return (raw: string) => raw.toLowerCase().includes(low);
        }
      }
      const low = f.text.toLowerCase();
      return (raw: string) => raw.toLowerCase().includes(low);
    };
    return {
      includeMatchers: filters.filter((f) => f.kind === 'include').map(compile),
      excludeMatchers: filters.filter((f) => f.kind === 'exclude').map(compile),
    };
  }, [filters]);

  const visible = lines.filter((l) => {
    if (!levels[l.level]) return false;
    if (!sources[l.source]) return false;
    // Includes: line must match AT LEAST ONE (OR). Zero active includes
    // = pass-through.
    if (includeMatchers.length > 0) {
      if (!includeMatchers.some((m) => m(l.raw))) return false;
    }
    // Excludes: line must match NONE. Any match kills it.
    if (excludeMatchers.some((m) => m(l.raw))) return false;
    return true;
  });

  // ─── File-export effect ──────────────────────────────────────────────────
  //
  // When `exportEnabled` is true, every line that passes the visible
  // filter gets appended to DEFAULT_EXPORT_PATH (JSONL — one JSON record
  // per line). `exportedKeysRef` tracks which lines have already been
  // exported in this session so we don't double-write across re-renders.
  //
  // Toggling export OFF + ON again doesn't replay history — only NEW
  // lines after the last toggle. Clear-file button is the explicit way
  // to wipe accumulated history.
  const exportedKeysRef = useRef<Set<string>>(new Set());
  useEffect(() => {
    if (!exportEnabled) return;
    const toExport: UnifiedLine[] = [];
    for (const l of visible) {
      const key = `${l.ts}|${l.source}|${l.level}|${l.raw}`;
      if (!exportedKeysRef.current.has(key)) {
        toExport.push(l);
        exportedKeysRef.current.add(key);
      }
    }
    if (toExport.length === 0) return;
    // Fire-and-forget append. Per-line invoke is fine at typical log
    // volumes (<100/sec); high-rate scenarios could batch but it's not
    // worth the complexity yet.
    const payload =
      toExport
        .map((l) =>
          JSON.stringify({
            ts: l.ts,
            source: l.source,
            level: l.level,
            raw: l.raw,
          }),
        )
        .join('\n') + '\n';
    invoke('manager_append_text_file', {
      path: DEFAULT_EXPORT_PATH,
      content: payload,
    }).catch((e) => {
      // Don't spam the hint state on every failure — just log.
      console.error('[ConsolePage] export append failed:', e);
    });
  }, [visible, exportEnabled]);

  const lineKey = (l: UnifiedLine) =>
    `${l.ts}|${l.source}|${l.level}|${l.raw}`;

  const formatLineForCopy = (l: UnifiedLine) =>
    `[${shortTs(l.ts)}] ${l.source.toUpperCase().padEnd(9)} ${l.level
      .toUpperCase()
      .padEnd(5)} ${l.raw}`;

  const copyLines = async (subset: UnifiedLine[]) => {
    if (subset.length === 0) {
      setCopyHint('Nothing to copy');
      return;
    }
    const text = subset.map(formatLineForCopy).join('\n');
    try {
      await navigator.clipboard.writeText(text);
      setCopyHint(`Copied ${subset.length} line${subset.length === 1 ? '' : 's'}`);
    } catch (e) {
      setCopyHint(`Copy failed: ${e}`);
    }
    setTimeout(() => setCopyHint(null), 2500);
  };

  const handleRowClick = (e: React.MouseEvent, line: UnifiedLine) => {
    const key = lineKey(line);
    if (e.shiftKey && anchorKey) {
      const anchorIdx = visible.findIndex((l) => lineKey(l) === anchorKey);
      const thisIdx = visible.findIndex((l) => lineKey(l) === key);
      if (anchorIdx >= 0 && thisIdx >= 0) {
        const [lo, hi] =
          thisIdx < anchorIdx ? [thisIdx, anchorIdx] : [anchorIdx, thisIdx];
        const next = new Set(selected);
        for (let i = lo; i <= hi; i++) next.add(lineKey(visible[i]));
        setSelected(next);
        return;
      }
    }
    if (e.ctrlKey || e.metaKey) {
      const next = new Set(selected);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      setSelected(next);
      setAnchorKey(key);
      return;
    }
    // Plain click → replace with single selection.
    setSelected(new Set([key]));
    setAnchorKey(key);
  };

  const selectedLines = visible.filter((l) => selected.has(lineKey(l)));

  const inputCls =
    'rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-1.5 font-mono text-xs text-turd-cream placeholder:text-turd-cream-dim/60 focus:border-turd-mustard focus:outline-none';

  return (
    <div className="flex h-full flex-col gap-3">
      <header>
        <p className="font-display text-xs tracking-[0.3em] text-turd-mustard">
          TurdMOD
        </p>
        <h1 className="mt-1 font-display text-3xl text-turd-cream">Console</h1>
      </header>

      {/* Filter management — input + sign + regex + active chips + history + export */}
      <div className="rounded-lg border border-turd-bronze/30 bg-turd-bg-mid p-3">
        <div className="flex flex-wrap items-center gap-2">
          <input
            type="text"
            placeholder='Filter pattern — Enter adds INCLUDE; substring by default. Toggle "rx" for regex.'
            value={filterInput}
            onChange={(e) => setFilterInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault();
                addFilter('include');
              }
            }}
            className={`${inputCls} w-96`}
          />
          <label className="flex cursor-pointer select-none items-center gap-1.5">
            <input
              type="checkbox"
              checked={filterInputRegex}
              onChange={(e) => setFilterInputRegex(e.target.checked)}
              className="accent-turd-mustard"
            />
            <span className="font-mono text-[10px] uppercase tracking-wider text-turd-cream-dim">
              rx (regex)
            </span>
          </label>
          <button
            type="button"
            onClick={() => addFilter('include')}
            disabled={!filterInput.trim()}
            className="rounded border border-turd-mustard-bright/60 bg-turd-mustard-bright/10 px-3 py-1.5 font-display text-xs uppercase tracking-wider text-turd-mustard-bright transition-colors hover:border-turd-mustard-bright hover:bg-turd-mustard-bright/20 disabled:cursor-not-allowed disabled:opacity-40"
            title="Add as INCLUDE filter — line must match AT LEAST ONE include"
          >
            + Include
          </button>
          <button
            type="button"
            onClick={() => addFilter('exclude')}
            disabled={!filterInput.trim()}
            className="rounded border border-turd-red/60 bg-turd-red/10 px-3 py-1.5 font-display text-xs uppercase tracking-wider text-turd-red transition-colors hover:border-turd-red hover:bg-turd-red/20 disabled:cursor-not-allowed disabled:opacity-40"
            title="Add as EXCLUDE filter — any matching line is hidden"
          >
            − Exclude
          </button>
          {filters.length > 0 && (
            <button
              type="button"
              onClick={clearAllFilters}
              className="rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-1.5 font-display text-xs uppercase tracking-wider text-turd-cream-dim transition-colors hover:border-turd-red hover:text-turd-red"
            >
              Clear active ({filters.length})
            </button>
          )}
          <span className="font-mono text-[10px] text-turd-cream-dim/60">
            INCLUDE: OR-of-matches (any wins). EXCLUDE: NONE-must-match (any kills).
          </span>
        </div>

        {filters.length > 0 && (
          <div className="mt-2 flex flex-wrap items-center gap-1.5">
            <span className="font-display text-[10px] uppercase tracking-wider text-turd-mustard-bright">
              Active:
            </span>
            {filters.map((f) => {
              const isInclude = f.kind === 'include';
              const chipCls = isInclude
                ? 'border-turd-mustard-bright/50 bg-turd-mustard-bright/10 text-turd-mustard-bright'
                : 'border-turd-red/50 bg-turd-red/10 text-turd-red';
              return (
                <span
                  key={filterKey(f)}
                  className={`inline-flex items-center gap-1 rounded-full border px-2 py-0.5 font-mono text-[11px] ${chipCls}`}
                  title={`${f.kind} • ${f.regex ? 'regex (case-insensitive)' : 'substring'} • "${f.text}"`}
                >
                  <span className="font-bold">{isInclude ? '+' : '−'}</span>
                  {f.regex && <span className="rounded bg-turd-bg-deep/40 px-1 text-[9px]">rx</span>}
                  {f.text}
                  <button
                    type="button"
                    onClick={() => removeFilter(f)}
                    className="text-turd-cream-dim hover:text-turd-red"
                    title="Remove this filter"
                  >
                    ×
                  </button>
                </span>
              );
            })}
          </div>
        )}

        {filterHistory.length > 0 && (
          <div className="mt-2 flex flex-wrap items-center gap-1.5">
            <span className="font-display text-[10px] uppercase tracking-wider text-turd-cream-dim">
              History:
            </span>
            {filterHistory.map((f) => {
              const isActive = filters.some((g) => sameFilter(g, f));
              const isInclude = f.kind === 'include';
              const baseColor = isInclude ? 'turd-mustard-bright' : 'turd-red';
              return (
                <span
                  key={filterKey(f)}
                  className={`inline-flex items-center gap-1 rounded-full border px-2 py-0.5 font-mono text-[11px] ${
                    isActive
                      ? 'cursor-default border-turd-bronze/30 bg-turd-bg-deep/60 text-turd-cream-dim/60'
                      : isInclude
                      ? `cursor-pointer border-${baseColor}/40 bg-turd-bg-soft text-${baseColor}/70 hover:border-${baseColor} hover:text-${baseColor}`
                      : `cursor-pointer border-${baseColor}/40 bg-turd-bg-soft text-${baseColor}/70 hover:border-${baseColor} hover:text-${baseColor}`
                  }`}
                  onClick={() => !isActive && reuseFromHistory(f)}
                  title={
                    isActive
                      ? 'Already active'
                      : `Click to add as ${f.kind} filter`
                  }
                >
                  <span className="font-bold">{isInclude ? '+' : '−'}</span>
                  {f.regex && <span className="rounded bg-turd-bg-deep/40 px-1 text-[9px]">rx</span>}
                  {f.text}
                  <button
                    type="button"
                    onClick={(e) => {
                      e.stopPropagation();
                      deleteFromHistory(f);
                    }}
                    className="text-turd-cream-dim/60 hover:text-turd-red"
                    title="Remove from history"
                  >
                    ×
                  </button>
                </span>
              );
            })}
          </div>
        )}

        {/* File-export controls — for Claude visibility + grep/find-in-text */}
        <div className="mt-3 flex flex-wrap items-center gap-2 border-t border-turd-bronze/20 pt-2">
          <label className="flex cursor-pointer select-none items-center gap-1.5">
            <input
              type="checkbox"
              checked={exportEnabled}
              onChange={(e) => setExportEnabled(e.target.checked)}
              className="accent-turd-mustard"
            />
            <span className="font-display text-[10px] uppercase tracking-wider text-turd-mustard-bright">
              Export matches to file (live)
            </span>
          </label>
          <button
            type="button"
            onClick={clearExportFile}
            className="rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-1.5 font-display text-[10px] uppercase tracking-wider text-turd-cream-dim transition-colors hover:border-turd-red hover:text-turd-red"
            title="Truncate the export file — useful at the start of a new test session"
          >
            Clear export file
          </button>
          <span className="font-mono text-[10px] text-turd-cream-dim/60">
            {DEFAULT_EXPORT_PATH}
          </span>
          {exportHint && (
            <span className="font-mono text-[10px] text-turd-mustard-bright">{exportHint}</span>
          )}
        </div>
      </div>

      {/* Toolbar */}
      <div className="flex flex-wrap items-center gap-3">
        <div className="flex items-center gap-3">
          {(['info', 'warn', 'error'] as const).map((lvl) => (
            <label key={lvl} className="flex cursor-pointer items-center gap-1.5">
              <input
                type="checkbox"
                checked={levels[lvl]}
                onChange={(e) =>
                  setLevels((prev) => ({ ...prev, [lvl]: e.target.checked }))
                }
                className="accent-turd-mustard"
              />
              <span
                className={`font-mono text-[10px] uppercase tracking-wider ${
                  lvl === 'error'
                    ? 'text-turd-red'
                    : lvl === 'warn'
                    ? 'text-turd-mustard'
                    : 'text-turd-cream-dim'
                }`}
              >
                {lvl}
              </span>
            </label>
          ))}
        </div>

        <div className="flex items-center gap-3">
          {ALL_SOURCES.map((src) => (
            <label key={src} className="flex cursor-pointer items-center gap-1.5">
              <input
                type="checkbox"
                checked={sources[src]}
                onChange={(e) =>
                  setSources((prev) => ({ ...prev, [src]: e.target.checked }))
                }
                className="accent-turd-mustard"
              />
              <span className={`font-mono text-[10px] uppercase tracking-wider ${SOURCE_STYLE[src]}`}>
                {src}
              </span>
            </label>
          ))}
        </div>

        <button
          type="button"
          onClick={() => copyLines(selectedLines)}
          disabled={selectedLines.length === 0}
          className="rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-1.5 font-display text-xs uppercase tracking-wider text-turd-cream-dim transition-colors hover:border-turd-cream hover:text-turd-cream disabled:cursor-not-allowed disabled:opacity-40"
          title="Copy the selected lines to clipboard"
        >
          Copy sel ({selectedLines.length})
        </button>

        <button
          type="button"
          onClick={() => copyLines(visible)}
          disabled={visible.length === 0}
          className="rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-1.5 font-display text-xs uppercase tracking-wider text-turd-cream-dim transition-colors hover:border-turd-cream hover:text-turd-cream disabled:cursor-not-allowed disabled:opacity-40"
          title="Copy every currently-visible line to clipboard"
        >
          Copy all
        </button>

        <button
          type="button"
          onClick={() => {
            setSelected(new Set());
            setAnchorKey(null);
          }}
          disabled={selected.size === 0}
          className="rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-1.5 font-display text-xs uppercase tracking-wider text-turd-cream-dim transition-colors hover:border-turd-cream hover:text-turd-cream disabled:cursor-not-allowed disabled:opacity-40"
        >
          Deselect
        </button>

        <button
          type="button"
          onClick={() => clearConsole()}
          className="rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-1.5 font-display text-xs uppercase tracking-wider text-turd-cream-dim transition-colors hover:border-turd-cream hover:text-turd-cream"
        >
          Clear
        </button>

        <span className="ml-auto flex items-center gap-3 font-mono text-[10px] text-turd-cream-dim">
          {copyHint && (
            <span className="text-turd-mustard-bright">{copyHint}</span>
          )}
          <span>
            {visible.length}/{lines.length} lines
          </span>
        </span>
      </div>

      {/* Console body */}
      <div className="relative flex-1 overflow-hidden rounded-lg border border-turd-bronze/30 bg-turd-bg-deep">
        {lines.length === 0 && !anythingRunning ? (
          <div className="flex h-full items-center justify-center p-10 text-center">
            <div>
              <p className="font-display text-turd-cream-dim">
                Nothing running.
              </p>
              <p className="mt-2 text-xs text-turd-cream-dim">
                <Link
                  to="/engine"
                  className="text-turd-mustard-bright underline hover:opacity-80"
                >
                  Start the Engine or Companion
                </Link>{' '}
                to see live output here.
              </p>
            </div>
          </div>
        ) : (
          <div
            ref={scrollRef}
            onScroll={handleScroll}
            className="h-full overflow-auto p-3"
          >
            {visible.map((line, i) => {
              const key = lineKey(line);
              const isSelected = selected.has(key);
              return (
                <div
                  key={`${key}-${i}`}
                  onClick={(e) => handleRowClick(e, line)}
                  title="Click select • Shift+click range • Ctrl+click toggle"
                  className={`flex cursor-pointer items-start gap-2 select-text leading-5 ${
                    isSelected
                      ? 'bg-turd-mustard-bright/15'
                      : 'hover:bg-white/5'
                  }`}
                >
                  <span className="shrink-0 font-mono text-[10px] text-turd-cream-dim/60">
                    {shortTs(line.ts)}
                  </span>
                  <SourceBadge source={line.source} />
                  <LevelBadge level={line.level} />
                  <span className="break-all font-mono text-[11px] text-turd-cream">
                    {line.raw}
                  </span>
                </div>
              );
            })}
            {visible.length === 0 && lines.length > 0 && (
              <p className="py-4 text-center font-mono text-xs text-turd-cream-dim">
                No lines match current filter.
              </p>
            )}
          </div>
        )}

        {!autoscroll && (
          <button
            type="button"
            onClick={resumeAutoscroll}
            className="absolute bottom-3 right-3 rounded-full border border-turd-mustard/60 bg-turd-bg-mid px-3 py-1 font-display text-[10px] uppercase tracking-wider text-turd-mustard-bright shadow-lg transition-colors hover:bg-turd-bronze/40"
          >
            Resume autoscroll
          </button>
        )}
      </div>
    </div>
  );
}
