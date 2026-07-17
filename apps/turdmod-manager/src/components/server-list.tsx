import type { ServerProfile } from '../lib/tauri-servers';
import { TRANSPORT_LABELS, testServer } from '../lib/tauri-servers';
import { useQueries } from '@tanstack/react-query';

interface ServerListProps {
  servers: ServerProfile[];
  selectedId: string | null;
  onSelect: (id: string) => void;
  onAdd: () => void;
  probeStatus: Record<
    string,
    { sftpOk: boolean; rconOk: boolean; latencyMs: number | null; error?: string | null } | undefined
  >;
}

type ServerLiveStatus =
  | { kind: 'probing' }
  | { kind: 'online'; latencyMs: number | null; sftpOk: boolean; rconOk: boolean }
  | { kind: 'auth-fail'; error: string }
  | { kind: 'offline'; error: string }
  | { kind: 'no-transport' };

const DOT_BY_KIND: Record<ServerLiveStatus['kind'], string> = {
  probing: 'bg-yellow-400 animate-pulse',
  online: 'bg-green-400',
  'auth-fail': 'bg-orange-400',
  offline: 'bg-red-500',
  'no-transport': 'bg-turd-cream-dim',
};

const LABEL_BY_KIND: Record<ServerLiveStatus['kind'], string> = {
  probing: 'Probing…',
  online: 'Online',
  'auth-fail': 'Auth fail',
  offline: 'Offline',
  'no-transport': 'Not configured',
};

function classifyProbe(
  probe: { sftpOk: boolean; rconOk: boolean; latencyMs: number | null; error: string | null } | undefined,
): ServerLiveStatus {
  if (!probe) return { kind: 'probing' };
  const err = (probe.error ?? '').toLowerCase();
  if (probe.sftpOk || probe.rconOk) {
    return {
      kind: 'online',
      latencyMs: probe.latencyMs,
      sftpOk: probe.sftpOk,
      rconOk: probe.rconOk,
    };
  }
  if (err.includes('auth') || err.includes('password') || err.includes('login')) {
    return { kind: 'auth-fail', error: probe.error ?? 'auth failed' };
  }
  if (!probe.error) return { kind: 'no-transport' };
  return { kind: 'offline', error: probe.error };
}

export function ServerList({
  servers,
  selectedId,
  onSelect,
  onAdd,
}: ServerListProps) {
  // Auto-poll every server's status. 20s interval is the sweet spot —
  // long enough that we don't hammer SSH/RCON, short enough that the
  // dot reacts within a reasonable window when a server bounces.
  const probes = useQueries({
    queries: servers.map((s) => ({
      queryKey: ['server-status', s.id],
      queryFn: () => testServer(s.id),
      refetchInterval: 20000,
      retry: false,
      staleTime: 10000,
    })),
  });

  return (
    <div className="flex h-full flex-col">
      <div className="mb-3 flex items-center justify-between">
        <h2 className="font-display text-sm uppercase tracking-widest text-turd-cream-dim">
          Connected Servers
        </h2>
        <button
          type="button"
          onClick={onAdd}
          className="rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-1 font-display text-xs uppercase tracking-wider text-turd-mustard-bright transition-colors hover:border-turd-mustard hover:text-turd-cream"
        >
          + Add server
        </button>
      </div>

      {servers.length === 0 ? (
        <div className="rounded-lg border border-dashed border-turd-bronze/30 bg-turd-bg-mid/30 p-6 text-center text-turd-cream-dim">
          <p className="font-display text-sm">No servers connected.</p>
          <p className="mt-1 text-xs">
            Add a SCUM dedicated server to push mods, view logs, and run RCON.
          </p>
        </div>
      ) : (
        <ul className="flex flex-col gap-2 overflow-auto pr-1">
          {servers.map((s, idx) => {
            const probe = probes[idx];
            const status = classifyProbe(probe?.data);
            const isSelected = s.id === selectedId;
            return (
              <li key={s.id}>
                <button
                  type="button"
                  onClick={() => onSelect(s.id)}
                  className={[
                    'w-full rounded-lg border px-3 py-3 text-left transition-colors',
                    isSelected
                      ? 'border-turd-mustard bg-turd-bg-soft'
                      : 'border-turd-bronze/30 bg-turd-bg-mid/40 hover:border-turd-bronze hover:bg-turd-bg-soft/60',
                  ].join(' ')}
                >
                  <div className="flex items-center justify-between gap-2">
                    <span className="truncate font-display text-sm text-turd-cream">
                      {s.name || 'Unnamed server'}
                    </span>
                    <StatusPill status={status} />
                  </div>
                  <div className="mt-1 flex items-center justify-between text-[11px] text-turd-cream-dim">
                    <span className="font-mono">
                      {s.host}:{s.port}
                    </span>
                    <span className="uppercase tracking-wider">
                      {TRANSPORT_LABELS[s.transport]}
                    </span>
                  </div>
                  {status.kind === 'online' && (
                    <div className="mt-1 flex gap-2 text-[10px] text-turd-cream-dim">
                      {status.latencyMs != null && (
                        <span>{status.latencyMs}ms</span>
                      )}
                      {status.sftpOk && <span className="text-green-300">SFTP</span>}
                      {status.rconOk && <span className="text-green-300">RCON</span>}
                    </div>
                  )}
                  {(status.kind === 'offline' || status.kind === 'auth-fail') && (
                    <p className="mt-1 truncate text-[10px] text-red-300/80" title={status.error}>
                      {status.error}
                    </p>
                  )}
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}

function StatusPill({ status }: { status: ServerLiveStatus }) {
  return (
    <span className="flex shrink-0 items-center gap-1 rounded-full border border-turd-bronze/30 bg-turd-bg-deep/50 px-2 py-0.5 text-[10px] text-turd-cream-dim">
      <span className={`inline-block h-2 w-2 rounded-full ${DOT_BY_KIND[status.kind]}`} />
      {LABEL_BY_KIND[status.kind]}
    </span>
  );
}
