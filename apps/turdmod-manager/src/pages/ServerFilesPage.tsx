import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { Link } from 'react-router-dom';
import { useDetectedInstalls } from '../hooks/useTarget';

// ---------------------------------------------------------------------------
// ServerFilesPage — one-stop browser for every editable SCUM config file.
//
// Three categories:
//   - INIs that have dedicated editors → link to the editor page
//   - JSONs / INIs without editors → open the generic in-page editor below
//   - List files (admin/ban/whitelist) → link to Admin page
//
// Status column shows file size + last-modified ISO so it's clear at a
// glance whether a file exists and when it was last touched.
// ---------------------------------------------------------------------------

type FileMeta = {
  existed: boolean;
  bytes: number;
  modifiedMs: number;
};

const SCUM_CONFIG_REL = 'SCUM/Saved/Config/WindowsServer';

// One canonical inventory of files this server cares about. Edit this
// to surface a new file in the UI.
type ServerFileSpec = {
  filename: string;
  label: string;
  description: string;
  category: 'core' | 'json' | 'users' | 'misc';
  dedicatedRoute?: string; // present when an existing page handles it
  language: 'ini' | 'json' | 'text';
};

const FILES: ServerFileSpec[] = [
  {
    filename: 'ServerSettings.ini',
    label: 'Server Settings',
    description: '~250 keys controlling everything: game rules, slots, vehicle counts, prison points, drone exclusion, more.',
    category: 'core',
    dedicatedRoute: '/server-settings',
    language: 'ini',
  },
  {
    filename: 'Notifications.json',
    label: 'Notifications',
    description: 'Server-wide banner messages. Schedule by day/time, full RGB colors, placeholders for player count + server name.',
    category: 'json',
    dedicatedRoute: '/notifications',
    language: 'json',
  },
  {
    filename: 'RaidTimes.json',
    label: 'Raid Times',
    description: 'PVP raid windows — defines when base raiding is allowed. Per-day schedule. Edit raw JSON here; in-game spawn of GlobalRaidProtectionManager depends on this.',
    category: 'json',
    language: 'json',
  },
  {
    filename: 'EconomyOverride.json',
    label: 'Economy Override',
    description: 'Per-item / per-class trader price multipliers. Use for "weekend sales" or category bumps without a server restart.',
    category: 'json',
    language: 'json',
  },
  {
    filename: 'AdminUsers.ini',
    label: 'Admins',
    description: 'List of admin Steam IDs (and per-line role/permission flags). Each player who can run #admin commands.',
    category: 'users',
    dedicatedRoute: '/admin',
    language: 'ini',
  },
  {
    filename: 'BannedUsers.ini',
    label: 'Banned',
    description: 'List of banned Steam IDs (with optional reason + duration).',
    category: 'users',
    dedicatedRoute: '/admin',
    language: 'ini',
  },
  {
    filename: 'WhitelistedUsers.ini',
    label: 'Whitelist',
    description: 'List of whitelisted Steam IDs. Only matters when whitelist mode is enabled in ServerSettings.',
    category: 'users',
    dedicatedRoute: '/admin',
    language: 'ini',
  },
  {
    filename: 'SilencedUsers.ini',
    label: 'Silenced (muted)',
    description: 'List of muted Steam IDs. Cannot use any chat channel until removed.',
    category: 'users',
    language: 'ini',
  },
  {
    filename: 'ExclusiveUsers.ini',
    label: 'Exclusive (whitelist override)',
    description: 'List of Steam IDs that bypass slot limits / queue. Common for streamers / staff.',
    category: 'users',
    language: 'ini',
  },
  {
    filename: 'GameUserSettings.ini',
    label: 'Game User Settings',
    description: 'Engine-level graphics + framerate caps (server-side mostly irrelevant). Edit only if you know why.',
    category: 'misc',
    language: 'ini',
  },
  {
    filename: 'Input.ini',
    label: 'Input',
    description: 'Server-side input bindings. Rarely useful to edit.',
    category: 'misc',
    language: 'ini',
  },
];

const CATEGORY_LABEL: Record<ServerFileSpec['category'], string> = {
  core: 'Core game rules',
  json: 'JSON configs',
  users: 'User lists',
  misc: 'Engine / misc',
};

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

function formatAgo(ms: number): string {
  if (!ms) return '—';
  const delta = Date.now() - ms;
  if (delta < 0) return new Date(ms).toLocaleString();
  if (delta < 60_000) return `${Math.floor(delta / 1000)}s ago`;
  if (delta < 3_600_000) return `${Math.floor(delta / 60_000)}m ago`;
  if (delta < 86_400_000) return `${Math.floor(delta / 3_600_000)}h ago`;
  return new Date(ms).toLocaleDateString();
}

export function ServerFilesPage() {
  const detected = useDetectedInstalls();
  const installPath = detected.data?.server ?? null;

  const [openFilename, setOpenFilename] = useState<string | null>(null);
  const openSpec = openFilename ? FILES.find((f) => f.filename === openFilename) ?? null : null;

  if (!installPath) {
    return (
      <div className="max-w-2xl">
        <h1 className="mb-2 font-display text-lg tracking-widest text-turd-mustard-bright">
          Server Files
        </h1>
        <div className="rounded glass p-4 text-sm text-turd-cream-dim">
          <p>
            No SCUM Server install detected. Set one via the{' '}
            <Link to="/settings" className="text-turd-mustard-bright underline">
              Settings
            </Link>{' '}
            page or start the Engine on the{' '}
            <Link to="/engine" className="text-turd-mustard-bright underline">
              Engine
            </Link>{' '}
            page first.
          </p>
        </div>
      </div>
    );
  }

  const grouped: Record<ServerFileSpec['category'], ServerFileSpec[]> = {
    core: [],
    json: [],
    users: [],
    misc: [],
  };
  for (const f of FILES) grouped[f.category].push(f);

  return (
    <div className="space-y-6">
      <header>
        <h1 className="font-display text-lg tracking-widest text-turd-mustard-bright">
          Server Files
        </h1>
        <p className="mt-1 max-w-3xl text-xs text-turd-cream-dim">
          Every editable SCUM server config file in one place. Click "Edit"
          for an in-app editor, or follow the dedicated-editor link for
          richer UIs on the heavier files.
        </p>
        <p className="mt-1 text-[10px] text-turd-cream-dim/60">
          Install path:{' '}
          <code className="font-mono text-turd-cream">{installPath}</code>
        </p>
      </header>

      {(Object.keys(grouped) as Array<keyof typeof grouped>).map((cat) => (
        <section key={cat}>
          <h2 className="mb-2 text-xs uppercase tracking-wider text-turd-cream-dim/60">
            {CATEGORY_LABEL[cat]}
          </h2>
          <div className="overflow-x-auto rounded border border-turd-bronze/30">
            <table className="w-full min-w-[640px] text-sm">
              <thead className="bg-turd-bg-mid/40">
                <tr className="text-left text-[10px] uppercase tracking-wider text-turd-cream-dim/60">
                  <th className="px-3 py-2 font-normal">File</th>
                  <th className="px-3 py-2 font-normal">Size</th>
                  <th className="px-3 py-2 font-normal">Modified</th>
                  <th className="px-3 py-2 text-right font-normal">Actions</th>
                </tr>
              </thead>
              <tbody>
                {grouped[cat].map((spec) => (
                  <FileRow
                    key={spec.filename}
                    spec={spec}
                    installPath={installPath}
                    onEdit={() => setOpenFilename(spec.filename)}
                  />
                ))}
              </tbody>
            </table>
          </div>
        </section>
      ))}

      {openSpec && (
        <FileEditor
          spec={openSpec}
          installPath={installPath}
          onClose={() => setOpenFilename(null)}
        />
      )}
    </div>
  );
}

function FileRow({
  spec,
  installPath,
  onEdit,
}: {
  spec: ServerFileSpec;
  installPath: string;
  onEdit: () => void;
}) {
  const fullPath = `${installPath}/${SCUM_CONFIG_REL}/${spec.filename}`;
  const meta = useQuery({
    queryKey: ['file-meta', fullPath],
    queryFn: () => invoke<FileMeta>('manager_file_meta', { path: fullPath }),
    refetchOnWindowFocus: false,
  });

  const openExternally = async () => {
    try {
      await invoke('manager_open_in_default_app', { path: fullPath });
    } catch (e) {
      alert(`Open externally failed: ${e}`);
    }
  };

  return (
    <tr className="border-t border-turd-bronze/20">
      <td className="px-3 py-2 align-top">
        <div className="font-mono text-xs text-turd-cream">{spec.filename}</div>
        <div className="mt-0.5 max-w-xl text-[11px] text-turd-cream-dim">
          {spec.description}
        </div>
      </td>
      <td className="px-3 py-2 align-top font-mono text-[11px] text-turd-cream-dim">
        {meta.data ? (meta.data.existed ? formatBytes(meta.data.bytes) : 'missing') : '…'}
      </td>
      <td className="px-3 py-2 align-top font-mono text-[11px] text-turd-cream-dim">
        {meta.data?.existed ? formatAgo(meta.data.modifiedMs) : '—'}
      </td>
      <td className="px-3 py-2 align-top">
        <div className="flex flex-wrap justify-end gap-1.5">
          {spec.dedicatedRoute && (
            <Link
              to={spec.dedicatedRoute}
              className="whitespace-nowrap rounded bg-turd-mustard-bright px-2 py-1 text-[11px] font-medium text-turd-bg-deep hover:bg-turd-mustard"
            >
              Editor
            </Link>
          )}
          <button
            onClick={onEdit}
            className="whitespace-nowrap rounded bg-turd-bg-soft px-2 py-1 text-[11px] text-turd-cream hover:bg-turd-bg-soft/80"
          >
            Raw
          </button>
          <button
            onClick={openExternally}
            className="whitespace-nowrap rounded bg-turd-bg-soft px-2 py-1 text-[11px] text-turd-cream hover:bg-turd-bg-soft/80"
            title="Open in OS default app (Notepad)"
          >
            External
          </button>
        </div>
      </td>
    </tr>
  );
}

function FileEditor({
  spec,
  installPath,
  onClose,
}: {
  spec: ServerFileSpec;
  installPath: string;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const fullPath = `${installPath}/${SCUM_CONFIG_REL}/${spec.filename}`;
  const [draft, setDraft] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const read = useQuery({
    queryKey: ['file-content', fullPath],
    queryFn: async () => {
      const r = await invoke<{ content: string; existed: boolean }>(
        'manager_read_text_file',
        { path: fullPath },
      );
      // Seed the draft on first load.
      setDraft((prev) => (prev === null ? r.content : prev));
      return r;
    },
    refetchOnWindowFocus: false,
  });

  const save = useMutation({
    mutationFn: async () => {
      if (draft == null) return;
      // Optional: validate JSON files before save so we don't write
      // broken JSON to a live SCUM config.
      if (spec.language === 'json') {
        try {
          JSON.parse(draft);
        } catch (e) {
          throw new Error(`JSON parse error: ${e}`);
        }
      }
      await invoke('manager_write_text_file', { path: fullPath, content: draft });
    },
    onSuccess: () => {
      setError(null);
      qc.invalidateQueries({ queryKey: ['file-meta', fullPath] });
      qc.invalidateQueries({ queryKey: ['file-content', fullPath] });
    },
    onError: (e) => setError(String(e)),
  });

  const dirty =
    draft != null && read.data != null && draft !== read.data.content;

  return (
    <div
      onClick={onClose}
      className="fixed inset-0 z-50 flex items-stretch bg-black/60 p-6"
    >
      <div
        onClick={(e) => e.stopPropagation()}
        className="flex w-full flex-col overflow-hidden rounded-lg border border-turd-bronze/40 bg-turd-bg-deep shadow-2xl"
      >
        <header className="flex items-center gap-3 border-b border-turd-bronze/30 bg-turd-bg-mid/40 px-4 py-3">
          <div>
            <h2 className="font-display text-sm tracking-widest text-turd-mustard-bright">
              {spec.label}
            </h2>
            <p className="text-[10px] text-turd-cream-dim">
              <code className="font-mono">{fullPath}</code>
            </p>
          </div>
          <div className="ml-auto flex gap-2">
            <button
              onClick={() => save.mutate()}
              disabled={!dirty || save.isPending}
              className="rounded bg-turd-mustard-bright px-3 py-1.5 text-xs font-medium text-turd-bg-deep transition-colors hover:bg-turd-mustard disabled:cursor-not-allowed disabled:opacity-40"
            >
              {save.isPending ? 'Saving…' : 'Save'}
            </button>
            <button
              onClick={onClose}
              className="rounded bg-turd-bg-soft px-3 py-1.5 text-xs text-turd-cream hover:bg-turd-bg-soft/80"
            >
              Close
            </button>
          </div>
        </header>

        <div className="border-b border-turd-bronze/20 bg-turd-bg-mid/20 px-4 py-2 text-[10px] text-turd-cream-dim">
          {spec.description}
          {dirty && (
            <span className="ml-3 text-turd-mustard-bright">
              • unsaved changes
            </span>
          )}
          {save.isSuccess && !dirty && (
            <span className="ml-3 text-turd-green">• saved</span>
          )}
          {error && <span className="ml-3 text-red-400">• {error}</span>}
        </div>

        {read.isPending && (
          <div className="flex flex-1 items-center justify-center text-xs text-turd-cream-dim">
            Loading…
          </div>
        )}
        {read.isError && (
          <div className="flex flex-1 items-center justify-center text-xs text-red-400">
            {String(read.error)}
          </div>
        )}
        {read.data && (
          <textarea
            value={draft ?? read.data.content}
            onChange={(e) => setDraft(e.target.value)}
            spellCheck={false}
            className="flex-1 resize-none bg-turd-bg-deep p-4 font-mono text-xs text-turd-cream focus:outline-none"
          />
        )}
      </div>
    </div>
  );
}
