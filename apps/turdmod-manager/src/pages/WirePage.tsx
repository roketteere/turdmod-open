import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { wireStore, WireEntry } from '../lib/wireStore';

const FILTER_STORAGE_KEY = 'turdmod.wire.filters';

const TYPE_COLORS: Record<string, string> = {
  'engine-event': 'var(--info)',
  'rpc-call': 'var(--warn)',
  'rpc-reply': 'var(--ok)',
  'lifecycle': 'var(--warn)',
  'error': 'var(--err)',
};

const TYPE_LABELS: Record<string, string> = {
  'engine-event': 'Engine Event',
  'rpc-call': 'RPC Call',
  'rpc-reply': 'RPC Reply',
  'lifecycle': 'Lifecycle',
  'error': 'Error',
};

function getDefaultFilters(): Record<string, boolean> {
  try {
    const stored = localStorage.getItem(FILTER_STORAGE_KEY);
    if (stored) return JSON.parse(stored);
  } catch {}
  return {
    'engine-event': true,
    'rpc-call': true,
    'rpc-reply': true,
    'lifecycle': true,
    'error': true,
  };
}

function persistFilters(filters: Record<string, boolean>) {
  localStorage.setItem(FILTER_STORAGE_KEY, JSON.stringify(filters));
}

function formatTimestamp(ts: number): string {
  const d = new Date(ts);
  return d.toLocaleTimeString('en-US', { hour12: false }) + '.' + String(d.getMilliseconds()).padStart(3, '0');
}

function stringifyPayload(payload: unknown): string {
  try {
    return JSON.stringify(payload, null, 2);
  } catch {
    return String(payload);
  }
}

const WirePage: React.FC = () => {
  const [entries, setEntries] = useState<WireEntry[]>([]);
  const [filters, setFilters] = useState<Record<string, boolean>>(getDefaultFilters);
  const [search, setSearch] = useState('');
  const [paused, setPaused] = useState(false);
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [stats, setStats] = useState(wireStore.getStats());
  const [queuedCount, setQueuedCount] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const unsub = wireStore.subscribe(() => {
      setEntries(wireStore.getEntries());
      setStats(wireStore.getStats());
      if (wireStore.isPaused()) {
        setQueuedCount((prev) => prev + 1);
      }
    });
    return unsub;
  }, []);

  const filtered = useMemo(() => {
    let result = entries;
    // Apply type filters
    const activeTypes = Object.entries(filters)
      .filter(([, v]) => v)
      .map(([k]) => k);
    if (activeTypes.length < Object.keys(filters).length) {
      result = result.filter((e) => activeTypes.includes(e.type));
    }
    // Apply search
    if (search.trim()) {
      const q = search.toLowerCase();
      result = result.filter((e) => {
        if (e.name.toLowerCase().includes(q)) return true;
        try {
          if (JSON.stringify(e.payload).toLowerCase().includes(q)) return true;
        } catch {}
        return false;
      });
    }
    return result;
  }, [entries, filters, search]);

  const toggleFilter = useCallback((type: string) => {
    setFilters((prev) => {
      const next = { ...prev, [type]: !prev[type] };
      persistFilters(next);
      return next;
    });
  }, []);

  const handlePause = useCallback(() => {
    wireStore.setPaused(!paused);
    setPaused(!paused);
    setQueuedCount(0);
  }, [paused]);

  const handleClear = useCallback(() => {
    wireStore.clear();
    setExpandedId(null);
  }, []);

  const handleExpand = useCallback((id: string) => {
    setExpandedId((prev) => (prev === id ? null : id));
  }, []);

  return (
    <div className="wire-page">
      <div className="wire-header">
        <div className="wire-stats">
          <span>Total: {stats.total}</span>
          {Object.entries(stats.byType).map(([type, count]) => (
            <span key={type} style={{ color: TYPE_COLORS[type] || 'var(--fg)' }}>
              {TYPE_LABELS[type] || type}: {count}
            </span>
          ))}
          <span>EPS: {stats.eps.toFixed(1)}</span>
        </div>
        <div className="wire-controls">
          <div className="wire-filters">
            {Object.keys(filters).map((type) => (
              <button
                key={type}
                className={`wire-filter-chip ${filters[type] ? 'active' : ''}`}
                style={{ borderColor: TYPE_COLORS[type] || 'var(--fg)' }}
                onClick={() => toggleFilter(type)}
              >
                {TYPE_LABELS[type] || type}
              </button>
            ))}
          </div>
          <input
            type="text"
            className="wire-search"
            placeholder="Search events..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
          <button className="wire-btn" onClick={handlePause}>
            {paused ? `Resume (${queuedCount} queued)` : 'Pause'}
          </button>
          <button className="wire-btn wire-btn-danger" onClick={handleClear}>
            Clear
          </button>
        </div>
      </div>
      <div className="wire-list" ref={listRef}>
        {filtered.map((entry) => (
          <div
            key={entry.id}
            className={`wire-entry ${expandedId === entry.id ? 'expanded' : ''}`}
            onClick={() => handleExpand(entry.id)}
          >
            <div className="wire-entry-header">
              <span className="wire-entry-ts">{formatTimestamp(entry.ts)}</span>
              <span className="wire-entry-type" style={{ color: TYPE_COLORS[entry.type] || 'var(--fg)' }}>
                {entry.type}
              </span>
              <span className="wire-entry-name">{entry.name}</span>
              {entry.durationMs !== undefined && (
                <span className="wire-entry-duration">{entry.durationMs}ms</span>
              )}
            </div>
            {expandedId === entry.id && (
              <pre className="wire-entry-payload">{stringifyPayload(entry.payload)}</pre>
            )}
          </div>
        ))}
      </div>
    </div>
  );
};

export default WirePage;