import { useState, useEffect, type MouseEvent as ReactMouseEvent, type ReactNode } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import {
  detectScum,
  getInstallPath,
  setInstallPath,
  type DetectResult,
} from '../lib/tauri';
import { useSettings, useUpdateSettings } from '../hooks/useSettings';
import { useCompanionStatus, useCompanionRestart } from '../hooks/useCompanion';
import { settingsTestLogsDir, settingsTestRcon } from '../lib/tauri-settings';

const APP_VERSION = '0.0.1';
const BUILD_DATE = '2026-05-10';

interface InstallState {
  detected: DetectResult | null;
  override: string | null;
}

async function loadInstallState(): Promise<InstallState> {
  const [detected, override] = await Promise.all([
    detectScum().catch(() => null),
    getInstallPath().catch(() => null),
  ]);
  return { detected, override };
}

const inputCls =
  'rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-2 font-mono text-sm text-turd-cream placeholder:text-turd-cream-dim/60 focus:border-turd-mustard focus:outline-none w-full';

export function SettingsPage() {
  const qc = useQueryClient();
  const installQuery = useQuery<InstallState, Error>({
    queryKey: ['install-state'],
    queryFn: loadInstallState,
  });
  const [pickError, setPickError] = useState<string | null>(null);

  const { data: settings } = useSettings();
  const updateSettings = useUpdateSettings();
  const statusQuery = useCompanionStatus();
  const restartCompanion = useCompanionRestart();

  // --- Server Logs form state ---
  const [logsDir, setLogsDir] = useState('');
  const [logsDirTest, setLogsDirTest] = useState<{ exists: boolean; hasLogFiles: boolean } | null>(null);
  const [logsDirTesting, setLogsDirTesting] = useState(false);

  // --- RCON form state ---
  const [rconHost, setRconHost] = useState('');
  const [rconPort, setRconPort] = useState('27015');
  const [rconPassword, setRconPassword] = useState('');
  const [rconTest, setRconTest] = useState<{ ok: boolean; error: string | null } | null>(null);
  const [rconTesting, setRconTesting] = useState(false);

  // --- Discord form state ---
  const [discordWebhook, setDiscordWebhook] = useState('');
  const [discordTesting, setDiscordTesting] = useState(false);
  const [discordTestResult, setDiscordTestResult] = useState<string | null>(null);

  // --- Toast ---
  const [toast, setToast] = useState<string | null>(null);

  // Sync form state from loaded settings.
  useEffect(() => {
    if (!settings) return;
    setLogsDir(settings.scumServerLogsDir ?? '');
    setRconHost(settings.rconHost ?? '');
    setRconPort(settings.rconPort?.toString() ?? '27015');
    setDiscordWebhook(settings.discordWebhook ?? '');
    // Never pre-fill the password field — user must type a new one to change it.
  }, [settings]);

  const data = installQuery.data;
  const detected = data?.detected ?? null;
  const override = data?.override ?? null;
  const effectivePath = override ?? detected?.active ?? null;
  const battleyeBlocked = detected?.battleyeDisabled === false;
  const modsDir =
    detected?.modsDir ??
    (effectivePath
      ? `${effectivePath}\\SCUM\\Binaries\\Win64\\turdmod-mods\\`
      : null);

  const pickFolder = async () => {
    setPickError(null);
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const picked = await open({
        directory: true,
        multiple: false,
        title: 'Select your SCUM install folder',
      });
      if (typeof picked === 'string') {
        await setInstallPath(picked);
        await qc.invalidateQueries({ queryKey: ['install-state'] });
      }
    } catch (e) {
      setPickError(e instanceof Error ? e.message : String(e));
    }
  };

  const pickLogsFolder = async () => {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const picked = await open({
        directory: true,
        multiple: false,
        title: 'Select SCUM server logs directory',
      });
      if (typeof picked === 'string') {
        setLogsDir(picked);
        setLogsDirTest(null);
      }
    } catch {
      // user cancelled
    }
  };

  const testLogsDir = async () => {
    if (!logsDir.trim()) return;
    setLogsDirTesting(true);
    setLogsDirTest(null);
    try {
      const result = await settingsTestLogsDir(logsDir.trim());
      setLogsDirTest(result);
    } finally {
      setLogsDirTesting(false);
    }
  };

  const testRcon = async () => {
    const port = parseInt(rconPort, 10);
    if (!rconHost.trim() || isNaN(port)) return;
    setRconTesting(true);
    setRconTest(null);
    try {
      const result = await settingsTestRcon(
        rconHost.trim(),
        port,
        rconPassword,
      );
      setRconTest(result);
    } finally {
      setRconTesting(false);
    }
  };

  const testDiscord = async () => {
    if (!discordWebhook.trim()) return;
    setDiscordTesting(true);
    setDiscordTestResult(null);
    try {
      const resp = await fetch(discordWebhook.trim(), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          content: 'TurdMOD Manager — webhook test successful.',
        }),
      });
      setDiscordTestResult(resp.ok ? 'ok' : `HTTP ${resp.status}`);
    } catch (e) {
      setDiscordTestResult(e instanceof Error ? e.message : 'Request failed');
    } finally {
      setDiscordTesting(false);
    }
  };

  const resetPrefs = async () => {
    try {
      const { load } = await import('@tauri-apps/plugin-store');
      const store = await load('preferences.json', { autoSave: false, defaults: {} });
      await store.clear();
      await store.save();
      await qc.invalidateQueries();
    } catch (e) {
      setPickError(e instanceof Error ? e.message : String(e));
    }
  };

  const showToast = (msg: string) => {
    setToast(msg);
    setTimeout(() => setToast(null), 3000);
  };

  const handleSave = async () => {
    const port = parseInt(rconPort, 10);
    await updateSettings.mutateAsync({
      scumServerLogsDir: logsDir.trim() || null,
      rconHost: rconHost.trim() || null,
      rconPort: !isNaN(port) && port > 0 ? port : null,
      rconPassword: rconPassword, // empty string = clear keyring
      discordWebhook: discordWebhook.trim() || null,
    });
    // Reset password field after save so the user doesn't accidentally re-submit.
    setRconPassword('');
    // If the companion is running, restart it so the new env vars take effect.
    if (statusQuery.data?.kind === 'running') {
      await restartCompanion.mutateAsync();
      showToast('Settings saved — companion restarted with new values.');
    } else {
      showToast('Settings saved.');
    }
  };

  return (
    <div className="space-y-6">
      <header>
        <p className="font-display text-xs tracking-[0.3em] text-turd-mustard">
          Configuration
        </p>
        <h1 className="mt-1 font-display text-3xl text-turd-cream">Settings</h1>
      </header>

      {/* ================================================================
          EXISTING: SCUM install
      ================================================================ */}
      <Section title="SCUM install">
        {installQuery.isLoading && (
          <p className="text-sm text-turd-cream-dim">Detecting…</p>
        )}
        {installQuery.isSuccess && effectivePath && (
          <div className="space-y-3">
            <div className="flex items-start gap-3">
              <Check />
              <div className="min-w-0">
                <p className="text-sm text-turd-cream">
                  {override ? 'Manually set' : 'Auto-detected'}
                </p>
                <p className="break-all font-mono text-xs text-turd-cream-dim">
                  {effectivePath}
                </p>
              </div>
            </div>
            <button
              type="button"
              onClick={pickFolder}
              className="rounded border border-turd-bronze px-4 py-1.5 text-sm text-turd-cream transition-colors hover:border-turd-mustard-bright hover:text-turd-mustard-bright"
            >
              Change install folder
            </button>
          </div>
        )}
        {installQuery.isSuccess && !effectivePath && (
          <div className="space-y-3">
            <p className="text-sm text-turd-cream-dim">
              Couldn't find SCUM — pick the install folder.
            </p>
            <button
              type="button"
              onClick={pickFolder}
              className="rounded bg-turd-mustard px-4 py-1.5 text-sm font-medium text-turd-bg-deep transition-colors hover:bg-turd-mustard-bright"
            >
              Pick SCUM folder
            </button>
          </div>
        )}
        {pickError && (
          <p className="mt-3 text-xs text-turd-red">{pickError}</p>
        )}
      </Section>

      {battleyeBlocked && (
        <Section title="BattlEye" tone="warn">
          <p className="text-sm text-turd-cream">
            BattlEye appears to be enabled. TurdMOD refuses to enable client-side
            mods while BattlEye is active — that's a guaranteed ban.
          </p>
          <p className="mt-2 text-sm text-turd-cream-dim">
            Disable BattlEye in your SCUM launcher settings, or use this manager
            for server-side mods only.
          </p>
        </Section>
      )}

      <Section title="Mods directory">
        <p className="break-all font-mono text-xs text-turd-cream-dim">
          {modsDir ?? '—'}
        </p>
        <p className="mt-2 text-xs text-turd-cream-dim">
          Installed mods are placed in this folder. Read-only here; the manager
          handles writes.
        </p>
      </Section>

      {/* ================================================================
          NEW: Server Logs
      ================================================================ */}
      <Section title="Server logs">
        <p className="mb-3 text-xs text-turd-cream-dim">
          Point this at your SCUM dedicated server's Logs directory. The companion
          will tail these files to detect in-game events.
        </p>
        <label className="block">
          <span className="font-display text-[10px] uppercase tracking-wider text-turd-cream-dim">
            SCUM Server Logs Directory
          </span>
          <div className="mt-1 flex gap-2">
            <input
              type="text"
              value={logsDir}
              onChange={(e) => { setLogsDir(e.target.value); setLogsDirTest(null); }}
              placeholder="C:\scum-server\SCUM\Saved\SaveFiles\Logs"
              className={inputCls}
            />
            <button
              type="button"
              onClick={pickLogsFolder}
              className="shrink-0 rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-2 font-display text-xs uppercase tracking-wider text-turd-cream-dim transition-colors hover:border-turd-cream hover:text-turd-cream"
            >
              Browse…
            </button>
          </div>
        </label>
        <div className="mt-2 flex items-center gap-3">
          <button
            type="button"
            onClick={testLogsDir}
            disabled={logsDirTesting || !logsDir.trim()}
            className="rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-1.5 font-display text-xs uppercase tracking-wider text-turd-mustard-bright transition-colors hover:border-turd-mustard disabled:opacity-40"
          >
            {logsDirTesting ? 'Testing…' : 'Test'}
          </button>
          {logsDirTest && (
            <span className="font-mono text-xs">
              {!logsDirTest.exists ? (
                <span className="text-turd-red">Path doesn't exist</span>
              ) : !logsDirTest.hasLogFiles ? (
                <span className="text-turd-mustard">Path exists but no .log files inside</span>
              ) : (
                <span className="text-turd-green">Found .log files</span>
              )}
            </span>
          )}
        </div>
      </Section>

      {/* ================================================================
          NEW: RCON
      ================================================================ */}
      <Section title="RCON">
        <p className="mb-3 text-xs text-turd-cream-dim">
          Used by the companion to send admin commands to your SCUM server.
          The password is stored in the OS keychain, never on disk.
        </p>
        <div className="grid grid-cols-2 gap-3">
          <label className="flex flex-col gap-1">
            <span className="font-display text-[10px] uppercase tracking-wider text-turd-cream-dim">
              Host
            </span>
            <input
              type="text"
              value={rconHost}
              onChange={(e) => { setRconHost(e.target.value); setRconTest(null); }}
              placeholder="127.0.0.1"
              className={inputCls}
            />
          </label>
          <label className="flex flex-col gap-1">
            <span className="font-display text-[10px] uppercase tracking-wider text-turd-cream-dim">
              Port
            </span>
            <input
              type="number"
              value={rconPort}
              onChange={(e) => { setRconPort(e.target.value); setRconTest(null); }}
              min={1}
              max={65535}
              className={inputCls}
            />
          </label>
          <label className="col-span-2 flex flex-col gap-1">
            <span className="font-display text-[10px] uppercase tracking-wider text-turd-cream-dim">
              Password
              {settings?.hasRconPassword && !rconPassword && (
                <span className="ml-2 text-turd-cream-dim/70 normal-case tracking-normal">
                  (currently set — type to change, leave empty to keep existing)
                </span>
              )}
            </span>
            <input
              type="password"
              value={rconPassword}
              onChange={(e) => { setRconPassword(e.target.value); setRconTest(null); }}
              placeholder={settings?.hasRconPassword ? '••••••' : 'Enter RCON password'}
              className={inputCls}
            />
          </label>
        </div>
        <div className="mt-2 flex items-center gap-3">
          <button
            type="button"
            onClick={testRcon}
            disabled={rconTesting || !rconHost.trim()}
            className="rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-1.5 font-display text-xs uppercase tracking-wider text-turd-mustard-bright transition-colors hover:border-turd-mustard disabled:opacity-40"
          >
            {rconTesting ? 'Testing…' : 'Test connection'}
          </button>
          {rconTest && (
            <span className="font-mono text-xs">
              {rconTest.ok ? (
                <span className="text-turd-green">Connection OK</span>
              ) : (
                <span className="text-turd-red">{rconTest.error ?? 'Failed'}</span>
              )}
            </span>
          )}
        </div>
      </Section>

      {/* ================================================================
          NEW: Discord
      ================================================================ */}
      <Section title="Discord">
        <p className="mb-3 text-xs text-turd-cream-dim">
          Paste a Discord webhook URL to receive companion event notifications in a channel.
        </p>
        <label className="flex flex-col gap-1">
          <span className="font-display text-[10px] uppercase tracking-wider text-turd-cream-dim">
            Webhook URL
          </span>
          <input
            type="url"
            value={discordWebhook}
            onChange={(e) => { setDiscordWebhook(e.target.value); setDiscordTestResult(null); }}
            placeholder="https://discord.com/api/webhooks/..."
            className={inputCls}
          />
        </label>
        <div className="mt-2 flex items-center gap-3">
          <button
            type="button"
            onClick={testDiscord}
            disabled={discordTesting || !discordWebhook.trim()}
            className="rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-1.5 font-display text-xs uppercase tracking-wider text-turd-mustard-bright transition-colors hover:border-turd-mustard disabled:opacity-40"
          >
            {discordTesting ? 'Sending…' : 'Test'}
          </button>
          {discordTestResult && (
            <span className="font-mono text-xs">
              {discordTestResult === 'ok' ? (
                <span className="text-turd-green">Message sent</span>
              ) : (
                <span className="text-turd-red">{discordTestResult}</span>
              )}
            </span>
          )}
        </div>
      </Section>

      {/* ================================================================
          Save button
      ================================================================ */}
      <div className="flex items-center justify-between">
        <div>
          {updateSettings.isError && (
            <p className="text-xs text-turd-red">
              Save failed: {updateSettings.error?.message}
            </p>
          )}
        </div>
        <button
          type="button"
          onClick={handleSave}
          disabled={updateSettings.isPending || restartCompanion.isPending}
          className="rounded border border-turd-mustard bg-turd-bg-soft px-6 py-2.5 font-display text-xs uppercase tracking-wider text-turd-mustard-bright transition-colors hover:bg-turd-bronze/40 disabled:opacity-40"
        >
          {updateSettings.isPending || restartCompanion.isPending
            ? 'Saving…'
            : 'Save settings'}
        </button>
      </div>

      {/* ================================================================
          EXISTING: Preferences + About
      ================================================================ */}
      <Section title="Preferences">
        <button
          type="button"
          onClick={resetPrefs}
          className="rounded border border-turd-bronze px-4 py-1.5 text-sm text-turd-cream transition-colors hover:border-turd-red hover:text-turd-red"
        >
          Reset preferences
        </button>
        <p className="mt-2 text-xs text-turd-cream-dim">
          Clears the local store. Doesn't touch installed mods.
        </p>
      </Section>

      <Section title="About">
        <dl className="grid gap-2 text-sm">
          <Row label="Version" value={`v${APP_VERSION}`} mono />
          <Row label="Build date" value={BUILD_DATE} mono />
          <Row
            label="Website"
            value={
              <ExternalLink href="https://turdmod.com">turdmod.com</ExternalLink>
            }
          />
        </dl>
      </Section>

      {/* Toast */}
      {toast && (
        <div className="fixed bottom-6 right-6 z-50 rounded-lg border border-turd-mustard/40 bg-turd-bg-mid px-5 py-3 font-display text-sm text-turd-mustard-bright shadow-xl">
          {toast}
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Sub-components (preserved from original)
// ---------------------------------------------------------------------------

function Section({
  title,
  tone = 'default',
  children,
}: {
  title: string;
  tone?: 'default' | 'warn';
  children: ReactNode;
}) {
  const border =
    tone === 'warn' ? 'border-turd-red/40' : 'border-turd-bronze/30';
  return (
    <section className={`rounded-lg border ${border} bg-turd-bg-mid/40 p-5`}>
      <h2 className="font-display text-sm uppercase tracking-widest text-turd-mustard">
        {title}
      </h2>
      <div className="mt-3">{children}</div>
    </section>
  );
}

function Row({
  label,
  value,
  mono = false,
}: {
  label: string;
  value: ReactNode;
  mono?: boolean;
}) {
  return (
    <div className="flex items-center justify-between gap-4 border-b border-turd-bronze/10 pb-2 last:border-0 last:pb-0">
      <dt className="font-mono text-[10px] uppercase tracking-wider text-turd-cream-dim">
        {label}
      </dt>
      <dd className={`text-turd-cream ${mono ? 'font-mono text-xs' : ''}`}>
        {value}
      </dd>
    </div>
  );
}

function Check() {
  return (
    <span
      aria-hidden
      className="mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-turd-green/20 font-mono text-xs text-turd-green"
    >
      ✓
    </span>
  );
}

function ExternalLink({
  href,
  children,
}: {
  href: string;
  children: ReactNode;
}) {
  const onClick = async (e: ReactMouseEvent) => {
    e.preventDefault();
    try {
      const { open } = await import('@tauri-apps/plugin-shell');
      await open(href);
    } catch {
      // fall through
    }
  };
  return (
    <a
      href={href}
      onClick={onClick}
      className="text-turd-mustard hover:text-turd-mustard-bright"
    >
      {children}
    </a>
  );
}