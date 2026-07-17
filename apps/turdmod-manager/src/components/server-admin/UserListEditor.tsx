// Steam64-ID list editor used for Bans / Admins / Whitelist / Silenced / Exclusive.

import { useState } from 'react';
import { FILE_HOT_RELOAD, isValidSteam64, type AdminFilename } from '../../lib/tauri-admin';
import { useSaveUserList, useUserList } from '../../hooks/useAdmin';
import { Toast } from './Toast';

interface UserListEditorProps {
  serverId: string;
  filename: AdminFilename;
  label: string;
}

export function UserListEditor({ serverId, filename, label }: UserListEditorProps) {
  const { ids, isLoading, error } = useUserList(serverId, filename);
  const save = useSaveUserList();

  const [localIds, setLocalIds] = useState<string[] | null>(null);
  const [addInput, setAddInput] = useState('');
  const [addError, setAddError] = useState('');
  const [toast, setToast] = useState<{ msg: string; kind: 'ok' | 'warn' } | null>(null);

  const displayed = localIds ?? ids ?? [];
  const isDirty = localIds !== null;
  const hotReload = FILE_HOT_RELOAD[filename];

  function initLocal() {
    if (localIds === null && ids !== undefined) setLocalIds([...ids]);
  }

  function handleAdd() {
    const id = addInput.trim();
    if (!id) return;
    if (!isValidSteam64(id)) {
      setAddError('Must be a 17-digit Steam64 ID starting with 7656119.');
      return;
    }
    if (displayed.includes(id)) {
      setAddError('Already in the list.');
      return;
    }
    setAddError('');
    initLocal();
    setLocalIds((prev) => [...(prev ?? displayed), id]);
    setAddInput('');
  }

  function handleRemove(id: string) {
    initLocal();
    setLocalIds((prev) => (prev ?? displayed).filter((x) => x !== id));
  }

  function handleSave() {
    save.mutate(
      { serverId, filename, ids: localIds ?? [] },
      {
        onSuccess: () => {
          setLocalIds(null);
          setToast({
            msg: hotReload
              ? 'Applied — active immediately.'
              : 'Saved — applies on next server restart.',
            kind: hotReload ? 'ok' : 'warn',
          });
        },
        onError: (err) => {
          setToast({ msg: `Save failed: ${err.message}`, kind: 'warn' });
        },
      },
    );
  }

  if (isLoading) {
    return <p className="text-xs text-turd-cream-dim">Loading {label}…</p>;
  }

  if (error) {
    return (
      <p className="text-xs text-turd-red">
        Failed to load {label}: {error.message}
      </p>
    );
  }

  return (
    <div className="flex flex-col gap-4">
      {toast && (
        <Toast kind={toast.kind} onClose={() => setToast(null)}>
          {toast.msg}
        </Toast>
      )}

      <div className="flex gap-2">
        <input
          type="text"
          value={addInput}
          onChange={(e) => {
            setAddInput(e.target.value);
            setAddError('');
          }}
          onKeyDown={(e) => {
            if (e.key === 'Enter') handleAdd();
          }}
          placeholder="76561198xxxxxxxxx"
          className="flex-1 rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-2 font-mono text-sm text-turd-cream placeholder:text-turd-cream-dim/60 focus:border-turd-mustard focus:outline-none"
        />
        <button
          type="button"
          onClick={handleAdd}
          className="rounded border border-turd-bronze/60 bg-turd-bg-soft px-4 py-2 font-display text-xs uppercase tracking-wider text-turd-mustard-bright transition-colors hover:border-turd-mustard"
        >
          Add
        </button>
      </div>

      {addError && <p className="text-xs text-turd-red">{addError}</p>}

      {displayed.length === 0 ? (
        <p className="text-xs text-turd-cream-dim">No entries. Add a Steam64 ID above.</p>
      ) : (
        <ul className="flex max-h-96 flex-col gap-1.5 overflow-y-auto pr-1">
          {displayed.map((id) => (
            <li
              key={id}
              className="flex items-center justify-between rounded border border-turd-bronze/20 bg-turd-bg-soft/40 px-3 py-2"
            >
              <span className="font-mono text-sm text-turd-cream">{id}</span>
              <button
                type="button"
                onClick={() => handleRemove(id)}
                className="rounded border border-turd-red/40 px-2 py-0.5 font-display text-[10px] uppercase tracking-wider text-turd-red transition-colors hover:border-turd-red"
              >
                Remove
              </button>
            </li>
          ))}
        </ul>
      )}

      <div className="flex items-center justify-between border-t border-turd-bronze/20 pt-3">
        <p className="text-[10px] text-turd-cream-dim">
          {hotReload
            ? 'Changes take effect immediately (no restart needed).'
            : 'Changes require a server restart to take effect.'}
        </p>
        <button
          type="button"
          disabled={!isDirty || save.isPending}
          onClick={handleSave}
          className="rounded border border-turd-bronze/60 bg-turd-bg-soft px-4 py-1.5 font-display text-xs uppercase tracking-wider text-turd-mustard-bright transition-colors hover:border-turd-mustard disabled:cursor-not-allowed disabled:opacity-40"
        >
          {save.isPending ? 'Saving…' : 'Save'}
        </button>
      </div>
    </div>
  );
}
