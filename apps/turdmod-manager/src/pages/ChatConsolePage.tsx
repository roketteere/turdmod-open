import { useEffect, useMemo, useRef, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { engineRpc } from '../lib/tauri-engine';
import { useAdminCommandsMetadata } from '../hooks/useAdminCommandsMetadata';
import TURD_COMMANDS from '../data/turd-commands.json';

// ─── Chat Console ───────────────────────────────────────────────────────────
//
// One unified in-game chat-line bar. Type:
//   #verb …  → SCUM native admin command (fires via runAdminCommand — the REAL
//              admin path that echoes; or runTestAdminCommand to bypass the
//              tier gate). Captures the server's reply via getAdminOutput.
//   !verb …  → TurdMOD chat command (sent into chat on the chosen channel).
//   plain    → a chat message (broadcast to all, or whispered to one player).
//
// Autosuggest fires on `#`/`!`; Tab accepts; ↑/↓ navigates suggestions (or
// recalls history when the list is closed); Enter sends. Channel + Target are
// dropdowns. This is the proof harness: fire from here, watch in-game react.
//
// @dep engineRpc → bridge handlers: runAdminCommand / runTestAdminCommand /
//      getAdminOutput / broadcastChat / sendChatLineToPlayer / getOnlinePlayers
// @inv channels: 0=Local 1=Squad 2=Global 3=Admin (EChatType, bridge-confirmed)

interface PlayerEntry { name: string; steamId?: string }
interface RosterResp { count?: number; players?: PlayerEntry[] }

type Mode = 'admin' | 'turd' | 'chat';

const CHANNELS: { value: number; label: string }[] = [
  { value: 0, label: 'Local' },
  { value: 1, label: 'Squad' },
  { value: 2, label: 'Global' },
  { value: 3, label: 'Admin' },
];

interface LogEntry {
  id: string;
  ts: Date;
  mode: Mode;
  line: string;       // what was fired
  via: string;        // which bridge path
  response: string;   // server echo / status
  isError: boolean;
}

function modeOf(text: string): Mode {
  const t = text.trimStart();
  if (t.startsWith('#')) return 'admin';
  if (t.startsWith('!')) return 'turd';
  return 'chat';
}

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

export function ChatConsolePage() {
  const [input, setInput] = useState('');
  const [channel, setChannel] = useState<number>(0);
  const [target, setTarget] = useState<string>('All');
  const [bypass, setBypass] = useState(false);
  const [busy, setBusy] = useState(false);
  const [log, setLog] = useState<LogEntry[]>([]);

  // autosuggest
  const [sugOpen, setSugOpen] = useState(false);
  const [sugIdx, setSugIdx] = useState(0);
  // history recall
  const [histIdx, setHistIdx] = useState<number | null>(null);

  const inputRef = useRef<HTMLInputElement>(null);
  const logEndRef = useRef<HTMLDivElement>(null);

  const adminMeta = useAdminCommandsMetadata();
  const turdVerbs = TURD_COMMANDS as string[];

  const roster = useQuery({
    queryKey: ['chat-console', 'roster'],
    queryFn: () => engineRpc<RosterResp>('getOnlinePlayers', {}),
    refetchInterval: 5000,
    retry: false,
  });
  const players = useMemo(
    () => (roster.data?.players ?? []).map((p) => p.name).filter(Boolean),
    [roster.data],
  );

  const mode = modeOf(input);

  // The verb token currently being typed (after the sigil, before a space).
  const { verb, pastVerb } = useMemo(() => {
    const t = input.trimStart();
    const body = mode === 'chat' ? t : t.slice(1);
    const sp = /\s/.test(body);
    return { verb: body.split(/\s/)[0] ?? '', pastVerb: sp };
  }, [input, mode]);

  // Suggestions for the current mode + typed prefix.
  const suggestions = useMemo(() => {
    if (pastVerb || mode === 'chat' || !verb) return [];
    const q = verb.toLowerCase();
    const pool =
      mode === 'admin' ? Array.from(adminMeta.byVerb.keys()) : turdVerbs;
    return pool
      .filter((v) => v.toLowerCase().startsWith(q))
      .sort((a, b) => a.localeCompare(b))
      .slice(0, 10);
  }, [verb, pastVerb, mode, adminMeta.byVerb, turdVerbs]);

  useEffect(() => {
    setSugIdx(0);
    setSugOpen(suggestions.length > 0 && suggestions[0] !== verb);
  }, [suggestions, verb]);

  const matchedAdmin = useMemo(
    () => (mode === 'admin' ? adminMeta.byVerb.get(verb) : undefined),
    [mode, verb, adminMeta.byVerb],
  );

  useEffect(() => {
    logEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [log]);

  const acceptSuggestion = (v: string) => {
    const sigil = mode === 'admin' ? '#' : '!';
    setInput(`${sigil}${v} `);
    setSugOpen(false);
    inputRef.current?.focus();
  };

  const pushLog = (e: Omit<LogEntry, 'id' | 'ts'>) => {
    setLog((prev) =>
      [
        ...prev,
        { ...e, id: `${Date.now()}-${Math.random().toString(36).slice(2, 7)}`, ts: new Date() },
      ].slice(-200),
    );
  };

  const send = async () => {
    const text = input.trim();
    if (!text || busy) return;
    setBusy(true);
    setSugOpen(false);
    setHistIdx(null);
    const m = modeOf(text);
    try {
      if (m === 'admin') {
        const method = bypass ? 'runTestAdminCommand' : 'runAdminCommand';
        // @inv (live-confirmed 2026-06-10): ProcessAdminCommand wants the BARE
        // verb. A leading '#' makes SCUM reply "Unrecognized command." — the
        // in-game chat parser strips '#' before this entry point, so we must too.
        const cmd = text.replace(/^#+\s*/, '');
        const params: Record<string, unknown> = { command: cmd };
        if (target !== 'All') params.playerName = target;
        // clear any stale echo, fire, then read what SCUM said back
        await engineRpc('getAdminOutput', {}).catch(() => {});
        const resp = await engineRpc<{ ok?: boolean; error?: string }>(method, params);
        let echo = '';
        if (!bypass) {
          await sleep(600);
          const out = await engineRpc<{ lines?: string[] }>('getAdminOutput', {});
          echo = (out.lines ?? []).join('  •  ').replace(/\?/g, '  ·  ');
        }
        pushLog({
          mode: m,
          line: text,
          via: method + (target !== 'All' ? ` (as ${target})` : ''),
          response:
            (resp?.error ? `error: ${resp.error}` : '') +
            (echo ? echo : bypass ? '(bypass path — no echo; effect fired server-side)' : '(dispatched; no echo returned)'),
          isError: !!resp?.error,
        });
      } else if (m === 'turd') {
        // TurdMOD ! command — surfaced into chat on the chosen channel.
        if (target === 'All') {
          await engineRpc('broadcastChat', { text, channel: String(channel) });
        } else {
          await engineRpc('sendChatLineToPlayer', {
            playerName: target,
            message: text,
            channel: String(channel),
          });
        }
        pushLog({
          mode: m,
          line: text,
          via: target === 'All' ? `broadcastChat ch:${CHANNELS[channel].label}` : `whisper → ${target}`,
          response:
            'sent to chat. NOTE: this DISPLAYS the ! line — true mod execution from the manager needs the /chat/simulate inject (next step).',
          isError: false,
        });
      } else {
        // plain chat message
        if (target === 'All') {
          await engineRpc('broadcastChat', { text, channel: String(channel) });
          pushLog({ mode: m, line: text, via: `broadcastChat ch:${CHANNELS[channel].label}`, response: 'broadcast to all', isError: false });
        } else {
          await engineRpc('sendChatLineToPlayer', { playerName: target, message: text, channel: String(channel) });
          pushLog({ mode: m, line: text, via: `whisper → ${target} ch:${CHANNELS[channel].label}`, response: `sent to ${target}`, isError: false });
        }
      }
      setInput('');
    } catch (e) {
      pushLog({ mode: m, line: text, via: 'error', response: String((e as Error)?.message ?? e), isError: true });
    } finally {
      setBusy(false);
      inputRef.current?.focus();
    }
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') { e.preventDefault(); void send(); return; }
    if (e.key === 'Escape') { setSugOpen(false); return; }
    if (e.key === 'Tab' && sugOpen && suggestions.length > 0) {
      e.preventDefault();
      acceptSuggestion(suggestions[sugIdx]);
      return;
    }
    if (sugOpen && suggestions.length > 0) {
      if (e.key === 'ArrowDown') { e.preventDefault(); setSugIdx((i) => Math.min(i + 1, suggestions.length - 1)); return; }
      if (e.key === 'ArrowUp')   { e.preventDefault(); setSugIdx((i) => Math.max(i - 1, 0)); return; }
    } else {
      // history recall
      const cmds = log.map((l) => l.line);
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        const ni = histIdx === null ? cmds.length - 1 : Math.max(0, histIdx - 1);
        if (ni >= 0 && ni < cmds.length) { setHistIdx(ni); setInput(cmds[ni]); }
        return;
      }
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        if (histIdx === null) return;
        const ni = histIdx + 1;
        if (ni < cmds.length) { setHistIdx(ni); setInput(cmds[ni]); }
        else { setHistIdx(null); setInput(''); }
        return;
      }
    }
  };

  const sigil = mode === 'admin' ? '#' : mode === 'turd' ? '!' : '›';
  const sigilColor =
    mode === 'admin' ? 'text-turd-mustard-bright' : mode === 'turd' ? 'text-turd-green' : 'text-turd-cream-dim';
  const modeLabel =
    mode === 'admin' ? `Admin command${bypass ? ' · BYPASS gate' : ' · real path'}`
    : mode === 'turd' ? 'TurdMOD ! command'
    : 'Chat message';

  return (
    <div className="flex h-full flex-col gap-3">
      <header>
        <p className="font-display text-xs tracking-[0.3em] text-turd-mustard">TurdMOD</p>
        <h1 className="mt-1 font-display text-3xl text-turd-cream">Chat Console</h1>
        <p className="mt-1 max-w-3xl text-xs text-turd-cream-dim">
          One line to the live server. <code className="text-turd-mustard-bright">#</code> = native admin command
          (fires through the bridge — watch in-game react), <code className="text-turd-green">!</code> = TurdMOD command,
          plain text = chat. Tab to accept autosuggest, ↑/↓ to recall history, Enter to send.
        </p>
      </header>

      {/* Controls */}
      <div className="flex flex-wrap items-end gap-3 glass rounded-xl p-3">
        <label className="flex flex-col gap-1 text-xs">
          <span className="uppercase tracking-wider text-turd-cream-dim/60">Channel</span>
          <select
            value={channel}
            onChange={(e) => setChannel(Number(e.target.value))}
            className="rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-2 py-1 text-sm text-turd-cream focus:border-turd-mustard-bright focus:outline-none"
          >
            {CHANNELS.map((c) => (
              <option key={c.value} value={c.value}>{c.label}</option>
            ))}
          </select>
        </label>

        <label className="flex flex-col gap-1 text-xs">
          <span className="uppercase tracking-wider text-turd-cream-dim/60">
            Target {roster.data?.count != null ? `· ${roster.data.count} online` : ''}
          </span>
          <select
            value={target}
            onChange={(e) => setTarget(e.target.value)}
            className="min-w-[160px] rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-2 py-1 text-sm text-turd-cream focus:border-turd-mustard-bright focus:outline-none"
          >
            <option value="All">All players</option>
            {players.map((p) => (
              <option key={p} value={p}>{p}</option>
            ))}
          </select>
        </label>

        <label className="flex flex-col gap-1 text-xs">
          <span className="uppercase tracking-wider text-turd-cream-dim/60">Admin path</span>
          <button
            type="button"
            onClick={() => setBypass((b) => !b)}
            className={`rounded border px-3 py-1 text-[11px] font-medium transition-colors ${
              bypass
                ? 'border-turd-red/60 bg-turd-red/15 text-turd-red'
                : 'border-turd-green/50 bg-turd-green/10 text-turd-green'
            }`}
            title="Real = runAdminCommand (tier-gated, echoes output). Bypass = runTestAdminCommand (no gate, fires effect, no echo)."
          >
            {bypass ? 'BYPASS gate (Test)' : 'Real (gated)'}
          </button>
        </label>

        <span className="ml-auto self-center rounded border border-turd-bronze/30 bg-turd-bg-deep/40 px-2 py-1 font-mono text-[10px] text-turd-cream-dim">
          {modeLabel}
        </span>
      </div>

      {/* Input bar */}
      <div className="relative">
        <div className="relative flex items-center rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-2 focus-within:border-turd-mustard">
          <span className={`font-mono text-lg ${sigilColor}`}>{sigil}</span>
          <input
            ref={inputRef}
            type="text"
            value={input}
            onChange={(e) => { setInput(e.target.value); setHistIdx(null); }}
            onKeyDown={onKeyDown}
            placeholder="#SetGodMode YOUR_OWNER_NAME 1   ·   !kit   ·   or just chat…"
            autoComplete="off"
            spellCheck={false}
            className="ml-2 flex-1 bg-transparent font-mono text-sm text-turd-cream outline-none placeholder:text-turd-cream-dim/50"
          />
          <button
            type="button"
            onClick={() => void send()}
            disabled={busy || !input.trim()}
            className="ml-2 rounded border border-turd-mustard/60 bg-turd-mustard/20 px-4 py-1 font-display text-xs uppercase tracking-wider text-turd-mustard-bright transition-colors hover:bg-turd-mustard/30 disabled:cursor-not-allowed disabled:opacity-30"
          >
            {busy ? 'Sending…' : 'Send'}
          </button>
        </div>

        {/* Autosuggest dropdown */}
        {sugOpen && suggestions.length > 0 && (
          <div className="absolute left-0 right-0 top-full z-50 mt-1 max-h-64 overflow-auto rounded border border-turd-bronze/40 bg-turd-bg-deep shadow-lg">
            {suggestions.map((v, idx) => {
              const meta = mode === 'admin' ? adminMeta.byVerb.get(v) : undefined;
              return (
                <button
                  key={v}
                  type="button"
                  onClick={() => acceptSuggestion(v)}
                  className={`flex w-full flex-col items-start gap-0.5 px-3 py-1.5 text-left transition-colors ${
                    idx === sugIdx ? 'bg-turd-mustard/20 text-turd-mustard-bright' : 'text-turd-cream hover:bg-turd-bg-soft'
                  }`}
                >
                  <span className="font-mono text-xs">
                    <span className={mode === 'admin' ? 'text-turd-mustard-bright' : 'text-turd-green'}>
                      {mode === 'admin' ? '#' : '!'}
                    </span>
                    {v}
                  </span>
                  {meta?.description && (
                    <span className="line-clamp-1 text-[10px] text-turd-cream-dim/80">{meta.description}</span>
                  )}
                </button>
              );
            })}
            <div className="border-t border-turd-bronze/20 px-3 py-1 text-[9px] uppercase tracking-wider text-turd-cream-dim/50">
              Tab to accept · ↑/↓ to navigate
            </div>
          </div>
        )}
      </div>

      {/* Admin arg-hint pane */}
      {matchedAdmin && (
        <div className="rounded border border-turd-bronze/20 bg-turd-bg-deep/40 px-3 py-2">
          <div className="flex items-baseline gap-2">
            <code className="font-mono text-xs text-turd-mustard-bright">#{matchedAdmin.verb}</code>
            <span className="text-[10px] text-turd-cream-dim/70">
              {matchedAdmin.numRequired} required · {Math.max(0, matchedAdmin.args.length - matchedAdmin.numRequired)} optional
            </span>
          </div>
          {matchedAdmin.description && (
            <p className="mt-1 text-[11px] text-turd-cream-dim">{matchedAdmin.description}</p>
          )}
          {matchedAdmin.args.length > 0 && (
            <div className="mt-2 space-y-1">
              {matchedAdmin.args.map((arg, i) => {
                const req = i < matchedAdmin.numRequired;
                return (
                  <div key={i} className="text-[10px]">
                    <span className={req ? 'text-turd-mustard' : 'text-turd-cream-dim'}>
                      {req ? '<' : '['}{arg.name || `arg${i + 1}`}{req ? '>' : ']'}
                    </span>
                    {arg.description && <span className="ml-2 text-turd-cream-dim/80">{arg.description}</span>}
                    {arg.completionValues.length > 0 && (
                      <span className="ml-2 text-turd-bronze">
                        ({arg.completionValues.slice(0, 6).join(' | ')}{arg.completionValues.length > 6 ? ` | +${arg.completionValues.length - 6}` : ''})
                      </span>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </div>
      )}

      {/* Output log — where in-game responses land */}
      <section className="flex min-h-0 flex-1 flex-col overflow-hidden glass rounded-xl">
        <div className="flex items-center justify-between border-b border-turd-bronze/30 px-4 py-2">
          <h2 className="font-display text-sm uppercase tracking-widest text-turd-mustard">
            Server responses · {log.length}
          </h2>
          <button
            type="button"
            onClick={() => setLog([])}
            disabled={log.length === 0}
            className="text-[11px] text-turd-cream-dim transition-colors hover:text-turd-cream disabled:opacity-30"
          >
            Clear
          </button>
        </div>
        {log.length === 0 ? (
          <div className="flex flex-1 items-center justify-center p-6">
            <p className="text-xs text-turd-cream-dim">
              Fire a command above. Try <code className="text-turd-mustard-bright">#Announce hello from the manager</code> — you'll see it in game.
            </p>
          </div>
        ) : (
          <div className="flex-1 overflow-auto p-3">
            <div className="space-y-2">
              {log.map((e) => {
                const sig = e.mode === 'admin' ? 'text-turd-mustard-bright' : e.mode === 'turd' ? 'text-turd-green' : 'text-turd-cream-dim';
                return (
                  <div key={e.id} className="flex flex-col gap-1 rounded border border-turd-bronze/20 bg-turd-bg-deep/60 p-2">
                    <div className="flex items-baseline gap-2">
                      <span className="font-mono text-[10px] text-turd-cream-dim/60">
                        {e.ts.toLocaleTimeString('en-US', { hour12: false })}
                      </span>
                      <code className={`truncate font-mono text-xs ${sig}`}>{e.line}</code>
                      <span className="ml-auto shrink-0 font-mono text-[9px] text-turd-cream-dim/50">{e.via}</span>
                    </div>
                    <p className={`whitespace-pre-wrap break-words font-mono text-[10px] ${e.isError ? 'text-red-400' : 'text-turd-cream-dim'}`}>
                      {e.response}
                    </p>
                  </div>
                );
              })}
              <div ref={logEndRef} />
            </div>
          </div>
        )}
      </section>
    </div>
  );
}
