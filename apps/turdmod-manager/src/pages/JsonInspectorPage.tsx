import { useMemo, useState } from 'react';

// JSON Inspector — paste a JSON blob (typically dumpAdminCommands /
// dumpClasses / any bridge RPC output), explore it as a collapsible
// tree, search keys/values, copy any sub-value. No dependencies; the
// tree renderer is a small recursive component below.
//
// Server-config JSON editing is intentionally NOT here — that lives on
// ServerFilesPage (generic) and NotificationsEditorPage (specialized).
// This page is for read-only inspection of arbitrary JSON.

const STORAGE_KEY = 'turdmod.jsonInspector.v1';

interface StoredState {
  source: string;
  searchTerm: string;
}

function loadState(): StoredState {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return { source: '', searchTerm: '' };
    const parsed = JSON.parse(raw) as Partial<StoredState>;
    return {
      source: typeof parsed.source === 'string' ? parsed.source : '',
      searchTerm: typeof parsed.searchTerm === 'string' ? parsed.searchTerm : '',
    };
  } catch {
    return { source: '', searchTerm: '' };
  }
}

function saveState(state: StoredState) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
  } catch {
    // ignore quota errors
  }
}

export function JsonInspectorPage() {
  const initial = useMemo(loadState, []);
  const [source, setSource] = useState(initial.source);
  const [searchTerm, setSearchTerm] = useState(initial.searchTerm);
  const [copyHint, setCopyHint] = useState<string | null>(null);

  const parsed = useMemo<{ ok: true; value: unknown } | { ok: false; error: string }>(() => {
    const trimmed = source.trim();
    if (!trimmed) return { ok: true, value: null };
    try {
      return { ok: true, value: JSON.parse(trimmed) as unknown };
    } catch (err) {
      return { ok: false, error: (err as Error).message };
    }
  }, [source]);

  const handleSourceChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
    const v = e.target.value;
    setSource(v);
    saveState({ source: v, searchTerm });
  };

  const handleSearchChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const v = e.target.value;
    setSearchTerm(v);
    saveState({ source, searchTerm: v });
  };

  const copyText = (text: string, label: string) => {
    void navigator.clipboard.writeText(text).catch(() => {});
    setCopyHint(`copied ${label}`);
    window.setTimeout(() => setCopyHint(null), 1200);
  };

  const stats = useMemo(() => {
    if (!parsed.ok) return null;
    const trimmed = source.trim();
    if (!trimmed) return null;
    let nodes = 0;
    const walk = (v: unknown) => {
      nodes++;
      if (Array.isArray(v)) {
        for (const item of v) walk(item);
      } else if (v && typeof v === 'object') {
        for (const k of Object.keys(v as Record<string, unknown>)) {
          walk((v as Record<string, unknown>)[k]);
        }
      }
    };
    walk(parsed.value);
    return { bytes: trimmed.length, nodes };
  }, [parsed, source]);

  return (
    <div className="flex h-full flex-col gap-3">
      <header>
        <p className="font-display text-xs tracking-[0.3em] text-turd-mustard">
          TurdMOD
        </p>
        <h1 className="mt-1 font-display text-3xl text-turd-cream">
          JSON Inspector
        </h1>
        <p className="mt-1 text-xs text-turd-cream-dim">
          Paste any JSON (bridge RPC output, config files, etc.) and explore
          it as a collapsible tree. Click any value to copy. State persists
          across Manager restarts.
        </p>
      </header>

      <div className="grid min-h-0 flex-1 grid-cols-[1fr_1fr] gap-3 overflow-hidden">
        {/* Left: raw JSON input */}
        <section className="flex min-h-0 flex-col overflow-hidden glass rounded-xl">
          <div className="flex items-center justify-between border-b border-turd-bronze/30 px-4 py-2">
            <h2 className="font-display text-sm uppercase tracking-widest text-turd-mustard">
              Source
            </h2>
            <div className="flex items-center gap-3 text-[10px] text-turd-cream-dim">
              {stats && <span>{stats.bytes} bytes · {stats.nodes} nodes</span>}
              <button
                type="button"
                onClick={() => {
                  setSource('');
                  saveState({ source: '', searchTerm });
                }}
                disabled={!source}
                className="text-turd-cream-dim hover:text-turd-cream disabled:opacity-30"
              >
                Clear
              </button>
            </div>
          </div>
          <textarea
            value={source}
            onChange={handleSourceChange}
            spellCheck={false}
            placeholder='{ "paste": "any JSON here" }'
            className={`flex-1 resize-none bg-turd-bg-deep/60 px-3 py-2 font-mono text-[11px] text-turd-cream outline-none placeholder:text-turd-cream-dim/40 ${
              !parsed.ok ? 'border-l-2 border-red-500/60' : ''
            }`}
          />
          {!parsed.ok && (
            <p className="border-t border-red-500/30 bg-red-500/10 px-3 py-1.5 font-mono text-[10px] text-red-300">
              {parsed.error}
            </p>
          )}
        </section>

        {/* Right: tree view */}
        <section className="flex min-h-0 flex-col overflow-hidden glass rounded-xl">
          <div className="flex items-center justify-between border-b border-turd-bronze/30 px-4 py-2">
            <h2 className="font-display text-sm uppercase tracking-widest text-turd-mustard">
              Tree
            </h2>
            <input
              type="text"
              value={searchTerm}
              onChange={handleSearchChange}
              placeholder="Highlight key/value…"
              className="w-48 rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-2 py-0.5 font-mono text-[11px] text-turd-cream placeholder:text-turd-cream-dim/40 focus:border-turd-mustard-bright focus:outline-none"
            />
          </div>
          <div className="flex-1 overflow-auto p-3">
            {!parsed.ok ? (
              <p className="text-xs text-turd-cream-dim">
                Fix the JSON parse error on the left to see the tree.
              </p>
            ) : source.trim() === '' ? (
              <p className="text-xs text-turd-cream-dim">
                Paste JSON on the left to start exploring.
              </p>
            ) : (
              <TreeNode
                value={parsed.value}
                path="$"
                depth={0}
                searchTerm={searchTerm.toLowerCase()}
                onCopy={copyText}
              />
            )}
          </div>
          {copyHint && (
            <div className="border-t border-turd-bronze/30 bg-turd-bg-deep/80 px-4 py-1 font-mono text-[10px] text-turd-mustard-bright">
              {copyHint}
            </div>
          )}
        </section>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// TreeNode — recursive collapsible viewer
// ---------------------------------------------------------------------------

interface TreeNodeProps {
  value: unknown;
  path: string;
  depth: number;
  searchTerm: string;
  keyLabel?: string;
  onCopy: (text: string, label: string) => void;
}

function isMatch(text: string, term: string): boolean {
  if (!term) return false;
  return text.toLowerCase().includes(term);
}

function TreeNode({ value, path, depth, searchTerm, keyLabel, onCopy }: TreeNodeProps) {
  // Auto-expand top two levels by default — most exploration starts at the
  // outermost keys, deeper nesting is opt-in.
  const [open, setOpen] = useState(depth < 2);

  const keyHit = keyLabel ? isMatch(keyLabel, searchTerm) : false;

  if (value === null) {
    return (
      <LeafLine
        keyLabel={keyLabel}
        valueText="null"
        valueClass="text-turd-cream-dim/70"
        path={path}
        searchTerm={searchTerm}
        keyHit={keyHit}
        onCopy={onCopy}
      />
    );
  }

  if (typeof value === 'boolean') {
    return (
      <LeafLine
        keyLabel={keyLabel}
        valueText={value ? 'true' : 'false'}
        valueClass={value ? 'text-green-400' : 'text-red-400'}
        path={path}
        searchTerm={searchTerm}
        keyHit={keyHit}
        onCopy={onCopy}
      />
    );
  }

  if (typeof value === 'number') {
    return (
      <LeafLine
        keyLabel={keyLabel}
        valueText={String(value)}
        valueClass="text-turd-mustard"
        path={path}
        searchTerm={searchTerm}
        keyHit={keyHit}
        onCopy={onCopy}
      />
    );
  }

  if (typeof value === 'string') {
    return (
      <LeafLine
        keyLabel={keyLabel}
        valueText={`"${value}"`}
        valueClass="text-turd-cream"
        path={path}
        searchTerm={searchTerm}
        keyHit={keyHit}
        onCopy={onCopy}
      />
    );
  }

  if (Array.isArray(value)) {
    const summary = `Array(${value.length})`;
    return (
      <div>
        <button
          type="button"
          onClick={() => setOpen((v) => !v)}
          className={`flex w-full items-baseline gap-1 text-left font-mono text-[11px] hover:bg-white/[0.03] ${
            keyHit ? 'bg-turd-mustard/15' : ''
          }`}
        >
          <span className="w-3 text-turd-cream-dim">{open ? '▾' : '▸'}</span>
          {keyLabel && (
            <span className="text-turd-mustard-bright">{keyLabel}:</span>
          )}
          <span className="text-turd-cream-dim">{summary}</span>
        </button>
        {open && (
          <div className="ml-3 border-l border-turd-bronze/20 pl-2">
            {value.map((item, i) => (
              <TreeNode
                key={i}
                value={item}
                path={`${path}[${i}]`}
                depth={depth + 1}
                searchTerm={searchTerm}
                keyLabel={String(i)}
                onCopy={onCopy}
              />
            ))}
          </div>
        )}
      </div>
    );
  }

  if (typeof value === 'object') {
    const obj = value as Record<string, unknown>;
    const keys = Object.keys(obj);
    const summary = `{${keys.length} key${keys.length !== 1 ? 's' : ''}}`;
    return (
      <div>
        <button
          type="button"
          onClick={() => setOpen((v) => !v)}
          className={`flex w-full items-baseline gap-1 text-left font-mono text-[11px] hover:bg-white/[0.03] ${
            keyHit ? 'bg-turd-mustard/15' : ''
          }`}
        >
          <span className="w-3 text-turd-cream-dim">{open ? '▾' : '▸'}</span>
          {keyLabel && (
            <span className="text-turd-mustard-bright">{keyLabel}:</span>
          )}
          <span className="text-turd-cream-dim">{summary}</span>
        </button>
        {open && (
          <div className="ml-3 border-l border-turd-bronze/20 pl-2">
            {keys.map((k) => (
              <TreeNode
                key={k}
                value={obj[k]}
                path={`${path}.${k}`}
                depth={depth + 1}
                searchTerm={searchTerm}
                keyLabel={k}
                onCopy={onCopy}
              />
            ))}
          </div>
        )}
      </div>
    );
  }

  return null;
}

interface LeafLineProps {
  keyLabel?: string;
  valueText: string;
  valueClass: string;
  path: string;
  searchTerm: string;
  keyHit: boolean;
  onCopy: (text: string, label: string) => void;
}

function LeafLine({ keyLabel, valueText, valueClass, path, searchTerm, keyHit, onCopy }: LeafLineProps) {
  const valueHit = isMatch(valueText, searchTerm);
  const isHit = keyHit || valueHit;
  return (
    <div
      className={`group flex items-baseline gap-1 font-mono text-[11px] ${
        isHit ? 'bg-turd-mustard/15' : ''
      }`}
    >
      <span className="w-3" />
      {keyLabel && (
        <span className="text-turd-mustard-bright">{keyLabel}:</span>
      )}
      <button
        type="button"
        onClick={() => onCopy(valueText.replace(/^"|"$/g, ''), keyLabel ?? path)}
        className={`truncate text-left hover:underline ${valueClass}`}
        title={`Click to copy · ${path}`}
      >
        {valueText}
      </button>
    </div>
  );
}
