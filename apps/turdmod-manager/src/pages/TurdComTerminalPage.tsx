import { useRef, useEffect, useState, useMemo } from 'react';
import { useMutation } from '@tanstack/react-query';
import { engineRpc } from '../lib/tauri-engine';
import { useAdminCommandsMetadata } from '../hooks/useAdminCommandsMetadata';

// TurdCOM Terminal — admin-command REPL. Mirrors the in-game `#` chat
// experience: type a verb, get autocomplete + arg hints from the live
// bridge dump, hit Enter to dispatch via runAdminCommand. Result history
// persists to localStorage so testing survives Manager restarts.

interface TerminalEntry {
  id: string;
  timestamp: Date;
  command: string;
  response: string;
  isError: boolean;
}

interface StoredEntry {
  timestamp: number;
  command: string;
  response: string;
  isError: boolean;
}

const STORAGE_KEY = 'turdmod.turdcomTerminal.v1';
const MAX_HISTORY = 200;

function loadHistory(): TerminalEntry[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const stored = JSON.parse(raw) as StoredEntry[];
    return stored.slice(-MAX_HISTORY).map((s, i) => ({
      id: `${s.timestamp}-${i}`,
      timestamp: new Date(s.timestamp),
      command: s.command,
      response: s.response,
      isError: s.isError,
    }));
  } catch {
    return [];
  }
}

function saveHistory(entries: TerminalEntry[]) {
  try {
    const toStore: StoredEntry[] = entries.slice(-MAX_HISTORY).map((e) => ({
      timestamp: e.timestamp.getTime(),
      command: e.command,
      response: e.response,
      isError: e.isError,
    }));
    localStorage.setItem(STORAGE_KEY, JSON.stringify(toStore));
  } catch {
    // localStorage full / disabled — silently skip.
  }
}

export function TurdComTerminalPage() {
  const [history, setHistory] = useState<TerminalEntry[]>(loadHistory);
  const [inputValue, setInputValue] = useState('');
  const [historyIndex, setHistoryIndex] = useState<number | null>(null);
  const [showAutocomplete, setShowAutocomplete] = useState(false);
  const [autocompleteMatches, setAutocompleteMatches] = useState<string[]>([]);
  const [selectedAutocompleteIdx, setSelectedAutocompleteIdx] = useState(0);

  const inputRef = useRef<HTMLInputElement>(null);
  const historyEndRef = useRef<HTMLDivElement>(null);

  const adminMetadata = useAdminCommandsMetadata();

  const mutation = useMutation<
    { ok?: boolean; error?: string; command?: string },
    Error,
    { command: string }
  >({
    mutationFn: (params) =>
      engineRpc<{ ok?: boolean; error?: string; command?: string }>(
        'runAdminCommand',
        params,
      ),
  });

  // Auto-scroll history pane on new entries.
  useEffect(() => {
    historyEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [history]);

  // Extract just the verb token (everything after the `#` up to the first
  // whitespace) for autocomplete + hint-pane matching.
  const typedVerb = useMemo(() => {
    const trimmed = inputValue.replace(/^#+\s*/, '').trim();
    return trimmed.split(/\s/)[0] ?? '';
  }, [inputValue]);

  // Recompute autocomplete matches when the typed verb changes. Only
  // shows matches when the verb is a strict prefix of >= 1 known verb,
  // and the user hasn't already moved past the verb (no space yet).
  useEffect(() => {
    const trimmed = inputValue.replace(/^#+\s*/, '');
    const hasMovedPastVerb = /\s/.test(trimmed);
    if (!typedVerb || hasMovedPastVerb) {
      setShowAutocomplete(false);
      setAutocompleteMatches([]);
      return;
    }
    const q = typedVerb.toLowerCase();
    const allVerbs = Array.from(adminMetadata.byVerb.keys());
    const matches = allVerbs
      .filter((v) => v.toLowerCase().startsWith(q))
      .sort()
      .slice(0, 8);
    setAutocompleteMatches(matches);
    setSelectedAutocompleteIdx(0);
    setShowAutocomplete(matches.length > 0 && matches[0] !== typedVerb);
  }, [typedVerb, inputValue, adminMetadata.byVerb]);

  const matchedCommand = useMemo(
    () => adminMetadata.byVerb.get(typedVerb),
    [typedVerb, adminMetadata.byVerb],
  );

  const handleInputChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    // The leading `#` is rendered as a separate sigil outside the input;
    // strip it if the user types one so we don't get ##verb.
    const v = e.target.value.replace(/^#+/, '');
    setInputValue(v);
    setHistoryIndex(null);
  };

  const acceptAutocomplete = (verb: string) => {
    const trimmed = inputValue.replace(/^#+\s*/, '').trim();
    const parts = trimmed.split(/\s+/);
    parts[0] = verb;
    // Add a trailing space so the user can immediately start typing args.
    setInputValue(parts.join(' ') + (parts.length === 1 ? ' ' : ''));
    setShowAutocomplete(false);
    setAutocompleteMatches([]);
    inputRef.current?.focus();
  };

  const sendCommand = async () => {
    let cmd = inputValue.trim();
    if (!cmd) return;
    if (!cmd.startsWith('#')) cmd = `#${cmd}`;

    setInputValue('');
    setHistoryIndex(null);
    setShowAutocomplete(false);

    const entryId = `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
    try {
      // ProcessAdminCommand wants the BARE verb — a leading '#' yields
      // "Unrecognized command." (live-confirmed 2026-06-10). Strip it for the
      // wire; the history pane below still shows the '#' form for readability.
      const resp = await mutation.mutateAsync({ command: cmd.replace(/^#+\s*/, '') });
      const responseText = resp.error
        ? resp.error
        : resp.command
          ? `dispatched: ${resp.command}`
          : 'dispatched';
      const newEntry: TerminalEntry = {
        id: entryId,
        timestamp: new Date(),
        command: cmd,
        response: responseText,
        isError: !!resp.error,
      };
      const updated = [...history, newEntry].slice(-MAX_HISTORY);
      setHistory(updated);
      saveHistory(updated);
    } catch (err) {
      const newEntry: TerminalEntry = {
        id: entryId,
        timestamp: new Date(),
        command: cmd,
        response: String((err as Error)?.message ?? err),
        isError: true,
      };
      const updated = [...history, newEntry].slice(-MAX_HISTORY);
      setHistory(updated);
      saveHistory(updated);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      sendCommand();
      return;
    }
    if (e.key === 'Escape') {
      e.preventDefault();
      setShowAutocomplete(false);
      return;
    }
    if (e.key === 'Tab' && showAutocomplete && autocompleteMatches.length > 0) {
      e.preventDefault();
      acceptAutocomplete(autocompleteMatches[selectedAutocompleteIdx]);
      return;
    }
    if (showAutocomplete) {
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        setSelectedAutocompleteIdx((idx) =>
          idx < autocompleteMatches.length - 1 ? idx + 1 : idx,
        );
        return;
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        setSelectedAutocompleteIdx((idx) => (idx > 0 ? idx - 1 : 0));
        return;
      }
    }
    if (!showAutocomplete) {
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        const nextIdx =
          historyIndex === null
            ? history.length - 1
            : Math.max(0, historyIndex - 1);
        if (nextIdx >= 0 && nextIdx < history.length) {
          setHistoryIndex(nextIdx);
          setInputValue(history[nextIdx].command.replace(/^#+/, ''));
        }
        return;
      }
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        if (historyIndex === null) return;
        const nextIdx = historyIndex + 1;
        if (nextIdx < history.length) {
          setHistoryIndex(nextIdx);
          setInputValue(history[nextIdx].command.replace(/^#+/, ''));
        } else {
          setHistoryIndex(null);
          setInputValue('');
        }
        return;
      }
    }
  };

  const clearHistory = () => {
    if (!window.confirm('Clear all TurdCOM Terminal history?')) return;
    setHistory([]);
    saveHistory([]);
  };

  const copyCommand = (cmd: string) => {
    void navigator.clipboard.writeText(cmd).catch(() => {});
  };

  // Syntax-highlighted preview of the current input. Renders the `#`
  // mustard-bright, the verb in mustard, and args in cream. Sits ABOVE
  // the input — purely informational.
  const tokens = useMemo(() => {
    const raw = inputValue.startsWith('#') ? inputValue.slice(1) : inputValue;
    const parts = raw.split(/(\s+)/);
    return parts;
  }, [inputValue]);

  return (
    <div className="flex h-full flex-col gap-3">
      <header>
        <p className="font-display text-xs tracking-[0.3em] text-turd-mustard">
          TurdMOD
        </p>
        <h1 className="mt-1 font-display text-3xl text-turd-cream">
          TurdCOM Terminal
        </h1>
        <p className="mt-1 text-xs text-turd-cream-dim">
          Interactive REPL for SCUM admin commands. Tab to accept autocomplete,
          Up/Down to recall history, Enter to fire. Dispatches via the bridge's
          runAdminCommand handler.
        </p>
      </header>

      <div className="flex flex-col gap-2 glass rounded-xl p-4">
        {/* Syntax-highlighted preview */}
        {inputValue.trim() && (
          <div className="font-mono text-[11px] text-turd-cream-dim/70">
            <span className="text-turd-mustard-bright">#</span>
            {tokens.map((tok, i) => {
              if (/^\s+$/.test(tok)) return <span key={i}>{tok}</span>;
              if (i === 0) {
                return (
                  <span key={i} className="text-turd-mustard">
                    {tok}
                  </span>
                );
              }
              return (
                <span key={i} className="text-turd-cream">
                  {tok}
                </span>
              );
            })}
          </div>
        )}

        <div className="relative">
          <div className="relative flex items-center rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-2 focus-within:border-turd-mustard">
            <span className="font-mono text-sm text-turd-mustard-bright">#</span>
            <input
              ref={inputRef}
              type="text"
              value={inputValue}
              onChange={handleInputChange}
              onKeyDown={handleKeyDown}
              placeholder="SetGodMode SampleAdmin 1"
              className="ml-1 flex-1 bg-transparent font-mono text-sm text-turd-cream outline-none placeholder:text-turd-cream-dim/60"
              autoComplete="off"
              spellCheck={false}
            />
            <button
              type="button"
              onClick={() => void sendCommand()}
              disabled={mutation.isPending || !inputValue.trim()}
              className="ml-2 rounded border border-turd-mustard/60 bg-turd-mustard/20 px-3 py-1 font-display text-xs uppercase tracking-wider text-turd-mustard-bright transition-colors hover:bg-turd-mustard/30 disabled:cursor-not-allowed disabled:opacity-30"
            >
              {mutation.isPending ? 'Sending…' : 'Send'}
            </button>
          </div>

          {/* Autocomplete dropdown */}
          {showAutocomplete && autocompleteMatches.length > 0 && (
            <div className="absolute left-0 right-0 top-full z-50 mt-1 max-h-56 overflow-auto rounded border border-turd-bronze/40 bg-turd-bg-deep shadow-lg">
              {autocompleteMatches.map((verb, idx) => {
                const meta = adminMetadata.byVerb.get(verb);
                return (
                  <button
                    key={verb}
                    type="button"
                    onClick={() => acceptAutocomplete(verb)}
                    className={`flex w-full flex-col items-start gap-0.5 px-3 py-1.5 text-left transition-colors ${
                      idx === selectedAutocompleteIdx
                        ? 'bg-turd-mustard/20 text-turd-mustard-bright'
                        : 'text-turd-cream hover:bg-turd-bg-soft'
                    }`}
                  >
                    <span className="font-mono text-xs">{verb}</span>
                    {meta?.description && (
                      <span className="line-clamp-1 text-[10px] text-turd-cream-dim/80">
                        {meta.description}
                      </span>
                    )}
                  </button>
                );
              })}
            </div>
          )}
        </div>

        {/* Verb-aware hint pane */}
        {matchedCommand && (
          <div className="rounded border border-turd-bronze/20 bg-turd-bg-deep/40 px-3 py-2">
            <div className="flex items-baseline gap-2">
              <code className="font-mono text-xs text-turd-mustard-bright">
                #{matchedCommand.verb}
              </code>
              <span className="text-[10px] text-turd-cream-dim/70">
                {matchedCommand.numRequired} required,{' '}
                {Math.max(0, matchedCommand.args.length - matchedCommand.numRequired)} optional
              </span>
            </div>
            {matchedCommand.description && (
              <p className="mt-1 text-[11px] text-turd-cream-dim">
                {matchedCommand.description}
              </p>
            )}
            {matchedCommand.args.length > 0 && (
              <div className="mt-2 space-y-1">
                {matchedCommand.args.map((arg, i) => {
                  const isRequired = i < matchedCommand.numRequired;
                  return (
                    <div key={i} className="text-[10px]">
                      <span
                        className={
                          isRequired ? 'text-turd-mustard' : 'text-turd-cream-dim'
                        }
                      >
                        {isRequired ? '<' : '['}
                        {arg.name || `arg${i + 1}`}
                        {isRequired ? '>' : ']'}
                      </span>
                      {arg.description && (
                        <span className="ml-2 text-turd-cream-dim/80">
                          {arg.description}
                        </span>
                      )}
                      {arg.completionValues.length > 0 && (
                        <span className="ml-2 text-turd-bronze">
                          ({arg.completionValues.slice(0, 6).join(' | ')}
                          {arg.completionValues.length > 6
                            ? ` | +${arg.completionValues.length - 6}`
                            : ''}
                          )
                        </span>
                      )}
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        )}
      </div>

      {/* History pane */}
      <section className="flex min-h-0 flex-1 flex-col overflow-hidden glass rounded-xl">
        <div className="flex items-center justify-between border-b border-turd-bronze/30 px-4 py-2">
          <h2 className="font-display text-sm uppercase tracking-widest text-turd-mustard">
            History · {history.length}
          </h2>
          <button
            type="button"
            onClick={clearHistory}
            disabled={history.length === 0}
            className="text-[11px] text-turd-cream-dim transition-colors hover:text-turd-cream disabled:opacity-30"
          >
            Clear
          </button>
        </div>

        {history.length === 0 ? (
          <div className="flex flex-1 items-center justify-center p-6">
            <p className="text-xs text-turd-cream-dim">
              No commands fired yet. Type a verb above to get started.
            </p>
          </div>
        ) : (
          <div className="flex-1 overflow-auto p-3">
            <div className="space-y-2">
              {history.map((entry) => (
                <div
                  key={entry.id}
                  className="flex flex-col gap-1 rounded border border-turd-bronze/20 bg-turd-bg-deep/60 p-2"
                >
                  <div className="flex items-baseline justify-between gap-2">
                    <div className="flex items-baseline gap-2 overflow-hidden">
                      <span className="font-mono text-[10px] text-turd-cream-dim/60">
                        {entry.timestamp.toLocaleTimeString('en-US', {
                          hour12: false,
                          hour: '2-digit',
                          minute: '2-digit',
                          second: '2-digit',
                        })}
                      </span>
                      <code className="truncate font-mono text-xs text-turd-mustard-bright">
                        {entry.command}
                      </code>
                    </div>
                    <button
                      type="button"
                      onClick={() => copyCommand(entry.command)}
                      className="shrink-0 rounded border border-turd-bronze/30 px-1.5 py-0.5 text-[10px] text-turd-cream-dim transition-colors hover:border-turd-bronze hover:text-turd-cream"
                      title="Copy command"
                    >
                      copy
                    </button>
                  </div>
                  <p
                    className={`font-mono text-[10px] ${
                      entry.isError ? 'text-red-400' : 'text-turd-cream-dim'
                    }`}
                  >
                    {entry.response}
                  </p>
                </div>
              ))}
              <div ref={historyEndRef} />
            </div>
          </div>
        )}
      </section>
    </div>
  );
}
