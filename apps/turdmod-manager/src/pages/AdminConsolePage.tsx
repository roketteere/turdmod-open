import { useState } from 'react';
import { CommandConsole } from '../components/admin/CommandConsole';
import { PlayersTab } from '../components/admin/PlayersTab';
import { BroadcastNotifyTab } from '../components/admin/BroadcastNotifyTab';
import { AdminUsersPage } from './AdminUsersPage';
import type { AdminPageProps } from './AdminPage';
import { SlideTabs, FadeSwap, type TabSpec } from '../lib/motion';

// Unified Admin Console — collapses the six scattered admin surfaces
// (Admin, Admin Users, Admin Commands, Server Panels, Chat Console, TurdCOM
// Terminal) into one page with internal sub-tabs. All command dispatch flows
// through lib/admin-dispatch (single path, Force = zero-admin).

type TabKey = 'command' | 'players' | 'broadcast' | 'users';

const TABS: TabSpec<TabKey>[] = [
  { key: 'command', label: 'Command' },
  { key: 'players', label: 'Players' },
  { key: 'broadcast', label: 'Broadcast & Notify' },
  { key: 'users', label: 'Admin Users' },
];

export function AdminConsolePage({ welcomeMod }: AdminPageProps) {
  const [tab, setTab] = useState<TabKey>('command');

  return (
    <div className="flex h-full min-h-0 flex-col gap-4">
      <header>
        <p className="font-display text-[11px] font-semibold tracking-[0.32em] text-turd-mustard/90">TURDMOD</p>
        <h1 className="mt-1 font-display text-[2rem] font-bold leading-none text-turd-cream">Admin Console</h1>
        <p className="mt-1.5 text-xs text-turd-cream-dim">
          One console for every admin action. Commands dispatch through the bridge; Force runs them with zero admins online.
        </p>
      </header>

      <SlideTabs tabs={TABS} value={tab} onChange={setTab} layoutId="admin-console-tab" />

      <div className="min-h-0 flex-1">
        <FadeSwap swapKey={tab} className="h-full min-h-0">
          {tab === 'command' && <CommandConsole />}
          {tab === 'players' && <PlayersTab />}
          {tab === 'broadcast' && <BroadcastNotifyTab welcomeMod={welcomeMod} />}
          {tab === 'users' && (
            <div className="h-full min-h-0 overflow-auto">
              <AdminUsersPage />
            </div>
          )}
        </FadeSwap>
      </div>
    </div>
  );
}
