import { useNavigate } from 'react-router-dom';
import { useCompanionStatus } from '../hooks/useCompanion';
import type { CompanionStatus } from '../lib/tauri-companion';

function statusLabel(s: CompanionStatus | undefined): {
  dot: string;
  text: string;
  color: string;
} {
  if (!s || s.kind === 'stopped') {
    return { dot: 'bg-turd-cream-dim/40', text: 'Stopped', color: 'text-turd-cream-dim' };
  }
  if (s.kind === 'starting') {
    return { dot: 'bg-turd-mustard animate-pulse', text: 'Starting', color: 'text-turd-mustard' };
  }
  if (s.kind === 'running') {
    return { dot: 'bg-turd-green', text: 'Running', color: 'text-turd-green' };
  }
  // crashed
  return { dot: 'bg-turd-red', text: 'Crashed', color: 'text-turd-red' };
}

export function CompanionStatusBadge() {
  const navigate = useNavigate();
  const query = useCompanionStatus();
  const { dot, text, color } = statusLabel(query.data);

  return (
    <button
      type="button"
      onClick={() => navigate('/engine')}
      title="Click to open Engine control"
      className="flex items-center gap-1.5 rounded px-2 py-1 transition-colors hover:bg-turd-bg-soft/60"
    >
      <span className={`inline-block h-2 w-2 rounded-full shrink-0 ${dot}`} />
      <span className={`font-mono text-[10px] uppercase tracking-wider ${color}`}>
        {text}
      </span>
    </button>
  );
}