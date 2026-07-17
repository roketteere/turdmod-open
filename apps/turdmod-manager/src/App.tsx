import { NavLink, Route, Routes, useLocation } from 'react-router-dom';
import { BrowsePage } from './pages/BrowsePage';
import { LibraryPage } from './pages/LibraryPage';
import { ModDetailPage } from './pages/ModDetailPage';
import { SettingsPage } from './pages/SettingsPage';
import { AccountPage } from './pages/AccountPage';
import { ServersPage } from './pages/ServersPage';
import { ClientEnginePage } from './pages/ClientEnginePage';
import { EnginePage } from './pages/EnginePage';
import { MapPage } from './pages/MapPage';
import { EconomyPage } from './pages/EconomyPage';
import { LootPage } from './pages/LootPage';
import { ConsolePage } from './pages/ConsolePage';
import { SchemaPage } from './pages/SchemaPage';
import { ReflectionDatabasePage } from './pages/ReflectionDatabasePage';
import { BridgeSmokePage } from './pages/BridgeSmokePage';
import { ServerFilesPage } from './pages/ServerFilesPage';
import { AdminCommandsPage } from './pages/AdminCommandsPage';
import { ChatConsolePage } from './pages/ChatConsolePage';
import { ScumDbPage } from './pages/ScumDbPage';
import { TurdComTerminalPage } from './pages/TurdComTerminalPage';
import { EngineConsolePage } from './pages/EngineConsolePage';
import { JsonInspectorPage } from './pages/JsonInspectorPage';
import { FunctionsPage } from './pages/FunctionsPage';
import { AdminPage } from './pages/AdminPage';
import { AdminConsolePage } from './pages/AdminConsolePage';
import { AdminUsersPage } from './pages/AdminUsersPage';
import { PilotPage } from './pages/PilotPage';
import { MonitorPage } from './pages/MonitorPage';
import { GuiBuilderPage } from './pages/GuiBuilderPage';
import { CustomPanelsPage } from './pages/CustomPanelsPage';
import { ServerPanelsPage } from './pages/ServerPanelsPage';
import { NotificationsEditorPage } from './pages/NotificationsEditorPage';
import { OllamaBridgePage } from './pages/OllamaBridgePage';
import { ServerSettingsPage } from './pages/ServerSettingsPage';
import { TradersConfigPage } from './pages/TradersConfigPage';
import { SafeZonesPage } from './pages/SafeZonesPage';
import { VehicleManagerPage } from './pages/VehicleManagerPage';
import { PermissionsPage } from './pages/PermissionsPage';
import { DumpManagementPage } from './pages/DumpManagementPage';
import { DumpArchivePage } from './pages/DumpArchivePage';
import { AiAssistantPage } from './pages/AiAssistantPage';
import { DumpUpdateBanner } from './components/DumpUpdateBanner';
import { useQueryClient } from '@tanstack/react-query';
import { motion } from 'framer-motion';
import { useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { getEngineHost } from './lib/engineHost';
import { startConsoleStore } from './lib/console-store';
import WirePage from './pages/WirePage';
import { initWireStore } from './lib/wireStore';
import { ClaudeStatusIndicator } from './components/ClaudeStatusIndicator';
import { CompanionStatusBadge } from './components/CompanionStatusBadge';
import { TargetSwitcher } from './components/TargetSwitcher';
import { EngineHostSwitcher } from './components/EngineHostSwitcher';
import { ThemeSwitcher } from './components/ThemeSwitcher';
import { TitleBar } from './components/TitleBar';
import { ContextMenuProvider } from './components/ContextMenuProvider';
import { FirstRunWizard } from './wizard/FirstRunWizard';
import { useSettings } from './hooks/useSettings';
import { useWelcomeMod } from './hooks/useWelcomeMod';

// Grouped nav. Leaves render as single sidebar entries; groups render
// as one sidebar entry whose label highlights when ANY child route is
// active. When a grouped child is active, a horizontal tab bar appears
// at the top of the content area listing the group's siblings — so
// clicking the group in the sidebar lands on its first child and the
// admin can flip between siblings via tabs.
//
// Routes themselves stay flat (no nested URL segments) so links and
// localStorage keys keyed by pathname don't break.
type NavLeaf = { kind: 'leaf'; to: string; label: string; exact?: boolean };
type NavGroup = { kind: 'group'; label: string; children: NavLeaf[] };
type NavItem = NavLeaf | NavGroup;

const NAV_ITEMS: NavItem[] = [
  { kind: 'leaf', to: '/', label: 'Browse', exact: true },
  { kind: 'leaf', to: '/library', label: 'Library' },
  { kind: 'leaf', to: '/servers', label: 'Servers' },
  { kind: 'leaf', to: '/map', label: 'Map' },
  {
    kind: 'group',
    label: 'Game',
    children: [
      { kind: 'leaf', to: '/economy', label: 'Economy' },
      { kind: 'leaf', to: '/loot', label: 'Loot' },
    ],
  },
  {
    kind: 'group',
    label: 'Engine',
    children: [
      { kind: 'leaf', to: '/engine', label: 'Server Engine' },
      { kind: 'leaf', to: '/client-engine', label: 'Client Engine' },
      { kind: 'leaf', to: '/console', label: 'Console' },
      { kind: 'leaf', to: '/bridge-smoke', label: 'Bridge Smoke' },
      { kind: 'leaf', to: '/wire', label: 'The Wire' },
    ],
  },
  {
    kind: 'group',
    label: 'Admin',
    children: [
      { kind: 'leaf', to: '/admin', label: 'Admin Console' },
      { kind: 'leaf', to: '/monitor', label: 'Live Monitor' },
      { kind: 'leaf', to: '/database', label: 'Game Database' },
      { kind: 'leaf', to: '/pilot', label: 'Pilot (AI)' },
      { kind: 'leaf', to: '/engine-console', label: 'Engine Console' },
    ],
  },
  {
    kind: 'group',
    label: 'Inspect',
    children: [
      { kind: 'leaf', to: '/schema', label: 'Schema' },
      { kind: 'leaf', to: '/reflection-db', label: 'Reflection DB' },
      { kind: 'leaf', to: '/functions', label: 'Functions' },
      { kind: 'leaf', to: '/json-inspector', label: 'JSON Inspector' },
    ],
  },
  {
    kind: 'group',
    label: 'Builder',
    children: [
      { kind: 'leaf', to: '/gui-builder', label: 'GUI Builder' },
      { kind: 'leaf', to: '/custom-panels', label: 'Custom Panels' },
      { kind: 'leaf', to: '/notifications', label: 'Notifications' },
      { kind: 'leaf', to: '/dump-management', label: 'Dump Management' },
      { kind: 'leaf', to: '/dump-archive', label: 'Forensic Archive' },
    ],
  },
  {
    kind: 'group',
    label: 'Server Config',
    children: [
      { kind: 'leaf', to: '/server-settings', label: 'Server Settings' },
      { kind: 'leaf', to: '/traders', label: 'Traders' },
      { kind: 'leaf', to: '/server-files', label: 'Server Files' },
      { kind: 'leaf', to: '/vehicle-manager', label: 'Vehicle Manager' },
      { kind: 'leaf', to: '/safe-zones', label: 'Safe Zones' },
      { kind: 'leaf', to: '/permissions', label: 'Permissions' },
    ],
  },
  {
    kind: 'group',
    label: 'Tools',
    children: [
      { kind: 'leaf', to: '/ollama-bridge', label: 'Ollama Bridge' },
      { kind: 'leaf', to: '/ai-assistant', label: 'AI Assistant' },
    ],
  },
  { kind: 'leaf', to: '/account', label: 'Account' },
  { kind: 'leaf', to: '/settings', label: 'Settings' },
];

function pathMatches(pathname: string, to: string, exact?: boolean): boolean {
  if (exact) return pathname === to;
  if (to === '/') return pathname === '/';
  return pathname === to || pathname.startsWith(to + '/');
}

function findActiveGroup(pathname: string): NavGroup | null {
  for (const item of NAV_ITEMS) {
    if (item.kind !== 'group') continue;
    if (item.children.some((c) => pathMatches(pathname, c.to, c.exact))) {
      return item;
    }
  }
  return null;
}

export function App() {
  const qc = useQueryClient();
  const { data: settings, isLoading } = useSettings();
  // Welcome Mod polling runs at App level so it keeps firing regardless of
  // which page the user is on. AdminPage owns the config UI.
  const welcomeMod = useWelcomeMod();

  // Kick off the global console subscriptions exactly once at App boot.
  // Lines accumulate in the module-level buffer regardless of which page
  // is mounted — Console page stays populated when you navigate away.
  useEffect(() => {
    startConsoleStore();
    initWireStore();
    // Auto-ensure the remote server SSH tunnel when targeting the remote host, so server
    // controls / monitor / admin-users reach remote server without a manual tunnel start.
    // Idempotent: only (re)starts the tunnel if its local /health is actually down.
    if (getEngineHost() === 'remote') {
      invoke('tunnel_ensure').catch(() => {});
    }
  }, []);

  if (isLoading) {
    return (
      <div className="flex h-full items-center justify-center bg-turd-bg-deep">
        <p className="font-display text-xs uppercase tracking-widest text-turd-cream-dim/40">
          Loading…
        </p>
      </div>
    );
  }

  if (settings && !settings.wizardCompleted) {
    return (
      <FirstRunWizard
        onDone={() => {
          qc.invalidateQueries({ queryKey: ['settings'] });
        }}
      />
    );
  }

  return (
    <ContextMenuProvider>
      <div className="flex h-full flex-col">
        <TitleBar />
        <div className="grid min-h-0 flex-1 grid-cols-[180px_1fr]">
          <aside className="flex min-h-0 flex-col border-r border-turd-bronze/25 bg-turd-bg-mid/50 p-3 backdrop-blur-xl">
            <div className="mb-3 px-2 pt-1">
              <div className="mb-2 flex items-center gap-2">
                <img
                  src="/turdmod-logo.svg"
                  alt="TurdMOD"
                  className="h-9 w-9 shrink-0 drop-shadow-[0_0_8px_rgba(0,212,255,0.5)]"
                />
                <span className="font-display text-sm font-black tracking-[0.18em] text-turd-cream">
                  TurdMOD
                </span>
              </div>
              <p className="text-[10px] uppercase tracking-wider text-turd-cream-dim">
                Manager
              </p>
              <div className="mt-2 space-y-1.5">
                <CompanionStatusBadge />
                <ClaudeStatusIndicator />
              </div>
            </div>
            <EngineHostSwitcher />
            <TargetSwitcher />
            <div className="mb-2">
              <ThemeSwitcher />
            </div>
            {/* The nav itself takes the remaining height and scrolls
                vertically. A bottom fade overlay hints that more items
                exist below when the list overflows. */}
            <div className="relative min-h-0 flex-1">
              <nav className="flex h-full flex-col gap-1 overflow-y-auto pr-1 text-sm">
                <SidebarNav />
              </nav>
              <div className="pointer-events-none absolute inset-x-0 bottom-0 h-6 bg-gradient-to-t from-turd-bg-mid/80 to-transparent" />
            </div>
            <div className="mt-3 px-2 text-[10px] text-turd-cream-dim">
              v{__APP_VERSION__}
            </div>
          </aside>
      <main className="flex min-h-0 flex-col overflow-hidden">
        <DumpUpdateBanner />
        <TabBar />
        <div className="flex-1 overflow-auto p-6">
        <Routes>
          <Route path="/" element={<BrowsePage />} />
          <Route path="/mods/:slug" element={<ModDetailPage />} />
          <Route path="/library" element={<LibraryPage />} />
          <Route path="/servers" element={<ServersPage />} />
          <Route path="/engine" element={<EnginePage />} />
          <Route path="/map" element={<MapPage />} />
          <Route path="/economy" element={<EconomyPage />} />
          <Route path="/loot" element={<LootPage />} />
          <Route path="/client-engine" element={<ClientEnginePage />} />
          <Route path="/console" element={<ConsolePage />} />
          <Route path="/schema" element={<SchemaPage />} />
          <Route path="/reflection-db" element={<ReflectionDatabasePage />} />
          <Route path="/bridge-smoke" element={<BridgeSmokePage />} />
          <Route path="/functions" element={<FunctionsPage />} />
          <Route path="/admin" element={<AdminConsolePage welcomeMod={welcomeMod} />} />
          {/* Legacy surfaces — folded into Admin Console, kept routable (not in nav) for back-compat. */}
          <Route path="/admin-legacy" element={<AdminPage welcomeMod={welcomeMod} />} />
          <Route path="/admin-users" element={<AdminUsersPage />} />
          <Route path="/pilot" element={<PilotPage />} />
          <Route path="/monitor" element={<MonitorPage />} />
          <Route path="/gui-builder" element={<GuiBuilderPage />} />
          <Route path="/custom-panels" element={<CustomPanelsPage />} />
          <Route path="/server-panels" element={<ServerPanelsPage />} />
          <Route path="/notifications" element={<NotificationsEditorPage />} />
          <Route path="/server-settings" element={<ServerSettingsPage />} />
          <Route path="/traders" element={<TradersConfigPage />} />
          <Route path="/dump-management" element={<DumpManagementPage />} />
          <Route path="/dump-archive" element={<DumpArchivePage />} />
          <Route path="/ai-assistant" element={<AiAssistantPage />} />
          <Route path="/server-files" element={<ServerFilesPage />} />
          <Route path="/vehicle-manager" element={<VehicleManagerPage />} />
          <Route path="/safe-zones" element={<SafeZonesPage />} />
          <Route path="/permissions" element={<PermissionsPage />} />
          <Route path="/admin-commands" element={<AdminCommandsPage />} />
          <Route path="/chat" element={<ChatConsolePage />} />
          <Route path="/database" element={<ScumDbPage />} />
          <Route path="/turdcom" element={<TurdComTerminalPage />} />
          <Route path="/engine-console" element={<EngineConsolePage />} />
          <Route path="/json-inspector" element={<JsonInspectorPage />} />
          <Route path="/ollama-bridge" element={<OllamaBridgePage />} />
          <Route path="/account" element={<AccountPage />} />
          <Route path="/settings" element={<SettingsPage />} />
          <Route path="/wire" element={<WirePage />} />
            </Routes>
            </div>
          </main>
        </div>
      </div>
    </ContextMenuProvider>
  );
}

// ---------------------------------------------------------------------------
// Sidebar — renders leaves as NavLinks, groups as a single NavLink-to-
// first-child whose active state reflects "any child route is active".
// ---------------------------------------------------------------------------

function SidebarNav() {
  const { pathname } = useLocation();
  return (
    <>
      {NAV_ITEMS.map((item) => {
        if (item.kind === 'leaf') {
          return (
            <NavLink
              key={item.to}
              to={item.to}
              end={item.exact}
              className={({ isActive }) =>
                [
                  'rounded-lg px-3 py-2',
                  isActive
                    ? 'bg-turd-mustard/10 font-medium text-turd-mustard-bright shadow-[inset_2px_0_0_0_var(--turd-mustard)]'
                    : 'text-turd-cream-dim hover:bg-turd-bg-soft/40 hover:text-turd-cream',
                ].join(' ')
              }
            >
              {item.label}
            </NavLink>
          );
        }
        // Group: navigate to first child; highlight when any child is active.
        const firstChild = item.children[0];
        const groupActive = item.children.some((c) =>
          pathMatches(pathname, c.to, c.exact),
        );
        return (
          <NavLink
            key={`group:${item.label}`}
            to={firstChild.to}
            className={[
              'flex items-center justify-between rounded-lg px-3 py-2',
              groupActive
                ? 'bg-turd-mustard/10 font-medium text-turd-mustard-bright shadow-[inset_2px_0_0_0_var(--turd-mustard)]'
                : 'text-turd-cream-dim hover:bg-turd-bg-soft/40 hover:text-turd-cream',
            ].join(' ')}
          >
            <span>{item.label}</span>
            <span className="rounded-full bg-turd-bronze/20 px-1.5 text-[9px] text-turd-cream-dim/70">
              {item.children.length}
            </span>
          </NavLink>
        );
      })}
    </>
  );
}

// ---------------------------------------------------------------------------
// TabBar — sits at the top of the content area. When the current route
// belongs to a grouped nav entry, renders a horizontal tab strip with
// the group's siblings. Singles render nothing (no tab bar).
// ---------------------------------------------------------------------------

function TabBar() {
  const { pathname } = useLocation();
  const group = findActiveGroup(pathname);
  if (!group) return null;
  return (
    <div className="flex shrink-0 items-center gap-1 border-b border-turd-bronze/25 bg-turd-bg-mid/30 px-4 backdrop-blur-md">
      {group.children.map((c) => {
        const active = pathMatches(pathname, c.to, c.exact);
        return (
          <NavLink
            key={c.to}
            to={c.to}
            end={c.exact}
            className={[
              'relative px-3 py-2 font-display text-[11px] uppercase tracking-wider',
              active ? 'text-turd-mustard-bright' : 'text-turd-cream-dim hover:text-turd-cream',
            ].join(' ')}
          >
            {c.label}
            {active && (
              <motion.span
                layoutId="tabbar-underline"
                transition={{ type: 'spring', stiffness: 420, damping: 34 }}
                className="absolute inset-x-2 -bottom-px h-0.5 rounded-full bg-turd-mustard-bright shadow-[0_0_8px_var(--turd-mustard)]"
              />
            )}
          </NavLink>
        );
      })}
    </div>
  );
}

