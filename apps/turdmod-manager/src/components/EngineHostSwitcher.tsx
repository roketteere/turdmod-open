// Sidebar dropdown — picks which SCUM server every page talks to (Local box
// or remote production). Backs the module-level engineHost store, so all pages
// that call engineRpc() (chat console, monitor, admin commands, map, db…)
// route to the selected host. Flipping it invalidates all queries so live
// views refetch against the new server immediately.

import { useQueryClient } from '@tanstack/react-query';
import { ENGINE_HOSTS, setEngineHost, useEngineHost, type EngineHost } from '../lib/engineHost';

export function EngineHostSwitcher() {
  const host = useEngineHost();
  const qc = useQueryClient();

  return (
    <div className="px-2 pb-3">
      <p className="mb-1.5 font-mono text-[9px] uppercase tracking-widest text-turd-cream-dim/60">
        Server
      </p>
      <select
        value={host}
        onChange={(e) => {
          setEngineHost(e.target.value as EngineHost);
          // Force every live view to refetch against the newly-selected host.
          void qc.invalidateQueries();
        }}
        aria-label="Active server"
        title="Which SCUM server every page talks to"
        className="w-full rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-2 py-1.5 font-display text-[11px] uppercase tracking-wider text-turd-cream focus:border-turd-mustard-bright focus:outline-none"
      >
        {ENGINE_HOSTS.map((h) => (
          <option key={h.value} value={h.value}>
            {h.label}
          </option>
        ))}
      </select>
    </div>
  );
}
