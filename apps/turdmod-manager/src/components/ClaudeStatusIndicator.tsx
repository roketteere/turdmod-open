import { invoke } from '@tauri-apps/api/core';
import { useQuery } from '@tanstack/react-query';

type ClaudeStatusState = 'online' | 'idle' | 'offline';

type ClaudeStatus = {
  state: ClaudeStatusState;
  lastUpdateMs: number;
  idleForMs: number;
  projectDir: string | null;
  sessionId: string | null;
  logPath: string | null;
};

function describeIdle(ms: number): string {
  if (ms >= Number.MAX_SAFE_INTEGER / 2) return 'never';
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${s % 60}s`;
  const h = Math.floor(m / 60);
  return `${h}h ${m % 60}m`;
}

export function ClaudeStatusIndicator() {
  const { data } = useQuery<ClaudeStatus>({
    queryKey: ['claude-status'],
    queryFn: () => invoke<ClaudeStatus>('claude_status'),
    refetchInterval: 3_000,
    refetchIntervalInBackground: false,
  });

  if (!data) {
    return (
      <div className="inline-flex items-center gap-2 rounded border border-turd-bronze/40 bg-turd-bg-soft px-2 py-1">
        <span className="h-2 w-2 rounded-full bg-turd-cream-dim/30" />
        <span className="font-mono text-[10px] uppercase tracking-wider text-turd-cream-dim">
          Claude …
        </span>
      </div>
    );
  }

  const isOnline = data.state === 'online';
  const isIdle = data.state === 'idle';

  const ledClass = isOnline
    ? 'bg-turd-green animate-pulse'
    : isIdle
    ? 'bg-turd-mustard'
    : 'bg-turd-cream-dim/50';

  const labelColor = isOnline
    ? 'text-turd-green'
    : isIdle
    ? 'text-turd-mustard'
    : 'text-turd-cream-dim';

  const label = isOnline
    ? 'Claude online'
    : isIdle
    ? `Claude idle ${describeIdle(data.idleForMs)}`
    : 'Claude offline';

  const tooltip = data.projectDir
    ? `Project: ${data.projectDir}\nLast activity: ${describeIdle(data.idleForMs)} ago\nSession: ${
        data.sessionId ?? '—'
      }\nLog: ${data.logPath ?? '—'}`
    : 'No active Claude Code session detected on this machine.';

  return (
    <div
      className="inline-flex items-center gap-2 rounded border border-turd-bronze/40 bg-turd-bg-soft px-2 py-1"
      title={tooltip}
    >
      <span className={`h-2 w-2 rounded-full ${ledClass}`} />
      <span className={`font-mono text-[10px] uppercase tracking-wider ${labelColor}`}>
        {label}
      </span>
    </div>
  );
}
