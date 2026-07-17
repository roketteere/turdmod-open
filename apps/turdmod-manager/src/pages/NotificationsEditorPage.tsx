import { useState, useEffect, useRef, useCallback, type RefObject, type ChangeEvent } from 'react';
import { useMutation } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { detectScum, getInstallPath } from '../lib/tauri';

type DayValue =
  | 'Monday'
  | 'Tuesday'
  | 'Wednesday'
  | 'Thursday'
  | 'Friday'
  | 'Saturday'
  | 'Sunday'
  | 'Monday-Friday'
  | 'Weekend'
  | 'Everyday';

const DAY_OPTIONS: DayValue[] = [
  'Everyday',
  'Monday-Friday',
  'Weekend',
  'Monday',
  'Tuesday',
  'Wednesday',
  'Thursday',
  'Friday',
  'Saturday',
  'Sunday',
];

// Four user-facing scheduling modes mapping to the three underlying JSON forms.
// 'allday'   → no time, wait set            → fires every N minutes all day
// 'specific' → time is string[]             → fires at explicit times
// 'window'   → time is "HH:MM-HH:MM", wait  → fires in window every N minutes
// 'once'     → time is single string        → fires once at that time
type ScheduleMode = 'allday' | 'specific' | 'window' | 'once';

interface NotificationEntry {
  id: string;
  day: DayValue;
  mode: ScheduleMode;
  specificTimes: string[];
  windowFrom: string;
  windowTo: string;
  onceTime: string;
  repeatEvery: string;
  duration: string;
  color: string;
  message: string;
}

interface ScumNotificationsFile {
  Notifications: Array<{
    day: string;
    time?: string | string[] | null;
    duration: string;
    wait?: string;
    color: string;
    message: string;
  }>;
}

function hexToScumColor(hex: string): string {
  const r = parseInt(hex.slice(1, 3), 16);
  const g = parseInt(hex.slice(3, 5), 16);
  const b = parseInt(hex.slice(5, 7), 16);
  return `${r}-${g}-${b}`;
}

function scumColorToHex(color: string): string {
  const parts = color.split('-').map(Number);
  if (parts.length !== 3 || parts.some(isNaN)) return '#ffffff';
  const [r, g, b] = parts;
  return `#${r.toString(16).padStart(2, '0')}${g.toString(16).padStart(2, '0')}${b.toString(16).padStart(2, '0')}`;
}

function makeId(): string {
  return Math.random().toString(36).slice(2, 10);
}

function detectMode(raw: ScumNotificationsFile['Notifications'][number]): ScheduleMode {
  if (raw.time === undefined || raw.time === null || raw.time === '') return 'allday';
  if (Array.isArray(raw.time)) return 'specific';
  if (typeof raw.time === 'string') {
    const halves = raw.time.split('-');
    if (halves.length === 2 && halves[0].includes(':') && halves[1].includes(':')) return 'window';
    return 'once';
  }
  return 'allday';
}

function rawEntryToEditorEntry(raw: ScumNotificationsFile['Notifications'][number]): NotificationEntry {
  const mode = detectMode(raw);

  let specificTimes: string[] = ['12:00'];
  let windowFrom = '15:00';
  let windowTo = '21:00';
  let onceTime = '12:00';

  if (mode === 'specific') {
    specificTimes = Array.isArray(raw.time) && raw.time.length > 0 ? raw.time : ['12:00'];
  } else if (mode === 'window' && typeof raw.time === 'string') {
    const parts = raw.time.split('-');
    windowFrom = parts[0] ?? '15:00';
    windowTo = parts[1] ?? '21:00';
  } else if (mode === 'once' && typeof raw.time === 'string') {
    onceTime = raw.time;
  }

  return {
    id: makeId(),
    day: (DAY_OPTIONS.includes(raw.day as DayValue) ? raw.day : 'Everyday') as DayValue,
    mode,
    specificTimes,
    windowFrom,
    windowTo,
    onceTime,
    repeatEvery: raw.wait ?? '15',
    duration: raw.duration ?? '60',
    color: raw.color ?? '255-255-255',
    message: raw.message ?? '',
  };
}

function editorEntryToRaw(entry: NotificationEntry): ScumNotificationsFile['Notifications'][number] {
  const base = {
    day: entry.day,
    duration: entry.duration,
    color: entry.color,
    message: entry.message,
  };

  switch (entry.mode) {
    case 'allday':
      return { ...base, wait: entry.repeatEvery };
    case 'specific': {
      // Always emit as array so the entry round-trips back to 'specific' mode
      // on reload, even when it has only one time entry.
      return { ...base, time: entry.specificTimes };
    }
    case 'window':
      return { ...base, time: `${entry.windowFrom}-${entry.windowTo}`, wait: entry.repeatEvery };
    case 'once':
      // wait=1440 (one full day) prevents repeats if SCUM requires wait set.
      return { ...base, time: entry.onceTime, wait: '1440' };
  }
}

function defaultEntry(): NotificationEntry {
  return {
    id: makeId(),
    day: 'Everyday',
    mode: 'allday',
    specificTimes: ['12:00'],
    windowFrom: '15:00',
    windowTo: '21:00',
    onceTime: '12:00',
    repeatEvery: '15',
    duration: '60',
    color: '255-255-255',
    message: 'New notification — edit me.',
  };
}

interface EntryError {
  message?: string;
  duration?: string;
  repeatEvery?: string;
  color?: string;
  time?: string;
}

const timeRegex = /^\d{1,2}:\d{2}$/;

function validateEntry(entry: NotificationEntry): EntryError {
  const errs: EntryError = {};

  if (!entry.message.trim()) errs.message = 'Message is required.';

  const dur = Number(entry.duration);
  if (!entry.duration || isNaN(dur) || dur <= 0) errs.duration = 'Must be > 0.';

  if (entry.mode === 'allday' || entry.mode === 'window') {
    const rep = Number(entry.repeatEvery);
    if (!entry.repeatEvery || isNaN(rep) || rep <= 0) errs.repeatEvery = 'Must be > 0.';
  }

  const colorParts = entry.color.split('-').map(Number);
  if (colorParts.length !== 3 || colorParts.some((p) => isNaN(p) || p < 0 || p > 255)) {
    errs.color = 'Must be R-G-B, 0-255 each.';
  }

  if (entry.mode === 'specific') {
    if (entry.specificTimes.length === 0 || entry.specificTimes.some((t) => !timeRegex.test(t))) {
      errs.time = 'Each time must be HH:MM.';
    }
  } else if (entry.mode === 'window') {
    if (!timeRegex.test(entry.windowFrom) || !timeRegex.test(entry.windowTo)) {
      errs.time = 'Both times must be HH:MM.';
    }
  } else if (entry.mode === 'once') {
    if (!timeRegex.test(entry.onceTime)) errs.time = 'Format: HH:MM';
  }

  return errs;
}

function hasErrors(e: EntryError): boolean {
  return Object.keys(e).length > 0;
}

const inputCls =
  'rounded border border-turd-bronze/60 bg-turd-bg-soft px-2 py-1 font-mono text-xs text-turd-cream placeholder:text-turd-cream-dim/60 focus:border-turd-mustard focus:outline-none';

const btnBase =
  'rounded border font-display text-xs uppercase tracking-wider transition-colors disabled:cursor-not-allowed disabled:opacity-30';

const btnPrimary =
  `${btnBase} border-turd-mustard/60 bg-turd-mustard/20 px-4 py-1.5 text-turd-mustard-bright hover:bg-turd-mustard/30`;

const btnGhost =
  `${btnBase} border-turd-bronze/60 bg-turd-bg-soft px-3 py-1.5 text-turd-cream-dim hover:border-turd-cream hover:text-turd-cream`;

const btnDanger =
  `${btnBase} border-turd-red/50 bg-turd-red/10 px-4 py-1.5 text-turd-red hover:bg-turd-red/20`;

const labelCls = 'font-mono text-[10px] uppercase tracking-wider text-turd-cream-dim';
const errorCls = 'font-mono text-[10px] text-turd-red';

const MODE_OPTIONS: { value: ScheduleMode; label: string }[] = [
  { value: 'allday', label: 'Every X minutes — all day' },
  { value: 'specific', label: 'At specific times' },
  { value: 'window', label: 'During a time window (repeating)' },
  { value: 'once', label: 'Once at a specific time' },
];

interface ColorFieldProps {
  value: string;
  onChange: (v: string) => void;
  error?: string;
}

function ColorField({ value, onChange, error }: ColorFieldProps) {
  const hex = scumColorToHex(value);
  const [rawText, setRawText] = useState(value);

  useEffect(() => {
    setRawText(value);
  }, [value]);

  const handleHexPick = (e: ChangeEvent<HTMLInputElement>) => {
    onChange(hexToScumColor(e.target.value));
  };

  const handleTextBlur = () => {
    const parts = rawText.split('-').map(Number);
    if (parts.length === 3 && !parts.some((p) => isNaN(p) || p < 0 || p > 255)) {
      onChange(rawText);
    } else {
      setRawText(value);
    }
  };

  return (
    <div className="flex flex-col gap-1">
      <span className={labelCls}>Color</span>
      <div className="flex items-center gap-2">
        <div className="relative h-7 w-7 shrink-0 overflow-hidden rounded border border-turd-bronze/60">
          <div className="absolute inset-0" style={{ backgroundColor: hex }} />
          <input
            type="color"
            value={hex}
            onChange={handleHexPick}
            className="absolute inset-0 h-full w-full cursor-pointer opacity-0"
            title="Pick color"
          />
        </div>
        <input
          type="text"
          value={rawText}
          onChange={(e) => setRawText(e.target.value)}
          onBlur={handleTextBlur}
          placeholder="255-255-255"
          className={`${inputCls} w-28`}
        />
        <span className="font-mono text-[10px] text-turd-cream-dim">{hex.toUpperCase()}</span>
      </div>
      {error && <p className={errorCls}>{error}</p>}
    </div>
  );
}

const PLACEHOLDERS = [
  '#NumPlayers',
  '#Date',
  '#Time',
  '#RestartIn(HH:MM)',
  '#RestartAt(HH:MM)',
];

interface PlaceholderChipsProps {
  textareaRef: RefObject<HTMLTextAreaElement | null>;
  onInsert: (text: string, cursorPos: number | null) => void;
}

function PlaceholderChips({ textareaRef, onInsert }: PlaceholderChipsProps) {
  return (
    <div className="flex flex-wrap gap-1">
      {PLACEHOLDERS.map((p) => (
        <button
          key={p}
          type="button"
          onClick={() => {
            const pos = textareaRef.current?.selectionStart ?? null;
            onInsert(p, pos);
          }}
          className="rounded border border-turd-bronze/40 px-2 py-0.5 font-mono text-[10px] text-turd-mustard/80 transition-colors hover:border-turd-mustard hover:text-turd-mustard-bright"
        >
          {p}
        </button>
      ))}
    </div>
  );
}

interface EntryRowProps {
  entry: NotificationEntry;
  index: number;
  total: number;
  errors: EntryError;
  onChange: (updated: NotificationEntry) => void;
  onRemove: () => void;
  onMoveUp: () => void;
  onMoveDown: () => void;
}

function EntryRow({
  entry,
  index,
  total,
  errors,
  onChange,
  onRemove,
  onMoveUp,
  onMoveDown,
}: EntryRowProps) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const set = <K extends keyof NotificationEntry>(key: K, value: NotificationEntry[K]) =>
    onChange({ ...entry, [key]: value });

  const insertPlaceholder = useCallback(
    (placeholder: string, cursorPos: number | null) => {
      const textarea = textareaRef.current;
      if (!textarea) {
        onChange({
          ...entry,
          message: entry.message + (entry.message ? ' ' : '') + placeholder,
        });
        return;
      }
      const pos = cursorPos ?? entry.message.length;
      const before = entry.message.slice(0, pos);
      const after = entry.message.slice(pos);
      const sep = before.length > 0 && !before.endsWith(' ') ? ' ' : '';
      const updated = `${before}${sep}${placeholder}${after}`;
      onChange({ ...entry, message: updated });
      setTimeout(() => {
        textarea.focus();
        const newPos = pos + sep.length + placeholder.length;
        textarea.setSelectionRange(newPos, newPos);
      }, 0);
    },
    [entry, onChange],
  );

  const isFirst = index === 0;
  const isLast = index === total - 1;

  return (
    <div className="glass rounded-xl">
      <div className="flex items-center justify-between border-b border-turd-bronze/20 px-4 py-2">
        <span className="font-display text-xs uppercase tracking-wider text-turd-mustard/70">
          Entry {index + 1}
        </span>
        <div className="flex items-center gap-1">
          <button
            type="button"
            onClick={onMoveUp}
            disabled={isFirst}
            className={`${btnBase} border-turd-bronze/40 px-2 py-0.5 text-turd-cream-dim hover:border-turd-cream hover:text-turd-cream`}
            title="Move up"
          >
            ↑
          </button>
          <button
            type="button"
            onClick={onMoveDown}
            disabled={isLast}
            className={`${btnBase} border-turd-bronze/40 px-2 py-0.5 text-turd-cream-dim hover:border-turd-cream hover:text-turd-cream`}
            title="Move down"
          >
            ↓
          </button>
          <button
            type="button"
            onClick={onRemove}
            className={`${btnBase} border-turd-bronze/40 px-2 py-0.5 text-turd-cream-dim/60 hover:border-turd-red/60 hover:bg-turd-red/10 hover:text-turd-red`}
            title="Remove entry"
          >
            ✕
          </button>
        </div>
      </div>

      <div className="flex flex-col gap-4 p-4">
        <div className="flex flex-wrap items-end gap-4">
          <div className="flex flex-col gap-1">
            <span className={labelCls}>Schedule mode</span>
            <select
              value={entry.mode}
              onChange={(e) => set('mode', e.target.value as ScheduleMode)}
              className={`${inputCls} pr-6`}
            >
              {MODE_OPTIONS.map((o) => (
                <option key={o.value} value={o.value}>
                  {o.label}
                </option>
              ))}
            </select>
          </div>

          <div className="flex flex-col gap-1">
            <span className={labelCls}>Days</span>
            <select
              value={entry.day}
              onChange={(e) => set('day', e.target.value as DayValue)}
              className={`${inputCls} pr-6`}
            >
              {DAY_OPTIONS.map((d) => (
                <option key={d} value={d}>
                  {d}
                </option>
              ))}
            </select>
          </div>
        </div>

        {entry.mode === 'allday' && (
          <div className="flex flex-wrap items-end gap-4">
            <div className="flex flex-col gap-1">
              <span className={labelCls}>Repeat every</span>
              <div className="flex items-center gap-1">
                <input
                  type="number"
                  value={entry.repeatEvery}
                  onChange={(e) => set('repeatEvery', e.target.value)}
                  min={1}
                  className={`${inputCls} w-20`}
                />
                <span className="font-mono text-[10px] text-turd-cream-dim">min</span>
              </div>
              {errors.repeatEvery && <p className={errorCls}>{errors.repeatEvery}</p>}
            </div>

            <div className="flex flex-col gap-1">
              <span className={labelCls}>Show for</span>
              <div className="flex items-center gap-1">
                <input
                  type="number"
                  value={entry.duration}
                  onChange={(e) => set('duration', e.target.value)}
                  min={1}
                  className={`${inputCls} w-20`}
                />
                <span className="font-mono text-[10px] text-turd-cream-dim">sec</span>
              </div>
              {errors.duration && <p className={errorCls}>{errors.duration}</p>}
            </div>
          </div>
        )}

        {entry.mode === 'specific' && (
          <div className="flex flex-col gap-2">
            <div className="flex flex-col gap-1">
              <span className={labelCls}>Times</span>
              <div className="flex flex-wrap items-center gap-1">
                {entry.specificTimes.map((t, i) => (
                  <div key={i} className="flex items-center gap-0.5">
                    <input
                      type="text"
                      value={t}
                      onChange={(e) => {
                        const next = [...entry.specificTimes];
                        next[i] = e.target.value;
                        set('specificTimes', next);
                      }}
                      placeholder="HH:MM"
                      className={`${inputCls} w-20`}
                    />
                    {entry.specificTimes.length > 1 && (
                      <button
                        type="button"
                        onClick={() => set('specificTimes', entry.specificTimes.filter((_, j) => j !== i))}
                        className="rounded px-1 py-0.5 font-mono text-[10px] text-turd-cream-dim/60 hover:bg-turd-red/20 hover:text-turd-red"
                        title="Remove time"
                      >
                        ✕
                      </button>
                    )}
                  </div>
                ))}
                <button
                  type="button"
                  onClick={() => set('specificTimes', [...entry.specificTimes, '12:00'])}
                  className="rounded border border-turd-bronze/40 px-2 py-0.5 font-mono text-[10px] text-turd-cream-dim hover:border-turd-cream hover:text-turd-cream"
                >
                  + time
                </button>
              </div>
              {errors.time && <p className={errorCls}>{errors.time}</p>}
            </div>

            <div className="flex flex-col gap-1">
              <span className={labelCls}>Show for</span>
              <div className="flex items-center gap-1">
                <input
                  type="number"
                  value={entry.duration}
                  onChange={(e) => set('duration', e.target.value)}
                  min={1}
                  className={`${inputCls} w-20`}
                />
                <span className="font-mono text-[10px] text-turd-cream-dim">sec</span>
              </div>
              {errors.duration && <p className={errorCls}>{errors.duration}</p>}
            </div>
          </div>
        )}

        {entry.mode === 'window' && (
          <div className="flex flex-wrap items-end gap-4">
            <div className="flex flex-col gap-1">
              <span className={labelCls}>From</span>
              <input
                type="text"
                value={entry.windowFrom}
                onChange={(e) => set('windowFrom', e.target.value)}
                placeholder="HH:MM"
                className={`${inputCls} w-20`}
              />
            </div>
            <div className="flex flex-col gap-1">
              <span className={labelCls}>To</span>
              <input
                type="text"
                value={entry.windowTo}
                onChange={(e) => set('windowTo', e.target.value)}
                placeholder="HH:MM"
                className={`${inputCls} w-20`}
              />
            </div>
            {errors.time && (
              <p className={`${errorCls} self-end pb-1`}>{errors.time}</p>
            )}

            <div className="flex flex-col gap-1">
              <span className={labelCls}>Repeat every</span>
              <div className="flex items-center gap-1">
                <input
                  type="number"
                  value={entry.repeatEvery}
                  onChange={(e) => set('repeatEvery', e.target.value)}
                  min={1}
                  className={`${inputCls} w-20`}
                />
                <span className="font-mono text-[10px] text-turd-cream-dim">min</span>
              </div>
              {errors.repeatEvery && <p className={errorCls}>{errors.repeatEvery}</p>}
            </div>

            <div className="flex flex-col gap-1">
              <span className={labelCls}>Show for</span>
              <div className="flex items-center gap-1">
                <input
                  type="number"
                  value={entry.duration}
                  onChange={(e) => set('duration', e.target.value)}
                  min={1}
                  className={`${inputCls} w-20`}
                />
                <span className="font-mono text-[10px] text-turd-cream-dim">sec</span>
              </div>
              {errors.duration && <p className={errorCls}>{errors.duration}</p>}
            </div>
          </div>
        )}

        {entry.mode === 'once' && (
          <div className="flex flex-wrap items-end gap-4">
            <div className="flex flex-col gap-1">
              <span className={labelCls}>Time</span>
              <input
                type="text"
                value={entry.onceTime}
                onChange={(e) => set('onceTime', e.target.value)}
                placeholder="HH:MM"
                className={`${inputCls} w-20`}
              />
              {errors.time && <p className={errorCls}>{errors.time}</p>}
            </div>

            <div className="flex flex-col gap-1">
              <span className={labelCls}>Show for</span>
              <div className="flex items-center gap-1">
                <input
                  type="number"
                  value={entry.duration}
                  onChange={(e) => set('duration', e.target.value)}
                  min={1}
                  className={`${inputCls} w-20`}
                />
                <span className="font-mono text-[10px] text-turd-cream-dim">sec</span>
              </div>
              {errors.duration && <p className={errorCls}>{errors.duration}</p>}
            </div>
          </div>
        )}

        <ColorField
          value={entry.color}
          onChange={(v) => set('color', v)}
          error={errors.color}
        />

        <div className="flex flex-col gap-1">
          <span className={labelCls}>Message</span>
          <textarea
            ref={textareaRef}
            value={entry.message}
            onChange={(e) => set('message', e.target.value)}
            rows={2}
            placeholder="Welcome to ServerName! #NumPlayers online."
            className={`${inputCls} w-full resize-y`}
          />
          {errors.message && <p className={errorCls}>{errors.message}</p>}
          <div className="mt-1">
            <span className="mr-2 font-mono text-[10px] text-turd-cream-dim/60">Insert:</span>
            <PlaceholderChips textareaRef={textareaRef} onInsert={insertPlaceholder} />
          </div>
        </div>
      </div>
    </div>
  );
}

interface FireNowState {
  message: string;
  color: string;
  duration: string;
  autoRemove: boolean;
}

function defaultFireNow(): FireNowState {
  return { message: '', color: '255-200-0', duration: '30', autoRemove: true };
}

type LoadState =
  | { kind: 'loading' }
  | { kind: 'no-install' }
  | { kind: 'parse-error'; message: string }
  | { kind: 'ready' };

export function NotificationsEditorPage() {
  const [notifPath, setNotifPath] = useState<string | null>(null);
  const [loadState, setLoadState] = useState<LoadState>({ kind: 'loading' });
  const [entries, setEntries] = useState<NotificationEntry[]>([]);
  const [baseline, setBaseline] = useState<NotificationEntry[]>([]);
  const [saveSuccess, setSaveSuccess] = useState(false);
  const [fireNowOpen, setFireNowOpen] = useState(false);
  const [fireNow, setFireNow] = useState<FireNowState>(defaultFireNow());
  const [fireNowStatus, setFireNowStatus] = useState<string | null>(null);

  const isDirty =
    loadState.kind === 'ready' &&
    JSON.stringify(entries) !== JSON.stringify(baseline);

  const allErrors: EntryError[] = entries.map(validateEntry);
  const anyErrors = allErrors.some(hasErrors);

  const loadFile = useCallback(async (path: string) => {
    setLoadState({ kind: 'loading' });
    try {
      const result = await invoke<{ content: string; existed: boolean }>(
        'manager_read_text_file',
        { path },
      );
      if (!result.existed) {
        setEntries([]);
        setBaseline([]);
        setLoadState({ kind: 'ready' });
        return;
      }
      let parsed: unknown;
      try {
        parsed = JSON.parse(result.content);
      } catch (e) {
        setLoadState({ kind: 'parse-error', message: String(e) });
        return;
      }
      const obj = parsed as Record<string, unknown>;
      if (!obj || typeof obj !== 'object' || !Array.isArray(obj['Notifications'])) {
        setLoadState({
          kind: 'parse-error',
          message: 'File parsed but "Notifications" key is missing or not an array.',
        });
        return;
      }
      const loaded = (obj['Notifications'] as ScumNotificationsFile['Notifications']).map(
        rawEntryToEditorEntry,
      );
      setEntries(loaded);
      setBaseline(loaded);
      setLoadState({ kind: 'ready' });
    } catch (e) {
      setLoadState({ kind: 'parse-error', message: String(e) });
    }
  }, []);

  useEffect(() => {
    (async () => {
      try {
        const [detected, override] = await Promise.all([
          detectScum().catch(() => null),
          getInstallPath().catch(() => null),
        ]);
        const serverRoot = override ?? detected?.server ?? detected?.active ?? null;
        if (!serverRoot) {
          setLoadState({ kind: 'no-install' });
          return;
        }
        const slash = serverRoot.replace(/\\/g, '/');
        const path = `${slash}/SCUM/Saved/Config/WindowsServer/Notifications.json`;
        setNotifPath(path);
        await loadFile(path);
      } catch {
        setLoadState({ kind: 'no-install' });
      }
    })();
  }, [loadFile]);

  const saveMutation = useMutation<void, Error, NotificationEntry[]>({
    mutationFn: async (entriesToSave) => {
      if (!notifPath) throw new Error('No path resolved.');
      const payload: ScumNotificationsFile = {
        Notifications: entriesToSave.map(editorEntryToRaw),
      };
      await invoke('manager_write_text_file', {
        path: notifPath,
        content: JSON.stringify(payload, null, 2),
      });
    },
    onSuccess: (_, saved) => {
      setBaseline(saved);
      setSaveSuccess(true);
      setTimeout(() => setSaveSuccess(false), 3000);
    },
  });

  const handleSave = () => {
    if (!isDirty || anyErrors) return;
    saveMutation.mutate(entries);
  };

  const handleReload = async () => {
    if (!notifPath) return;
    if (isDirty && !confirm('Discard local changes and reload from disk?')) return;
    await loadFile(notifPath);
  };

  const handleDiscard = () => {
    setEntries(baseline.map((e) => ({ ...e })));
  };

  const handleAddEntry = () => {
    setEntries((prev) => [defaultEntry(), ...prev]);
  };

  const handleResetToEmpty = () => {
    setEntries([]);
    setLoadState({ kind: 'ready' });
  };

  const openInExplorer = async () => {
    if (!notifPath) return;
    try {
      const { open } = await import('@tauri-apps/plugin-shell');
      const dir = notifPath.replace(/\/[^/]+$/, '');
      await open(dir);
    } catch {
      // shell plugin unavailable — silently ignore
    }
  };

  // Writes entries directly to disk. Pair to Fire Now flow; keeps baseline
  // untouched so the editor's dirty state reflects "user has not saved this
  // intentionally yet" — they can still review and remove the one-shot entry.
  const writeEntriesToDisk = useCallback(
    async (entriesToWrite: NotificationEntry[]) => {
      if (!notifPath) throw new Error('No path resolved.');
      const payload: ScumNotificationsFile = {
        Notifications: entriesToWrite.map(editorEntryToRaw),
      };
      await invoke('manager_write_text_file', {
        path: notifPath,
        content: JSON.stringify(payload, null, 2),
      });
    },
    [notifPath],
  );

  const handleFireNow = async () => {
    if (!notifPath) return;
    const fnDur = Number(fireNow.duration);
    if (!fireNow.message.trim()) { setFireNowStatus('Message is required.'); return; }
    if (isNaN(fnDur) || fnDur <= 0) { setFireNowStatus('"Show for" must be > 0.'); return; }

    setFireNowStatus('Writing…');

    // 'allday' + wait=1 (instead of 'once' at HH:MM) — SCUM's scheduler matches
    // 'once' against the wall clock and may skip an entry whose HH:MM is "in
    // the past this minute" until tomorrow. The wait-interval form fires on
    // the next scheduler tick after hot-reload regardless of time-of-day, so
    // the entry shows up within ~1 minute reliably.
    const shotEntry: NotificationEntry = {
      id: makeId(),
      day: 'Everyday',
      mode: 'allday',
      specificTimes: ['12:00'],
      windowFrom: '15:00',
      windowTo: '21:00',
      onceTime: '12:00',
      repeatEvery: '1',
      duration: fireNow.duration,
      color: fireNow.color,
      message: fireNow.message,
    };

    const withShot = [...entries, shotEntry];

    try {
      await writeEntriesToDisk(withShot);
    } catch (e) {
      setFireNowStatus(`Write failed: ${String(e)}`);
      return;
    }

    setEntries(withShot);
    setFireNowStatus(
      'Written. SCUM polls at minute granularity — expect the banner within ~1 minute.',
    );

    if (fireNow.autoRemove) {
      setTimeout(async () => {
        setEntries((current) => {
          const trimmed = current.filter((e) => e.id !== shotEntry.id);
          writeEntriesToDisk(trimmed).catch(() => {});
          return trimmed;
        });
        setFireNowStatus('One-shot entry auto-removed.');
        setTimeout(() => setFireNowStatus(null), 4000);
      }, 2 * 60 * 1000);
    }
  };

  const setFN = <K extends keyof FireNowState>(key: K, value: FireNowState[K]) =>
    setFireNow((prev) => ({ ...prev, [key]: value }));

  return (
    <div className="flex h-full flex-col gap-3">
      <header>
        <p className="font-display text-xs tracking-[0.3em] text-turd-mustard">
          TurdMOD
        </p>
        <h1 className="mt-1 font-display text-3xl text-turd-cream">Notifications</h1>
        <p className="mt-1 text-xs text-turd-cream-dim">
          Edit SCUM's server-wide banner overlay schedule. Hot-reloads — no restart needed.
        </p>
        {notifPath && (
          <p className="mt-0.5 break-all font-mono text-[10px] text-turd-cream-dim/60">
            {notifPath}
          </p>
        )}
      </header>

      {loadState.kind === 'no-install' && (
        <div className="glass rounded-xl p-8 text-center">
          <p className="font-display text-sm text-turd-cream-dim">
            No SCUM server install configured.
          </p>
          <p className="mt-1 text-xs text-turd-cream-dim/60">
            Configure your SCUM server install in{' '}
            <a
              href="/settings"
              className="text-turd-mustard hover:text-turd-mustard-bright"
              onClick={(e) => {
                e.preventDefault();
                window.location.hash = '/settings';
              }}
            >
              Settings
            </a>
            {' '}first.
          </p>
        </div>
      )}

      {loadState.kind === 'loading' && (
        <div className="glass rounded-xl p-4 font-mono text-xs text-turd-cream-dim">
          Loading…
        </div>
      )}

      {loadState.kind === 'parse-error' && (
        <div className="rounded-lg border border-turd-red/40 bg-turd-bg-mid/40 p-4">
          <p className="font-display text-sm text-turd-red">Failed to parse Notifications.json</p>
          <p className="mt-1 break-all font-mono text-[11px] text-turd-red/80">
            {loadState.message}
          </p>
          <button type="button" onClick={handleResetToEmpty} className={`${btnGhost} mt-3`}>
            Reset to empty file
          </button>
        </div>
      )}

      {loadState.kind === 'ready' && (
        <>
          <div className="flex flex-wrap items-center gap-2">
            <button type="button" onClick={handleAddEntry} className={btnPrimary}>
              + Add entry
            </button>
            <button
              type="button"
              onClick={() => { setFireNowOpen((o) => !o); setFireNowStatus(null); }}
              className={btnDanger}
            >
              Fire test message NOW
            </button>
            <button type="button" onClick={handleReload} className={btnGhost}>
              Reload from disk
            </button>
            <button
              type="button"
              onClick={handleSave}
              disabled={!isDirty || anyErrors || saveMutation.isPending}
              className={btnPrimary}
            >
              {saveMutation.isPending ? 'Saving…' : 'Save changes'}
            </button>
            {isDirty && (
              <span className="font-mono text-[10px] text-turd-mustard/70">• unsaved</span>
            )}
            {saveSuccess && !isDirty && (
              <span className="font-mono text-[10px] text-turd-green">✓ saved</span>
            )}
            <button
              type="button"
              onClick={handleDiscard}
              disabled={!isDirty}
              className={`${btnGhost} disabled:opacity-30`}
            >
              Discard changes
            </button>
            <button
              type="button"
              onClick={openInExplorer}
              className={`${btnGhost} ml-auto`}
              title="Open config folder in Explorer"
            >
              Open folder ↗
            </button>
          </div>

          {fireNowOpen && (
            <div className="rounded-lg border border-turd-red/30 bg-turd-bg-mid/60 p-4">
              <div className="mb-3 flex items-start justify-between gap-4">
                <div>
                  <p className="font-display text-xs uppercase tracking-wider text-turd-red/80">
                    Fire test message NOW
                  </p>
                  <p className="mt-0.5 font-mono text-[10px] text-turd-cream-dim/70">
                    Writes a wait=1-min entry and hot-reloads. SCUM's banner scheduler is
                    minute-granularity by design — true "instant" isn't possible from the JSON
                    file. For instant text, use Admin → broadcast chat (bridge path).
                  </p>
                </div>
                <button
                  type="button"
                  onClick={() => setFireNowOpen(false)}
                  className="shrink-0 rounded border border-turd-bronze/40 px-2 py-0.5 font-mono text-[10px] text-turd-cream-dim/60 hover:text-turd-cream"
                >
                  Cancel
                </button>
              </div>

              <div className="flex flex-wrap items-end gap-4">
                <div className="flex min-w-[14rem] flex-1 flex-col gap-1">
                  <span className={labelCls}>Message</span>
                  <input
                    type="text"
                    value={fireNow.message}
                    onChange={(e) => setFN('message', e.target.value)}
                    placeholder="Server restart in 10 minutes!"
                    className={`${inputCls} w-full`}
                  />
                </div>

                <ColorField
                  value={fireNow.color}
                  onChange={(v) => setFN('color', v)}
                />

                <div className="flex flex-col gap-1">
                  <span className={labelCls}>Show for</span>
                  <div className="flex items-center gap-1">
                    <input
                      type="number"
                      value={fireNow.duration}
                      onChange={(e) => setFN('duration', e.target.value)}
                      min={1}
                      className={`${inputCls} w-20`}
                    />
                    <span className="font-mono text-[10px] text-turd-cream-dim">sec</span>
                  </div>
                </div>

                <div className="flex flex-col gap-1">
                  <label className="flex cursor-pointer items-center gap-1.5">
                    <input
                      type="checkbox"
                      checked={fireNow.autoRemove}
                      onChange={(e) => setFN('autoRemove', e.target.checked)}
                      className="accent-turd-mustard"
                    />
                    <span className="font-mono text-[10px] text-turd-cream-dim">
                      Auto-remove after 2 min
                    </span>
                  </label>
                  <span className="font-mono text-[9px] leading-tight text-turd-cream-dim/50">
                    Deletes the entry from the file after it fires.
                  </span>
                </div>

                <button
                  type="button"
                  onClick={handleFireNow}
                  className={btnDanger}
                >
                  Fire
                </button>
              </div>

              {fireNowStatus && (
                <p
                  className={`mt-2 font-mono text-[10px] ${
                    fireNowStatus.startsWith('Write failed') ||
                    fireNowStatus.startsWith('Message') ||
                    fireNowStatus.startsWith('"Show')
                      ? 'text-turd-red'
                      : 'text-turd-mustard/80'
                  }`}
                >
                  {fireNowStatus}
                </p>
              )}
            </div>
          )}

          {saveMutation.isError && (
            <div className="rounded border border-turd-red/40 bg-turd-bg-deep px-3 py-2 font-mono text-[11px] text-turd-red">
              Save failed: {saveMutation.error?.message}
            </div>
          )}

          {anyErrors && (
            <div className="rounded border border-turd-mustard/40 bg-turd-bg-deep px-3 py-2 font-mono text-[11px] text-turd-mustard/80">
              Fix validation errors before saving.
            </div>
          )}

          <div className="flex flex-col gap-3">
            <p className="font-display text-xs uppercase tracking-wider text-turd-cream-dim">
              Entries ({entries.length})
            </p>
            {entries.length === 0 && (
              <div className="rounded-lg border border-turd-bronze/20 bg-turd-bg-deep p-8 text-center">
                <p className="font-mono text-xs text-turd-cream-dim">
                  No entries. Click "+ Add entry" to create one.
                </p>
              </div>
            )}
            {entries.map((entry, i) => (
              <EntryRow
                key={entry.id}
                entry={entry}
                index={i}
                total={entries.length}
                errors={allErrors[i]}
                onChange={(updated) =>
                  setEntries((prev) => prev.map((e, j) => (j === i ? updated : e)))
                }
                onRemove={() =>
                  setEntries((prev) => prev.filter((_, j) => j !== i))
                }
                onMoveUp={() => {
                  if (i === 0) return;
                  setEntries((prev) => {
                    const next = [...prev];
                    [next[i - 1], next[i]] = [next[i], next[i - 1]];
                    return next;
                  });
                }}
                onMoveDown={() => {
                  if (i === entries.length - 1) return;
                  setEntries((prev) => {
                    const next = [...prev];
                    [next[i], next[i + 1]] = [next[i + 1], next[i]];
                    return next;
                  });
                }}
              />
            ))}
          </div>
        </>
      )}
    </div>
  );
}
