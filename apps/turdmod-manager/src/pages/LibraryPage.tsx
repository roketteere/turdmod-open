import { Link } from 'react-router-dom';
import { useMemo, useState } from 'react';
import {
  useInstallMod,
  useInstalledMods,
  useMods,
  useUninstallMod,
} from '../hooks/useMods';
import {
  useModState,
  useSetModFrozen,
  useModManifest,
  useModConfig,
  useWriteModConfig,
} from '../hooks/useModState';
import { useCompanionStatus } from '../hooks/useCompanion';
import { ConfigModal } from '../components/ConfigModal';
import type { InstalledMod } from '../lib/tauri';
import type { ModConfig } from '../lib/tauri-mod-state';

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

function formatDate(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toISOString().slice(0, 10);
}

export function LibraryPage() {
  const installedQuery = useInstalledMods();
  const catalogQuery = useMods({});
  const modStateQuery = useModState();
  const uninstall = useUninstallMod();
  const install = useInstallMod();
  const companionStatus = useCompanionStatus();

  const [configuringSlug, setConfiguringSlug] = useState<string | null>(null);

  const installed = installedQuery.data ?? [];
  const frozenSlugs = new Set(modStateQuery.data?.frozen ?? []);

  const catalogBySlug = useMemo(() => {
    const map = new Map<string, NonNullable<typeof catalogQuery.data>[number]>();
    for (const m of catalogQuery.data ?? []) map.set(m.slug, m);
    return map;
  }, [catalogQuery.data]);

  const updatable = installed.filter((i) => {
    const latest = catalogBySlug.get(i.slug);
    return latest && latest.version && latest.version !== i.version;
  });

  const updateAll = () => {
    for (const i of updatable) {
      install.mutate({ slug: i.slug });
    }
  };

  const companionRunning =
    companionStatus.data?.kind === 'running' ||
    companionStatus.data?.kind === 'starting';

  return (
    <div className="space-y-6">
      <header className="flex flex-wrap items-end justify-between gap-3">
        <div>
          <p className="font-display text-xs tracking-[0.3em] text-turd-mustard">
            Local
          </p>
          <h1 className="mt-1 font-display text-3xl text-turd-cream">Library</h1>
          <p className="mt-1 text-sm text-turd-cream-dim">
            Mods installed on this machine.
          </p>
        </div>
        {updatable.length > 0 && (
          <button
            type="button"
            onClick={updateAll}
            disabled={install.isPending}
            className="rounded bg-turd-mustard px-4 py-2 text-sm font-medium text-turd-bg-deep transition-colors hover:bg-turd-mustard-bright disabled:cursor-not-allowed disabled:opacity-60"
          >
            Update all ({updatable.length})
          </button>
        )}
      </header>

      {companionRunning && frozenSlugs.size > 0 && (
        <div className="rounded-lg border border-turd-mustard/30 bg-turd-bg-mid/40 px-4 py-3 text-sm text-turd-mustard">
          Freeze changes take effect on the next companion restart.
        </div>
      )}

      {installedQuery.isLoading && (
        <p className="py-12 text-center text-sm text-turd-cream-dim">Loading…</p>
      )}

      {installedQuery.isError && (
        <div className="rounded-lg border border-turd-red/40 bg-turd-bg-mid/40 p-6">
          <p className="text-sm text-turd-red">
            Couldn't read the local library: {installedQuery.error.message}
          </p>
        </div>
      )}

      {installedQuery.isSuccess && installed.length === 0 && (
        <div className="glass rounded-xl p-8 text-center">
          <p className="font-display text-lg text-turd-cream">
            Nothing installed yet
          </p>
          <p className="mt-2 text-sm text-turd-cream-dim">
            Pick a mod from the catalog to get started.
          </p>
          <Link
            to="/"
            className="mt-4 inline-block rounded bg-turd-mustard px-4 py-2 text-sm font-medium text-turd-bg-deep transition-colors hover:bg-turd-mustard-bright"
          >
            Browse mods
          </Link>
        </div>
      )}

      {installedQuery.isSuccess && installed.length > 0 && (
        <ul className="space-y-3">
          {installed.map((mod) => (
            <LibraryRow
              key={mod.slug}
              mod={mod}
              latestVersion={catalogBySlug.get(mod.slug)?.version ?? null}
              frozen={frozenSlugs.has(mod.slug)}
              onUninstall={() => uninstall.mutate(mod.slug)}
              onUpdate={() => install.mutate({ slug: mod.slug })}
              onConfigure={() => setConfiguringSlug(mod.slug)}
              busy={uninstall.isPending || install.isPending}
            />
          ))}
        </ul>
      )}

      {configuringSlug && (
        <ConfigModalConnector
          slug={configuringSlug}
          onClose={() => setConfiguringSlug(null)}
        />
      )}
    </div>
  );
}

function ConfigModalConnector({
  slug,
  onClose,
}: {
  slug: string;
  onClose: () => void;
}) {
  const manifestQuery = useModManifest(slug);
  const configQuery = useModConfig(slug);
  const writeConfig = useWriteModConfig(slug);

  const schema = manifestQuery.data?.configSchema ?? [];
  const modName = manifestQuery.data?.name ?? slug;
  const initialConfig = configQuery.data ?? {};

  if (manifestQuery.isLoading || configQuery.isLoading) {
    return null;
  }

  if (schema.length === 0) {
    return null;
  }

  function handleSave(config: ModConfig) {
    writeConfig.mutate(config, { onSuccess: onClose });
  }

  return (
    <ConfigModal
      modName={modName}
      slug={slug}
      schema={schema}
      initialConfig={initialConfig}
      onSave={handleSave}
      onClose={onClose}
      saving={writeConfig.isPending}
    />
  );
}

function LibraryRow({
  mod,
  latestVersion,
  frozen,
  onUninstall,
  onUpdate,
  onConfigure,
  busy,
}: {
  mod: InstalledMod;
  latestVersion: string | null;
  frozen: boolean;
  onUninstall: () => void;
  onUpdate: () => void;
  onConfigure: () => void;
  busy: boolean;
}) {
  const hasUpdate = latestVersion && latestVersion !== mod.version;
  const setFrozen = useSetModFrozen();
  const manifestQuery = useModManifest(mod.slug);
  const hasConfig = (manifestQuery.data?.configSchema?.length ?? 0) > 0;

  const toggleFrozen = () => {
    setFrozen.mutate({ slug: mod.slug, frozen: !frozen });
  };

  return (
    <li
      className={`glass rounded-xl p-5 transition-opacity ${frozen ? 'opacity-60' : ''}`}
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <Link
              to={`/mods/${mod.slug}`}
              className="font-display text-lg text-turd-cream hover:text-turd-mustard-bright"
            >
              {mod.name ?? mod.slug}
            </Link>
            {frozen && (
              <span className="rounded bg-turd-bg-soft px-2 py-0.5 font-mono text-[10px] uppercase tracking-wider text-turd-cream-dim">
                Frozen
              </span>
            )}
          </div>
          <div className="mt-1 flex flex-wrap items-center gap-3 font-mono text-[10px] text-turd-cream-dim">
            <span className="text-turd-mustard">v{mod.version}</span>
            {hasUpdate && (
              <span className="text-turd-mustard-bright">
                → v{latestVersion} available
              </span>
            )}
            <span>installed {formatDate(mod.installedAt)}</span>
            <span>{formatBytes(mod.sizeBytes)}</span>
          </div>
          <p className="mt-2 break-all font-mono text-[10px] text-turd-cream-dim">
            {mod.installPath}
          </p>
        </div>
        <div className="flex shrink-0 flex-wrap items-center gap-2">
          <button
            type="button"
            onClick={toggleFrozen}
            disabled={setFrozen.isPending}
            aria-pressed={frozen}
            aria-label={frozen ? `Unfreeze ${mod.name ?? mod.slug}` : `Freeze ${mod.name ?? mod.slug}`}
            title={frozen ? 'Unfreeze — mod will load on next companion start' : 'Freeze — mod will be skipped on next companion start'}
            className={`rounded border px-3 py-1.5 text-sm transition-colors disabled:cursor-not-allowed disabled:opacity-60 ${
              frozen
                ? 'border-turd-mustard/60 text-turd-mustard hover:border-turd-mustard hover:bg-turd-mustard/10'
                : 'border-turd-bronze/60 text-turd-cream-dim hover:border-turd-mustard/60 hover:text-turd-mustard'
            }`}
          >
            {frozen ? 'Unfreeze' : 'Freeze'}
          </button>

          {hasConfig && (
            <button
              type="button"
              onClick={onConfigure}
              className="rounded border border-turd-bronze px-3 py-1.5 text-sm text-turd-cream transition-colors hover:border-turd-mustard hover:text-turd-mustard"
            >
              Configure
            </button>
          )}

          {hasUpdate && (
            <button
              type="button"
              onClick={onUpdate}
              disabled={busy}
              className="rounded bg-turd-mustard px-4 py-1.5 text-sm font-medium text-turd-bg-deep transition-colors hover:bg-turd-mustard-bright disabled:cursor-not-allowed disabled:opacity-60"
            >
              Update
            </button>
          )}
          <button
            type="button"
            onClick={onUninstall}
            disabled={busy}
            className="rounded border border-turd-bronze px-4 py-1.5 text-sm text-turd-cream transition-colors hover:border-turd-red hover:text-turd-red disabled:cursor-not-allowed disabled:opacity-60"
          >
            Uninstall
          </button>
        </div>
      </div>
    </li>
  );
}
