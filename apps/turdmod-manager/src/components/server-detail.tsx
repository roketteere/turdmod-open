import { useState } from 'react';
import {
  useKnownLogs,
  usePushMod,
  useRcon,
  useRemoteMods,
  useRemoveRemoteMod,
  useRemoveServer,
  useTailLog,
  useTestServer,
} from '../hooks/useServers';
import { useInstalledMods } from '../hooks/useMods';
import type { ServerProfile } from '../lib/tauri-servers';
import { TRANSPORT_LABELS } from '../lib/tauri-servers';
import { AdminTab } from './server-admin/AdminTab';

interface ServerDetailProps {
  server: ServerProfile;
}

type Tab = 'mods' | 'logs' | 'rcon' | 'admin';

export function ServerDetail({ server }: ServerDetailProps) {
  const [tab, setTab] = useState<Tab>('mods');
  const test = useTestServer();
  const remove = useRemoveServer();

  return (
    <div className="flex h-full flex-col glass rounded-xl">
      <header className="flex items-start justify-between border-b border-turd-bronze/30 p-4">
        <div>
          <h2 className="font-display text-xl text-turd-cream">{server.name}</h2>
          <p className="mt-1 font-mono text-xs text-turd-cream-dim">
            {server.username}@{server.host}:{server.port} ·{' '}
            {TRANSPORT_LABELS[server.transport]}
          </p>
          <p className="mt-1 font-mono text-[10px] text-turd-cream-dim">
            SCUM root: {server.scumRoot}
          </p>
        </div>
        <div className="flex flex-col items-end gap-2">
          <button
            type="button"
            onClick={() => test.mutate(server.id)}
            disabled={test.isPending}
            className="rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-1 font-display text-xs uppercase tracking-wider text-turd-mustard-bright transition-colors hover:border-turd-mustard disabled:opacity-50"
          >
            {test.isPending ? 'Testing...' : 'Test connection'}
          </button>
          <button
            type="button"
            onClick={() => {
              if (
                window.confirm(
                  `Remove ${server.name}? Stored credentials will be deleted.`,
                )
              ) {
                remove.mutate(server.id);
              }
            }}
            className="rounded border border-turd-red/40 bg-turd-bg-soft px-3 py-1 font-display text-xs uppercase tracking-wider text-turd-red transition-colors hover:border-turd-red"
          >
            Remove
          </button>
        </div>
      </header>

      {test.data && (
        <div className="border-b border-turd-bronze/30 px-4 py-2 font-mono text-[11px] text-turd-cream-dim">
          {test.data.sftpOk ? 'File transport OK' : 'File transport DOWN'} ·{' '}
          {test.data.rconOk ? 'RCON OK' : 'RCON not verified'} ·{' '}
          {test.data.latencyMs != null ? `${test.data.latencyMs} ms` : 'no latency'}
          {test.data.error && (
            <span className="ml-2 text-turd-red">{test.data.error}</span>
          )}
        </div>
      )}

      <nav className="flex gap-1 border-b border-turd-bronze/30 px-3 pt-2">
        {(['mods', 'logs', 'rcon', 'admin'] as Tab[]).map((t) => (
          <button
            key={t}
            type="button"
            onClick={() => setTab(t)}
            className={[
              'rounded-t px-4 py-2 font-display text-xs uppercase tracking-wider transition-colors',
              tab === t
                ? 'bg-turd-bg-soft text-turd-mustard-bright'
                : 'text-turd-cream-dim hover:bg-turd-bg-soft/40 hover:text-turd-cream',
            ].join(' ')}
          >
            {t}
          </button>
        ))}
      </nav>

      <div className="flex-1 overflow-auto p-4">
        {tab === 'mods' && <ModsPanel server={server} />}
        {tab === 'logs' && <LogsPanel server={server} />}
        {tab === 'rcon' && <RconPanel server={server} />}
        {tab === 'admin' && <AdminTab serverId={server.id} />}
      </div>
    </div>
  );
}

function ModsPanel({ server }: { server: ServerProfile }) {
  const installed = useInstalledMods();
  const remote = useRemoteMods(server.id);
  const push = usePushMod();
  const removeRemote = useRemoveRemoteMod();
  const fileOpsDisabled = server.transport === 'RconOnly';

  if (fileOpsDisabled) {
    return (
      <p className="text-sm text-turd-cream-dim">
        This server is configured for RCON-only access. File transfers are not
        available; switch to SFTP or FTP to push mods.
      </p>
    );
  }

  return (
    <div className="grid gap-6 lg:grid-cols-2">
      <section>
        <h3 className="font-display text-sm uppercase tracking-widest text-turd-cream-dim">
          Installed locally
        </h3>
        <ul className="mt-2 flex flex-col gap-2">
          {(installed.data ?? []).map((m) => {
            const onRemote = remote.data?.some((r) => r.slug === m.slug);
            const pushing =
              push.isPending && push.variables?.slug === m.slug;
            return (
              <li
                key={m.slug}
                className="flex items-center justify-between rounded border border-turd-bronze/30 bg-turd-bg-soft/40 px-3 py-2"
              >
                <div>
                  <p className="font-display text-sm text-turd-cream">
                    {m.slug}
                  </p>
                  <p className="font-mono text-[10px] text-turd-cream-dim">
                    v{m.version}
                  </p>
                </div>
                <button
                  type="button"
                  disabled={pushing}
                  onClick={() => push.mutate({ id: server.id, slug: m.slug })}
                  className="rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-1 font-display text-xs uppercase tracking-wider text-turd-mustard-bright transition-colors hover:border-turd-mustard disabled:opacity-50"
                >
                  {pushing ? 'Pushing...' : onRemote ? 'Re-push' : 'Push to server'}
                </button>
              </li>
            );
          })}
          {(installed.data ?? []).length === 0 && (
            <li className="text-xs text-turd-cream-dim">
              Install a mod from the Browse tab first.
            </li>
          )}
        </ul>
        {push.isError && (
          <p className="mt-2 text-xs text-turd-red">
            Push failed: {push.error?.message}
          </p>
        )}
      </section>

      <section>
        <h3 className="font-display text-sm uppercase tracking-widest text-turd-cream-dim">
          On the server
        </h3>
        <ul className="mt-2 flex flex-col gap-2">
          {remote.isLoading && (
            <li className="text-xs text-turd-cream-dim">Loading...</li>
          )}
          {remote.error && (
            <li className="text-xs text-turd-red">
              Listing failed: {remote.error.message}
            </li>
          )}
          {(remote.data ?? []).map((m) => (
            <li
              key={m.slug}
              className="flex items-center justify-between rounded border border-turd-bronze/30 bg-turd-bg-soft/40 px-3 py-2"
            >
              <div>
                <p className="font-display text-sm text-turd-cream">{m.slug}</p>
                <p className="font-mono text-[10px] text-turd-cream-dim">
                  {formatBytes(m.sizeBytes)}
                </p>
              </div>
              <button
                type="button"
                onClick={() =>
                  removeRemote.mutate({ id: server.id, slug: m.slug })
                }
                className="rounded border border-turd-red/40 bg-turd-bg-soft px-3 py-1 font-display text-xs uppercase tracking-wider text-turd-red transition-colors hover:border-turd-red"
              >
                Remove from server
              </button>
            </li>
          ))}
          {remote.data && remote.data.length === 0 && (
            <li className="text-xs text-turd-cream-dim">
              No mods on the server yet.
            </li>
          )}
        </ul>
      </section>
    </div>
  );
}

function LogsPanel({ server }: { server: ServerProfile }) {
  const known = useKnownLogs();
  const [logName, setLogName] = useState('gameplay');
  const [lines, setLines] = useState(200);
  const tail = useTailLog({ id: server.id, log: logName, lines });

  if (server.transport === 'RconOnly') {
    return (
      <p className="text-sm text-turd-cream-dim">
        Log access requires SFTP or FTP transport.
      </p>
    );
  }

  return (
    <div className="flex h-full flex-col gap-3">
      <div className="flex items-center gap-2">
        <label className="font-display text-xs uppercase tracking-wider text-turd-cream-dim">
          Log
        </label>
        <select
          value={logName}
          onChange={(e) => setLogName(e.target.value)}
          className="rounded border border-turd-bronze/60 bg-turd-bg-soft px-2 py-1 font-mono text-xs text-turd-cream"
        >
          {(known.data ?? ['gameplay']).map((n) => (
            <option key={n} value={n}>
              {n}
            </option>
          ))}
        </select>
        <label className="ml-3 font-display text-xs uppercase tracking-wider text-turd-cream-dim">
          Lines
        </label>
        <input
          type="number"
          min={10}
          max={5000}
          value={lines}
          onChange={(e) => setLines(Number(e.target.value) || 200)}
          className="w-20 rounded border border-turd-bronze/60 bg-turd-bg-soft px-2 py-1 font-mono text-xs text-turd-cream"
        />
        <button
          type="button"
          onClick={() => tail.refetch()}
          disabled={tail.isFetching}
          className="ml-auto rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-1 font-display text-xs uppercase tracking-wider text-turd-mustard-bright disabled:opacity-50"
        >
          {tail.isFetching ? 'Fetching...' : 'Refresh'}
        </button>
      </div>
      <pre className="flex-1 overflow-auto rounded border border-turd-bronze/30 bg-turd-bg-deep p-3 font-mono text-[11px] leading-snug text-turd-cream">
        {tail.error && (
          <span className="text-turd-red">{tail.error.message}</span>
        )}
        {(tail.data ?? []).join('\n') ||
          (tail.isLoading ? 'Loading...' : 'No log lines.')}
      </pre>
    </div>
  );
}

function RconPanel({ server }: { server: ServerProfile }) {
  const [command, setCommand] = useState('');
  const [history, setHistory] = useState<{ cmd: string; resp: string }[]>([]);
  const rcon = useRcon();
  const disabled = !server.rconPort;

  const onSend = async () => {
    if (!command.trim()) return;
    try {
      const resp = await rcon.mutateAsync({ id: server.id, command });
      setHistory((h) => [...h, { cmd: command, resp }]);
      setCommand('');
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setHistory((h) => [...h, { cmd: command, resp: `ERROR: ${msg}` }]);
    }
  };

  if (disabled) {
    return (
      <p className="text-sm text-turd-cream-dim">
        This server has no RCON port configured. Edit the profile to enable
        RCON.
      </p>
    );
  }

  return (
    <div className="flex h-full flex-col gap-3">
      <div className="flex gap-2">
        <input
          type="text"
          value={command}
          onChange={(e) => setCommand(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') void onSend();
          }}
          placeholder="Announce Hello world"
          className="flex-1 rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-2 font-mono text-sm text-turd-cream placeholder:text-turd-cream-dim/60"
        />
        <button
          type="button"
          onClick={() => void onSend()}
          disabled={rcon.isPending}
          className="rounded border border-turd-bronze/60 bg-turd-bg-soft px-4 py-2 font-display text-xs uppercase tracking-wider text-turd-mustard-bright transition-colors hover:border-turd-mustard disabled:opacity-50"
        >
          {rcon.isPending ? 'Sending...' : 'Send'}
        </button>
      </div>
      <div className="flex-1 overflow-auto rounded border border-turd-bronze/30 bg-turd-bg-deep p-3 font-mono text-[11px] text-turd-cream">
        {history.length === 0 ? (
          <p className="text-turd-cream-dim">
            RCON responses will appear here.
          </p>
        ) : (
          history.map((entry, i) => (
            <div key={i} className="mb-2">
              <p className="text-turd-mustard-bright">&gt; {entry.cmd}</p>
              <pre className="whitespace-pre-wrap text-turd-cream">
                {entry.resp}
              </pre>
            </div>
          ))
        )}
      </div>
    </div>
  );
}

function formatBytes(b: number): string {
  if (b < 1024) return `${b} B`;
  if (b < 1024 * 1024) return `${(b / 1024).toFixed(1)} KB`;
  return `${(b / 1024 / 1024).toFixed(2)} MB`;
}

