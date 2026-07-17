import { useState } from 'react';
import { save } from '@tauri-apps/plugin-dialog';
import { invoke } from '@tauri-apps/api/core';

// ---------------------------------------------------------------------------
// ExportControls — reusable export buttons for the Schema / Functions pages
// and any future inventory page. Two scopes (current selection, full visible
// list) × two destinations (clipboard, save-to-file). Render only the
// buttons whose data is available.
//
// Save-to-file uses Tauri's dialog + fs plugins (already in Manager). Falls
// back to clipboard silently if the user cancels the save dialog.
// ---------------------------------------------------------------------------

export interface ExportControlsProps {
  /** Optional: the single currently-selected item. Renders a "Selected" pair if non-null. */
  selected?: unknown;
  /** Label for the selected payload — used in filename + button label. e.g. "ChatWidget". */
  selectedLabel?: string;
  /** Optional: the full visible list. Renders an "All" pair if non-null. */
  all?: unknown;
  /** Label for the all-visible payload — used in filename + button label. e.g. "widgets". */
  allLabel?: string;
  /** Extension for save-as. Default 'json'. */
  ext?: string;
}

type CopyState = 'idle' | 'copied' | 'error';

export function ExportControls(props: ExportControlsProps) {
  const ext = props.ext ?? 'json';
  const [state, setState] = useState<CopyState>('idle');
  const [errMsg, setErrMsg] = useState<string | null>(null);

  // Flash "done"/"failed" feedback for ~3s (slightly longer so the error
  // tooltip is readable). Errors carry their actual message for hover.
  const flash = (s: CopyState, msg?: string) => {
    setState(s);
    setErrMsg(msg ?? null);
    setTimeout(() => {
      setState('idle');
      setErrMsg(null);
    }, 3000);
  };

  const stringify = (data: unknown) =>
    JSON.stringify(data, null, 2);

  const copy = async (data: unknown) => {
    try {
      await navigator.clipboard.writeText(stringify(data));
      flash('copied');
    } catch (e) {
      flash('error', String((e as Error)?.message ?? e));
    }
  };

  const saveAs = async (data: unknown, suggestedName: string) => {
    try {
      const path = await save({
        defaultPath: `${suggestedName}.${ext}`,
        filters: [{ name: ext.toUpperCase(), extensions: [ext] }],
      });
      if (!path) return; // user cancelled
      // Goes through Rust-side write instead of plugin-fs writeTextFile,
      // which would reject the user-chosen path under the default scope.
      await invoke('manager_write_text_file', { path, content: stringify(data) });
      flash('copied');
    } catch (e) {
      flash('error', String((e as Error)?.message ?? e));
    }
  };

  const btn =
    'rounded border border-turd-bronze/60 bg-turd-bg-soft px-2.5 py-1 font-mono text-[10px] uppercase tracking-wider text-turd-cream-dim transition-colors hover:border-turd-cream hover:text-turd-cream disabled:cursor-not-allowed disabled:opacity-30';

  return (
    <div className="flex items-center gap-2">
      {props.selected !== undefined && props.selected !== null && (
        <>
          <button
            type="button"
            onClick={() => copy(props.selected)}
            className={btn}
            title="Copy selected as JSON"
          >
            Copy Sel
          </button>
          <button
            type="button"
            onClick={() =>
              saveAs(
                props.selected,
                (props.selectedLabel ?? 'selection').replace(/[^A-Za-z0-9_-]/g, '_'),
              )
            }
            className={btn}
            title="Save selected as JSON file"
          >
            Save Sel
          </button>
        </>
      )}
      {props.all !== undefined && props.all !== null && (
        <>
          <button
            type="button"
            onClick={() => copy(props.all)}
            className={btn}
            title="Copy all visible as JSON"
          >
            Copy All
          </button>
          <button
            type="button"
            onClick={() =>
              saveAs(props.all, (props.allLabel ?? 'export').replace(/[^A-Za-z0-9_-]/g, '_'))
            }
            className={btn}
            title="Save all visible as JSON file"
          >
            Save All
          </button>
        </>
      )}
      {state === 'copied' && (
        <span className="font-mono text-[10px] text-turd-green">✓ done</span>
      )}
      {state === 'error' && (
        <span
          className="font-mono text-[10px] text-turd-red"
          title={errMsg ?? 'unknown error'}
        >
          failed{errMsg ? `: ${errMsg.slice(0, 60)}${errMsg.length > 60 ? '…' : ''}` : ''}
        </span>
      )}
    </div>
  );
}
