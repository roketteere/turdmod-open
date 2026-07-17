// Admin tab — left section list + right editor pane.

import { useState } from 'react';
import type { AdminFilename } from '../../lib/tauri-admin';
import { UserListEditor } from './UserListEditor';
import { ServerSettingsForm } from './ServerSettingsForm';

interface AdminTabProps {
  serverId: string;
}

type Section =
  | 'bans'
  | 'admins'
  | 'whitelist'
  | 'silenced'
  | 'exclusive'
  | 'server-config';

interface SectionDef {
  id: Section;
  label: string;
  description: string;
}

const SECTIONS: SectionDef[] = [
  { id: 'bans', label: 'Bans', description: 'Steam64 IDs blocked from joining.' },
  { id: 'admins', label: 'Admins', description: 'Steam64 IDs with admin privileges.' },
  { id: 'whitelist', label: 'Whitelist', description: 'Only these IDs can join when whitelist is enforced.' },
  { id: 'silenced', label: 'Silenced', description: 'Steam64 IDs whose chat is muted.' },
  { id: 'exclusive', label: 'Exclusive', description: 'Priority queue / reserved slots.' },
  { id: 'server-config', label: 'Server Config', description: 'Edit common ServerSettings.ini fields.' },
];

const FILE_FOR_SECTION: Partial<Record<Section, AdminFilename>> = {
  bans: 'BannedUsers.ini',
  admins: 'AdminUsers.ini',
  whitelist: 'WhitelistedUsers.ini',
  silenced: 'SilencedUsers.ini',
  exclusive: 'ExclusiveUsers.ini',
};

export function AdminTab({ serverId }: AdminTabProps) {
  const [section, setSection] = useState<Section>('bans');
  const current = SECTIONS.find((s) => s.id === section)!;
  const filename = FILE_FOR_SECTION[section];

  return (
    <div className="flex h-full gap-0">
      <nav className="w-44 shrink-0 border-r border-turd-bronze/20 pr-3">
        <p className="mb-2 font-mono text-[9px] uppercase tracking-widest text-turd-cream-dim/60">
          Section
        </p>
        <ul className="flex flex-col gap-0.5">
          {SECTIONS.map((s) => (
            <li key={s.id}>
              <button
                type="button"
                onClick={() => setSection(s.id)}
                className={[
                  'w-full rounded px-3 py-2 text-left font-display text-xs uppercase tracking-wider transition-colors',
                  section === s.id
                    ? 'bg-turd-bronze/20 text-turd-mustard-bright'
                    : 'text-turd-cream-dim hover:bg-turd-bg-soft/30 hover:text-turd-cream',
                ].join(' ')}
              >
                {s.label}
              </button>
            </li>
          ))}
        </ul>
      </nav>

      <div className="flex-1 overflow-auto pl-5">
        <div className="mb-4">
          <h3 className="font-display text-base text-turd-cream">{current.label}</h3>
          <p className="mt-0.5 text-xs text-turd-cream-dim">{current.description}</p>
        </div>

        {filename && section !== 'server-config' ? (
          <UserListEditor
            serverId={serverId}
            filename={filename}
            label={current.label}
          />
        ) : (
          <ServerSettingsForm serverId={serverId} />
        )}
      </div>
    </div>
  );
}
