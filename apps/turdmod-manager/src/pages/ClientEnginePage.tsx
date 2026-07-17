import { type ReactElement } from 'react';
import { Link } from 'react-router-dom';
import { useLoaderStatus } from '../hooks/useLoaderStatus';
import { useDetectedInstalls } from '../hooks/useTarget';
import type { LoaderPing } from '../lib/tauri-loader';

function LoaderStatusPill({ ping, isError }: { ping: LoaderPing | undefined; isError: boolean }) {
  if (isError || !ping) {
    return (
      <div className="inline-flex items-center gap-2 rounded-full border border-turd-bronze/30 bg-turd-bg-soft px-4 py-2">
        <span className="h-3 w-3 rounded-full bg-turd-cream-dim/30" />
        <span className="font-display text-sm text-turd-cream-dim">Loader offline</span>
      </div>
    );
  }
  return (
    <div className="inline-flex items-center gap-2 rounded-full border border-turd-green/30 bg-turd-bg-soft px-4 py-2">
      <span className="h-3 w-3 rounded-full bg-turd-green" />
      <span className="font-display text-sm text-turd-green">
        Loader online — v{ping.version} · PID {ping.pid}
      </span>
    </div>
  );
}

function InfoRow({ label, value }: { label: string; value: string | ReactElement }) {
  return (
    <div className="flex items-baseline gap-3 py-1">
      <span className="w-40 shrink-0 font-mono text-[10px] uppercase tracking-wider text-turd-cream-dim">
        {label}
      </span>
      <span className="font-mono text-xs text-turd-cream">{value}</span>
    </div>
  );
}

export function ClientEnginePage() {
  const loaderStatus = useLoaderStatus();
  const detected = useDetectedInstalls();
  const gamePath = detected.data?.game ?? null;

  const isOnline = loaderStatus.isSuccess && !!loaderStatus.data;

  return (
    <div className="flex flex-col gap-6">
      <header>
        <p className="font-display text-xs tracking-[0.3em] text-turd-mustard">TurdMOD</p>
        <h1 className="mt-1 font-display text-3xl text-turd-cream">Client Engine</h1>
        <p className="mt-1 text-xs text-turd-cream-dim">
          Manage the in-game client mod (turdmod-loader). Required for custom UI overlays
          and pak-based widget rendering on the player's screen.
        </p>
      </header>

      <section className="glass rounded-xl p-5">
        <h2 className="mb-4 font-display text-lg text-turd-cream">Loader Status</h2>
        <LoaderStatusPill ping={loaderStatus.data} isError={loaderStatus.isError} />

        <div className="mt-4 border-t border-turd-bronze/20 pt-4">
          <InfoRow
            label="Game Install"
            value={gamePath ?? 'Not detected'}
          />
          <InfoRow
            label="Loader DLL"
            value={gamePath ? `${gamePath}\\Binaries\\Win64\\dxgi.dll` : 'N/A'}
          />
          <InfoRow
            label="Pipe"
            value={isOnline ? '\\\\.\\pipe\\turdmod-loader' : 'Not connected'}
          />
          <InfoRow
            label="Pak Bypass"
            value="Not installed (V2)"
          />
        </div>
      </section>

      <section className="glass rounded-xl p-5">
        <h2 className="mb-4 font-display text-lg text-turd-cream">Quick Actions</h2>
        <div className="flex flex-wrap gap-3">
          <Link
            to="/custom-panels"
            className="rounded border border-turd-mustard/60 bg-turd-mustard/20 px-4 py-2 font-display text-xs uppercase tracking-wider text-turd-mustard-bright transition-colors hover:bg-turd-mustard/30"
          >
            Custom Panels
          </Link>
          <Link
            to="/gui-builder"
            className="rounded border border-turd-green/60 bg-turd-green/20 px-4 py-2 font-display text-xs uppercase tracking-wider text-turd-green transition-colors hover:bg-turd-green/30"
          >
            GUI Builder
          </Link>
        </div>
      </section>

      <section className="rounded-lg border border-turd-bronze/30 bg-turd-bg-deep p-5">
        <h2 className="mb-3 font-display text-lg text-turd-cream">How It Works</h2>
        <div className="space-y-2 font-mono text-[11px] text-turd-cream-dim leading-relaxed">
          <p>
            <span className="text-turd-mustard">ImGui path</span> — turdmod-loader hooks the game's DX11 Present call
            and renders custom panels over the game frame. No paks needed. Works now when the loader is installed.
          </p>
          <p>
            <span className="text-turd-green">UMG path</span> — custom UE4 widget Blueprints in a pak, rendered natively
            by the engine. Requires pak bypass on both server and client. Server bypass is live; client bypass is V2.
          </p>
          <p>
            <span className="text-turd-cream">Open source client</span> — the client mod (loader + pak) contains no
            proprietary engine code. It can be distributed freely; only the server engine is paid.
          </p>
        </div>
      </section>
    </div>
  );
}
