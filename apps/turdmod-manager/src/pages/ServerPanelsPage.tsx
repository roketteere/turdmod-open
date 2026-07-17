import { useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { engineRpc } from '../lib/tauri-engine';

// Server Panels — wraps SCUM's #SendNotification admin command, which is
// the closest thing to a "panel-on-a-player's-screen" available without
// any client-side mod. The notification renders into the vanilla
// UI_NotificationWidget (single line of text + 1 of 5 styled icons).
//
// Architecture:
//   - Dispatch via runAdminCommand bridge handler (= SCUM's real admin
//     parser). The bridge is in SCUMServer.exe; no per-player install.
//   - Recipient: a SteamID from the live online roster (getOnlinePlayers
//     RPC), free-text fallback, or broadcast via the literal `-1`.
//   - Type: integer 1..5. The visual mapping for each type is UNVERIFIED
//     today — Gamepires hard-codes it in native C++. The page exposes a
//     "Field test all 5" button that fires #SendNotification 1..5 in
//     sequence with labeled messages so an admin can see the toasts roll
//     in on their own client and report back what each looks like.
//
// History is persisted to localStorage so repeat sends + replays survive
// reloads. Last 12 entries kept.
//
// Format spec: `#SendNotification <Type 1..5> <UserID> <Message>`
//   - Type:    AdminCommandArgumentDataType_Numeric, no completion
//   - UserID:  AdminCommandArgumentDataType_String,  `-1` = broadcast
//   - Message: AdminCommandArgumentDataType_String
//
// See docs/scum-internals/20-umg-server-driven-surfaces.md §4 + §7 for
// the upstream investigation that established this is the only
// per-player "panel-shaped" surface in vanilla SCUM.

const STORAGE_KEY = 'turdmod.serverPanels.v1';
const HISTORY_KEY = 'turdmod.serverPanels.history.v1';
const MAX_HISTORY = 12;

type NotificationType = 1 | 2 | 3 | 4 | 5;

interface PlayerEntry {
  name: string;
  steamId?: string;
  ptr?: string;
}

interface GetOnlinePlayersResp {
  count?: number;
  players?: PlayerEntry[];
}

interface RunAdminCommandResp {
  ok?: boolean;
  error?: string;
  command?: string;
}

interface StoredState {
  type: NotificationType;
  message: string;
  recipientMode: 'specific' | 'broadcast';
  recipientSteamId: string;
}

interface HistoryEntry {
  tsMs: number;
  type: NotificationType;
  userId: string;
  recipientLabel: string;
  message: string;
  outcome: 'ok' | 'error';
  errorText?: string;
}

const TYPE_INFO: Record<NotificationType, { label: string; hint: string }> = {
  // Visual mapping unverified — Gamepires-controlled native code. Update
  // these labels once a field test reports what each actually looks like.
  1: { label: 'Type 1', hint: 'style TBD — field-test to confirm' },
  2: { label: 'Type 2', hint: 'style TBD — field-test to confirm' },
  3: { label: 'Type 3', hint: 'style TBD — field-test to confirm' },
  4: { label: 'Type 4', hint: 'style TBD — field-test to confirm' },
  5: { label: 'Type 5', hint: 'style TBD — field-test to confirm' },
};

function defaults(): StoredState {
  return {
    type: 1,
    message: 'Welcome, prisoner.',
    recipientMode: 'broadcast',
    recipientSteamId: '',
  };
}

function loadState(): StoredState {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return defaults();
    const parsed = JSON.parse(raw) as Partial<StoredState>;
    return {
      type: clampType(parsed.type),
      message: parsed.message ?? defaults().message,
      recipientMode: parsed.recipientMode === 'specific' ? 'specific' : 'broadcast',
      recipientSteamId: parsed.recipientSteamId ?? '',
    };
  } catch {
    return defaults();
  }
}

function clampType(n: unknown): NotificationType {
  const v = Number(n);
  if (v >= 1 && v <= 5 && Number.isInteger(v)) return v as NotificationType;
  return 1;
}

function saveState(s: StoredState) {
  try { localStorage.setItem(STORAGE_KEY, JSON.stringify(s)); } catch { /* ignore */ }
}

function loadHistory(): HistoryEntry[] {
  try {
    const raw = localStorage.getItem(HISTORY_KEY);
    if (!raw) return [];
    return JSON.parse(raw) as HistoryEntry[];
  } catch { return []; }
}

function saveHistory(h: HistoryEntry[]) {
  try { localStorage.setItem(HISTORY_KEY, JSON.stringify(h.slice(0, MAX_HISTORY))); } catch { /* ignore */ }
}

// Quote any value containing whitespace so the SCUM admin parser sees a
// single argument. The parser splits on unquoted whitespace.
function quote(v: string): string {
  if (!v) return '""';
  return /\s/.test(v) && !v.startsWith('"') ? `"${v}"` : v;
}

function buildCommand(type: NotificationType, userId: string, message: string): string {
  return `#SendNotification ${type} ${quote(userId)} ${quote(message)}`;
}

async function fireNotification(
  type: NotificationType,
  userId: string,
  message: string,
): Promise<RunAdminCommandResp> {
  const command = buildCommand(type, userId, message);
  return engineRpc<RunAdminCommandResp>('runAdminCommand', { command });
}

export function ServerPanelsPage() {
  const initial = useMemo(loadState, []);
  const [type, setType] = useState<NotificationType>(initial.type);
  const [message, setMessage] = useState(initial.message);
  const [recipientMode, setRecipientMode] = useState<'specific' | 'broadcast'>(initial.recipientMode);
  const [recipientSteamId, setRecipientSteamId] = useState(initial.recipientSteamId);
  const [history, setHistory] = useState<HistoryEntry[]>(loadHistory);
  const [busy, setBusy] = useState(false);
  const [lastError, setLastError] = useState<string | null>(null);

  const roster = useQuery<GetOnlinePlayersResp, Error>({
    queryKey: ['server-panels', 'online-players'],
    queryFn: () => engineRpc<GetOnlinePlayersResp>('getOnlinePlayers', {}),
    refetchInterval: 5000,
    retry: false,
  });

  const persist = (next: Partial<StoredState>) => {
    saveState({ type, message, recipientMode, recipientSteamId, ...next });
  };

  const players = roster.data?.players ?? [];
  const effectiveUserId = recipientMode === 'broadcast' ? '-1' : recipientSteamId.trim();
  const recipientLabel = recipientMode === 'broadcast'
    ? 'broadcast (all players)'
    : players.find((p) => p.steamId === recipientSteamId)?.name || recipientSteamId || '(no recipient)';
  const canSend =
    !!message.trim() &&
    (recipientMode === 'broadcast' || effectiveUserId.length > 0);

  const recordOutcome = (entry: HistoryEntry) => {
    setHistory((prev) => {
      const next = [entry, ...prev].slice(0, MAX_HISTORY);
      saveHistory(next);
      return next;
    });
  };

  const sendOne = async () => {
    if (!canSend || busy) return;
    setBusy(true);
    setLastError(null);
    try {
      const resp = await fireNotification(type, effectiveUserId, message);
      if (resp?.error) throw new Error(resp.error);
      recordOutcome({
        tsMs: Date.now(),
        type,
        userId: effectiveUserId,
        recipientLabel,
        message,
        outcome: 'ok',
      });
    } catch (e) {
      const msg = (e as Error).message;
      setLastError(msg);
      recordOutcome({
        tsMs: Date.now(),
        type,
        userId: effectiveUserId,
        recipientLabel,
        message,
        outcome: 'error',
        errorText: msg,
      });
    } finally {
      setBusy(false);
    }
  };

  const fieldTestAll = async () => {
    // Field test the 5 variants. Fire 1..5 in sequence against whatever
    // recipient is currently configured, with labeled messages so the
    // observer can see which toast is which. 1.2s gap leaves room for
    // the previous toast to settle without timing out the queue.
    //
    // Sending to broadcast (-1) is fine but noisy on a populated server
    // — the page nudges admins toward picking themselves when in
    // specific-recipient mode.
    if (busy) return;
    setBusy(true);
    setLastError(null);
    try {
      for (const t of [1, 2, 3, 4, 5] as NotificationType[]) {
        try {
          const testMessage = `${message.trim() || 'Field test'} — type ${t}`;
          const resp = await fireNotification(t, effectiveUserId, testMessage);
          if (resp?.error) throw new Error(resp.error);
          recordOutcome({
            tsMs: Date.now(),
            type: t,
            userId: effectiveUserId,
            recipientLabel,
            message: testMessage,
            outcome: 'ok',
          });
        } catch (e) {
          const msg = (e as Error).message;
          setLastError(msg);
          recordOutcome({
            tsMs: Date.now(),
            type: t,
            userId: effectiveUserId,
            recipientLabel,
            message: `${message.trim() || 'Field test'} — type ${t}`,
            outcome: 'error',
            errorText: msg,
          });
        }
        if (t < 5) {
          await new Promise((r) => setTimeout(r, 1200));
        }
      }
    } finally {
      setBusy(false);
    }
  };

  const replayFromHistory = (entry: HistoryEntry) => {
    setType(entry.type);
    setMessage(entry.message);
    if (entry.userId === '-1') {
      setRecipientMode('broadcast');
      persist({ type: entry.type, message: entry.message, recipientMode: 'broadcast' });
    } else {
      setRecipientMode('specific');
      setRecipientSteamId(entry.userId);
      persist({ type: entry.type, message: entry.message, recipientMode: 'specific', recipientSteamId: entry.userId });
    }
  };

  return (
    <div className="flex h-full flex-col gap-3">
      <header className="flex items-start justify-between gap-4">
        <div>
          <p className="font-display text-xs tracking-[0.3em] text-turd-mustard">
            TurdMOD
          </p>
          <h1 className="mt-1 font-display text-3xl text-turd-cream">
            Server Panels
          </h1>
          <p className="mt-1 max-w-2xl text-xs text-turd-cream-dim">
            Push a SCUM notification toast to one player or everyone via
            the engine bridge's <code className="font-mono">runAdminCommand</code>.
            No client-side install — renders in the vanilla{' '}
            <code className="font-mono">UI_NotificationWidget</code>. The
            5 type styles are picked from a Gamepires-controlled native
            table; the labels below are placeholders until field-tested
            (use the button at the bottom).
          </p>
        </div>
      </header>

      <div className="grid min-h-0 flex-1 grid-cols-[1fr_320px] gap-3 overflow-hidden">
        {/* Left: composer */}
        <section className="flex min-h-0 flex-col overflow-hidden glass rounded-xl">
          <div className="flex-1 overflow-auto p-4 space-y-4">
            {/* Recipient */}
            <fieldset className="space-y-2">
              <legend className="text-[10px] uppercase tracking-wider text-turd-cream-dim">
                Recipient
              </legend>
              <div className="flex gap-2">
                <RecipientTab
                  active={recipientMode === 'broadcast'}
                  onClick={() => { setRecipientMode('broadcast'); persist({ recipientMode: 'broadcast' }); }}
                  label="Broadcast (all players)"
                />
                <RecipientTab
                  active={recipientMode === 'specific'}
                  onClick={() => { setRecipientMode('specific'); persist({ recipientMode: 'specific' }); }}
                  label="Specific player"
                />
              </div>
              {recipientMode === 'specific' && (
                <div className="space-y-2">
                  {players.length > 0 ? (
                    <select
                      value={recipientSteamId}
                      onChange={(e) => { setRecipientSteamId(e.target.value); persist({ recipientSteamId: e.target.value }); }}
                      className="w-full rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-2 py-1 font-mono text-xs text-turd-cream focus:border-turd-mustard-bright focus:outline-none"
                    >
                      <option value="">— pick a player —</option>
                      {players.map((p) => {
                        // Fall back to player name when the bridge roster
                        // didn't return a steamId (which it currently
                        // doesn't for most rows). SCUM's admin parser
                        // accepts the player name as a target for most
                        // commands; if SendNotification specifically
                        // requires a SteamID64, paste it into the text
                        // input below to override.
                        const value = p.steamId || p.name;
                        return (
                          <option key={value} value={value}>
                            {p.name}{p.steamId ? ` · ${p.steamId}` : ''}
                          </option>
                        );
                      })}
                    </select>
                  ) : (
                    <p className="text-[10px] text-turd-cream-dim">
                      {roster.isError
                        ? `Roster unavailable — ${roster.error.message}`
                        : roster.isPending
                          ? 'Loading online roster…'
                          : 'No players currently online (or roster RPC returned empty).'}
                    </p>
                  )}
                  <input
                    type="text"
                    placeholder="…or paste a SteamID64 / name directly"
                    value={recipientSteamId}
                    onChange={(e) => { setRecipientSteamId(e.target.value); persist({ recipientSteamId: e.target.value }); }}
                    className="w-full rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-2 py-1 font-mono text-xs text-turd-cream focus:border-turd-mustard-bright focus:outline-none"
                  />
                </div>
              )}
            </fieldset>

            {/* Type picker */}
            <fieldset className="space-y-2">
              <legend className="text-[10px] uppercase tracking-wider text-turd-cream-dim">
                Notification type
              </legend>
              <div className="grid grid-cols-5 gap-2">
                {([1, 2, 3, 4, 5] as NotificationType[]).map((t) => (
                  <TypeButton
                    key={t}
                    active={type === t}
                    info={TYPE_INFO[t]}
                    onClick={() => { setType(t); persist({ type: t }); }}
                  />
                ))}
              </div>
            </fieldset>

            {/* Message */}
            <fieldset className="space-y-1">
              <legend className="text-[10px] uppercase tracking-wider text-turd-cream-dim">
                Message
              </legend>
              <textarea
                value={message}
                onChange={(e) => { setMessage(e.target.value); persist({ message: e.target.value }); }}
                rows={3}
                placeholder="Welcome, prisoner."
                className="w-full resize-y rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-2 py-1 font-mono text-xs text-turd-cream focus:border-turd-mustard-bright focus:outline-none"
              />
              <p className="text-[10px] text-turd-cream-dim">
                Single-line toast. SCUM's notification widget doesn't wrap
                long text gracefully — keep it short.
              </p>
            </fieldset>

            {/* Preview */}
            <details className="rounded border border-turd-bronze/20 bg-turd-bg-deep/30 px-3 py-2 text-[10px] text-turd-cream-dim">
              <summary className="cursor-pointer text-turd-cream-dim/80">
                Command preview
              </summary>
              <pre className="mt-2 overflow-auto font-mono text-[10px] text-turd-mustard">
                {buildCommand(type, effectiveUserId || '<recipient>', message || '<message>')}
              </pre>
            </details>
          </div>

          <div className="flex items-center justify-between gap-3 border-t border-turd-bronze/30 px-4 py-2">
            <div className="min-w-0 flex-1 text-[10px] text-turd-cream-dim">
              {lastError && <span className="text-red-300">{lastError}</span>}
              {!lastError && busy && <span>Sending…</span>}
            </div>
            <div className="flex shrink-0 items-center gap-2">
              <button
                type="button"
                onClick={fieldTestAll}
                disabled={busy || (recipientMode === 'specific' && !recipientSteamId.trim())}
                className="rounded border border-turd-bronze/40 px-3 py-1 text-xs text-turd-cream-dim hover:bg-turd-bg-soft/40 hover:text-turd-cream disabled:cursor-not-allowed disabled:opacity-40"
                title="Fires #SendNotification 1..5 with labeled messages so you can identify each style"
              >
                Field test all 5
              </button>
              <button
                type="button"
                onClick={sendOne}
                disabled={!canSend || busy}
                className="rounded border border-turd-mustard/40 bg-turd-mustard/15 px-3 py-1 text-xs font-medium text-turd-mustard-bright hover:bg-turd-mustard/25 disabled:cursor-not-allowed disabled:opacity-40"
              >
                Send notification
              </button>
            </div>
          </div>
        </section>

        {/* Right: history */}
        <section className="flex min-h-0 flex-col overflow-hidden glass rounded-xl">
          <div className="flex items-center justify-between border-b border-turd-bronze/30 px-4 py-2">
            <h2 className="font-display text-sm uppercase tracking-widest text-turd-mustard">
              Recent sends
            </h2>
            {history.length > 0 && (
              <button
                type="button"
                onClick={() => { setHistory([]); saveHistory([]); }}
                className="text-[10px] text-turd-cream-dim hover:text-turd-cream"
              >
                Clear
              </button>
            )}
          </div>
          <div className="flex-1 overflow-auto p-2">
            {history.length === 0 ? (
              <p className="px-2 py-1 text-xs text-turd-cream-dim">
                Nothing sent yet. Pick a type + message and press{' '}
                <strong>Send notification</strong>.
              </p>
            ) : (
              <ul className="space-y-1">
                {history.map((entry) => (
                  <li key={entry.tsMs}>
                    <button
                      type="button"
                      onClick={() => replayFromHistory(entry)}
                      className="w-full rounded border border-transparent px-2 py-1 text-left hover:border-turd-bronze/30 hover:bg-turd-bg-soft/40"
                      title="Click to load into the composer"
                    >
                      <div className="flex items-baseline gap-2">
                        <span className={`text-[9px] ${entry.outcome === 'ok' ? 'text-turd-mustard-bright' : 'text-red-300'}`}>
                          {entry.outcome === 'ok' ? '●' : '✕'}
                        </span>
                        <span className="font-mono text-[10px] text-turd-cream-dim">
                          T{entry.type}
                        </span>
                        <span className="font-mono text-[10px] text-turd-cream-dim">
                          {new Date(entry.tsMs).toLocaleTimeString()}
                        </span>
                      </div>
                      <p className="mt-0.5 truncate pl-4 text-[11px] text-turd-cream">
                        {entry.message}
                      </p>
                      <p className="truncate pl-4 text-[9px] text-turd-cream-dim/70">
                        → {entry.recipientLabel}
                      </p>
                      {entry.errorText && (
                        <p className="truncate pl-4 font-mono text-[9px] text-red-300/80">
                          {entry.errorText}
                        </p>
                      )}
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </section>
      </div>
    </div>
  );
}

function RecipientTab({
  active, onClick, label,
}: { active: boolean; onClick: () => void; label: string }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={[
        'flex-1 rounded border px-3 py-1 text-[11px]',
        active
          ? 'border-turd-mustard/40 bg-turd-mustard/15 text-turd-mustard-bright'
          : 'border-turd-bronze/30 text-turd-cream-dim hover:bg-turd-bg-soft/40 hover:text-turd-cream',
      ].join(' ')}
    >
      {label}
    </button>
  );
}

function TypeButton({
  active, info, onClick,
}: { active: boolean; info: { label: string; hint: string }; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={[
        'flex flex-col items-center rounded border px-2 py-2 text-center transition-colors',
        active
          ? 'border-turd-mustard/50 bg-turd-mustard/15 text-turd-mustard-bright'
          : 'border-turd-bronze/30 text-turd-cream-dim hover:border-turd-bronze/50 hover:bg-turd-bg-soft/40 hover:text-turd-cream',
      ].join(' ')}
      title={info.hint}
    >
      <span className="font-display text-base">{info.label}</span>
      <span className="mt-0.5 text-[9px] uppercase tracking-wider text-turd-cream-dim/70">
        {info.hint}
      </span>
    </button>
  );
}
