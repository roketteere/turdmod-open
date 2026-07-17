// Form editor for the most-touched ServerSettings.ini scalars + raw .ini toggle.

import { useState } from 'react';
import { type SettingsPatch } from '../../lib/tauri-admin';
import {
  useSaveServerSettings,
  useServerSettingsForm,
  useWriteAdminFile,
} from '../../hooks/useAdmin';
import { Toast } from './Toast';

interface ServerSettingsFormProps {
  serverId: string;
}

const inputCls =
  'rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-2 font-mono text-sm text-turd-cream placeholder:text-turd-cream-dim/60 focus:border-turd-mustard focus:outline-none w-full';

const labelCls = 'block mb-1 text-sm font-medium text-turd-cream';
const hintCls = 'mt-0.5 text-[10px] text-turd-cream-dim';

export function ServerSettingsForm({ serverId }: ServerSettingsFormProps) {
  const query = useServerSettingsForm(serverId);
  const savePartial = useSaveServerSettings();
  const saveRaw = useWriteAdminFile();

  const [localValues, setLocalValues] = useState<SettingsPatch | null>(null);
  const [rawText, setRawText] = useState<string | null>(null);
  const [rawDirty, setRawDirty] = useState(false);
  const [showRaw, setShowRaw] = useState(false);
  const [toast, setToast] = useState<{ msg: string; kind: 'ok' | 'warn' } | null>(null);

  const form = query.data;
  const isDirty = localValues !== null && Object.keys(localValues).length > 0;

  function get(field: keyof SettingsPatch): string {
    return localValues?.[field] ?? form?.[field] ?? '';
  }

  function set(field: keyof SettingsPatch, value: string) {
    setLocalValues((prev) => ({ ...(prev ?? {}), [field]: value }));
  }

  function handleSave() {
    if (!localValues) return;
    savePartial.mutate(
      { serverId, patch: localValues },
      {
        onSuccess: () => {
          setLocalValues(null);
          setToast({ msg: 'Saved — applies on next server restart.', kind: 'warn' });
        },
        onError: (err) => {
          setToast({ msg: `Save failed: ${err.message}`, kind: 'warn' });
        },
      },
    );
  }

  function handleRawSave() {
    if (!rawDirty || rawText === null) return;
    saveRaw.mutate(
      { serverId, filename: 'ServerSettings.ini', contents: rawText },
      {
        onSuccess: () => {
          setRawDirty(false);
          setToast({ msg: 'Raw .ini saved — applies on next server restart.', kind: 'warn' });
        },
        onError: (err) => {
          setToast({ msg: `Save failed: ${err.message}`, kind: 'warn' });
        },
      },
    );
  }

  function handleToggleRaw() {
    if (!showRaw) {
      setRawText(form?.rawIni ?? '');
      setRawDirty(false);
    }
    setShowRaw((v) => !v);
  }

  if (query.isLoading) {
    return <p className="text-xs text-turd-cream-dim">Loading ServerSettings.ini…</p>;
  }

  if (query.error) {
    return (
      <p className="text-xs text-turd-red">
        Failed to load settings: {query.error.message}
      </p>
    );
  }

  return (
    <div className="flex flex-col gap-5">
      {toast && (
        <Toast kind={toast.kind} onClose={() => setToast(null)}>
          {toast.msg}
        </Toast>
      )}

      {!showRaw && (
        <>
          <section className="grid gap-4 sm:grid-cols-2">
            <div>
              <label className={labelCls}>Server Name</label>
              <input
                type="text"
                value={get('serverName')}
                onChange={(e) => set('serverName', e.target.value)}
                className={inputCls}
                placeholder="My SCUM Server"
              />
            </div>
            <div>
              <label className={labelCls}>Password</label>
              <input
                type="text"
                value={get('serverPassword')}
                onChange={(e) => set('serverPassword', e.target.value)}
                className={inputCls}
                placeholder="Leave blank for public"
              />
            </div>
            <div className="sm:col-span-2">
              <label className={labelCls}>Description</label>
              <textarea
                value={get('serverDescription')}
                onChange={(e) => set('serverDescription', e.target.value)}
                rows={2}
                className={inputCls}
                placeholder="Shown in the server browser"
              />
            </div>
            <div className="sm:col-span-2">
              <label className={labelCls}>Message of the Day</label>
              <textarea
                value={get('messageOfTheDay')}
                onChange={(e) => set('messageOfTheDay', e.target.value)}
                rows={2}
                className={inputCls}
                placeholder="Shown to players on join"
              />
            </div>
          </section>

          <section className="grid gap-4 sm:grid-cols-3">
            <div>
              <label className={labelCls}>Max Players</label>
              <input
                type="number"
                min={1}
                max={64}
                value={get('maxPlayers')}
                onChange={(e) => set('maxPlayers', e.target.value)}
                className={inputCls}
              />
            </div>
            <div>
              <label className={labelCls}>Playstyle</label>
              <input
                type="text"
                value={get('serverPlaystyle')}
                onChange={(e) => set('serverPlaystyle', e.target.value)}
                className={inputCls}
                placeholder="PVE / PVP / 0 / 1"
              />
              <p className={hintCls}>SCUM accepts PVE/PVP or 0/1</p>
            </div>
            <div>
              <label className={labelCls}>Respawn Time (s)</label>
              <input
                type="number"
                min={0}
                value={get('respawnTime')}
                onChange={(e) => set('respawnTime', e.target.value)}
                className={inputCls}
              />
            </div>
          </section>

          <section>
            <p className="mb-3 font-display text-xs uppercase tracking-widest text-turd-cream-dim">
              Toggles
            </p>
            <div className="grid gap-3 sm:grid-cols-2">
              {(
                [
                  ['enableWhitelist', 'Whitelist enforced'],
                  ['enableBattlEye', 'BattlEye anti-cheat'],
                  ['allowFirstPerson', 'Allow first-person view'],
                  ['allowThirdPerson', 'Allow third-person view'],
                  ['allowCrosshair', 'Allow crosshair'],
                  ['enableNewPlayerProtection', 'New-player protection'],
                  ['allowVoting', 'Allow player voting'],
                ] as [keyof SettingsPatch, string][]
              ).map(([field, label]) => (
                <BoolRow
                  key={field}
                  label={label}
                  value={get(field)}
                  onChange={(v) => set(field, v)}
                />
              ))}
            </div>
          </section>

          <section>
            <p className="mb-3 font-display text-xs uppercase tracking-widest text-turd-cream-dim">
              Multipliers
            </p>
            <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
              <FloatField
                label="Day cycle speed"
                hint="1.0 = real-time"
                value={get('dayCycleSpeedMultiplier')}
                onChange={(v) => set('dayCycleSpeedMultiplier', v)}
              />
              <FloatField
                label="Nighttime speed"
                hint="1.0 = real-time"
                value={get('nighttimeSpeedMultiplier')}
                onChange={(v) => set('nighttimeSpeedMultiplier', v)}
              />
              <FloatField
                label="Economy"
                hint="Trader prices"
                value={get('economyMultiplier')}
                onChange={(v) => set('economyMultiplier', v)}
              />
              <FloatField
                label="XP"
                hint="Experience gain"
                value={get('xpMultiplier')}
                onChange={(v) => set('xpMultiplier', v)}
              />
            </div>
          </section>

          {isBoolTruthy(get('enableNewPlayerProtection')) && (
            <div className="max-w-xs">
              <label className={labelCls}>
                New-player protection duration (h)
              </label>
              <input
                type="number"
                min={0}
                value={get('newPlayerProtectionDuration')}
                onChange={(e) => set('newPlayerProtectionDuration', e.target.value)}
                className={inputCls}
              />
            </div>
          )}

          <div className="flex items-center justify-between border-t border-turd-bronze/20 pt-3">
            <button
              type="button"
              onClick={handleToggleRaw}
              className="font-display text-xs uppercase tracking-wider text-turd-cream-dim underline-offset-2 hover:text-turd-cream hover:underline"
            >
              Edit raw .ini
            </button>
            <button
              type="button"
              disabled={!isDirty || savePartial.isPending}
              onClick={handleSave}
              className="rounded border border-turd-bronze/60 bg-turd-bg-soft px-4 py-1.5 font-display text-xs uppercase tracking-wider text-turd-mustard-bright transition-colors hover:border-turd-mustard disabled:cursor-not-allowed disabled:opacity-40"
            >
              {savePartial.isPending ? 'Saving…' : 'Save (restart required)'}
            </button>
          </div>
        </>
      )}

      {showRaw && (
        <div className="flex flex-col gap-3">
          <div className="flex items-center justify-between">
            <p className="font-display text-xs uppercase tracking-widest text-turd-mustard">
              Raw ServerSettings.ini
            </p>
            <button
              type="button"
              onClick={handleToggleRaw}
              className="font-display text-xs uppercase tracking-wider text-turd-cream-dim hover:text-turd-cream"
            >
              Back to form
            </button>
          </div>
          <textarea
            value={rawText ?? ''}
            onChange={(e) => {
              setRawText(e.target.value);
              setRawDirty(true);
            }}
            rows={24}
            spellCheck={false}
            className="w-full rounded border border-turd-bronze/60 bg-turd-bg-deep px-3 py-2 font-mono text-xs text-turd-cream focus:border-turd-mustard focus:outline-none"
          />
          <div className="flex items-center justify-between">
            <p className="text-[10px] text-turd-cream-dim">
              Applies on next server restart.
            </p>
            <button
              type="button"
              disabled={!rawDirty || saveRaw.isPending}
              onClick={handleRawSave}
              className="rounded border border-turd-bronze/60 bg-turd-bg-soft px-4 py-1.5 font-display text-xs uppercase tracking-wider text-turd-mustard-bright transition-colors hover:border-turd-mustard disabled:cursor-not-allowed disabled:opacity-40"
            >
              {saveRaw.isPending ? 'Saving…' : 'Save raw (restart required)'}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

function isBoolTruthy(v: string): boolean {
  return v === 'True' || v === 'true' || v === '1' || v === 'yes';
}

function BoolRow({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
}) {
  const isOn = isBoolTruthy(value);
  return (
    <label className="flex cursor-pointer items-center gap-3">
      <input
        type="checkbox"
        checked={isOn}
        onChange={(e) => onChange(e.target.checked ? 'True' : 'False')}
        className="h-4 w-4 rounded border-turd-bronze bg-turd-bg-soft accent-turd-mustard"
      />
      <span className="text-sm text-turd-cream">{label}</span>
    </label>
  );
}

function FloatField({
  label,
  hint,
  value,
  onChange,
}: {
  label: string;
  hint: string;
  value: string;
  onChange: (v: string) => void;
}) {
  return (
    <div>
      <label className={labelCls}>{label}</label>
      <input
        type="number"
        step="0.1"
        min="0"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className={inputCls}
        placeholder="1.0"
      />
      <p className={hintCls}>{hint}</p>
    </div>
  );
}
