// Inline Local/Remote segmented toggle, bound to the global engineHost store.
// Drop into any page header so the active server is visible + switchable
// right on the tab. Stays in sync with the sidebar dropdown (one store).
// Flipping invalidates all queries so live views refetch against the new host.

import { useQueryClient } from '@tanstack/react-query';
import { ENGINE_HOSTS, setEngineHost, useEngineHost } from '../lib/engineHost';

export function HostToggle({ className = '' }: { className?: string }) {
  const host = useEngineHost();
  const qc = useQueryClient();
  return (
    <div className={`flex shrink-0 overflow-hidden rounded border border-turd-bronze/40 ${className}`}>
      {ENGINE_HOSTS.map((h) => (
        <button
          key={h.value}
          type="button"
          onClick={() => {
            setEngineHost(h.value);
            void qc.invalidateQueries();
          }}
          title={`Target ${h.label}`}
          className={`px-3 py-1 text-[11px] font-medium transition-colors ${
            host === h.value
              ? 'bg-turd-mustard-bright text-turd-bg-deep'
              : 'bg-turd-bg-deep/60 text-turd-cream-dim hover:text-turd-cream'
          }`}
        >
          {h.label}
        </button>
      ))}
    </div>
  );
}
