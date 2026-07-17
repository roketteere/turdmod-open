import { useEffect, useRef, useState, type FormEvent } from 'react';
import type { ConfigField, ModConfig } from '../lib/tauri-mod-state';

interface ConfigModalProps {
  modName: string;
  slug: string;
  schema: ConfigField[];
  initialConfig: ModConfig;
  onSave: (config: ModConfig) => void;
  onClose: () => void;
  saving: boolean;
}

export function ConfigModal({
  modName,
  schema,
  initialConfig,
  onSave,
  onClose,
  saving,
}: ConfigModalProps) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const headingId = 'config-modal-heading';

  const [values, setValues] = useState<Record<string, string>>(() => {
    const init: Record<string, string> = {};
    for (const field of schema) {
      const saved = initialConfig[field.key];
      init[field.key] = saved !== undefined ? String(saved) : String(field.default);
    }
    return init;
  });

  useEffect(() => {
    const el = dialogRef.current;
    if (!el) return;
    el.showModal();
    const onClickOutside = (e: MouseEvent) => {
      if (e.target === el) onClose();
    };
    el.addEventListener('click', onClickOutside);
    return () => el.removeEventListener('click', onClickOutside);
  }, [onClose]);

  useEffect(() => {
    const el = dialogRef.current;
    if (!el) return;
    const onCancel = (e: Event) => {
      e.preventDefault();
      onClose();
    };
    el.addEventListener('cancel', onCancel);
    return () => el.removeEventListener('cancel', onCancel);
  }, [onClose]);

  function handleSubmit(e: FormEvent) {
    e.preventDefault();
    const coerced: ModConfig = {};
    for (const field of schema) {
      const raw = values[field.key] ?? String(field.default);
      if (field.type === 'number') {
        coerced[field.key] = Number(raw);
      } else if (field.type === 'boolean') {
        coerced[field.key] = raw === 'true';
      } else {
        coerced[field.key] = raw;
      }
    }
    onSave(coerced);
  }

  function set(key: string, value: string) {
    setValues((prev) => ({ ...prev, [key]: value }));
  }

  const inputCls =
    'rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-2 font-mono text-sm text-turd-cream placeholder:text-turd-cream-dim/60 focus:border-turd-mustard focus:outline-none w-full';

  return (
    <dialog
      ref={dialogRef}
      aria-labelledby={headingId}
      aria-modal="true"
      className="w-full max-w-lg rounded-xl border border-turd-bronze/40 bg-turd-bg-deep p-0 text-turd-cream shadow-2xl backdrop:bg-black/60 open:flex open:flex-col"
    >
      <header className="border-b border-turd-bronze/30 px-6 py-4">
        <p className="font-display text-xs tracking-[0.3em] text-turd-mustard">
          Configure
        </p>
        <h2 id={headingId} className="mt-0.5 font-display text-xl text-turd-cream">
          {modName}
        </h2>
      </header>

      <form onSubmit={handleSubmit} className="flex flex-col overflow-y-auto">
        <div className="space-y-5 px-6 py-5">
          {schema.map((field) => (
            <FieldRow
              key={field.key}
              field={field}
              value={values[field.key] ?? String(field.default)}
              onChange={(v) => set(field.key, v)}
              inputCls={inputCls}
            />
          ))}
        </div>

        <footer className="flex justify-end gap-2 border-t border-turd-bronze/30 px-6 py-4">
          <button
            type="button"
            onClick={onClose}
            disabled={saving}
            className="rounded border border-turd-bronze px-4 py-1.5 text-sm text-turd-cream transition-colors hover:border-turd-mustard hover:text-turd-mustard disabled:cursor-not-allowed disabled:opacity-60"
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={saving}
            className="rounded bg-turd-mustard px-4 py-1.5 text-sm font-medium text-turd-bg-deep transition-colors hover:bg-turd-mustard-bright disabled:cursor-not-allowed disabled:opacity-60"
          >
            {saving ? 'Saving…' : 'Save'}
          </button>
        </footer>
      </form>
    </dialog>
  );
}

function FieldRow({
  field,
  value,
  onChange,
  inputCls,
}: {
  field: ConfigField;
  value: string;
  onChange: (v: string) => void;
  inputCls: string;
}) {
  return (
    <div>
      <label
        htmlFor={`cfg-${field.key}`}
        className="mb-1.5 block text-sm font-medium text-turd-cream"
      >
        {field.label ?? field.key}
      </label>
      {field.description && (
        <p className="mb-2 text-xs text-turd-cream-dim">{field.description}</p>
      )}
      <FieldInput field={field} value={value} onChange={onChange} inputCls={inputCls} />
    </div>
  );
}

function FieldInput({
  field,
  value,
  onChange,
  inputCls,
}: {
  field: ConfigField;
  value: string;
  onChange: (v: string) => void;
  inputCls: string;
}) {
  const id = `cfg-${field.key}`;

  if (field.type === 'boolean') {
    return (
      <div className="flex items-center gap-3">
        <input
          id={id}
          type="checkbox"
          checked={value === 'true'}
          onChange={(e) => onChange(e.target.checked ? 'true' : 'false')}
          className="h-4 w-4 rounded border-turd-bronze bg-turd-bg-soft accent-turd-mustard"
          aria-label={field.label ?? field.key}
        />
        <span className="text-sm text-turd-cream-dim">
          {value === 'true' ? 'Enabled' : 'Disabled'}
        </span>
      </div>
    );
  }

  if (field.type === 'enum') {
    return (
      <select
        id={id}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className={inputCls}
        aria-label={field.label ?? field.key}
      >
        {field.options.map((opt) => (
          <option key={opt} value={opt}>
            {opt}
          </option>
        ))}
      </select>
    );
  }

  if (field.type === 'number') {
    return (
      <input
        id={id}
        type="number"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        min={field.min}
        max={field.max}
        step={1}
        className={inputCls}
        aria-label={field.label ?? field.key}
      />
    );
  }

  return (
    <input
      id={id}
      type="text"
      value={value}
      onChange={(e) => onChange(e.target.value)}
      className={inputCls}
      aria-label={field.label ?? field.key}
    />
  );
}
