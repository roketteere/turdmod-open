// Boot-time SCUM update banner.
//
// Calls dump_check_updates once at App mount via useDumpUpdateCheck.
// Renders a dismissible bar across the top of the layout when Steam's
// SCUM build differs from the latest extracted dump. Dismiss is
// per-session — refreshes on the next app launch.
//
// The Dump Management page (/dump-management) has the same info in its
// own status pane; this banner exists so admins notice without having
// to visit the page first.

import { Link } from 'react-router-dom';
import { useState } from 'react';
import { useDumpUpdateCheck } from '../hooks/useDumpStatus';

export function DumpUpdateBanner() {
  const [dismissed, setDismissed] = useState(false);
  const { data } = useDumpUpdateCheck();

  if (dismissed) return null;
  if (!data || !data.updateAvailable) return null;

  return (
    <div className="flex items-center justify-between gap-3 border-b border-turd-mustard/40 bg-turd-mustard/15 px-4 py-2 text-sm text-turd-mustard-bright">
      <span>
        ⚠ SCUM updated to build <strong>{data.steamBuild}</strong> — last
        extracted is <strong>v{data.extractedBuild ?? '?'}</strong>.
      </span>
      <div className="flex items-center gap-3">
        <Link
          to="/dump-management"
          className="rounded border border-turd-mustard/40 px-3 py-1 text-xs uppercase tracking-widest hover:bg-turd-mustard/20"
        >
          Open Dump Management
        </Link>
        <button
          onClick={() => setDismissed(true)}
          className="text-xs text-turd-mustard-bright/60 hover:text-turd-mustard-bright"
          aria-label="Dismiss"
        >
          ✕
        </button>
      </div>
    </div>
  );
}
