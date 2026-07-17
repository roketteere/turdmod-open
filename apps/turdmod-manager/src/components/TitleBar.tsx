import { useEffect, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';

// ---------------------------------------------------------------------------
// Custom title bar — replaces native Windows chrome (tauri.conf.json sets
// decorations: false). Drag region covers most of the bar so the user
// can grab and move the window. Min / max / close call into the Tauri
// window API.
// ---------------------------------------------------------------------------

export function TitleBar() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    const win = getCurrentWindow();
    let mounted = true;
    let unlisten: (() => void) | null = null;

    win.isMaximized().then((v) => mounted && setMaximized(v));

    // Re-check on resize since the user might double-click to max/restore
    // without going through our buttons.
    win
      .onResized(async () => {
        if (!mounted) return;
        setMaximized(await win.isMaximized());
      })
      .then((u) => {
        unlisten = u;
      });

    return () => {
      mounted = false;
      if (unlisten) unlisten();
    };
  }, []);

  const win = getCurrentWindow();

  return (
    <header
      data-tauri-drag-region
      className="flex h-9 shrink-0 select-none items-center gap-3 border-b border-turd-bronze/30 bg-turd-bg-deep/80 px-3"
    >
      <span
        data-tauri-drag-region
        className="font-display text-xs text-turd-mustard-bright"
      >
        TurdMOD
      </span>
      <span
        data-tauri-drag-region
        className="text-[10px] uppercase tracking-wider text-turd-cream-dim/60"
      >
        — v{__APP_VERSION__}
      </span>
      <div data-tauri-drag-region className="flex-1" />

      <button
        onClick={() => win.minimize()}
        title="Minimize"
        className="flex h-9 w-10 items-center justify-center text-turd-cream-dim transition-colors hover:bg-turd-bg-soft/60 hover:text-turd-cream"
      >
        <svg width="12" height="12" viewBox="0 0 12 12">
          <rect x="2" y="5.5" width="8" height="1" fill="currentColor" />
        </svg>
      </button>

      <button
        onClick={() => win.toggleMaximize()}
        title={maximized ? 'Restore' : 'Maximize'}
        className="flex h-9 w-10 items-center justify-center text-turd-cream-dim transition-colors hover:bg-turd-bg-soft/60 hover:text-turd-cream"
      >
        {maximized ? (
          <svg width="12" height="12" viewBox="0 0 12 12">
            <rect x="3" y="3" width="6" height="6" fill="none" stroke="currentColor" strokeWidth="1" />
            <rect x="4.5" y="1.5" width="6" height="6" fill="none" stroke="currentColor" strokeWidth="1" />
          </svg>
        ) : (
          <svg width="12" height="12" viewBox="0 0 12 12">
            <rect x="2" y="2" width="8" height="8" fill="none" stroke="currentColor" strokeWidth="1" />
          </svg>
        )}
      </button>

      <button
        onClick={() => win.close()}
        title="Close"
        className="flex h-9 w-10 items-center justify-center text-turd-cream-dim transition-colors hover:bg-red-600 hover:text-white"
      >
        <svg width="12" height="12" viewBox="0 0 12 12">
          <line x1="2" y1="2" x2="10" y2="10" stroke="currentColor" strokeWidth="1.2" />
          <line x1="10" y1="2" x2="2" y2="10" stroke="currentColor" strokeWidth="1.2" />
        </svg>
      </button>
    </header>
  );
}
