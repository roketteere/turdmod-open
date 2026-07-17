import { useEffect, useMemo, useState } from 'react';
import { ServerList } from '../components/server-list';
import { ServerDetail } from '../components/server-detail';
import { useAddServer, useServers, useTestServer } from '../hooks/useServers';
import {
  TRANSPORT_LABELS,
  TRANSPORTS,
  defaultPortFor,
  type ServerProbeResult,
  type ServerProfileInput,
  type Transport,
} from '../lib/tauri-servers';

export function ServersPage() {
  const servers = useServers();
  const test = useTestServer();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [showAdd, setShowAdd] = useState(false);
  const [probes, setProbes] = useState<
    Record<string, ServerProbeResult | undefined>
  >({});

  useEffect(() => {
    if (!servers.data) return;
    if (selectedId && servers.data.some((s) => s.id === selectedId)) return;
    setSelectedId(servers.data[0]?.id ?? null);
  }, [servers.data, selectedId]);

  useEffect(() => {
    const id = test.variables;
    if (test.data && typeof id === 'string') {
      setProbes((prev) => ({ ...prev, [id]: test.data }));
    }
  }, [test.data, test.variables]);

  const selected = useMemo(
    () => servers.data?.find((s) => s.id === selectedId) ?? null,
    [servers.data, selectedId],
  );

  return (
    <div className="grid h-full grid-cols-[320px_1fr] gap-6">
      <div className="flex flex-col">
        <div>
          <h1 className="font-display text-2xl text-turd-cream">Servers</h1>
          <p className="mt-1 text-xs text-turd-cream-dim">
            Connect to your SCUM dedicated servers to push mods, view logs, and
            run admin commands remotely.
          </p>
        </div>
        <div className="mt-4 flex-1 overflow-hidden">
          <ServerList
            servers={servers.data ?? []}
            selectedId={selectedId}
            onSelect={setSelectedId}
            onAdd={() => setShowAdd(true)}
            probeStatus={probes}
          />
        </div>
      </div>
      <div className="overflow-hidden">
        {selected ? (
          <ServerDetail server={selected} />
        ) : (
          <div className="flex h-full items-center justify-center rounded-lg border border-dashed border-turd-bronze/30 bg-turd-bg-mid/30 p-10 text-center text-turd-cream-dim">
            <div>
              <p className="font-display text-lg text-turd-cream">
                Select or add a server
              </p>
              <p className="mt-2 text-xs">
                Once connected you can push installed mods to its mods folder,
                tail any of the 19 SCUM logs, or send RCON admin commands.
              </p>
            </div>
          </div>
        )}
      </div>

      {showAdd && <AddServerModal onClose={() => setShowAdd(false)} />}
    </div>
  );
}

function AddServerModal({ onClose }: { onClose: () => void }) {
  const add = useAddServer();
  const [transport, setTransport] = useState<Transport>('Sftp');
  const [name, setName] = useState('');
  const [host, setHost] = useState('');
  const [port, setPort] = useState<number>(22);
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [scumRoot, setScumRoot] = useState('/home/scum/scum-server');
  const [rconPort, setRconPort] = useState<number | ''>('');
  const [rconPassword, setRconPassword] = useState('');

  useEffect(() => {
    setPort(defaultPortFor(transport));
  }, [transport]);

  const onSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    const profile: ServerProfileInput = {
      name: name.trim() || `${host}:${port}`,
      host: host.trim(),
      port,
      transport,
      username: username.trim(),
      scumRoot: scumRoot.trim(),
      rconPort: typeof rconPort === 'number' && rconPort > 0 ? rconPort : null,
      rconPasswordRef: null,
      createdAt: 0,
    };
    const secretSsh = transport === 'RconOnly' ? null : password || null;
    const secretRcon = rconPassword || null;
    try {
      await add.mutateAsync({ profile, secretSsh, secretRcon });
      onClose();
    } catch {
      // error surfaced via add.error below
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-turd-bg-deep/80 p-6">
      <form
        onSubmit={onSubmit}
        className="w-full max-w-lg rounded-lg border border-turd-bronze/40 bg-turd-bg-mid p-6 shadow-xl"
      >
        <h2 className="font-display text-xl text-turd-cream">Add server</h2>
        <p className="mt-1 text-xs text-turd-cream-dim">
          Credentials are stored locally via Tauri's secure store, scoped to the
          server profile id.
        </p>

        <div className="mt-4 grid grid-cols-2 gap-3">
          <Field label="Name">
            <input
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              required
              className={inputCls}
            />
          </Field>
          <Field label="Transport">
            <select
              value={transport}
              onChange={(e) => setTransport(e.target.value as Transport)}
              className={inputCls}
            >
              {TRANSPORTS.map((t) => (
                <option key={t} value={t}>
                  {TRANSPORT_LABELS[t]}
                </option>
              ))}
            </select>
          </Field>
          <Field label="Host">
            <input
              type="text"
              value={host}
              onChange={(e) => setHost(e.target.value)}
              required
              className={inputCls}
              placeholder="play.example.com"
            />
          </Field>
          <Field label="Port">
            <input
              type="number"
              value={port}
              onChange={(e) => setPort(Number(e.target.value))}
              required
              min={1}
              max={65535}
              className={inputCls}
            />
          </Field>
          {transport !== 'RconOnly' && (
            <>
              <Field label="Username">
                <input
                  type="text"
                  value={username}
                  onChange={(e) => setUsername(e.target.value)}
                  required
                  className={inputCls}
                />
              </Field>
              <Field label="Password">
                <input
                  type="password"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  required
                  className={inputCls}
                />
              </Field>
              <Field label="SCUM root path" colSpan={2}>
                <input
                  type="text"
                  value={scumRoot}
                  onChange={(e) => setScumRoot(e.target.value)}
                  required
                  className={inputCls}
                  placeholder="/home/scum/scum-server"
                />
              </Field>
            </>
          )}
          <Field label="RCON port (optional)">
            <input
              type="number"
              value={rconPort}
              onChange={(e) =>
                setRconPort(
                  e.target.value === '' ? '' : Number(e.target.value),
                )
              }
              min={1}
              max={65535}
              className={inputCls}
              placeholder="27015"
            />
          </Field>
          <Field label="RCON password (optional)">
            <input
              type="password"
              value={rconPassword}
              onChange={(e) => setRconPassword(e.target.value)}
              className={inputCls}
            />
          </Field>
        </div>

        {add.isError && (
          <p className="mt-3 text-xs text-turd-red">
            Add failed: {add.error?.message}
          </p>
        )}

        <div className="mt-6 flex items-center justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="rounded border border-turd-bronze/40 bg-turd-bg-soft px-4 py-2 font-display text-xs uppercase tracking-wider text-turd-cream-dim hover:text-turd-cream"
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={add.isPending}
            className="rounded border border-turd-mustard bg-turd-bg-soft px-4 py-2 font-display text-xs uppercase tracking-wider text-turd-mustard-bright transition-colors hover:bg-turd-bronze/40 disabled:opacity-50"
          >
            {add.isPending ? 'Saving...' : 'Save'}
          </button>
        </div>
      </form>
    </div>
  );
}

function Field({
  label,
  children,
  colSpan,
}: {
  label: string;
  children: React.ReactNode;
  colSpan?: number;
}) {
  return (
    <label
      className={[
        'flex flex-col gap-1',
        colSpan === 2 ? 'col-span-2' : '',
      ].join(' ')}
    >
      <span className="font-display text-[10px] uppercase tracking-wider text-turd-cream-dim">
        {label}
      </span>
      {children}
    </label>
  );
}

const inputCls =
  'rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-2 font-mono text-sm text-turd-cream placeholder:text-turd-cream-dim/60 focus:border-turd-mustard focus:outline-none';
