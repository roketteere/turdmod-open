import { useState, type ReactNode } from 'react';
import { useInstallMod, useUninstallMod } from '../hooks/useMods';
import type { InstallProgress, InstalledMod } from '../lib/tauri';

type InstallState = 'not-installed' | 'installed-current' | 'installed-old';

export interface InstallButtonProps {
  slug: string;
  latestVersion?: string;
  installed?: InstalledMod | null;
  variant?: 'compact' | 'full';
}

function deriveState(
  latestVersion: string | undefined,
  installed: InstalledMod | null | undefined,
): InstallState {
  if (!installed) return 'not-installed';
  // Unknown latest — don't claim there's an update; treat as current.
  if (!latestVersion) return 'installed-current';
  if (installed.version === latestVersion) return 'installed-current';
  return 'installed-old';
}

function progressLabel(p: InstallProgress): string {
  switch (p.kind) {
    case 'Downloading':
      return 'Downloading';
    case 'Verifying':
      return 'Verifying';
    case 'Extracting':
      return 'Extracting';
    case 'Done':
      return 'Done';
  }
}

function progressPercent(p: InstallProgress): number {
  if (p.kind === 'Downloading') {
    if (!p.total || p.total <= 0) return 0;
    return Math.min(100, Math.floor((p.received / p.total) * 100));
  }
  if (p.kind === 'Verifying') return 95;
  if (p.kind === 'Extracting') return 98;
  return 100;
}

export function InstallButton({
  slug,
  latestVersion,
  installed,
  variant = 'compact',
}: InstallButtonProps) {
  const install = useInstallMod();
  const uninstall = useUninstallMod();
  const [progress, setProgress] = useState<InstallProgress | null>(null);
  const state = deriveState(latestVersion, installed);

  const isBusy = install.isPending || uninstall.isPending;

  const triggerInstall = () => {
    setProgress(null);
    install.mutate(
      {
        slug,
        onProgress: (p) => setProgress(p),
      },
      {
        onSettled: () => {
          setProgress(null);
        },
      },
    );
  };

  const triggerUninstall = () => uninstall.mutate(slug);

  if (install.isPending && progress) {
    const pct = progressPercent(progress);
    return (
      <div className="w-full">
        <div className="mb-1 flex items-center justify-between font-mono text-[10px] uppercase tracking-wider text-turd-cream-dim">
          <span>{progressLabel(progress)}</span>
          <span>{pct}%</span>
        </div>
        <div className="h-2 w-full overflow-hidden rounded bg-turd-bg-soft">
          <div
            className="h-full bg-turd-mustard transition-all"
            style={{ width: `${pct}%` }}
          />
        </div>
      </div>
    );
  }

  if (install.isError) {
    return (
      <div className="space-y-2">
        <p className="text-xs text-turd-red">
          Install failed: {install.error.message}
        </p>
        <PrimaryButton onClick={triggerInstall} variant={variant}>
          Retry install
        </PrimaryButton>
      </div>
    );
  }

  if (state === 'not-installed') {
    return (
      <PrimaryButton onClick={triggerInstall} variant={variant} disabled={isBusy}>
        Install
      </PrimaryButton>
    );
  }

  if (state === 'installed-old') {
    return (
      <div className={variant === 'full' ? 'space-y-2' : 'flex items-center gap-2'}>
        <span className="rounded bg-turd-mustard/20 px-2 py-0.5 font-mono text-[10px] uppercase tracking-wider text-turd-mustard">
          Installed v{installed?.version}
        </span>
        <PrimaryButton onClick={triggerInstall} variant={variant} disabled={isBusy}>
          Update to v{latestVersion}
        </PrimaryButton>
        <SecondaryButton onClick={triggerUninstall} variant={variant} disabled={isBusy}>
          Uninstall
        </SecondaryButton>
      </div>
    );
  }

  return (
    <div className={variant === 'full' ? 'space-y-2' : 'flex items-center gap-2'}>
      <span className="rounded bg-turd-green/20 px-2 py-0.5 font-mono text-[10px] uppercase tracking-wider text-turd-green">
        Installed
      </span>
      <SecondaryButton onClick={triggerInstall} variant={variant} disabled={isBusy}>
        Reinstall
      </SecondaryButton>
      <SecondaryButton onClick={triggerUninstall} variant={variant} disabled={isBusy}>
        Uninstall
      </SecondaryButton>
    </div>
  );
}

function PrimaryButton({
  children,
  onClick,
  variant,
  disabled,
}: {
  children: ReactNode;
  onClick: () => void;
  variant: 'compact' | 'full';
  disabled?: boolean;
}) {
  const sizing =
    variant === 'full'
      ? 'w-full px-5 py-3 text-base'
      : 'px-4 py-1.5 text-sm';
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className={`rounded bg-turd-mustard ${sizing} font-medium text-turd-bg-deep transition-colors hover:bg-turd-mustard-bright disabled:cursor-not-allowed disabled:opacity-60`}
    >
      {children}
    </button>
  );
}

function SecondaryButton({
  children,
  onClick,
  variant,
  disabled,
}: {
  children: ReactNode;
  onClick: () => void;
  variant: 'compact' | 'full';
  disabled?: boolean;
}) {
  const sizing =
    variant === 'full'
      ? 'w-full px-5 py-3 text-base'
      : 'px-4 py-1.5 text-sm';
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className={`rounded border border-turd-bronze ${sizing} text-turd-cream transition-colors hover:border-turd-mustard-bright hover:text-turd-mustard-bright disabled:cursor-not-allowed disabled:opacity-60`}
    >
      {children}
    </button>
  );
}
