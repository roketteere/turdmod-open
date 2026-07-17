import { useEffect, useMemo, useRef, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { motion, AnimatePresence } from 'framer-motion';
import { Reveal } from '../../lib/motion';
import { useAdminCommandsMetadata, type BridgeArg } from '../../hooks/useAdminCommandsMetadata';
import { loadIconEntries, type IconEntry } from '../../lib/item-icons';
import {
  classifyInput,
  dispatchAdmin,
  dispatchBroadcast,
  dispatchMod,
  dumpCommandCatalog,
  fetchRoster,
  type AdminResult,
} from '../../lib/admin-dispatch';
import ADMIN_CATALOG from '../../data/admin-commands.json';

// Verb pool for autocomplete = static catalog ∪ live bridge dump ∪ verbs
// the admin has actually fired. The bridge dump only covers AdminCommand_*
// BP classes; the static catalog (231 verbs) and the learned set fill the
// gaps (e.g. SetWeather, which lives in neither dump nor the bare catalog).
const STATIC_VERBS: string[] = Object.values(ADMIN_CATALOG as Record<string, string[]>).flat();
const LEARNED_KEY = 'turdmod.adminConsole.learnedVerbs.v1';
// v2 — v1 stored capture-buffer noise from the abandoned help-probe approach.
const SYNCED_KEY = 'turdmod.adminConsole.syncedVerbs.v2';

function loadVerbList(key: string): string[] {
  try {
    return JSON.parse(localStorage.getItem(key) ?? '[]') as string[];
  } catch {
    return [];
  }
}

// Unified command console — the single input surface replacing TurdCOM
// Terminal, Chat Console, and the Admin Commands dispatcher. One smart bar:
//   #verb  -> SCUM admin command (Force toggle = zero-admin gate bypass)
//   !verb  -> TurdMOD mod command
//   text   -> server broadcast (channel selector)
// Autosuggest is arg-aware: Player args fill from the live roster, enum args
// from the bridge's completionValues, Item args from the icon catalog.

interface Entry {
  id: string;
  ts: number;
  mode: 'admin' | 'mod' | 'broadcast';
  display: string;   // what the user typed (with sigil)
  result: string;
  isError: boolean;
}

const STORAGE_KEY = 'turdmod.adminConsole.history.v1';
const MAX_HISTORY = 200;
const ROSTER_MS = 2000;

const CHAT_TYPES = [
  { value: '0', label: 'Local' },
  { value: '1', label: 'Squad' },
  { value: '2', label: 'Global' },
  { value: '3', label: 'Admin' },
];

function loadHistory(): Entry[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return raw ? (JSON.parse(raw) as Entry[]).slice(-MAX_HISTORY) : [];
  } catch {
    return [];
  }
}
function saveHistory(entries: Entry[]) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(entries.slice(-MAX_HISTORY)));
  } catch {
    /* localStorage full/disabled — skip */
  }
}

// True when an arg expects a player name (auto-fill from roster).
function isPlayerArg(arg: BridgeArg | undefined): boolean {
  return !!arg && /player/i.test(arg.completionClass ?? '');
}
// True when an arg expects an item code (offer icon picker).
function isItemArg(arg: BridgeArg | undefined): boolean {
  return !!arg && /item|weapon|cloth|food|resource/i.test(arg.completionClass ?? '');
}

export function CommandConsole() {
  const [input, setInput] = useState('');
  const [force, setForce] = useState(true);
  const [chatType, setChatType] = useState('2');
  const [history, setHistory] = useState<Entry[]>(loadHistory);
  const [busy, setBusy] = useState(false);
  const [histCursor, setHistCursor] = useState<number | null>(null);
  const [verbPickIdx, setVerbPickIdx] = useState(0);
  const [learned, setLearned] = useState<string[]>(() => loadVerbList(LEARNED_KEY));
  const [synced, setSynced] = useState<string[]>(() => loadVerbList(SYNCED_KEY));
  const [syncing, setSyncing] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const endRef = useRef<HTMLDivElement>(null);

  const meta = useAdminCommandsMetadata();

  // De-duped union of every verb source, case-insensitive (prefer the
  // bridge/static casing). Drives autocomplete so any known OR previously
  // fired verb surfaces. Synced (live in-game dump) is the most authoritative.
  const verbPool = useMemo(() => {
    const byLower = new Map<string, string>();
    for (const v of STATIC_VERBS) byLower.set(v.toLowerCase(), v);
    for (const v of meta.byVerb.keys()) byLower.set(v.toLowerCase(), v);
    for (const v of learned) byLower.set(v.toLowerCase(), v);
    for (const v of synced) byLower.set(v.toLowerCase(), v);
    return Array.from(byLower.values());
  }, [meta.byVerb, learned, synced]);
  const roster = useQuery({ queryKey: ['engine', 'getOnlinePlayers'], queryFn: fetchRoster, refetchInterval: ROSTER_MS, retry: false });
  const players = useMemo(() => (roster.data?.players ?? []).map((p) => p.name).filter(Boolean), [roster.data]);
  const items = useQuery({ queryKey: ['iconEntries'], queryFn: loadIconEntries, staleTime: Infinity });

  const cls = useMemo(() => classifyInput(input), [input]);
  const trailingSpace = /\s$/.test(input);

  // Verb autocomplete (admin mode, still typing the verb).
  const verbMatches = useMemo(() => {
    if (cls.mode !== 'admin' || trailingSpace || cls.args.length > 0) return [];
    const q = cls.verb.toLowerCase();
    if (!q) return [];
    // Prefix matches first, then substring matches, so "weather" surfaces
    // SetWeather* even though the verb doesn't start with the query.
    const starts = verbPool.filter((v) => v.toLowerCase().startsWith(q));
    const contains = verbPool.filter((v) => !v.toLowerCase().startsWith(q) && v.toLowerCase().includes(q));
    const m = [...starts.sort(), ...contains.sort()].slice(0, 8);
    return m.length === 1 && m[0] === cls.verb ? [] : m;
  }, [cls, trailingSpace, verbPool]);

  const matched = cls.mode === 'admin' ? meta.byVerb.get(cls.verb) : undefined;
  // Retain the last matched spec so the hint pane can animate OUT with its
  // content intact instead of blanking before it finishes collapsing.
  const [hintCmd, setHintCmd] = useState(matched);
  useEffect(() => { if (matched) setHintCmd(matched); }, [matched]);

  // Which arg is the cursor currently on? -1 == still editing the verb.
  const argIdx = cls.args.length === 0 ? (trailingSpace ? 0 : -1) : trailingSpace ? cls.args.length : cls.args.length - 1;
  const currentArg = matched && argIdx >= 0 ? matched.args[argIdx] : undefined;
  const currentToken = argIdx >= 0 ? cls.args[argIdx] ?? '' : '';

  // Item picker matches for the current item-arg token.
  const itemMatches = useMemo<IconEntry[]>(() => {
    if (!isItemArg(currentArg) || !items.data) return [];
    const q = currentToken.toLowerCase().replace(/[^a-z0-9]/g, '');
    const pool = items.data;
    if (!q) return pool.slice(0, 18);
    return pool.filter((e) => e.code.toLowerCase().replace(/[^a-z0-9]/g, '').includes(q)).slice(0, 18);
  }, [currentArg, currentToken, items.data]);

  useEffect(() => { setVerbPickIdx(0); }, [cls.verb]);
  useEffect(() => { endRef.current?.scrollIntoView({ behavior: 'smooth' }); }, [history]);

  // Replace the token at `argIdx` (or append a new one) and refocus.
  const fillToken = (value: string) => {
    const sigil = cls.mode === 'admin' ? '#' : cls.mode === 'mod' ? '!' : '';
    const parts = [cls.verb, ...cls.args];
    const target = argIdx < 0 ? 0 : argIdx;
    parts[target] = value;
    setInput(`${sigil}${parts.join(' ')} `);
    inputRef.current?.focus();
  };

  const acceptVerb = (verb: string) => {
    setInput(`#${verb} `);
    inputRef.current?.focus();
  };

  const push = (e: Entry) => {
    setHistory((h) => {
      const next = [...h, e].slice(-MAX_HISTORY);
      saveHistory(next);
      return next;
    });
  };

  const send = async () => {
    const raw = input.trim();
    if (!raw || busy) return;
    setBusy(true);
    setHistCursor(null);
    const c = classifyInput(raw);
    let res: AdminResult;
    if (c.mode === 'admin') res = await dispatchAdmin({ command: c.text, force, executor: players[0] ?? null });
    else if (c.mode === 'mod') res = await dispatchMod({ command: c.text });
    else res = await dispatchBroadcast({ text: c.text, chatType });

    // Learn any admin verb that fired without error and isn't already known,
    // so catalog gaps (e.g. SetWeather) self-heal after first successful use.
    if (c.mode === 'admin' && !res.isError && c.verb && !verbPool.some((v) => v.toLowerCase() === c.verb.toLowerCase())) {
      const next = [...learned, c.verb];
      setLearned(next);
      try { localStorage.setItem(LEARNED_KEY, JSON.stringify(next)); } catch { /* skip */ }
    }
    push({
      id: `${Date.now()}-${history.length}`,
      ts: Date.now(),
      mode: c.mode,
      display: c.mode === 'broadcast' ? raw : `${c.mode === 'admin' ? '#' : '!'}${c.text}`,
      result: res.text,
      isError: res.isError,
    });
    if (!res.isError) setInput('');
    setBusy(false);
  };

  // Refresh the authoritative catalog from the live game's dumpAdminCommands
  // (231-verb BP walk). Player-less; re-run after a SCUM update. Natives like
  // SetWeather aren't in any dump — learn-on-use covers those.
  const runSync = async () => {
    if (syncing) return;
    setSyncing(true);
    const res = await dumpCommandCatalog();
    if (res.verbs.length > 0) {
      setSynced(res.verbs);
      try { localStorage.setItem(SYNCED_KEY, JSON.stringify(res.verbs)); } catch { /* skip */ }
    }
    push({
      id: `${Date.now()}-sync`,
      ts: Date.now(),
      mode: 'admin',
      display: '⟳ refresh catalog',
      result: res.verbs.length > 0
        ? `refreshed ${res.verbs.length} commands from dumpAdminCommands`
        : 'dump failed — is the engine reachable?',
      isError: res.verbs.length === 0,
    });
    setSyncing(false);
  };

  const onKey = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (verbMatches.length > 0) {
      if (e.key === 'ArrowDown') { e.preventDefault(); setVerbPickIdx((i) => Math.min(i + 1, verbMatches.length - 1)); return; }
      if (e.key === 'ArrowUp') { e.preventDefault(); setVerbPickIdx((i) => Math.max(i - 1, 0)); return; }
      if (e.key === 'Tab') { e.preventDefault(); acceptVerb(verbMatches[verbPickIdx]); return; }
    }
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); void send(); return; }
    if (verbMatches.length === 0 && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) {
      e.preventDefault();
      if (history.length === 0) return;
      const cur = histCursor === null ? history.length : histCursor;
      const nxt = e.key === 'ArrowUp' ? Math.max(0, cur - 1) : Math.min(history.length, cur + 1);
      setHistCursor(nxt);
      setInput(nxt >= history.length ? '' : history[nxt].display);
    }
  };

  const modeBadge = cls.mode === 'admin' ? '#' : cls.mode === 'mod' ? '!' : '»';
  const modeColor =
    cls.mode === 'admin' ? 'text-turd-mustard-bright' : cls.mode === 'mod' ? 'text-turd-green' : 'text-turd-cream-dim';

  return (
    <div className="flex h-full min-h-0 flex-col gap-3">
      {/* Input row */}
      <div className="glass flex flex-col gap-2 rounded-xl p-4">
        <div className="flex items-center gap-2">
          <span className={`font-mono text-sm ${modeColor}`}>{modeBadge}</span>
          <div className="relative flex flex-1 items-center rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-2 focus-within:border-turd-mustard">
            <input
              ref={inputRef}
              value={input}
              onChange={(e) => { setInput(e.target.value); setHistCursor(null); }}
              onKeyDown={onKey}
              placeholder="#SetGodMode Lilac 1   ·   !kit   ·   plain text = broadcast"
              className="flex-1 bg-transparent font-mono text-sm text-turd-cream outline-none placeholder:text-turd-cream-dim/50"
              autoComplete="off"
              spellCheck={false}
            />
            {/* Verb autocomplete */}
            <AnimatePresence>
            {verbMatches.length > 0 && (
              <motion.div
                initial={{ opacity: 0, y: -4, scale: 0.98 }}
                animate={{ opacity: 1, y: 0, scale: 1 }}
                exit={{ opacity: 0, y: -4, scale: 0.98 }}
                transition={{ duration: 0.14, ease: [0.22, 1, 0.36, 1] }}
                className="glass absolute left-0 right-0 top-full z-50 mt-1.5 max-h-56 overflow-auto rounded-lg shadow-elev-3"
              >
                {verbMatches.map((v, idx) => {
                  const m = meta.byVerb.get(v);
                  return (
                    <button
                      key={v}
                      type="button"
                      onMouseDown={(e) => { e.preventDefault(); acceptVerb(v); }}
                      className={`flex w-full flex-col items-start px-3 py-1.5 text-left ${idx === verbPickIdx ? 'bg-turd-mustard/20 text-turd-mustard-bright' : 'text-turd-cream hover:bg-turd-bg-soft'}`}
                    >
                      <span className="font-mono text-xs">{v}</span>
                      {m?.description && <span className="line-clamp-1 text-[10px] text-turd-cream-dim/80">{m.description}</span>}
                    </button>
                  );
                })}
              </motion.div>
            )}
            </AnimatePresence>
          </div>

          {cls.mode === 'broadcast' && (
            <select
              value={chatType}
              onChange={(e) => setChatType(e.target.value)}
              className="rounded border border-turd-bronze/60 bg-turd-bg-soft px-2 py-2 font-mono text-xs text-turd-cream focus:border-turd-mustard focus:outline-none"
            >
              {CHAT_TYPES.map((c) => <option key={c.value} value={c.value}>{c.label}</option>)}
            </select>
          )}

          {cls.mode === 'admin' && (
            <label
              title="Force: flip the CanExecute auth gate so the command runs with ZERO admins online. Needs ≥1 player on as the executor channel."
              className={`flex cursor-pointer select-none items-center gap-1.5 rounded border px-2.5 py-2 ${force ? 'border-turd-mustard/60 bg-turd-mustard/15 text-turd-mustard-bright' : 'border-turd-bronze/40 text-turd-cream-dim'}`}
            >
              <input type="checkbox" checked={force} onChange={(e) => setForce(e.target.checked)} className="h-3 w-3 accent-turd-mustard" />
              <span className="font-display text-[10px] uppercase tracking-wider">Force</span>
            </label>
          )}

          <button
            type="button"
            onClick={() => void send()}
            disabled={busy || !input.trim()}
            className="rounded border border-turd-mustard/60 bg-turd-mustard/20 px-4 py-2 font-display text-xs uppercase tracking-wider text-turd-mustard-bright hover:bg-turd-mustard/30 disabled:cursor-not-allowed disabled:opacity-30"
          >
            {busy ? 'Sending…' : 'Send'}
          </button>
        </div>

        {/* Force needs a player online */}
        <Reveal show={cls.mode === 'admin' && force && players.length === 0}>
          <p className="pt-1 font-mono text-[10px] text-turd-red">Force needs ≥1 player online (executor channel). None connected — uncheck Force or wait for a join.</p>
        </Reveal>

        {/* Arg-aware hint + quick-fill */}
        <Reveal show={!!matched}>
          {hintCmd && (
          <motion.div layout className="glass-soft mt-0.5 rounded-lg px-3 py-2.5">
            <div className="flex items-baseline gap-2">
              <code className="font-mono text-xs text-turd-mustard-bright">#{hintCmd.verb}</code>
              <span className="text-[10px] text-turd-cream-dim/70">{hintCmd.numRequired} required · {Math.max(0, hintCmd.args.length - hintCmd.numRequired)} optional</span>
            </div>
            {hintCmd.description && <p className="mt-1 text-[11px] text-turd-cream-dim">{hintCmd.description}</p>}
            {hintCmd.args.length > 0 && (
              <div className="mt-2 flex flex-wrap gap-1.5">
                {hintCmd.args.map((arg, i) => (
                  <span key={i} className={`rounded px-1.5 py-0.5 font-mono text-[10px] transition-colors ${i === argIdx ? 'bg-turd-mustard/25 text-turd-mustard-bright' : i < hintCmd.numRequired ? 'text-turd-mustard/80' : 'text-turd-cream-dim'}`}>
                    {i < hintCmd.numRequired ? '<' : '['}{arg.name || `arg${i + 1}`}{i < hintCmd.numRequired ? '>' : ']'}
                  </span>
                ))}
              </div>
            )}
            {currentArg?.description && <p className="mt-1.5 text-[10px] text-turd-cream-dim/80">{currentArg.description}</p>}

            {/* Player quick-fill */}
            {isPlayerArg(currentArg) && players.length > 0 && (
              <div className="mt-2 flex flex-wrap gap-1.5">
                {players.map((p) => (
                  <button key={p} type="button" onClick={() => fillToken(p)} className="rounded border border-turd-bronze/40 bg-turd-bg-soft px-2 py-0.5 font-mono text-[10px] text-turd-cream hover:border-turd-mustard hover:text-turd-mustard-bright">{p}</button>
                ))}
              </div>
            )}

            {/* Enum quick-fill */}
            {currentArg && currentArg.completionValues.length > 0 && (
              <div className="mt-2 flex flex-wrap gap-1.5">
                {currentArg.completionValues.slice(0, 24).map((v) => (
                  <button key={v} type="button" onClick={() => fillToken(v)} className="rounded border border-turd-bronze/40 bg-turd-bg-soft px-2 py-0.5 font-mono text-[10px] text-turd-cream hover:border-turd-mustard hover:text-turd-mustard-bright">{v}</button>
                ))}
              </div>
            )}

            {/* Item icon picker */}
            {isItemArg(currentArg) && itemMatches.length > 0 && (
              <div className="mt-2 grid grid-cols-6 gap-1.5">
                {itemMatches.map((it) => (
                  <button key={it.code} type="button" onClick={() => fillToken(it.code)} title={it.code} className="flex flex-col items-center gap-0.5 rounded border border-turd-bronze/30 bg-turd-bg-soft p-1 hover:border-turd-mustard">
                    <img src={it.url} alt="" className="h-7 w-7 object-contain" loading="lazy" />
                    <span className="line-clamp-1 w-full text-center font-mono text-[8px] text-turd-cream-dim">{it.code}</span>
                  </button>
                ))}
              </div>
            )}
          </motion.div>
          )}
        </Reveal>
      </div>

      {/* History */}
      <section className="glass flex min-h-0 flex-1 flex-col overflow-hidden rounded-xl">
        <div className="flex items-center justify-between border-b border-turd-bronze/30 px-4 py-2">
          <h2 className="font-display text-sm uppercase tracking-widest text-turd-mustard">History · {history.length}</h2>
          <div className="flex items-center gap-3">
            <span className="font-mono text-[10px] text-turd-cream-dim/70" title="Total verbs in the autocomplete pool (static catalog + bridge dump + learned + synced)">{verbPool.length} verbs{synced.length > 0 ? ` · ${synced.length} synced` : ''}</span>
            <button
              type="button"
              onClick={() => void runSync()}
              disabled={syncing}
              title="Refresh the command catalog from the live game (dumpAdminCommands, ~231 verbs). Re-run after each SCUM update. Native commands like SetWeather are added automatically as you use them."
              className="rounded border border-turd-mustard/50 bg-turd-mustard/10 px-2 py-0.5 font-display text-[10px] uppercase tracking-wider text-turd-mustard-bright hover:bg-turd-mustard/20 disabled:opacity-40"
            >
              {syncing ? 'Refreshing…' : '⟳ Refresh catalog'}
            </button>
            <button type="button" onClick={() => { setHistory([]); saveHistory([]); }} disabled={history.length === 0} className="text-[11px] text-turd-cream-dim hover:text-turd-cream disabled:opacity-30">Clear</button>
          </div>
        </div>
        {history.length === 0 ? (
          <div className="flex flex-1 items-center justify-center p-6"><p className="text-xs text-turd-cream-dim">No commands fired yet. Type <code className="text-turd-mustard">#</code> for admin, <code className="text-turd-green">!</code> for mod, or plain text to broadcast.</p></div>
        ) : (
          <div className="flex-1 overflow-auto p-3">
            <div className="space-y-1.5">
              {history.map((e) => (
                <div key={e.id} className="flex flex-col gap-0.5 rounded border border-turd-bronze/20 bg-turd-bg-deep/60 p-2">
                  <div className="flex items-baseline gap-2">
                    <span className="font-mono text-[10px] text-turd-cream-dim/60">{new Date(e.ts).toLocaleTimeString('en-US', { hour12: false })}</span>
                    <span className={`font-mono text-[9px] uppercase ${e.mode === 'admin' ? 'text-turd-mustard' : e.mode === 'mod' ? 'text-turd-green' : 'text-turd-cream-dim'}`}>{e.mode}</span>
                    <code className="truncate font-mono text-xs text-turd-cream">{e.display}</code>
                  </div>
                  <p className={`font-mono text-[10px] ${e.isError ? 'text-turd-red' : 'text-turd-cream-dim'}`}>{e.result}</p>
                </div>
              ))}
              <div ref={endRef} />
            </div>
          </div>
        )}
      </section>
    </div>
  );
}
