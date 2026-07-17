import { useEffect, useMemo, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { adapterEngineRpc } from '../lib/adapter';
import { useEngineEvents, type EngineEvent } from '../hooks/useEngineEvents';

type OnlinePlayer = {
  name?: string;
  steamId?: string;
  playerName?: string;
  steamID64?: string;
  // bridge returns a few different shapes across handler versions
  [key: string]: unknown;
};

// ---------------------------------------------------------------------------
// BridgeSmokePage — exercise the bridge handlers shipped 2026-05-18 night.
//
// One section per new handler with the params it accepts and a "Fire" button.
// Result + error get rendered inline. No retries / no batching — each
// invocation is one named-pipe round trip; per the bridge-rpc-one-at-a-time
// memory we never parallelize bridge calls.
// ---------------------------------------------------------------------------

type CallState =
  | { kind: 'idle' }
  | { kind: 'pending' }
  | { kind: 'ok'; response: unknown }
  | { kind: 'error'; message: string };

function useBridgeCall() {
  const [state, setState] = useState<CallState>({ kind: 'idle' });
  const fire = async (method: string, params?: Record<string, unknown>) => {
    setState({ kind: 'pending' });
    try {
      const response = await adapterEngineRpc<unknown>(method, params);
      setState({ kind: 'ok', response });
    } catch (e) {
      setState({ kind: 'error', message: String(e) });
    }
  };
  return { state, fire };
}

function ResultBlock({ state }: { state: CallState }) {
  if (state.kind === 'idle') return null;
  if (state.kind === 'pending') {
    return <p className="mt-2 text-xs text-turd-cream-dim">Firing…</p>;
  }
  if (state.kind === 'error') {
    return (
      <p className="mt-2 break-words text-xs text-red-400">{state.message}</p>
    );
  }
  return (
    <pre className="mt-2 max-h-[20vh] overflow-auto rounded bg-turd-bg-deep/60 p-2 font-mono text-xs text-turd-cream">
      {JSON.stringify(state.response, null, 2)}
    </pre>
  );
}

function HandlerCard({
  title,
  description,
  children,
}: {
  title: string;
  description: string;
  children: React.ReactNode;
}) {
  return (
    <section className="rounded glass p-4">
      <h2 className="mb-1 font-display text-sm tracking-widest text-turd-mustard-bright">
        {title}
      </h2>
      <p className="mb-3 text-xs text-turd-cream-dim">{description}</p>
      {children}
    </section>
  );
}

function Input({
  label,
  value,
  onChange,
  placeholder,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
}) {
  return (
    <label className="flex flex-col gap-1">
      <span className="text-[10px] uppercase tracking-wider text-turd-cream-dim/60">
        {label}
      </span>
      <input
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        className="rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-2 py-1 text-sm text-turd-cream focus:border-turd-mustard-bright focus:outline-none"
      />
    </label>
  );
}

function FireButton({
  onClick,
  busy,
  disabled = false,
  children,
}: {
  onClick: () => void;
  busy: boolean;
  disabled?: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      onClick={onClick}
      disabled={busy || disabled}
      className="rounded bg-turd-mustard-bright px-3 py-1.5 text-sm font-medium text-turd-bg-deep transition-colors hover:bg-turd-mustard disabled:cursor-not-allowed disabled:opacity-40"
    >
      {busy ? 'Firing…' : children}
    </button>
  );
}

// In-app viewer for engine-rpc.log. Avoids the tauri-plugin-shell
// path-scope failure that the "Open in Notepad" button hit (LOCALAPPDATA
// paths fall outside the default scope). Reads the last N lines via a
// Rust command and renders them in a selectable <pre>.
function RpcLogViewer() {
  const [open, setOpen] = useState(false);
  const [tail, setTail] = useState<string>('');
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [path, setPath] = useState<string>('');

  const refresh = async () => {
    setLoading(true);
    setErr(null);
    try {
      const [t, p] = await Promise.all([
        invoke<string>('manager_engine_rpc_log_tail', { lines: 200 }),
        invoke<string>('manager_engine_rpc_log_path'),
      ]);
      setTail(t);
      setPath(p);
    } catch (e) {
      setErr(String(e));
    } finally {
      setLoading(false);
    }
  };

  const openExternally = async () => {
    try {
      await invoke('manager_open_in_default_app', { path });
    } catch (e) {
      setErr(String(e));
    }
  };

  useEffect(() => {
    if (open) void refresh();
  }, [open]);

  return (
    <section className="mb-4 rounded glass p-3">
      <div className="flex items-center gap-2">
        <button
          onClick={() => setOpen((v) => !v)}
          className="rounded bg-turd-bg-soft px-2 py-1 text-[11px] text-turd-cream hover:bg-turd-bg-soft/80"
        >
          {open ? 'Hide RPC log' : 'View RPC log'}
        </button>
        {open && (
          <>
            <button
              onClick={refresh}
              disabled={loading}
              className="rounded bg-turd-bg-soft px-2 py-1 text-[11px] text-turd-cream hover:bg-turd-bg-soft/80 disabled:opacity-40"
            >
              {loading ? 'Loading…' : 'Refresh'}
            </button>
            <button
              onClick={openExternally}
              className="rounded bg-turd-bg-soft px-2 py-1 text-[11px] text-turd-cream hover:bg-turd-bg-soft/80"
              title="Open in OS default app (Notepad)"
            >
              Open externally
            </button>
          </>
        )}
        <span className="ml-auto text-[10px] text-turd-cream-dim/60">
          Every Fire writes a JSON line to{' '}
          <code className="font-mono">{path || '%LOCALAPPDATA%/TurdMOD/engine-rpc.log'}</code>
        </span>
      </div>
      {open && (
        <div className="mt-2">
          {err && <p className="mb-2 text-xs text-red-400">{err}</p>}
          <pre className="max-h-[40vh] overflow-auto rounded bg-turd-bg-deep/60 p-2 font-mono text-[11px] text-turd-cream">
            {tail || (loading ? 'Loading…' : '— log is empty — fire a handler to populate —')}
          </pre>
        </div>
      )}
    </section>
  );
}

function PlayerPicker({
  value,
  onChange,
  players,
}: {
  value: string;
  onChange: (v: string) => void;
  players: string[];
}) {
  // If we have a roster, use a dropdown; if not, fall back to free-text
  // input so the page is still usable when getOnlinePlayers fails.
  if (players.length === 0) {
    return (
      <Input
        label="Player name (no roster loaded — type manually)"
        value={value}
        onChange={onChange}
        placeholder="YOUR_OWNER_NAME"
      />
    );
  }
  return (
    <label className="flex flex-col gap-1">
      <span className="text-[10px] uppercase tracking-wider text-turd-cream-dim/60">
        Player name
      </span>
      <select
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-2 py-1 text-sm text-turd-cream focus:border-turd-mustard-bright focus:outline-none"
      >
        <option value="">— pick a player —</option>
        {players.map((p) => (
          <option key={p} value={p}>
            {p}
          </option>
        ))}
      </select>
    </label>
  );
}

export function BridgeSmokePage() {
  // broadcastRaidBanner
  const raid = useBridgeCall();
  const [raidKind, setRaidKind] = useState<'concluded' | 'allowed' | 'end'>(
    'concluded',
  );
  const [raidSeconds, setRaidSeconds] = useState('60');

  // setFamePoints
  const fame = useBridgeCall();
  const [fameName, setFameName] = useState('');
  const [fameValue, setFameValue] = useState('1000');

  // setCurrencyBalance
  const currency = useBridgeCall();
  const [currName, setCurrName] = useState('');
  const [currType, setCurrType] = useState('0');
  const [currBalance, setCurrBalance] = useState('10000');

  // probeQuestHandlers
  const probe = useBridgeCall();

  // setEconomy
  const economy = useBridgeCall();
  const [econKind, setEconKind] = useState<'gold' | 'tradeable'>('gold');
  const [econDataVersion, setEconDataVersion] = useState('1');
  const [econValue, setEconValue] = useState('1.0');

  // listClassInstances
  const listInst = useBridgeCall();
  const [listPattern, setListPattern] = useState('RaidProtection');

  // dumpAdminCommands
  const adminDump = useBridgeCall();

  // kickPlayer (Wave 2 of scumpilot bridge-gap contract)
  const kick = useBridgeCall();
  const [kickName, setKickName] = useState('');
  const [kickReason, setKickReason] = useState('Kicked by admin');

  // setTimeOfDay (Wave 2)
  const time = useBridgeCall();
  const [timeHours, setTimeHours] = useState('12');

  // runHelloWorld (Phase C P1 verification)
  const hello = useBridgeCall();
  const [helloMessage, setHelloMessage] = useState('hello from pak');

  // Shared player roster — load once via getOnlinePlayers, pick names
  // from the dropdown for the fame/currency handlers. Avoids typo-driven
  // "player not found" round-trips.
  const [players, setPlayers] = useState<OnlinePlayer[]>([]);
  const [playersLoading, setPlayersLoading] = useState(false);
  const [playersError, setPlayersError] = useState<string | null>(null);

  const loadPlayers = async () => {
    setPlayersLoading(true);
    setPlayersError(null);
    try {
      const resp = await adapterEngineRpc<{ players?: OnlinePlayer[] } | OnlinePlayer[]>(
        'getOnlinePlayers',
      );
      const arr = Array.isArray(resp) ? resp : (resp?.players ?? []);
      setPlayers(arr);
    } catch (e) {
      setPlayersError(String(e));
    } finally {
      setPlayersLoading(false);
    }
  };

  useEffect(() => {
    // Best-effort: try to load on first mount; users can retry via the
    // Refresh button if it fails (server not yet up etc.).
    void loadPlayers();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const playerNames = players
    .map((p) => (p.name as string) ?? (p.playerName as string) ?? '')
    .filter(Boolean);

  return (
    <div>
      <h1 className="mb-2 font-display text-lg tracking-widest text-turd-mustard-bright">
        Bridge Smoke Tests
      </h1>
      <p className="mb-4 max-w-2xl text-xs text-turd-cream-dim">
        Exercise the four handlers shipped 2026-05-18. Run one at a time per{' '}
        <code className="font-mono">[[bridge-rpc-one-at-a-time]]</code> — the
        bridge serializes RPCs on the game thread. Watch the SCUM server logs
        in tandem to confirm in-game effect.
      </p>

      <EngineEventFirehose />

      <RpcLogViewer />


      <section className="mb-4 flex flex-wrap items-center gap-3 rounded glass p-3 text-xs">
        <span className="uppercase tracking-wider text-turd-cream-dim/60">
          Online players:
        </span>
        <span className="min-w-0 flex-1 break-words text-turd-cream">
          {playersLoading
            ? 'loading…'
            : playerNames.length === 0
              ? playersError ?? 'none'
              : playerNames.join(', ')}
        </span>
        <button
          onClick={loadPlayers}
          disabled={playersLoading}
          className="rounded bg-turd-bg-soft px-2 py-0.5 text-[10px] text-turd-cream hover:bg-turd-bg-soft/80 disabled:opacity-40"
        >
          Refresh
        </button>
      </section>

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <HandlerCard
          title="broadcastRaidBanner"
          description="Fires GlobalRaidProtectionManager NetMulticast_ShowRaid*Message. The two parameterless variants (concluded/allowed) plus the int-seconds 'end' variant."
        >
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <label className="flex flex-col gap-1">
              <span className="text-[10px] uppercase tracking-wider text-turd-cream-dim/60">
                Kind
              </span>
              <select
                value={raidKind}
                onChange={(e) =>
                  setRaidKind(e.target.value as 'concluded' | 'allowed' | 'end')
                }
                className="rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-2 py-1 text-sm text-turd-cream focus:border-turd-mustard-bright focus:outline-none"
              >
                <option value="concluded">concluded</option>
                <option value="allowed">allowed</option>
                <option value="end">end (with seconds)</option>
              </select>
            </label>
            <Input
              label="Seconds (end only)"
              value={raidSeconds}
              onChange={setRaidSeconds}
            />
          </div>
          <div className="mt-3">
            <FireButton
              busy={raid.state.kind === 'pending'}
              onClick={() =>
                raid.fire('broadcastRaidBanner', {
                  kind: raidKind,
                  seconds: raidSeconds,
                })
              }
            >
              Fire
            </FireButton>
          </div>
          <ResultBlock state={raid.state} />
        </HandlerCard>

        <HandlerCard
          title="setFamePoints"
          description="ConZPlayerController::SetFamePoints(float). Targets a connected player by display name."
        >
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <PlayerPicker
              value={fameName}
              onChange={setFameName}
              players={playerNames}
            />
            <Input label="Value" value={fameValue} onChange={setFameValue} />
          </div>
          <div className="mt-3">
            <FireButton
              busy={fame.state.kind === 'pending'}
              disabled={!fameName || !fameValue}
              onClick={() =>
                fame.fire('setFamePoints', { name: fameName, value: fameValue })
              }
            >
              Fire
            </FireButton>
          </div>
          <ResultBlock state={fame.state} />
        </HandlerCard>

        <HandlerCard
          title="setCurrencyBalance"
          description="ConZPlayerController::SetCurrencyBalanceRep(byte CurrencyType, int64 currencyBalance). CurrencyType is a byte enum — try 0 = Cash, 1 = Gold and confirm via Reflection DB."
        >
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
            <PlayerPicker
              value={currName}
              onChange={setCurrName}
              players={playerNames}
            />
            <Input
              label="Currency type"
              value={currType}
              onChange={setCurrType}
            />
            <Input
              label="Balance"
              value={currBalance}
              onChange={setCurrBalance}
            />
          </div>
          <div className="mt-3">
            <FireButton
              busy={currency.state.kind === 'pending'}
              disabled={!currName || !currBalance}
              onClick={() =>
                currency.fire('setCurrencyBalance', {
                  name: currName,
                  currencyType: currType,
                  balance: currBalance,
                })
              }
            >
              Fire
            </FireButton>
          </div>
          <ResultBlock state={currency.state} />
        </HandlerCard>

        <HandlerCard
          title="kickPlayer"
          description="ConZGameMode::KickPlayer(Player, reason). Wave 2 of scumpilot's bridge-gap contract. Direct primitive — bypasses the SCUM admin parser. v1: lookup by display name only; steamId-only path needs PS.UniqueId read."
        >
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <PlayerPicker
              value={kickName}
              onChange={setKickName}
              players={playerNames}
            />
            <Input label="Reason" value={kickReason} onChange={setKickReason} />
          </div>
          <div className="mt-3">
            <FireButton
              busy={kick.state.kind === 'pending'}
              disabled={!kickName}
              onClick={() =>
                kick.fire('kickPlayer', {
                  playerName: kickName,
                  reason: kickReason,
                })
              }
            >
              Fire
            </FireButton>
          </div>
          <ResultBlock state={kick.state} />
        </HandlerCard>

        <HandlerCard
          title="setTimeOfDay"
          description="Wave 2 — writes WeatherController2._timeOfDay (float, 0..24 hours) directly + fires NetMulticast_SendStateSnapshot. Watch the firehose for a `timeChanged` event landing as confirmation. 0 = midnight · 6 = sunrise · 12 = noon · 18 = sunset · 23 = late night."
        >
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <Input
              label="Hours (0..24, float ok)"
              value={timeHours}
              onChange={setTimeHours}
              placeholder="12"
            />
          </div>
          <div className="mt-3">
            <FireButton
              busy={time.state.kind === 'pending'}
              disabled={!timeHours}
              onClick={() =>
                time.fire('setTimeOfDay', {
                  hours: timeHours,
                })
              }
            >
              Fire
            </FireButton>
          </div>
          <ResultBlock state={time.state} />
        </HandlerCard>

        <HandlerCard
          title="runHelloWorld (Phase C P1)"
          description="Verifies the full pak pipeline. Resolves BP_HelloWorld from TurdMODHelloWorld_P.pak via GUObjectArray walk, finds its BroadcastHelloWorld(message:FString) UFunction, dispatches via ProcessEvent. ok:true here = every Phase C gate passed. Prerequisite: scripts/helloworld-pak/cook.ps1 + deploy.ps1 + restart engine with TURDMOD_PAK_BYPASS=1."
        >
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <Input
              label="Message"
              value={helloMessage}
              onChange={setHelloMessage}
              placeholder="hello from pak"
            />
          </div>
          <div className="mt-3">
            <FireButton
              busy={hello.state.kind === 'pending'}
              disabled={!helloMessage}
              onClick={() =>
                hello.fire('runHelloWorld', { message: helloMessage })
              }
            >
              Fire
            </FireButton>
          </div>
          <ResultBlock state={hello.state} />
        </HandlerCard>

        <HandlerCard
          title="probeQuestHandlers"
          description="Smoke-tests PlayerQuestComponent::Server_StartQuest with zero-init params. Tells us whether SCUM's Server_* UFunctions are reachable from our PE bypass. Check SCUM logs after firing — silent reject ≠ ProcessEvent error."
        >
          <FireButton
            busy={probe.state.kind === 'pending'}
            onClick={() => probe.fire('probeQuestHandlers')}
          >
            Probe
          </FireButton>
          <ResultBlock state={probe.state} />
        </HandlerCard>

        <HandlerCard
          title="setEconomy"
          description="ConZEconomyManager::NetMulticast_Update* family — server-wide price multiplier without restart. 'gold' = float gold-price master multiplier. 'tradeable' = int tradeable-price multiplier factor. dataVersion bumps on each change so clients accept the new value."
        >
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
            <label className="flex flex-col gap-1">
              <span className="text-[10px] uppercase tracking-wider text-turd-cream-dim/60">
                Kind
              </span>
              <select
                value={econKind}
                onChange={(e) =>
                  setEconKind(e.target.value as 'gold' | 'tradeable')
                }
                className="rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-2 py-1 text-sm text-turd-cream focus:border-turd-mustard-bright focus:outline-none"
              >
                <option value="gold">gold (float)</option>
                <option value="tradeable">tradeable (int)</option>
              </select>
            </label>
            <Input
              label="dataVersion"
              value={econDataVersion}
              onChange={setEconDataVersion}
            />
            <Input label="value" value={econValue} onChange={setEconValue} />
          </div>
          <div className="mt-3">
            <FireButton
              busy={economy.state.kind === 'pending'}
              disabled={!econValue}
              onClick={() =>
                economy.fire('setEconomy', {
                  kind: econKind,
                  dataVersion: econDataVersion,
                  value: econValue,
                })
              }
            >
              Fire
            </FireButton>
          </div>
          <ResultBlock state={economy.state} />
        </HandlerCard>

        <HandlerCard
          title="listClassInstances (diagnostic)"
          description="Scan GUObjectArray for non-CDO instances whose class name contains the pattern. Use when a 'no instance' error is unhelpful — discover what's actually spawned so we know what class to target."
        >
          <Input
            label="Pattern (substring)"
            value={listPattern}
            onChange={setListPattern}
            placeholder="RaidProtection / Economy / Bunker"
          />
          <div className="mt-3">
            <FireButton
              busy={listInst.state.kind === 'pending'}
              disabled={!listPattern}
              onClick={() =>
                listInst.fire('listClassInstances', { pattern: listPattern })
              }
            >
              Scan
            </FireButton>
          </div>
          <ResultBlock state={listInst.state} />
        </HandlerCard>

        <HandlerCard
          title="dumpAdminCommands (one-shot)"
          description="Walks every AdminCommand_* CDO and dumps argument metadata for all ~214 verbs (numRequired, dataType class per arg, completion source class, literal values for _Constant completions like Normal/Gold/Sunny). Run ONCE per SCUM game version. Use the Copy JSON button after it succeeds and paste it back to Claude — we'll save it as src/data/admin-commands-full.json and the Admin Commands page will use the authoritative shape instead of curated hints."
        >
          <div className="flex flex-wrap items-center gap-2">
            <FireButton
              busy={adminDump.state.kind === 'pending'}
              onClick={() => adminDump.fire('dumpAdminCommands')}
            >
              Run dump
            </FireButton>
            {adminDump.state.kind === 'ok' && (
              <CopyJsonButton payload={adminDump.state.response} />
            )}
          </div>
          {adminDump.state.kind === 'ok' && (() => {
            const data = adminDump.state.response as
              | { adminCommands?: number; emitted?: number; commands?: unknown[] }
              | undefined;
            const total = data?.adminCommands ?? 0;
            const emitted = data?.emitted ?? 0;
            const sample = (data?.commands ?? []).slice(0, 3);
            return (
              <div className="mt-2 text-[11px] text-turd-cream-dim">
                <p>
                  Scanned <span className="text-turd-cream">{total}</span> AdminCommand_* classes ·
                  {' '}emitted <span className="text-turd-cream">{emitted}</span>.
                </p>
                {sample.length > 0 && (
                  <pre className="mt-1 max-h-48 overflow-auto rounded bg-turd-bg-deep/60 p-2 font-mono text-[10px] text-turd-cream">
                    {JSON.stringify(sample, null, 2)}
                  </pre>
                )}
              </div>
            );
          })()}
          <ResultBlock state={adminDump.state} />
        </HandlerCard>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// EngineEventFirehose — live tail of every event the bridge pushes through
// the named-pipe broadcaster (chat, login, vehicleSpawned, weatherChanged,
// custom smoke ticks, etc). One row per event, newest at the top. Pause/
// resume + clear + per-kind filter chips. Bounded at 500 events in memory.
//
// Wire path: bridge calls turdmod_engine_emit_event(name, json) → C-ABI
// → server-loader EventBroadcaster → pipe frame → Manager engine_events.rs
// listener → Tauri `engine://event` → useEngineEvents() hook → here.
// ---------------------------------------------------------------------------

function EngineEventFirehose() {
  const [paused, setPaused] = useState(false);
  const [activeKinds, setActiveKinds] = useState<string[] | null>(null);
  const { events, countsByKind, totalSeen, clear } = useEngineEvents({
    paused,
    filter: activeKinds,
    bufferSize: 500,
  });

  const knownKinds = useMemo(
    () => Object.keys(countsByKind).sort(),
    [countsByKind],
  );

  // Auto-scroll to bottom on new events — matches the dump-management
  // log pane pattern. Smart pause when user scrolls up so they can read
  // older entries without being yanked back down.
  const logRef = useRef<HTMLDivElement | null>(null);
  const stickToBottomRef = useRef(true);
  useEffect(() => {
    if (logRef.current && stickToBottomRef.current) {
      logRef.current.scrollTop = logRef.current.scrollHeight;
    }
  }, [events.length]);
  const onScroll = () => {
    if (!logRef.current) return;
    const { scrollTop, scrollHeight, clientHeight } = logRef.current;
    // 32px slack — user is "at bottom" if within one row of it.
    stickToBottomRef.current = scrollHeight - scrollTop - clientHeight < 32;
  };

  const toggleKind = (kind: string) => {
    setActiveKinds((prev) => {
      if (prev === null) return [kind];
      if (prev.includes(kind)) {
        const next = prev.filter((k) => k !== kind);
        return next.length === 0 ? null : next;
      }
      return [...prev, kind];
    });
  };

  return (
    <section className="mb-5 rounded border border-cyan-400/40 bg-turd-bg-mid/40 p-4">
      <div className="mb-3 flex flex-wrap items-baseline justify-between gap-2">
        <div>
          <h2 className="font-display text-sm tracking-widest text-cyan-300">
            Engine Event Firehose
          </h2>
          <p className="mt-0.5 text-[11px] text-turd-cream-dim">
            Live tail of every <code className="font-mono">turdmod_engine_emit_event</code>{' '}
            push from the bridge. Auto-reconnects when the engine isn't running.
            Bounded at 500 events in memory.
          </p>
        </div>
        <div className="flex items-center gap-2 text-[11px] text-turd-cream-dim">
          <span>
            seen <span className="font-mono text-cyan-300">{totalSeen}</span>{' '}
            · buffered{' '}
            <span className="font-mono text-cyan-300">{events.length}</span>
          </span>
          <button
            type="button"
            onClick={() => setPaused((p) => !p)}
            className={`rounded border px-2 py-0.5 text-[11px] ${
              paused
                ? 'border-amber-400/60 bg-amber-400/15 text-amber-200'
                : 'border-cyan-400/40 bg-cyan-400/10 text-cyan-300 hover:bg-cyan-400/20'
            }`}
          >
            {paused ? '▶ Resume' : '❚❚ Pause'}
          </button>
          <button
            type="button"
            onClick={clear}
            disabled={events.length === 0}
            className="rounded border border-turd-bronze/40 bg-turd-bg-soft px-2 py-0.5 text-[11px] text-turd-cream hover:border-turd-bronze disabled:opacity-40"
          >
            Clear
          </button>
        </div>
      </div>

      {knownKinds.length > 0 && (
        <div className="mb-3 flex flex-wrap items-center gap-1.5 text-[10px]">
          <span className="uppercase tracking-wider text-turd-cream-dim/60">
            Filter:
          </span>
          <button
            type="button"
            onClick={() => setActiveKinds(null)}
            className={`rounded border px-2 py-0.5 ${
              activeKinds === null
                ? 'border-cyan-400 bg-cyan-400/20 text-cyan-200'
                : 'border-turd-bronze/30 bg-turd-bg-deep/40 text-turd-cream-dim hover:border-turd-bronze'
            }`}
          >
            all ({totalSeen})
          </button>
          {knownKinds.map((kind) => {
            const active = activeKinds?.includes(kind);
            return (
              <button
                key={kind}
                type="button"
                onClick={() => toggleKind(kind)}
                className={`rounded border px-2 py-0.5 ${
                  active
                    ? 'border-cyan-400 bg-cyan-400/20 text-cyan-200'
                    : 'border-turd-bronze/30 bg-turd-bg-deep/40 text-turd-cream-dim hover:border-turd-bronze'
                }`}
              >
                {kind} ({countsByKind[kind]})
              </button>
            );
          })}
        </div>
      )}

      <div
        ref={logRef}
        onScroll={onScroll}
        className="h-80 overflow-y-auto overflow-x-hidden rounded bg-turd-bg-deep/60 font-mono text-[11px]"
      >
        {events.length === 0 ? (
          <p className="p-3 text-turd-cream-dim/60">
            {paused
              ? 'Paused — resume to receive new events.'
              : 'No events yet. Bridge emits will appear here when GameServer.exe is running and the engine bridge is loaded.'}
          </p>
        ) : (
          <ul className="divide-y divide-turd-bronze/15">
            {events.map((evt, idx) => (
              <EngineEventRow key={`${evt.receivedAtMs}-${idx}`} evt={evt} />
            ))}
          </ul>
        )}
      </div>
    </section>
  );
}

function EngineEventRow({ evt }: { evt: EngineEvent }) {
  const [open, setOpen] = useState(false);
  const ts = useMemo(() => {
    const d = new Date(evt.receivedAtMs);
    const hh = String(d.getHours()).padStart(2, '0');
    const mm = String(d.getMinutes()).padStart(2, '0');
    const ss = String(d.getSeconds()).padStart(2, '0');
    const ms = String(d.getMilliseconds()).padStart(3, '0');
    return `${hh}:${mm}:${ss}.${ms}`;
  }, [evt.receivedAtMs]);

  // Compact one-liner for the row; full JSON in the expand.
  const summary = useMemo(() => {
    if (evt.data === null || evt.data === undefined) return '—';
    if (typeof evt.data !== 'object') return String(evt.data);
    try {
      const s = JSON.stringify(evt.data);
      return s.length > 140 ? `${s.slice(0, 140)}…` : s;
    } catch {
      return '<unserializable>';
    }
  }, [evt.data]);

  return (
    <li>
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        className="flex w-full items-baseline gap-2 px-3 py-1.5 text-left hover:bg-turd-bg-soft/40"
      >
        <span className="text-turd-cream-dim/70">{ts}</span>
        <span className="rounded bg-cyan-400/15 px-1.5 py-0.5 text-cyan-300">
          {evt.event}
        </span>
        <span className="flex-1 truncate text-turd-cream">{summary}</span>
        <span className="text-turd-cream-dim/40">{open ? '▾' : '▸'}</span>
      </button>
      {open && (
        <pre className="overflow-x-auto whitespace-pre-wrap break-all bg-turd-bg-deep px-3 pb-2 text-[10px] text-turd-cream">
          {JSON.stringify(evt.data, null, 2)}
        </pre>
      )}
    </li>
  );
}

// CopyJsonButton — small utility to copy a JSON payload to the
// clipboard so the user can paste it back to the assistant (which then
// commits it as a static asset).
function CopyJsonButton({ payload }: { payload: unknown }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      onClick={async () => {
        try {
          const text = typeof payload === 'string'
            ? payload
            : JSON.stringify(payload, null, 2);
          await navigator.clipboard.writeText(text);
          setCopied(true);
          setTimeout(() => setCopied(false), 1500);
        } catch (e) {
          console.warn('clipboard write failed', e);
        }
      }}
      className="rounded border border-turd-bronze/40 bg-turd-bg-soft px-3 py-1.5 text-[11px] text-turd-cream hover:border-turd-bronze"
    >
      {copied ? '✓ Copied JSON' : 'Copy JSON to clipboard'}
    </button>
  );
}
