import { useState } from 'react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { engineRpc } from '../lib/tauri-engine';
import {
  CHANNEL_OPTIONS,
  type WelcomeModConfig,
  type WelcomeModStatus,
} from '../hooks/useWelcomeMod';

// ---------------------------------------------------------------------------
// AdminPage — interactive admin tools wired to the bridge handlers we
// already shipped:
//
//   - broadcastChat   → send a server-side chat line to every player
//   - getOnlinePlayers → live roster, auto-refreshes every 2s
//   - teleportPlayer  → move a selected player's pawn to (x, y, z)
//
// First Manager page that does writes (not just reads/inspect). Each
// action shows feedback inline; errors render the raw engine response.
// ---------------------------------------------------------------------------

const ROSTER_REFRESH_MS = 2000;

interface PlayerEntry {
  name: string;          // display name from PlayerState.PlayerName
  controller: string;    // PC UObject name
  class: string;         // PC UClass FName
  ptr: string;           // hex UObject pointer (opaque key)
}

interface GetOnlinePlayersResponse {
  count: number;
  players: PlayerEntry[];
}

interface BroadcastChatResponse {
  ok?: boolean;
  sent?: string;
  error?: string;
}

interface TeleportResponse {
  ok?: boolean;
  teleported?: boolean;
  x?: number;
  y?: number;
  z?: number;
  error?: string;
}

const CHAT_TYPE_OPTIONS = [
  { value: '0', label: 'Local (0)' },
  { value: '1', label: 'Squad (1)' },
  { value: '2', label: 'Global (2)' },
  { value: '3', label: 'Admin (3)' },
];

// The Welcome Mod hook state is owned by App.tsx (so polling continues
// regardless of page) and threaded down so AdminPage owns the UI.
export interface AdminPageProps {
  welcomeMod: {
    config: WelcomeModConfig;
    setConfig: (update: Partial<WelcomeModConfig>) => void;
    status: WelcomeModStatus;
  };
}

export function AdminPage({ welcomeMod }: AdminPageProps) {
  return (
    <div className="flex h-full flex-col gap-3">
      <header>
        <p className="font-display text-xs tracking-[0.3em] text-turd-mustard">
          TurdMOD
        </p>
        <h1 className="mt-1 font-display text-3xl text-turd-cream">Admin</h1>
        <p className="mt-1 text-xs text-turd-cream-dim">
          Interactive admin tools wired to the bridge. Engine must be running.
        </p>
      </header>

      <BroadcastSection />

      <NotificationToastSection />

      <WelcomeModSection
        config={welcomeMod.config}
        setConfig={welcomeMod.setConfig}
        status={welcomeMod.status}
      />

      <div className="grid flex-1 grid-cols-[1fr_360px] gap-3 overflow-hidden">
        <PlayerRosterSection />
        <TeleportSection />
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Welcome Mod — auto-sends a configurable chat line on player join. First
// real player-visible TurdMOD mod. The polling lives in useWelcomeMod (at
// App level); this section is the config + status surface.
// ---------------------------------------------------------------------------

interface WelcomeModSectionProps {
  config: WelcomeModConfig;
  setConfig: (update: Partial<WelcomeModConfig>) => void;
  status: WelcomeModStatus;
}

function relativeTime(date: Date): string {
  const diffSec = Math.floor((Date.now() - date.getTime()) / 1000);
  if (diffSec < 60) return `${diffSec}s ago`;
  const diffMin = Math.floor(diffSec / 60);
  if (diffMin < 60) return `${diffMin} min ago`;
  return `${Math.floor(diffMin / 60)}h ago`;
}

export function WelcomeModSection({ config, setConfig, status }: WelcomeModSectionProps) {
  return (
    <section className="glass rounded-xl p-4">
      <h2 className="font-display text-sm uppercase tracking-widest text-turd-mustard">
        Welcome Mod
      </h2>
      <p className="mt-0.5 text-[11px] text-turd-cream-dim">
        Auto-sends a chat line when a new player joins. Polls every 5 s.
      </p>

      <div className="mt-3 flex flex-col gap-3">
        <label className="flex cursor-pointer select-none items-center gap-2">
          <input
            type="checkbox"
            checked={config.enabled}
            onChange={(e) => setConfig({ enabled: e.target.checked })}
            className="h-3.5 w-3.5 accent-turd-mustard"
          />
          <span className="font-mono text-xs text-turd-cream">Enabled</span>
        </label>

        <div className="flex items-start gap-3">
          <div className="flex flex-col gap-1">
            <span className="font-mono text-[10px] uppercase tracking-wider text-turd-cream-dim">
              Channel
            </span>
            <select
              value={config.channel}
              onChange={(e) =>
                setConfig({ channel: e.target.value as WelcomeModConfig['channel'] })
              }
              className="rounded border border-turd-bronze/60 bg-turd-bg-soft px-2 py-1.5 font-mono text-xs text-turd-cream focus:border-turd-mustard focus:outline-none"
            >
              {CHANNEL_OPTIONS.map((ch) => (
                <option key={ch} value={ch}>
                  {ch}
                </option>
              ))}
            </select>
          </div>

          <div className="flex flex-1 flex-col gap-1">
            <span className="font-mono text-[10px] uppercase tracking-wider text-turd-cream-dim">
              Message
            </span>
            <input
              type="text"
              value={config.message}
              onChange={(e) => setConfig({ message: e.target.value })}
              placeholder="Welcome to the server, {playerName}!"
              className="w-full rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-1.5 font-mono text-xs text-turd-cream placeholder:text-turd-cream-dim/60 focus:border-turd-mustard focus:outline-none"
            />
            <p className="text-[10px] text-turd-cream-dim/70">
              Use <code className="font-mono text-turd-mustard/80">{'{playerName}'}</code> to
              insert the joining player's name.
            </p>
          </div>
        </div>

        <div className="rounded bg-turd-bg-deep px-3 py-2 font-mono text-[11px]">
          {status.lastFiredAt ? (
            <p className="text-turd-cream">
              Last fired:{' '}
              <span className="text-turd-mustard-bright">{status.lastFiredPlayer}</span>
              {' · '}
              {relativeTime(status.lastFiredAt)}
              {' · '}
              {status.totalFiredCount} total
            </p>
          ) : (
            <p className="text-turd-cream-dim">No welcomes fired this session.</p>
          )}
          <p
            className={`mt-0.5 ${config.enabled ? 'text-turd-green' : 'text-turd-cream-dim'}`}
          >
            Status:{' '}
            {config.enabled ? 'Active — watching for joins' : 'Disabled'}
          </p>
          {status.lastError && (
            <p className="mt-1 text-turd-red">{status.lastError}</p>
          )}
        </div>
      </div>
    </section>
  );
}

// ---------------------------------------------------------------------------
// Broadcast — single textbox + chat type + send
// ---------------------------------------------------------------------------

function BroadcastSection() {
  const [text, setText] = useState('');
  const [chatType, setChatType] = useState('0');

  const mutation = useMutation<BroadcastChatResponse, Error, { text: string; chatType: string }>({
    mutationFn: (params) => engineRpc<BroadcastChatResponse>('broadcastChat', params),
  });

  const send = () => {
    if (!text.trim()) return;
    mutation.mutate(
      { text, chatType },
      { onSuccess: () => setText('') },
    );
  };

  return (
    <section className="glass rounded-xl p-4">
      <h2 className="font-display text-sm uppercase tracking-widest text-turd-mustard">
        Broadcast
      </h2>
      <div className="mt-3 flex gap-2">
        <input
          type="text"
          placeholder="Message to broadcast…"
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey) {
              e.preventDefault();
              send();
            }
          }}
          className="flex-1 rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-1.5 font-mono text-xs text-turd-cream placeholder:text-turd-cream-dim/60 focus:border-turd-mustard focus:outline-none"
        />
        <select
          value={chatType}
          onChange={(e) => setChatType(e.target.value)}
          className="rounded border border-turd-bronze/60 bg-turd-bg-soft px-2 py-1.5 font-mono text-xs text-turd-cream focus:border-turd-mustard focus:outline-none"
        >
          {CHAT_TYPE_OPTIONS.map((opt) => (
            <option key={opt.value} value={opt.value}>
              {opt.label}
            </option>
          ))}
        </select>
        <button
          type="button"
          onClick={send}
          disabled={!text.trim() || mutation.isPending}
          className="rounded border border-turd-mustard/60 bg-turd-mustard/20 px-4 py-1.5 font-display text-xs uppercase tracking-wider text-turd-mustard-bright transition-colors hover:bg-turd-mustard/30 disabled:cursor-not-allowed disabled:opacity-30"
        >
          {mutation.isPending ? 'Sending…' : 'Send'}
        </button>
      </div>
      {mutation.isError && (
        <p className="mt-2 font-mono text-[11px] text-turd-red">
          {String(mutation.error?.message ?? mutation.error)}
        </p>
      )}
      {mutation.isSuccess && mutation.data?.error && (
        <p className="mt-2 font-mono text-[11px] text-turd-red">
          {mutation.data.error}
        </p>
      )}
      {mutation.isSuccess && mutation.data?.ok && (
        <p className="mt-2 font-mono text-[11px] text-turd-green">
          ✓ Sent: {mutation.data.sent}
        </p>
      )}
    </section>
  );
}

// ---------------------------------------------------------------------------
// Notification Toast — fire SCUM's per-player UI_NotificationWidget toast
// (5 fixed visual styles). Picks target player from the live roster.
// ---------------------------------------------------------------------------

interface NotificationResponse {
  ok?: boolean;
  playerName?: string;
  type?: number;
  message?: string;
  calledFunction?: string;
  error?: string;
}

const NOTIFICATION_TYPE_OPTIONS = [
  { value: 1, label: 'Type 1' },
  { value: 2, label: 'Type 2' },
  { value: 3, label: 'Type 3' },
  { value: 4, label: 'Type 4' },
  { value: 5, label: 'Type 5' },
];

function NotificationToastSection() {
  const [playerName, setPlayerName] = useState('');
  const [notifType, setNotifType] = useState(1);
  const [message, setMessage] = useState('');
  const [feedback, setFeedback] = useState<{ ok: boolean; text: string } | null>(null);

  const roster = useQuery<GetOnlinePlayersResponse, Error>({
    queryKey: ['engine', 'getOnlinePlayers'],
    queryFn: () => engineRpc<GetOnlinePlayersResponse>('getOnlinePlayers', {}),
    refetchInterval: ROSTER_REFRESH_MS,
    retry: false,
  });

  const players = roster.data?.players ?? [];

  const mutation = useMutation<
    NotificationResponse,
    Error,
    { playerName: string; type: number; message: string }
  >({
    mutationFn: (params) => engineRpc<NotificationResponse>('sendNotification', params),
    onSuccess: (data) => {
      if (data?.ok) {
        setFeedback({
          ok: true,
          text: `✓ Sent to ${data.playerName} via ${data.calledFunction ?? 'unknown function'}`,
        });
      } else {
        setFeedback({ ok: false, text: data?.error ?? 'Unknown error' });
      }
      setTimeout(() => setFeedback(null), 4000);
    },
    onError: (err) => {
      setFeedback({ ok: false, text: String(err?.message ?? err) });
      setTimeout(() => setFeedback(null), 4000);
    },
  });

  const canSend = playerName.trim() !== '' && message.trim() !== '' && !mutation.isPending;

  const send = () => {
    if (!canSend) return;
    mutation.mutate({ playerName: playerName.trim(), type: notifType, message: message.trim() });
  };

  return (
    <section className="glass rounded-xl p-4">
      <h2 className="font-display text-sm uppercase tracking-widest text-turd-mustard">
        Notification Toast
      </h2>
      <p className="mt-0.5 text-[11px] text-turd-cream-dim">
        Sends a per-player UI_NotificationWidget toast. Fires via ProcessEvent — no admin auth required.
      </p>

      <div className="mt-3 flex gap-2">
        <select
          value={playerName}
          onChange={(e) => setPlayerName(e.target.value)}
          disabled={players.length === 0}
          className="rounded border border-turd-bronze/60 bg-turd-bg-soft px-2 py-1.5 font-mono text-xs text-turd-cream focus:border-turd-mustard focus:outline-none disabled:cursor-not-allowed disabled:opacity-40"
        >
          <option value="">
            {players.length === 0 ? '(no players online)' : '— select player —'}
          </option>
          {players.map((p) => (
            <option key={p.ptr} value={p.name}>
              {p.name || '(no name)'}
            </option>
          ))}
        </select>

        <div className="flex flex-col gap-0.5">
          <select
            value={notifType}
            onChange={(e) => setNotifType(Number(e.target.value))}
            className="rounded border border-turd-bronze/60 bg-turd-bg-soft px-2 py-1.5 font-mono text-xs text-turd-cream focus:border-turd-mustard focus:outline-none"
          >
            {NOTIFICATION_TYPE_OPTIONS.map((opt) => (
              <option key={opt.value} value={opt.value}>
                {opt.label}
              </option>
            ))}
          </select>
          <span className="text-[9px] text-turd-cream-dim/70">
            5 fixed visual styles. Test to see what each one looks like in-game.
          </span>
        </div>

        <input
          type="text"
          placeholder="Notification text"
          value={message}
          onChange={(e) => setMessage(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey) {
              e.preventDefault();
              send();
            }
          }}
          className="flex-1 rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-1.5 font-mono text-xs text-turd-cream placeholder:text-turd-cream-dim/60 focus:border-turd-mustard focus:outline-none"
        />

        <button
          type="button"
          onClick={send}
          disabled={!canSend}
          className="rounded border border-turd-mustard/60 bg-turd-mustard/20 px-4 py-1.5 font-display text-xs uppercase tracking-wider text-turd-mustard-bright transition-colors hover:bg-turd-mustard/30 disabled:cursor-not-allowed disabled:opacity-30"
        >
          {mutation.isPending ? 'Sending…' : 'Send'}
        </button>
      </div>

      {feedback && (
        <p
          className={`mt-2 font-mono text-[11px] ${
            feedback.ok ? 'text-turd-green' : 'text-turd-red'
          }`}
        >
          {feedback.text}
        </p>
      )}
    </section>
  );
}

// ---------------------------------------------------------------------------
// Player roster — auto-refreshing table
// ---------------------------------------------------------------------------

// Shared selected-player state via a small lift. Could move to context
// later; for now props-drilling between sibling sections is cheap.
const selectedPlayerStore = (() => {
  let listeners: Array<(p: PlayerEntry | null) => void> = [];
  let current: PlayerEntry | null = null;
  return {
    get: () => current,
    set: (p: PlayerEntry | null) => {
      current = p;
      listeners.forEach((l) => l(p));
    },
    subscribe: (l: (p: PlayerEntry | null) => void) => {
      listeners.push(l);
      return () => {
        listeners = listeners.filter((x) => x !== l);
      };
    },
  };
})();

function useSelectedPlayer() {
  const [p, setP] = useState<PlayerEntry | null>(selectedPlayerStore.get());
  useState(() => selectedPlayerStore.subscribe(setP));
  return [p, selectedPlayerStore.set] as const;
}

function PlayerRosterSection() {
  const query = useQuery<GetOnlinePlayersResponse, Error>({
    queryKey: ['engine', 'getOnlinePlayers'],
    queryFn: () => engineRpc<GetOnlinePlayersResponse>('getOnlinePlayers'),
    refetchInterval: ROSTER_REFRESH_MS,
    retry: false,
  });

  const [selected, setSelected] = useSelectedPlayer();

  const players = query.data?.players ?? [];

  return (
    <section className="flex flex-col overflow-hidden glass rounded-xl">
      <header className="flex items-center justify-between border-b border-turd-bronze/30 p-3">
        <h2 className="font-display text-sm uppercase tracking-widest text-turd-mustard">
          Online Players
        </h2>
        <span className="font-mono text-[10px] text-turd-cream-dim">
          {query.isLoading
            ? 'loading…'
            : `${query.data?.count ?? 0} connected · refreshes every ${ROSTER_REFRESH_MS / 1000}s`}
        </span>
      </header>

      {query.isError && (
        <div className="p-3 font-mono text-[11px] text-turd-red">
          {String(query.error?.message ?? query.error)}
        </div>
      )}

      <div className="flex-1 overflow-auto">
        {players.length === 0 && !query.isLoading ? (
          <p className="px-3 py-6 text-center text-xs text-turd-cream-dim">
            No players connected.
          </p>
        ) : (
          <table className="w-full font-mono text-[11px]">
            <thead className="sticky top-0 bg-turd-bg-mid/95">
              <tr className="border-b border-turd-bronze/30 text-left text-turd-cream-dim">
                <th className="px-3 py-2 font-normal">Name</th>
                <th className="px-3 py-2 font-normal">Class</th>
                <th className="px-3 py-2 font-normal">Controller</th>
              </tr>
            </thead>
            <tbody>
              {players.map((p) => (
                <tr
                  key={p.ptr}
                  onClick={() => setSelected(p)}
                  className={`cursor-pointer border-b border-turd-bronze/10 transition-colors ${
                    selected?.ptr === p.ptr
                      ? 'bg-turd-mustard/20 text-turd-mustard-bright'
                      : 'text-turd-cream hover:bg-white/5'
                  }`}
                >
                  <td className="px-3 py-1.5">{p.name || <em className="text-turd-cream-dim">(no name)</em>}</td>
                  <td className="px-3 py-1.5 text-turd-cream-dim">{p.class}</td>
                  <td className="px-3 py-1.5 text-turd-cream-dim/70 text-[10px]">
                    {p.controller}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </section>
  );
}

// ---------------------------------------------------------------------------
// Teleport — coords + button. Reads selected from PlayerRoster.
// ---------------------------------------------------------------------------

function TeleportSection() {
  const [selected] = useSelectedPlayer();
  const [x, setX] = useState('');
  const [y, setY] = useState('');
  const [z, setZ] = useState('');

  const mutation = useMutation<
    TeleportResponse,
    Error,
    { ptr: string; x: string; y: string; z: string }
  >({
    mutationFn: (params) => engineRpc<TeleportResponse>('teleportPlayer', params),
  });

  const send = () => {
    if (!selected || !x || !y || !z) return;
    mutation.mutate({ ptr: selected.ptr, x, y, z });
  };

  const labelCls =
    'font-mono text-[10px] uppercase tracking-wider text-turd-cream-dim';
  const inputCls =
    'rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-1.5 font-mono text-xs text-turd-cream placeholder:text-turd-cream-dim/60 focus:border-turd-mustard focus:outline-none';

  return (
    <section className="flex flex-col glass rounded-xl p-4">
      <h2 className="font-display text-sm uppercase tracking-widest text-turd-mustard">
        Teleport
      </h2>

      <div className="mt-3 rounded bg-turd-bg-deep p-3">
        {selected ? (
          <>
            <p className={labelCls}>Target</p>
            <p className="mt-1 font-mono text-xs text-turd-cream">
              {selected.name || selected.controller}
            </p>
            <p className="font-mono text-[10px] text-turd-cream-dim">{selected.ptr}</p>
          </>
        ) : (
          <p className="font-mono text-xs text-turd-cream-dim">
            Select a player from the roster.
          </p>
        )}
      </div>

      <div className="mt-3 grid grid-cols-3 gap-2">
        <div>
          <p className={labelCls}>X</p>
          <input
            type="text"
            placeholder="100000"
            value={x}
            onChange={(e) => setX(e.target.value)}
            className={`${inputCls} mt-1 w-full`}
          />
        </div>
        <div>
          <p className={labelCls}>Y</p>
          <input
            type="text"
            placeholder="-10000"
            value={y}
            onChange={(e) => setY(e.target.value)}
            className={`${inputCls} mt-1 w-full`}
          />
        </div>
        <div>
          <p className={labelCls}>Z</p>
          <input
            type="text"
            placeholder="300"
            value={z}
            onChange={(e) => setZ(e.target.value)}
            className={`${inputCls} mt-1 w-full`}
          />
        </div>
      </div>

      <button
        type="button"
        onClick={send}
        disabled={!selected || !x || !y || !z || mutation.isPending}
        className="mt-4 rounded border border-turd-mustard/60 bg-turd-mustard/20 px-4 py-2 font-display text-xs uppercase tracking-wider text-turd-mustard-bright transition-colors hover:bg-turd-mustard/30 disabled:cursor-not-allowed disabled:opacity-30"
      >
        {mutation.isPending ? 'Teleporting…' : 'Teleport'}
      </button>

      {mutation.isError && (
        <p className="mt-3 font-mono text-[11px] text-turd-red">
          {String(mutation.error?.message ?? mutation.error)}
        </p>
      )}
      {mutation.isSuccess && mutation.data?.error && (
        <p className="mt-3 font-mono text-[11px] text-turd-red">
          {mutation.data.error}
        </p>
      )}
      {mutation.isSuccess && mutation.data?.ok && (
        <p className="mt-3 font-mono text-[11px] text-turd-green">
          ✓ Teleported {mutation.data.teleported === false ? '(engine returned false)' : ''} to (
          {mutation.data.x}, {mutation.data.y}, {mutation.data.z})
        </p>
      )}
    </section>
  );
}
