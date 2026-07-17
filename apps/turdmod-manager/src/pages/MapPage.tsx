import { useCallback, useMemo, useRef, useState, useEffect } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import { engineRpc } from '../lib/tauri-engine';
import { useEngineHost, setEngineHost } from '../lib/engineHost';
import { PlayerSheet } from '../components/PlayerSheet';
import { SpawnBrowser } from '../components/SpawnBrowser';
import ServerControls from '../components/ServerControls';
import DraggablePanel from '../components/DraggablePanel';
import ScheduleControls from '../components/ScheduleControls';
import ElevatedUsers from '../components/ElevatedUsers';
import LootPanel from '../components/LootPanel';
import SkillSettings from '../components/SkillSettings';
import BaitGuide from '../components/BaitGuide';

type MapServer = 'local' | 'remote';

// SCUM map transform — AUTHORITATIVE values from scummap/apps/web/src/lib/coords.ts
// (the same image source). The full map spans game cm [-904800, +619200] on both
// axes (1,524,000 wide), with +X = WEST (image LEFT) and +Y = NORTH (image TOP).
// 5×5 sector grid: rows Z/A/B/C/D + cols 0-4, each cell 304800 cm. Verified live:
// player X=345494 Y=-531589 = A4 (dot lands left-lower, where A4 is on the image).
const GAME_MIN = -904800;          // east / south edge
const GAME_MAX = 619200;           // west / north edge
const GAME_SPAN = GAME_MAX - GAME_MIN; // 1_524_000
const CELL = GAME_SPAN / 5;        // 304_800
const ROW_LABELS = ['Z', 'A', 'B', 'C', 'D'];

// world cm → map pixel (image drawn at -250..250) + inverse. +X→left, +Y→top.
const worldToPx = (x: number) => ((GAME_MAX - x) / GAME_SPAN) * 500 - 250;
const worldToPy = (y: number) => ((GAME_MAX - y) / GAME_SPAN) * 500 - 250;
const pxToWorldX = (px: number) => GAME_MAX - ((px + 250) / 500) * GAME_SPAN;
const pyToWorldY = (py: number) => GAME_MAX - ((py + 250) / 500) * GAME_SPAN;

function scumToGrid(x: number, y: number): string {
  const col = Math.min(4, Math.max(0, Math.floor((x - GAME_MIN) / CELL)));
  const row = Math.min(4, Math.max(0, Math.floor((y - GAME_MIN) / CELL)));
  return `${ROW_LABELS[row]}${col}`;
}

// Marker assets in public/map-markers/.
const VEHICLE_MARKERS = ['bicycle', 'car_4w', 'cruiser', 'dinghy', 'kinglet', 'laika', 'motorboat', 'motorcycle', 'motorcycle_2w', 'sidecarbike', 'tractor', 'wolfswagen'];
const POI_MARKERS = ['base-flag', 'boat_spawns', 'car_spawns', 'cargo', 'motorbike_bicycle_spawns', 'motorbike_spawns', 'trader_car_shops', 'wall-flag'];
function vehicleMarker(cls: string): string {
  const c = cls.toLowerCase();
  if (c.includes('laika')) return 'laika';
  if (c.includes('wolf')) return 'wolfswagen';
  if (c.includes('dinghy')) return 'dinghy';
  if (c.includes('kinglet')) return 'kinglet';
  if (c.includes('cruiser')) return 'cruiser';
  if (c.includes('tractor')) return 'tractor';
  if (c.includes('motorboat') || c.includes('sup')) return 'motorboat';
  if (c.includes('sidecar')) return 'sidecarbike';
  if (c.includes('bicycle')) return 'bicycle';
  if (c.includes('motorcycle') || c.includes('motorbike') || c.includes('cycle')) return 'motorcycle';
  return 'car_4w';
}
// Geometry + cursor for an edge/corner resize handle.
function resizeHandleStyle(d: string): React.CSSProperties {
  const t = 7;
  const base: React.CSSProperties = { position: 'absolute' };
  const cur = d.length === 2 ? (d === 'ne' || d === 'sw' ? 'nesw-resize' : 'nwse-resize') : (d === 'n' || d === 's' ? 'ns-resize' : 'ew-resize');
  const s: React.CSSProperties = { ...base, cursor: cur };
  if (d.includes('n')) s.top = -t / 2; if (d.includes('s')) s.bottom = -t / 2;
  if (d.includes('e')) s.right = -t / 2; if (d.includes('w')) s.left = -t / 2;
  if (d === 'n' || d === 's') { s.left = t; s.right = t; s.height = t; }
  else if (d === 'e' || d === 'w') { s.top = t; s.bottom = t; s.width = t; }
  else { s.width = t * 2; s.height = t * 2; }
  return s;
}

// Persist small map UI prefs (filters/zoom) so they survive reload + page nav.
function lsRead<T>(key: string, fallback: T): T {
  try {
    const s = localStorage.getItem(key);
    if (s != null) return JSON.parse(s) as T;
  } catch {
    /* ignore */
  }
  return fallback;
}

interface VehicleMarker {
  id: number;
  class: string;
  x: number;
  y: number;
  z?: number;
  owner?: string | null;
  locked?: boolean;
}

// A persisted death record (service death_notes mod → death_notes.json).
interface DeathNote {
  at: number;
  victim: string;
  victim_steam?: string | null;
  killer?: string | null;
  killer_steam?: string | null;
  cause?: string;
  weapon?: string | null;
  headshot?: boolean;
  distance_m?: number | null;
  x?: number | null;
  y?: number | null;
  z?: number | null;
  sector?: string;
  inventory?: { class: string; entityId?: number }[];
}

interface OnlinePlayer {
  name: string;
  steamId?: string;
  x?: number;
  y?: number;
  z?: number;
}

// A roster entry: every player in user_profile (online + offline), merged with
// the live position feed. Stats are plain user_profile/prisoner columns.
interface RosterPlayer {
  key: string;          // steamId || name (stable selection id)
  name: string;
  steamId?: string;
  profileId?: number;   // user_profile.id (bank join key)
  prisonerId?: number;
  fame?: number;        // user_profile.fame_points
  cash?: number;        // user_profile.money_balance (on hand)
  bank?: number;        // bank currency_type 1
  gold?: number;        // bank currency_type 2
  playTime?: number;    // user_profile.play_time (seconds)
  lastLogin?: number;   // user_profile.last_login_time (unix s)
  online: boolean;
  x?: number;
  y?: number;
  z?: number;
}

const keyOf = (p: { steamId?: string; name: string }) => p.steamId || p.name;
const num = (v: unknown): number | undefined => {
  const n = typeof v === 'number' ? v : typeof v === 'string' ? Number(v) : NaN;
  return Number.isFinite(n) ? n : undefined;
};
const fmtAgo = (unixS?: number): string => {
  if (!unixS) return '—';
  const s = Math.max(0, Date.now() / 1000 - unixS);
  if (s < 90) return 'just now';
  if (s < 3600) return `${Math.round(s / 60)}m ago`;
  if (s < 86400) return `${Math.round(s / 3600)}h ago`;
  return `${Math.round(s / 86400)}d ago`;
};
const fmtDur = (sec?: number): string => {
  if (!sec) return '—';
  const h = Math.floor(sec / 3600);
  return h >= 1 ? `${h}h` : `${Math.round(sec / 60)}m`;
};

interface ContextMenuState {
  visible: boolean;
  screenX: number;
  screenY: number;
  scumX: number;
  scumY: number;
  sector: string;
}

export function MapPage() {
  const queryClient = useQueryClient();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  // Movable stats panel — position relative to the map container; null = default top-right.
  const panelRef = useRef<HTMLDivElement>(null);
  const [panelPos, setPanelPos] = useState<{ x: number; y: number } | null>(null);
  const [panelSize, setPanelSize] = useState<{ w: number; h: number } | null>(null);
  // Resize from any edge/corner. dir is a combination of n/s/e/w.
  const startResize = (dir: string) => (e: React.MouseEvent) => {
    if (!panelRef.current) return;
    e.preventDefault(); e.stopPropagation();
    const parent = panelRef.current.offsetParent as HTMLElement | null;
    const pr = parent?.getBoundingClientRect();
    const rect = panelRef.current.getBoundingClientRect();
    const base = { x: rect.left - (pr?.left ?? 0), y: rect.top - (pr?.top ?? 0), w: rect.width, h: rect.height };
    const sx = e.clientX, sy = e.clientY;
    const move = (ev: MouseEvent) => {
      let { x, y, w, h } = base;
      const dx = ev.clientX - sx, dy = ev.clientY - sy;
      if (dir.includes('e')) w = Math.max(224, base.w + dx);
      if (dir.includes('s')) h = Math.max(140, base.h + dy);
      if (dir.includes('w')) { w = Math.max(224, base.w - dx); x = base.x + (base.w - w); }
      if (dir.includes('n')) { h = Math.max(140, base.h - dy); y = base.y + (base.h - h); }
      setPanelPos({ x, y }); setPanelSize({ w, h });
    };
    const up = () => { window.removeEventListener('mousemove', move); window.removeEventListener('mouseup', up); };
    window.addEventListener('mousemove', move); window.addEventListener('mouseup', up);
  };
  const startPanelDrag = (e: React.MouseEvent) => {
    if (!panelRef.current) return;
    e.preventDefault();
    const parent = panelRef.current.offsetParent as HTMLElement | null;
    const pr = parent?.getBoundingClientRect();
    const rect = panelRef.current.getBoundingClientRect();
    const baseX = rect.left - (pr?.left ?? 0);
    const baseY = rect.top - (pr?.top ?? 0);
    const sx = e.clientX, sy = e.clientY;
    const move = (ev: MouseEvent) => setPanelPos({ x: baseX + (ev.clientX - sx), y: baseY + (ev.clientY - sy) });
    const up = () => { window.removeEventListener('mousemove', move); window.removeEventListener('mouseup', up); };
    window.addEventListener('mousemove', move);
    window.addEventListener('mouseup', up);
  };
  const mapImgRef = useRef<HTMLImageElement | null>(null);
  const [mapLoaded, setMapLoaded] = useState(false);
  const [zoom, setZoom] = useState(1);
  const [offset, setOffset] = useState({ x: 0, y: 0 });
  const [dragging, setDragging] = useState(false);
  const [dragStart, setDragStart] = useState({ x: 0, y: 0 });
  const [hoverCoord, setHoverCoord] = useState<{ x: number; y: number; sector: string } | null>(null);
  const [contextMenu, setContextMenu] = useState<ContextMenuState>({ visible: false, screenX: 0, screenY: 0, scumX: 0, scumY: 0, sector: '' });
  const [actionResult, setActionResult] = useState<string | null>(null);
  // Bound to the global server selector (sidebar) — one source of truth.
  const server = useEngineHost();
  const [showLive, setShowLive] = useState(() => lsRead('map.showLive', true)); // false = static server map only
  const [selectedKeys, setSelectedKeys] = useState<Set<string>>(new Set()); // multi-select
  const [showRoster, setShowRoster] = useState(() => lsRead('map.showRoster', true));
  // Control panels are grouped into category tabs (toolbar "Panels") to declutter the map —
  // one group shown at a time, or none. Persisted across reload/nav.
  const [mapTab, setMapTab] = useState<'server' | 'loot' | 'admin' | 'bait' | null>(() => lsRead('map.tab', null));
  // Admin-map overlay layers (in-game-style filters) — persisted across reload/nav.
  const [layers, setLayers] = useState(() => ({
    players: true, vehicles: false, zones: true, traders: true, bunkers: true, deaths: false, lastKnown: false,
    ...lsRead<Record<string, boolean>>('map.layers', {}),
  }));
  const [hoveredVehicle, setHoveredVehicle] = useState<{ v: VehicleMarker; sx: number; sy: number } | null>(null);
  const [poi, setPoi] = useState<{ kind: string; name: string; x: number; y: number; color: string }[]>([]);
  const [showDeaths, setShowDeaths] = useState(false);
  const [selectedDeath, setSelectedDeath] = useState<DeathNote | null>(null);
  const [findCarFor, setFindCarFor] = useState<string | null>(null);
  const [markerScale, setMarkerScale] = useState(() => lsRead('map.markerScale', 1)); // 0.5..3, marker/icon size
  // Persist map UI prefs whenever they change (survive reload + page navigation).
  useEffect(() => {
    try {
      localStorage.setItem('map.showLive', JSON.stringify(showLive));
      localStorage.setItem('map.showRoster', JSON.stringify(showRoster));
      localStorage.setItem('map.tab', JSON.stringify(mapTab));
      localStorage.setItem('map.layers', JSON.stringify(layers));
      localStorage.setItem('map.markerScale', JSON.stringify(markerScale));
    } catch {
      /* ignore */
    }
  }, [showLive, showRoster, mapTab, layers, markerScale]);
  const [spawnOpen, setSpawnOpen] = useState(false); // spawn browser modal
  const markerImgs = useRef<Map<string, HTMLImageElement>>(new Map());
  const [markersLoaded, setMarkersLoaded] = useState(0);

  // Load map image
  useEffect(() => {
    const img = new Image();
    img.onload = () => { mapImgRef.current = img; setMapLoaded(true); };
    img.src = '/map/scum-map.png';
  }, []);

  // Preload vehicle + POI marker icons for the canvas.
  useEffect(() => {
    const load = (key: string, url: string) => {
      const img = new Image();
      img.onload = () => { markerImgs.current.set(key, img); setMarkersLoaded((n) => n + 1); };
      img.src = url;
    };
    VEHICLE_MARKERS.forEach((v) => load(`veh:${v}`, `/map-markers/vehicle/${v}.png`));
    POI_MARKERS.forEach((p) => load(`poi:${p}`, `/map-markers/poi/${p}.png`));
  }, []);

  // Static vanilla POI markers (safe zones / traders / restricted / bunkers).
  useEffect(() => {
    fetch('/vanilla-zones.json').then((r) => r.json()).then((d) => {
      const out: typeof poi = [];
      const push = (arr: { kind: string; name: string; game_cm_x: number; game_cm_y: number }[] | undefined, color: string) =>
        (arr ?? []).forEach((z) => out.push({ kind: z.kind, name: z.name, x: z.game_cm_x, y: z.game_cm_y, color }));
      push(d.trader_safe_zones, '#22c55e');
      push(d.fishing_traders, '#38bdf8');
      push(d.restricted_zones, '#ef4444');
      push(d.abandoned_bunkers, '#f59e0b');
      push(d.bunker_entrances, '#f59e0b');
      setPoi(out);
    }).catch(() => {});
  }, []);

  // Vehicles layer (host-aware), only polled when the layer is on.
  const vehiclesQuery = useQuery<{ vehicles: VehicleMarker[] }>({
    queryKey: ['map-vehicles', server],
    queryFn: () => invoke('scumdb_map_vehicles', { target: server }),
    enabled: layers.vehicles || selectedKeys.size === 1,
    refetchInterval: layers.vehicles ? 20000 : false,
    retry: false,
  });
  const vehicles = vehiclesQuery.data?.vehicles ?? [];

  // Real zones from SCUM.db (custom_zone_region + config colour/name).
  const zonesQuery = useQuery<{ zones: { name: string; x: number; y: number; sizeX: number; sizeY: number; color: string }[] }>({
    queryKey: ['map-zones', server],
    queryFn: () => invoke('scumdb_map_zones', { target: server }),
    enabled: layers.zones,
    retry: false,
  });
  const zones = zonesQuery.data?.zones ?? [];

  // Death markers — persisted death_notes.json (service death_notes mod). Loads
  // once when the layer/panel is on; manual refetch only (NO polling).
  const deathsQuery = useQuery<DeathNote[]>({
    queryKey: ['map-deaths', server],
    queryFn: async () => {
      const r = await invoke<{ ok: boolean; contents?: string }>('remote_data_file_get', { target: server, name: 'death_notes.json' });
      const arr = r.contents?.trim() ? JSON.parse(r.contents) : [];
      return Array.isArray(arr) ? (arr as DeathNote[]) : [];
    },
    enabled: layers.deaths || showDeaths || selectedKeys.size === 1,
    retry: false,
  });
  const deaths = deathsQuery.data ?? [];
  // Persist a filtered set (clear-all = [], per-row = filter one out).
  const persistDeaths = async (keep: DeathNote[]) => {
    await invoke('remote_data_file_set', { target: server, name: 'death_notes.json', contents: JSON.stringify(keep, null, 2) });
    setSelectedDeath(null);
    deathsQuery.refetch();
  };

  // Last-known positions (crash-proof, scum.db) — batch, one query. On when the
  // layer is on OR a single player is selected (for the dossier). No polling.
  interface LastPos { steam: string; name: string; x: number; y: number; z: number; sector: string; last_login?: string | null }
  const lastKnownQuery = useQuery<{ players: LastPos[] }>({
    queryKey: ['map-lastknown', server],
    queryFn: () => invoke('scumdb_last_positions', { target: server }),
    enabled: layers.lastKnown || selectedKeys.size === 1,
    retry: false,
  });
  const lastKnown = lastKnownQuery.data?.players ?? [];

  // Poll live player POSITIONS every 3s. getPlayerPositions returns flat
  // {name, steamId, x, y, z} for REAL players (no phantom-population spoof,
  // unlike /map/players) — this is the live position feed the map needs.
  // getOnlinePlayers (the old source) carries NO coords, so nothing rendered.
  const playersQuery = useQuery<{ players: OnlinePlayer[] }>({
    queryKey: ['map-positions', server],
    // engineRpc routes to the active host (local pipe or remote server tunnel) and
    // returns the unwrapped {players} either way. queryKey carries `server`
    // so a host switch refetches automatically.
    queryFn: () => engineRpc<{ players: OnlinePlayer[] }>('getPlayerPositions'),
    refetchInterval: showLive ? 3000 : false,
    enabled: showLive,
    retry: false,
  });

  // Drop the empty-name ghost controller(s) that linger after respawn/menu.
  const players: OnlinePlayer[] = (playersQuery.data?.players ?? []).filter(
    (p) => p.name && p.name.trim().length > 0,
  );

  // Full roster (online + offline) from SCUM.db user_profile, via the host-aware
  // scumdb_rows endpoint (local or remote server). Refreshed lazily; merged with live feed.
  const rosterQuery = useQuery<{ rows: Record<string, unknown>[] }>({
    queryKey: ['map-roster', server],
    queryFn: () => invoke('scumdb_rows', { target: server, table: 'user_profile', limit: 500, offset: 0 }),
    refetchInterval: 30000,
    retry: false,
  });

  // Bank balances — join bank_account_registry → bank_account_registry_currencies.
  // currency_type 1 = money (bank), 2 = gold (confirmed against live DB).
  const bankAcctsQuery = useQuery<{ rows: Record<string, unknown>[] }>({
    queryKey: ['map-bank-accts', server],
    queryFn: () => invoke('scumdb_rows', { target: server, table: 'bank_account_registry', limit: 2000, offset: 0 }),
    refetchInterval: 60000, retry: false,
  });
  const bankCurrQuery = useQuery<{ rows: Record<string, unknown>[] }>({
    queryKey: ['map-bank-curr', server],
    queryFn: () => invoke('scumdb_rows', { target: server, table: 'bank_account_registry_currencies', limit: 4000, offset: 0 }),
    refetchInterval: 60000, retry: false,
  });

  const bankByProfile = useMemo(() => {
    const acctBal = new Map<number, { bank: number; gold: number }>();
    for (const c of bankCurrQuery.data?.rows ?? []) {
      const acct = num(c.bank_account_id); if (acct == null) continue;
      const t = num(c.currency_type); const bal = num(c.account_balance) ?? 0;
      const e = acctBal.get(acct) ?? { bank: 0, gold: 0 };
      if (t === 1) e.bank += bal; else if (t === 2) e.gold += bal;
      acctBal.set(acct, e);
    }
    const byProfile = new Map<number, { bank: number; gold: number }>();
    for (const a of bankAcctsQuery.data?.rows ?? []) {
      const acctId = num(a.id);
      const owner = num(a.account_owner_user_profile_id) ?? num(a.user_profile_id);
      if (acctId == null || owner == null) continue;
      const bal = acctBal.get(acctId); if (!bal) continue;
      const e = byProfile.get(owner) ?? { bank: 0, gold: 0 };
      e.bank += bal.bank; e.gold += bal.gold;
      byProfile.set(owner, e);
    }
    return byProfile;
  }, [bankAcctsQuery.data, bankCurrQuery.data]);

  const roster: RosterPlayer[] = useMemo(() => {
    const liveByKey = new Map<string, OnlinePlayer>();
    for (const p of players) {
      liveByKey.set(keyOf(p), p);
      if (p.name) liveByKey.set(p.name, p); // also key by name for steam-less feeds
    }
    const seen = new Set<string>();
    const out: RosterPlayer[] = [];
    for (const r of rosterQuery.data?.rows ?? []) {
      const steam = r.user_id != null ? String(r.user_id) : undefined;
      const name = (r.name as string) || (r.fake_name as string) || '(unknown)';
      const key = steam || name;
      if (seen.has(key)) continue;
      seen.add(key);
      const live = liveByKey.get(key) ?? liveByKey.get(name);
      const profileId = num(r.id);
      const bank = profileId != null ? bankByProfile.get(profileId) : undefined;
      out.push({
        key, name, steamId: steam, profileId, prisonerId: num(r.prisoner_id),
        fame: num(r.fame_points), cash: num(r.money_balance),
        bank: bank?.bank, gold: bank?.gold, playTime: num(r.play_time),
        lastLogin: num(r.last_login_time), online: !!live,
        x: live?.x, y: live?.y, z: live?.z,
      });
    }
    // Any live player not in user_profile (rare) still shows up.
    for (const p of players) {
      const key = keyOf(p);
      if (seen.has(key) || (p.name && seen.has(p.name))) continue;
      seen.add(key);
      out.push({ key, name: p.name, steamId: p.steamId, online: true, x: p.x, y: p.y, z: p.z });
    }
    out.sort((a, b) => Number(b.online) - Number(a.online) || (b.lastLogin ?? 0) - (a.lastLogin ?? 0));
    return out;
  }, [rosterQuery.data, players, bankByProfile]);

  // Selection ops — additive (shift/ctrl) toggles; plain click replaces.
  const toggleSelect = useCallback((key: string, additive: boolean) => {
    setSelectedKeys((prev) => {
      const next = new Set(additive ? prev : []);
      if (additive && prev.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }, []);
  const clearSelection = useCallback(() => setSelectedKeys(new Set()), []);

  const selectedList = useMemo(() => roster.filter((r) => selectedKeys.has(r.key)), [roster, selectedKeys]);
  const selectedOnline = selectedList.filter((r) => r.online).length;

  // Spawn targets — from the LIVE player feed (authoritative online list).
  // Selected online players, else everyone online.
  const spawnTargets = useMemo(() => {
    const sel = players.filter((p) => !!p.name && (selectedKeys.has(keyOf(p)) || selectedKeys.has(p.name)));
    const list = sel.length ? sel : players;
    return list.filter((p) => !!p.name).map((p) => ({ name: p.name }));
  }, [players, selectedKeys]);
  const spawnTargetLabel = spawnTargets.length ? spawnTargets.map((t) => t.name).join(', ') : 'no online player';
  // Spawn via the REAL admin command (#SpawnItem / #SpawnVehicle) — the bridge
  // spawnItem RPC creates an orphan that never lands in inventory. Echo confirms.
  const handleSpawn = async (cls: string, count: number, veh: boolean) => {
    const host = server === 'remote' ? 'Remote' : 'Local';
    const targets = veh ? spawnTargets.slice(0, 1) : spawnTargets;
    const who = targets[0]?.name;
    if (!who) { setActionResult(`No online player on ${host} — switch to ☁ Remote (top toggle)`); setTimeout(() => setActionResult(null), 5000); return; }
    const cmd = veh ? `SpawnVehicle ${cls}` : `SpawnItem ${cls} ${count}`;
    setActionResult(`${veh ? '🚗' : '🎁'} ${cls}…`);
    try {
      await engineRpc('getAdminOutput', {}).catch(() => {}); // clear stale echo
      for (const t of targets) await engineRpc('runAdminCommand', { command: cmd, playerName: t.name });
      await new Promise((r) => setTimeout(r, 550));
      const out = await engineRpc<{ lines?: string[] }>('getAdminOutput', {});
      const echo = (out.lines ?? []).join(' · ').replace(/\?/g, '·').slice(0, 90);
      // "Spawned X with ID 0" = SCUM accepted the string but registered nothing
      // (wrong class name → null spawn at origin). Real spawns get a non-zero ID.
      const ok = /spawned/i.test(echo) && !/\bID 0\b/.test(echo);
      setActionResult(`${ok ? '✓' : '✗'} ${echo || cls} → ${who} [${host}]`);
    } catch (e) {
      setActionResult(`✗ [${host}] ${cls}: ${String(e)}`);
    }
    setTimeout(() => setActionResult(null), 5500);
  };

  // On-the-fly admin edits — fire SCUM admin commands at the player (like in-game).
  // runAdminCommand resolves the target via find_pc_by_player_name (proven path).
  const [editFame, setEditFame] = useState('');
  const [editMoney, setEditMoney] = useState('');
  const [editGold, setEditGold] = useState('');
  // Admin-command injection (last resort). @brk: runAdminCommand runs via the
  // TARGET's PlayerRpcChannel → SCUM "no permission" on non-admins; runTestAdminCommand
  // (bypass=true) skips auth but IGNORES playerName → silently edits the wrong/no
  // player. Prefer the direct name-targeted RPCs (setGod/setFame/setCurrency) for
  // anything that has one. Only SetInfiniteStamina still rides this path (no RPC yet).
  const adminCmd = useCallback(async (command: string, playerName: string, bypass = false) => {
    const method = bypass ? 'runTestAdminCommand' : 'runAdminCommand';
    setActionResult(`… ${command}`);
    try {
      await engineRpc(method, { command, playerName });
      setActionResult(`✓ ${command} → ${playerName}`);
      queryClient.invalidateQueries({ queryKey: ['map-roster'] });
    } catch (e) {
      setActionResult(`✗ ${command}: ${String(e)}`);
    }
    setTimeout(() => setActionResult(null), 3500);
  }, [queryClient]);

  // Direct bridge RPC for currency — wraps ConZPlayerController::SetCurrencyBalanceRep
  // (byte type, no admin tier / no enum string). type 1=money, 2=gold (bank-table convention).
  const setCurrency = useCallback(async (playerName: string, type: number, balance: string) => {
    const label = type === 2 ? 'gold' : 'money';
    setActionResult(`… set ${label}=${balance}`);
    try {
      await engineRpc('setCurrencyBalance', { name: playerName, currencyType: String(type), balance: String(balance) });
      setActionResult(`✓ ${label}=${balance} → ${playerName}`);
      queryClient.invalidateQueries({ queryKey: ['map-bank-curr'] });
      queryClient.invalidateQueries({ queryKey: ['map-roster'] });
    } catch (e) {
      setActionResult(`✗ ${label}: ${String(e)}`);
    }
    setTimeout(() => setActionResult(null), 3500);
  }, [queryClient]);

  // Direct bridge RPCs — name-targeted, NO admin-command injection. @brk: the map
  // used to fire #SetFamePoints / #SetGodMode through runAdminCommand (target's
  // channel → SCUM "no permission" on non-admins) or runTestAdminCommand (ignores
  // playerName → silently edits the wrong/no player). These wrappers hit the same
  // proven path as setCurrency: setGodMode/setImmortal are what safe_zones drives
  // every 5s. @dep: bridge handle_set_god_mode/handle_set_fame_points (deployed).
  const setGod = useCallback(async (playerName: string, enable: boolean) => {
    setActionResult(`… god ${enable ? 'ON' : 'OFF'} → ${playerName}`);
    try {
      await engineRpc('setGodMode', { playerName, enable });
      await engineRpc('setImmortal', { playerName, enable });
      setActionResult(`✓ god ${enable ? 'ON' : 'OFF'} → ${playerName}`);
      queryClient.invalidateQueries({ queryKey: ['map-roster'] });
    } catch (e) {
      setActionResult(`✗ god: ${String(e)}`);
    }
    setTimeout(() => setActionResult(null), 3500);
  }, [queryClient]);

  const setFame = useCallback(async (playerName: string, value: string) => {
    setActionResult(`… fame=${value} → ${playerName}`);
    try {
      await engineRpc('setFamePoints', { name: playerName, value: String(value) });
      setActionResult(`✓ fame=${value} → ${playerName}`);
      queryClient.invalidateQueries({ queryKey: ['map-roster'] });
    } catch (e) {
      setActionResult(`✗ fame: ${String(e)}`);
    }
    setTimeout(() => setActionResult(null), 3500);
  }, [queryClient]);

  const soloKey = selectedList.length === 1 ? selectedList[0].key : null;
  useEffect(() => { setEditFame(''); setEditMoney(''); setEditGold(''); }, [soloKey]);

  // Convert canvas coords to SCUM coords
  const canvasToScum = useCallback((cx: number, cy: number) => {
    const canvas = canvasRef.current;
    if (!canvas) return { x: 0, y: 0 };
    const mapX = (cx - canvas.width / 2 - offset.x) / zoom;
    const mapY = (cy - canvas.height / 2 - offset.y) / zoom;
    return {
      x: Math.round(pxToWorldX(mapX)),
      y: Math.round(pyToWorldY(mapY)),
    };
  }, [offset, zoom]);

  // Draw
  const draw = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    const w = canvas.width;
    const h = canvas.height;

    ctx.clearRect(0, 0, w, h);
    ctx.save();
    ctx.translate(w / 2 + offset.x, h / 2 + offset.y);
    ctx.scale(zoom, zoom);

    // Map image
    if (mapImgRef.current && mapLoaded) {
      ctx.drawImage(mapImgRef.current, -250, -250, 500, 500);
    } else {
      ctx.fillStyle = '#1a1a2e';
      ctx.fillRect(-250, -250, 500, 500);
    }

    // SCUM sector grid — 6 boundary lines per axis (at GAME_MIN + i·CELL) + labels.
    // High-vis so it reads over the map image.
    ctx.strokeStyle = 'rgba(255, 200, 80, 0.85)';
    ctx.lineWidth = 1.2 / zoom;
    for (let i = 0; i <= 5; i++) {
      const x = worldToPx(GAME_MIN + i * CELL);
      ctx.beginPath(); ctx.moveTo(x, -250); ctx.lineTo(x, 250); ctx.stroke();
      const y = worldToPy(GAME_MIN + i * CELL);
      ctx.beginPath(); ctx.moveTo(-250, y); ctx.lineTo(250, y); ctx.stroke();
    }
    // Sector labels (Z/A/B/C/D × 0-4) at each cell centre, with a dark backing
    ctx.font = `bold ${14 / zoom}px monospace`;
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    for (let c = 0; c < 5; c++) {
      const cx = worldToPx(GAME_MIN + (c + 0.5) * CELL);
      for (let r = 0; r < 5; r++) {
        const cy = worldToPy(GAME_MIN + (r + 0.5) * CELL);
        const label = `${ROW_LABELS[r]}${c}`;
        ctx.lineWidth = 3 / zoom;
        ctx.strokeStyle = 'rgba(0,0,0,0.85)';
        ctx.strokeText(label, cx, cy);
        ctx.fillStyle = 'rgba(255, 210, 90, 0.95)';
        ctx.fillText(label, cx, cy);
      }
    }

    // Real zones from SCUM.db — circle (size_y≈0 → radius size_x) or rectangle
    // (half-extents size_x × size_y), in the zone's true colour, + label.
    if (layers.zones) for (const z of zones) {
      if (z.x == null || z.y == null) continue;
      const px = worldToPx(z.x), py = worldToPy(z.y);
      const col = z.color || '#888888';
      ctx.lineWidth = 1.8 / zoom; ctx.strokeStyle = col; ctx.fillStyle = col;
      if (z.sizeY > 200) {
        const wPx = (z.sizeX / GAME_SPAN) * 500, hPx = (z.sizeY / GAME_SPAN) * 500;
        ctx.globalAlpha = 0.13; ctx.fillRect(px - wPx, py - hPx, wPx * 2, hPx * 2);
        ctx.globalAlpha = 0.9; ctx.strokeRect(px - wPx, py - hPx, wPx * 2, hPx * 2);
      } else {
        const rPx = (Math.max(z.sizeX, 8000) / GAME_SPAN) * 500;
        ctx.globalAlpha = 0.13; ctx.beginPath(); ctx.arc(px, py, rPx, 0, Math.PI * 2); ctx.fill();
        ctx.globalAlpha = 0.9; ctx.beginPath(); ctx.arc(px, py, rPx, 0, Math.PI * 2); ctx.stroke();
      }
      ctx.globalAlpha = 1;
      ctx.font = `${(9 * markerScale) / zoom}px monospace`; ctx.textAlign = 'center'; ctx.textBaseline = 'middle';
      ctx.lineWidth = 3 / zoom; ctx.strokeStyle = 'rgba(0,0,0,0.8)'; ctx.strokeText(z.name, px, py);
      ctx.fillStyle = col; ctx.fillText(z.name, px, py);
    }

    // Vanilla POI markers — traders + bunkers only (zones now come from the DB).
    for (const m of poi) {
      const isTrader = m.kind.includes('trader') || m.kind.includes('safe') || m.kind.includes('fishing');
      const isBunker = m.kind.includes('bunker');
      const show = isTrader ? layers.traders : isBunker ? layers.bunkers : false;
      if (!show) continue;
      const px = worldToPx(m.x), py = worldToPy(m.y);
      ctx.globalAlpha = 1; ctx.fillStyle = m.color;
      ctx.beginPath(); ctx.arc(px, py, (3 * markerScale) / zoom, 0, Math.PI * 2); ctx.fill();
      ctx.font = `${(8 * markerScale) / zoom}px monospace`; ctx.textAlign = 'left'; ctx.textBaseline = 'middle';
      ctx.lineWidth = 2.5 / zoom; ctx.strokeStyle = 'rgba(0,0,0,0.75)';
      ctx.strokeText(m.name, px + 5 / zoom, py); ctx.fillStyle = m.color; ctx.fillText(m.name, px + 5 / zoom, py);
    }

    // Vehicles — real type icon (fallback dot if not loaded).
    if (layers.vehicles) {
      const sz = (16 * markerScale) / zoom;
      for (const v of vehicles) {
        if (v.x == null || v.y == null) continue;
        const px = worldToPx(v.x), py = worldToPy(v.y);
        // "Tech, Where's My Car?" — green ring on the selected player's vehicles.
        if (findCarFor && v.owner === findCarFor) {
          ctx.strokeStyle = '#7CFC00'; ctx.lineWidth = 2.5 / zoom;
          ctx.beginPath(); ctx.arc(px, py, (13 * markerScale) / zoom, 0, Math.PI * 2); ctx.stroke();
        }
        const img = markerImgs.current.get(`veh:${vehicleMarker(v.class)}`);
        if (img) ctx.drawImage(img, px - sz / 2, py - sz / 2, sz, sz);
        else { ctx.fillStyle = '#a78bfa'; ctx.beginPath(); ctx.arc(px, py, sz / 3, 0, Math.PI * 2); ctx.fill(); }
      }
    }

    // Death markers — red ✕ per recorded death (orange = environment); ringed if
    // selected in the Deaths panel. Persist until the admin clears them.
    if (layers.deaths) for (const d of deaths) {
      if (d.x == null || d.y == null) continue;
      const px = worldToPx(d.x), py = worldToPy(d.y);
      const r = (5 * markerScale) / zoom;
      const sel = selectedDeath != null && selectedDeath.at === d.at && selectedDeath.victim === d.victim;
      ctx.strokeStyle = d.cause === 'pvp' ? '#ff4d4d' : '#ff9d4d';
      ctx.lineWidth = (sel ? 3 : 2) / zoom;
      ctx.beginPath();
      ctx.moveTo(px - r, py - r); ctx.lineTo(px + r, py + r);
      ctx.moveTo(px + r, py - r); ctx.lineTo(px - r, py + r);
      ctx.stroke();
      if (sel) { ctx.beginPath(); ctx.arc(px, py, r * 2, 0, Math.PI * 2); ctx.stroke(); }
    }

    // Last-known location — faint blue dot at each OFFLINE player's last saved
    // position (crash-proof, scum.db). Skips anyone currently live.
    if (layers.lastKnown) {
      const onlineSteam = new Set(players.map((p) => p.steamId));
      for (const lp of lastKnown) {
        if (onlineSteam.has(lp.steam) || lp.x == null || lp.y == null) continue;
        const px = worldToPx(lp.x), py = worldToPy(lp.y);
        const s = markerScale;
        ctx.globalAlpha = 0.5; ctx.fillStyle = '#64b5f6';
        ctx.beginPath(); ctx.arc(px, py, (3.5 * s) / zoom, 0, Math.PI * 2); ctx.fill();
        ctx.globalAlpha = 0.9; ctx.font = `${(8 * s) / zoom}px monospace`; ctx.textAlign = 'left'; ctx.textBaseline = 'middle';
        ctx.lineWidth = 2.5 / zoom; ctx.strokeStyle = 'rgba(0,0,0,0.7)';
        ctx.strokeText(lp.name, px + 5 / zoom, py); ctx.fillStyle = '#90caf9'; ctx.fillText(lp.name, px + 5 / zoom, py);
        ctx.globalAlpha = 1;
      }
    }

    // Player markers (size-scaled).
    if (layers.players) for (const p of players) {
      if (p.x == null || p.y == null) continue;
      const px = worldToPx(p.x), py = worldToPy(p.y);
      const s = markerScale;
      if (selectedKeys.has(keyOf(p))) {
        ctx.strokeStyle = '#7CFC00'; ctx.lineWidth = 2 / zoom;
        ctx.beginPath(); ctx.arc(px, py, (15 * s) / zoom, 0, Math.PI * 2); ctx.stroke();
      }
      ctx.fillStyle = 'rgba(255,191,71,0.25)'; ctx.beginPath(); ctx.arc(px, py, (10 * s) / zoom, 0, Math.PI * 2); ctx.fill();
      ctx.strokeStyle = '#ffffff'; ctx.lineWidth = 1.5 / zoom; ctx.beginPath(); ctx.arc(px, py, (4.5 * s) / zoom, 0, Math.PI * 2); ctx.stroke();
      ctx.fillStyle = '#ffbf47'; ctx.beginPath(); ctx.arc(px, py, (4 * s) / zoom, 0, Math.PI * 2); ctx.fill();
      ctx.font = `bold ${(10 * s) / zoom}px monospace`; ctx.textAlign = 'left'; ctx.textBaseline = 'alphabetic';
      const tw = ctx.measureText(p.name).width;
      ctx.fillStyle = 'rgba(0,0,0,0.6)'; ctx.fillRect(px + 6 / zoom, py - (12 * s) / zoom, tw + 4 / zoom, (12 * s) / zoom);
      ctx.fillStyle = '#ffffff'; ctx.fillText(p.name, px + 8 / zoom, py - (3 * s) / zoom);
    }

    ctx.restore();
  }, [zoom, offset, mapLoaded, players, selectedKeys, layers, poi, vehicles, zones, markerScale, markersLoaded, deaths, selectedDeath, findCarFor, lastKnown]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const resize = () => {
      const rect = canvas.parentElement?.getBoundingClientRect();
      if (rect) { canvas.width = rect.width; canvas.height = rect.height; }
      draw();
    };
    resize();
    window.addEventListener('resize', resize);
    return () => window.removeEventListener('resize', resize);
  }, [draw]);

  useEffect(() => { draw(); }, [draw]);

  // Close context menu on click outside
  useEffect(() => {
    if (!contextMenu.visible) return;
    const close = () => setContextMenu(prev => ({ ...prev, visible: false }));
    window.addEventListener('click', close);
    return () => window.removeEventListener('click', close);
  }, [contextMenu.visible]);

  // Hit-test a canvas point against player markers (screen-space, ~12px radius).
  const playerAtCanvas = (cx: number, cy: number): OnlinePlayer | null => {
    const canvas = canvasRef.current;
    if (!canvas) return null;
    const cw = canvas.width / 2;
    const ch = canvas.height / 2;
    for (const p of players) {
      if (p.x == null || p.y == null) continue;
      const sx = cw + offset.x + zoom * worldToPx(p.x);
      const sy = ch + offset.y + zoom * worldToPy(p.y);
      if (Math.hypot(cx - sx, cy - sy) <= 12) return p;
    }
    return null;
  };

  // Hit-test a canvas point against vehicle markers (screen-space, ~12px). Returns
  // the vehicle + its screen position so a detail tooltip can anchor to it.
  const vehicleAtCanvas = (cx: number, cy: number): { v: VehicleMarker; sx: number; sy: number } | null => {
    const canvas = canvasRef.current;
    if (!canvas || !layers.vehicles) return null;
    const cw = canvas.width / 2;
    const ch = canvas.height / 2;
    let best: { v: VehicleMarker; sx: number; sy: number; d: number } | null = null;
    for (const v of vehicles) {
      if (v.x == null || v.y == null) continue;
      const sx = cw + offset.x + zoom * worldToPx(v.x);
      const sy = ch + offset.y + zoom * worldToPy(v.y);
      const d = Math.hypot(cx - sx, cy - sy);
      if (d <= 12 && (!best || d < best.d)) best = { v, sx, sy, d };
    }
    return best ? { v: best.v, sx: best.sx, sy: best.sy } : null;
  };

  const handleMouseDown = (e: React.MouseEvent) => {
    if (e.button === 2) return; // right click handled separately
    const additive = e.shiftKey || e.ctrlKey || e.metaKey;
    const canvas = canvasRef.current;
    if (canvas) {
      const rect = canvas.getBoundingClientRect();
      const hit = playerAtCanvas(e.clientX - rect.left, e.clientY - rect.top);
      if (hit) { toggleSelect(keyOf(hit), additive); return; } // click player → (de)select, don't pan
    }
    if (!additive) clearSelection();
    setDragging(true);
    setDragStart({ x: e.clientX - offset.x, y: e.clientY - offset.y });
  };

  const handleMouseMove = (e: React.MouseEvent) => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    if (dragging) {
      setOffset({ x: e.clientX - dragStart.x, y: e.clientY - dragStart.y });
      return;
    }
    const rect = canvas.getBoundingClientRect();
    const cx = e.clientX - rect.left;
    const cy = e.clientY - rect.top;
    setHoveredVehicle(vehicleAtCanvas(cx, cy));
    const scum = canvasToScum(cx, cy);
    setHoverCoord({ ...scum, sector: scumToGrid(scum.x, scum.y) });
  };

  const handleMouseUp = () => setDragging(false);
  const handleMouseLeave = () => { setDragging(false); setHoverCoord(null); setHoveredVehicle(null); };

  const handleContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    const canvas = canvasRef.current;
    if (!canvas) return;
    const rect = canvas.getBoundingClientRect();
    const scum = canvasToScum(e.clientX - rect.left, e.clientY - rect.top);
    setContextMenu({
      visible: true,
      screenX: e.clientX - rect.left,
      screenY: e.clientY - rect.top,
      scumX: scum.x,
      scumY: scum.y,
      sector: scumToGrid(scum.x, scum.y),
    });
  };

  // Zoom by `factor` while keeping the canvas point (ax, ay) fixed on screen.
  // The transform draws map-origin at (w/2+offset); to anchor an arbitrary
  // point we shift offset so that point's world position is unchanged.
  // @inv c = (center + offset) + zoom·m  ⇒  offset' = (a-center) - k·(a-center-offset)
  const zoomAround = useCallback((factor: number, ax: number, ay: number) => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const z1 = Math.max(0.3, Math.min(15, zoom * factor));
    const k = z1 / zoom;
    const cw = canvas.width / 2;
    const ch = canvas.height / 2;
    setOffset((o) => ({ x: (ax - cw) - k * (ax - cw - o.x), y: (ay - ch) - k * (ay - ch - o.y) }));
    setZoom(z1);
  }, [zoom]);

  // Wheel → zoom toward the cursor.
  const handleWheel = (e: React.WheelEvent) => {
    e.preventDefault();
    const canvas = canvasRef.current;
    if (!canvas) return;
    const rect = canvas.getBoundingClientRect();
    zoomAround(e.deltaY > 0 ? 0.9 : 1.1, e.clientX - rect.left, e.clientY - rect.top);
  };

  // Buttons → zoom toward the viewport centre.
  const zoomCenter = (factor: number) => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    zoomAround(factor, canvas.width / 2, canvas.height / 2);
  };

  // Context menu actions
  const doAction = async (action: string) => {
    setContextMenu(prev => ({ ...prev, visible: false }));
    const pos = { x: contextMenu.scumX, y: contextMenu.scumY, z: 10000 };
    // @inv teleportPlayer wants `name` + STRING x/y/z — large bare numbers parse
    // to 0 in the bridge (see jail.rs / reference_bridge_teleport_string_coords).
    const tp = (name: string) =>
      engineRpc('teleportPlayer', { name, x: String(pos.x), y: String(pos.y), z: String(pos.z) });

    try {
      if (action === 'copy') {
        await navigator.clipboard.writeText(`X=${pos.x} Y=${pos.y} Z=0`);
        setActionResult('Coordinates copied');
      } else if (action === 'tp-self') {
        await tp('YOUR_OWNER_NAME');
        setActionResult(`Teleported to ${contextMenu.sector}`);
      } else if (action === 'tp-all') {
        const res = await engineRpc<{ players: OnlinePlayer[] }>('getOnlinePlayers');
        for (const p of res.players ?? []) await tp(p.name);
        setActionResult(`Teleported all to ${contextMenu.sector}`);
      } else if (action === 'tp-selected') {
        const targets = roster.filter((r) => selectedKeys.has(r.key) && r.online);
        if (targets.length === 0) {
          setActionResult('No online selected players to teleport');
        } else {
          for (const t of targets) await tp(t.name);
          setActionResult(`Teleported ${targets.length} player${targets.length !== 1 ? 's' : ''} to ${contextMenu.sector}`);
        }
      } else if (action === 'set-spawn') {
        const name = prompt('Spawn point name:');
        if (name) {
          setActionResult(`Spawn point "${name}" saved at ${contextMenu.sector}`);
        }
      } else if (action === 'time-day') {
        await engineRpc('setTimeOfDay', { hours: 12.0 }); setActionResult('☀ Time → day');
      } else if (action === 'time-night') {
        await engineRpc('setTimeOfDay', { hours: 0.0 }); setActionResult('🌙 Time → night');
      } else if (action === 'weather-storm') {
        await engineRpc('setWeather', { severity: 1.0 }); await engineRpc('forceWeatherSnapshot', {}).catch(() => {}); setActionResult('⛈ Weather → storm');
      } else if (action === 'weather-clear') {
        await engineRpc('setWeather', { severity: 0.0 }); await engineRpc('forceWeatherSnapshot', {}).catch(() => {}); setActionResult('☀ Weather → clear');
      } else if (action === 'broadcast') {
        const msg = prompt('Broadcast to all players:');
        if (msg) { await engineRpc('broadcastChat', { text: msg }); setActionResult('📢 Broadcast sent'); }
      } else if (action === 'spawn-item' || action === 'spawn-vehicle') {
        setSpawnOpen(true);
      }
    } catch (err) {
      setActionResult(`Failed: ${String(err)}`);
    }
    setTimeout(() => setActionResult(null), 3000);
  };

  const menuBtnCls = 'block w-full px-3 py-1.5 text-left font-mono text-[11px] text-turd-cream hover:bg-white/10 transition-colors';

  return (
    <div className="flex h-full flex-col gap-2">
      <header className="flex items-center justify-between gap-4">
        <div>
          <p className="font-display text-xs tracking-[0.3em] text-turd-mustard">TurdMOD</p>
          <h1 className="mt-0.5 font-display text-2xl text-turd-cream">Live Map</h1>
        </div>
        <div className="flex flex-wrap items-center justify-end gap-2">
          {/* Server selector */}
          <div className="flex overflow-hidden rounded border border-turd-bronze/40">
            {(['local', 'remote'] as MapServer[]).map((s) => (
              <button
                key={s}
                onClick={() => setEngineHost(s)}
                className={`px-3 py-1 text-[11px] ${server === s ? 'bg-turd-mustard-bright text-turd-bg-deep' : 'bg-turd-bg-deep/60 text-turd-cream-dim hover:text-turd-cream'}`}
              >
                {s === 'local' ? '🏠 Local' : '☁ Remote'}
              </button>
            ))}
          </div>
          {/* Static vs Live */}
          <button
            onClick={() => setShowLive((v) => !v)}
            className={`rounded border px-3 py-1 text-[11px] ${showLive ? 'border-turd-green/50 bg-turd-green/10 text-turd-green' : 'border-turd-bronze/40 bg-turd-bg-deep/60 text-turd-cream-dim'}`}
            title="Toggle live player overlay vs the plain server map"
          >
            {showLive ? '● Live' : '○ Static map'}
          </button>
          {/* Panel category tabs — show one group at a time (declutters the map). */}
          <div className="flex items-center gap-1 rounded border border-turd-bronze/20 bg-turd-bg-deep/40 px-1.5 py-1">
            <span className="mr-0.5 font-mono text-[9px] uppercase tracking-wider text-turd-cream-dim/50">Panels</span>
            {([
              ['server', '🖥 Server'],
              ['loot', '📦 Loot'],
              ['admin', '🛡 Admin'],
              ['bait', '🪤 Bait'],
            ] as const).map(([key, label]) => (
              <button
                key={key}
                onClick={() => setMapTab((t) => (t === key ? null : key))}
                className={`rounded border px-2 py-0.5 text-[11px] ${mapTab === key ? 'border-turd-mustard/60 bg-turd-mustard/10 text-turd-mustard-bright' : 'border-turd-bronze/40 bg-turd-bg-deep/60 text-turd-cream-dim'}`}
                title={`Show ${label} panels`}
              >
                {label}
              </button>
            ))}
          </div>
          {/* Admin-map layer filters */}
          <div className="flex items-center gap-1 rounded border border-turd-bronze/20 bg-turd-bg-deep/40 px-1.5 py-1">
            <span className="mr-0.5 font-mono text-[9px] uppercase tracking-wider text-turd-cream-dim/50">Layers</span>
            {([
              ['players', 'Players', '#ffbf47'],
              ['vehicles', 'Vehicles', '#a78bfa'],
              ['traders', 'Traders', '#22c55e'],
              ['zones', 'Zones', '#ef4444'],
              ['bunkers', 'Bunkers', '#f59e0b'],
              ['deaths', 'Deaths', '#ff6b6b'],
              ['lastKnown', 'Last Seen', '#64b5f6'],
            ] as const).map(([key, label, color]) => (
              <button key={key}
                onClick={() => setLayers((L) => ({ ...L, [key]: !L[key] }))}
                className={`rounded px-2 py-0.5 text-[10px] transition-colors ${layers[key] ? 'bg-turd-bg-soft' : 'text-turd-cream-dim/40 hover:text-turd-cream-dim'}`}
                style={layers[key] ? { color } : undefined}
                title={`Toggle ${key} overlay`}
              >{label}</button>
            ))}
          </div>
          {/* Zoom */}
          <div className="flex gap-1">
            <button onClick={() => zoomCenter(1.3)} title="Zoom in" className="rounded border border-turd-bronze/40 bg-turd-bg-soft px-2 py-1 font-mono text-xs text-turd-cream hover:bg-white/5">+</button>
            <button onClick={() => zoomCenter(1 / 1.3)} title="Zoom out" className="rounded border border-turd-bronze/40 bg-turd-bg-soft px-2 py-1 font-mono text-xs text-turd-cream hover:bg-white/5">−</button>
            <button onClick={() => { setZoom(1); setOffset({ x: 0, y: 0 }); }} className="rounded border border-turd-bronze/40 bg-turd-bg-soft px-2 py-1 font-mono text-xs text-turd-cream hover:bg-white/5">Reset</button>
          </div>
          {/* Marker/icon size */}
          <div className="flex items-center gap-1 rounded border border-turd-bronze/20 bg-turd-bg-deep/40 px-1.5 py-1">
            <span className="font-mono text-[9px] uppercase tracking-wider text-turd-cream-dim/50">Icons</span>
            <button onClick={() => setMarkerScale((s) => Math.max(0.5, +(s - 0.25).toFixed(2)))} title="Smaller icons" className="rounded px-1.5 text-xs text-turd-cream hover:bg-white/10">−</button>
            <button onClick={() => setMarkerScale(1)} title="Reset icon size" className="min-w-[34px] rounded px-1 font-mono text-[10px] text-turd-mustard-bright hover:bg-white/10">{markerScale}×</button>
            <button onClick={() => setMarkerScale((s) => Math.min(3, +(s + 0.25).toFixed(2)))} title="Larger icons" className="rounded px-1.5 text-xs text-turd-cream hover:bg-white/10">+</button>
          </div>
        </div>
      </header>

      <div className="relative flex-1 overflow-hidden rounded-lg border border-turd-bronze/30 bg-turd-bg-deep">
        <canvas
          ref={canvasRef}
          className="h-full w-full cursor-crosshair"
          onMouseDown={handleMouseDown}
          onMouseMove={handleMouseMove}
          onMouseUp={handleMouseUp}
          onMouseLeave={handleMouseLeave}
          onContextMenu={handleContextMenu}
          onWheel={handleWheel}
        />

        {/* Hover coordinate HUD — fixed overlay (never reflows the layout). */}
        <div className="pointer-events-none absolute bottom-3 right-3 z-30 rounded border border-turd-bronze/30 bg-turd-bg-deep/85 px-2 py-1 font-mono text-[10px] text-turd-cream-dim backdrop-blur-sm">
          {hoverCoord ? (
            <span><span className="text-turd-green">{hoverCoord.sector}</span> · X={hoverCoord.x} Y={hoverCoord.y}</span>
          ) : (
            <span className="text-turd-cream-dim/40">hover for coords</span>
          )}
        </div>

        {/* Action toast — fixed overlay. */}
        {actionResult && (
          <div className="pointer-events-none absolute bottom-3 left-1/2 z-40 -translate-x-1/2 rounded-md border border-turd-mustard/40 bg-turd-bg-deep/95 px-3 py-1.5 font-mono text-xs text-turd-mustard shadow-lg">
            {actionResult}
          </div>
        )}

        {/* Vehicle detail tooltip — on hover over a vehicle marker. */}
        {hoveredVehicle && (
          <div
            className="pointer-events-none absolute z-[60] rounded-md border border-turd-bronze/60 bg-turd-bg-deep/95 px-2.5 py-1.5 font-mono text-[11px] text-turd-cream shadow-xl"
            style={{ left: hoveredVehicle.sx + 14, top: hoveredVehicle.sy + 14, maxWidth: 240 }}
          >
            <div className="font-display text-xs text-turd-mustard-bright">
              {hoveredVehicle.v.class.replace(/_ES$/, '')}
              {hoveredVehicle.v.locked ? ' 🔒' : ''}
            </div>
            <div className="text-turd-cream-dim">ID {hoveredVehicle.v.id}</div>
            <div>
              owner:{' '}
              {hoveredVehicle.v.owner ? (
                <span className="text-turd-green">{hoveredVehicle.v.owner}</span>
              ) : (
                <span className="text-turd-cream-dim/60">unclaimed</span>
              )}
            </div>
            <div className="text-turd-cream-dim/70">
              {scumToGrid(hoveredVehicle.v.x, hoveredVehicle.v.y)} · ({Math.round(hoveredVehicle.v.x)},{' '}
              {Math.round(hoveredVehicle.v.y)})
            </div>
          </div>
        )}

        {/* Control panels grouped into category tabs (toolbar "Panels") — one group at a time. */}
        {/* Server: lifecycle (stop/start/restart) + recurring restart schedule. */}
        {mapTab === 'server' && <ServerControls target={server} />}
        {mapTab === 'server' && <ScheduleControls target={server} />}
        {/* Economy/trader config moved to Server Config → Traders tab. */}
        {/* Loot: one-click the loot admin commands (export defaults / live reload) as an online admin. */}
        {mapTab === 'loot' && <LootPanel target={server} />}
        {/* Admin: elevated/dev users + live skill XP-multiplier. */}
        {mapTab === 'admin' && <ElevatedUsers target={server} />}
        {mapTab === 'admin' && <SkillSettings target={server} onClose={() => setMapTab(null)} />}
        {/* Bait: reference guide — what attracts which animal + spawn ring (global game data). */}
        {mapTab === 'bait' && <BaitGuide onClose={() => setMapTab(null)} />}

        {/* Right-click context menu */}
        {contextMenu.visible && (
          <div
            className="absolute z-50 min-w-48 rounded-lg border border-turd-bronze/60 bg-turd-bg-deep shadow-2xl"
            style={{ left: contextMenu.screenX, top: contextMenu.screenY }}
          >
            <div className="border-b border-turd-bronze/30 px-3 py-2">
              <p className="font-mono text-[10px] text-turd-mustard">
                {contextMenu.sector} · ({contextMenu.scumX}, {contextMenu.scumY})
              </p>
            </div>
            <div className="py-1">
              <button className={menuBtnCls} onClick={() => doAction('copy')}>
                Copy Coordinates
              </button>
              <button className={menuBtnCls} onClick={() => doAction('tp-self')}>
                Teleport Here
              </button>
              {selectedOnline > 0 && (
                <button className={`${menuBtnCls} text-turd-green`} onClick={() => doAction('tp-selected')}>
                  ▶ Teleport selected ({selectedOnline}) here
                </button>
              )}
              <button className={menuBtnCls} onClick={() => doAction('tp-all')}>
                Teleport All Players Here
              </button>
              <button className={menuBtnCls} onClick={() => doAction('set-spawn')}>
                Set Spawn Point Here
              </button>
              <button className={menuBtnCls} onClick={() => doAction('spawn-item')}>
                🎁 Spawn item / vehicle…  <span className="text-turd-cream-dim/50">→ {spawnTargetLabel}</span>
              </button>
              <div className="my-1 border-t border-turd-bronze/30" />
              <div className="px-3 pb-0.5 pt-1 text-[9px] uppercase tracking-wider text-turd-cream-dim/50">Server</div>
              <div className="flex flex-wrap gap-1 px-2 pb-1.5">
                {([
                  ['time-day', '☀ Day'], ['time-night', '🌙 Night'],
                  ['weather-clear', '☀ Clear'], ['weather-storm', '⛈ Storm'],
                  ['broadcast', '📢 Say'],
                ] as const).map(([act, label]) => (
                  <button key={act} onClick={() => doAction(act)}
                    className="rounded border border-turd-bronze/40 px-2 py-0.5 font-mono text-[10px] text-turd-cream hover:bg-white/10">{label}</button>
                ))}
              </div>
            </div>
          </div>
        )}

        {/* Roster — every player (online + offline). Click to select; shift/ctrl = multi. */}
        {showRoster ? (
          <DraggablePanel
            title={`Players · ${roster.filter((r) => r.online).length}/${roster.length}`}
            defaultCorner="tl"
            defaultWidth={240}
            defaultHeight={460}
            minH={160}
            onClose={() => setShowRoster(false)}
          >
            <div className="flex h-full flex-col">
              <div className="min-h-0 flex-1 overflow-auto py-1">
                {roster.length === 0 && (
                  <p className="px-3 py-2 text-[10px] text-turd-cream-dim">
                    {rosterQuery.isError ? 'DB unreachable on this server.' : rosterQuery.isPending ? 'Loading roster…' : 'No players found.'}
                  </p>
                )}
                {roster.map((r) => {
                  const sel = selectedKeys.has(r.key);
                  return (
                    <button
                      key={r.key}
                      onClick={(e) => toggleSelect(r.key, e.shiftKey || e.ctrlKey || e.metaKey)}
                      className={`flex w-full items-center gap-2 px-3 py-1 text-left transition-colors ${sel ? 'bg-turd-mustard/20' : 'hover:bg-white/5'}`}
                    >
                      <span className={`h-1.5 w-1.5 shrink-0 rounded-full ${r.online ? 'bg-turd-green' : 'bg-turd-cream-dim/30'}`} />
                      <span className={`flex-1 truncate font-mono text-[11px] ${sel ? 'text-turd-mustard-bright' : 'text-turd-cream'}`}>{r.name}</span>
                      {r.online && r.x != null && r.y != null ? (
                        <span className="shrink-0 font-mono text-[9px] text-turd-green">{scumToGrid(r.x, r.y)}</span>
                      ) : (
                        <span className="shrink-0 font-mono text-[9px] text-turd-cream-dim/50">{fmtAgo(r.lastLogin)}</span>
                      )}
                    </button>
                  );
                })}
              </div>
              {selectedKeys.size > 0 && (
                <div className="shrink-0 border-t border-turd-bronze/30 px-3 py-1.5 text-[9px] text-turd-cream-dim">
                  {selectedKeys.size} selected · {selectedOnline} online · right-click map → teleport
                  <button onClick={clearSelection} className="ml-2 text-turd-mustard hover:text-turd-mustard-bright">clear</button>
                </div>
              )}
            </div>
          </DraggablePanel>
        ) : (
          <button onClick={() => setShowRoster(true)} className="absolute left-3 top-3 z-50 rounded border border-turd-bronze/40 bg-turd-bg-deep/90 px-2 py-1 font-mono text-[10px] text-turd-cream hover:bg-white/5">
            Players ▸
          </button>
        )}

        {/* Death notes — forensic registry; map markers persist until cleared. */}
        {showDeaths ? (
          <DraggablePanel
            title={`Deaths · ${deaths.length}`}
            defaultCorner="br"
            defaultWidth={300}
            defaultHeight={420}
            minH={160}
            onClose={() => setShowDeaths(false)}
          >
            <div className="flex h-full flex-col">
              <div className="flex shrink-0 items-center justify-between border-b border-turd-bronze/30 px-3 py-1.5">
                <button onClick={() => { setLayers((L) => ({ ...L, deaths: true })); deathsQuery.refetch(); }} className="font-mono text-[10px] text-turd-cream-dim hover:text-turd-cream">↻ refresh</button>
                <button onClick={() => void persistDeaths([])} disabled={deaths.length === 0} className="font-mono text-[10px] text-turd-red hover:text-turd-red/80 disabled:opacity-30">clear all</button>
              </div>
              <div className="min-h-0 flex-1 overflow-auto py-1">
                {deaths.length === 0 && (
                  <p className="px-3 py-2 text-[10px] text-turd-cream-dim">{deathsQuery.isPending ? 'Loading…' : 'No deaths recorded yet.'}</p>
                )}
                {[...deaths].reverse().map((d, i) => {
                  const sel = selectedDeath?.at === d.at && selectedDeath?.victim === d.victim;
                  return (
                    <div key={`${d.at}-${d.victim}-${i}`} className={`px-3 py-1.5 ${sel ? 'bg-turd-mustard/15' : 'hover:bg-white/5'}`}>
                      <button
                        onClick={() => { setSelectedDeath(d); setLayers((L) => ({ ...L, deaths: true })); if (d.x != null && d.y != null) setOffset({ x: -zoom * worldToPx(d.x), y: -zoom * worldToPy(d.y) }); }}
                        className="block w-full text-left"
                      >
                        <div className="flex items-center justify-between gap-2">
                          <span className="truncate font-mono text-[11px] text-turd-cream">💀 {d.victim}</span>
                          <span className="shrink-0 font-mono text-[9px] text-turd-cream-dim">{fmtAgo(d.at)}</span>
                        </div>
                        <div className="truncate font-mono text-[9px] text-turd-cream-dim">
                          {d.cause === 'pvp' ? `by ${d.killer ?? '?'}` : 'environment'}{d.weapon ? ` · ${d.weapon}` : ''}{d.headshot ? ' · HS' : ''}{d.sector ? ` · ${d.sector}` : ''}
                        </div>
                      </button>
                      {sel && (
                        <div className="mt-1 rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-2 py-1 font-mono text-[9px] text-turd-cream-dim">
                          {d.distance_m != null && <div>distance: {d.distance_m}m</div>}
                          {d.x != null && <div>coords: {Math.round(d.x)}, {Math.round(d.y ?? 0)} ({d.sector ?? '—'})</div>}
                          <div className="mt-0.5 text-turd-cream">had {d.inventory?.length ?? 0} item(s) at death:</div>
                          <div className="max-h-24 overflow-auto">{(d.inventory ?? []).slice(0, 40).map((it, j) => <div key={j} className="truncate">· {it.class}</div>)}</div>
                          <button onClick={() => void persistDeaths(deaths.filter((x) => !(x.at === d.at && x.victim === d.victim)))} className="mt-1 text-turd-red hover:text-turd-red/80">clear this marker</button>
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            </div>
          </DraggablePanel>
        ) : (
          <button onClick={() => { setShowDeaths(true); setLayers((L) => ({ ...L, deaths: true })); }} className="absolute bottom-12 left-3 z-50 rounded border border-turd-red/40 bg-turd-bg-deep/90 px-2 py-1 font-mono text-[10px] text-turd-red hover:bg-white/5">
            💀 Deaths ▸
          </button>
        )}

        {/* Selection detail — single → stats sheet; multi → summary. */}
        {selectedList.length === 1 && (() => {
          const p = selectedList[0];
          const money = (v?: number) => (v != null ? v.toLocaleString() : '—');
          const row = (label: React.ReactNode, val: React.ReactNode, cls = 'text-turd-cream') => (
            <div className="flex justify-between gap-3"><span>{label}</span><span className={`${cls} truncate`}>{val}</span></div>
          );
          return (
            <div
              ref={panelRef}
              className={`absolute z-50 overflow-hidden rounded-lg border border-turd-bronze/50 bg-turd-bg-deep shadow-2xl ${panelPos ? '' : 'right-3 top-3'}`}
              style={{ width: panelSize?.w ?? 264, height: panelSize?.h ?? 480, maxHeight: '88%', minWidth: 224, minHeight: 140, ...(panelPos ? { left: panelPos.x, top: panelPos.y } : {}) }}
            >
              {(['n', 's', 'e', 'w', 'ne', 'nw', 'se', 'sw'] as const).map((d) => (
                <div key={d} onMouseDown={startResize(d)} style={resizeHandleStyle(d)} className="z-20" />
              ))}
              <div className="absolute inset-0 overflow-auto">
              <div onMouseDown={startPanelDrag} className="flex cursor-move select-none items-center gap-2 border-b border-turd-bronze/30 bg-turd-bg-mid/40 px-3 py-2">
                <img src="/game-icons/general.png" alt="" className="h-6 w-6 shrink-0" onError={(e) => { e.currentTarget.style.display = 'none'; }} />
                <span className="flex-1 truncate font-display text-sm text-turd-mustard-bright">{p.name}</span>
                <span className={`shrink-0 text-[9px] ${p.online ? 'text-turd-green' : 'text-turd-red'}`}>{p.online ? '● online' : '○ offline'}</span>
                <button onMouseDown={(e) => e.stopPropagation()} onClick={clearSelection} className="text-xs text-turd-cream-dim hover:text-turd-cream">✕</button>
              </div>
              {/* Economy */}
              <div className="space-y-1.5 border-b border-turd-bronze/20 px-3 py-2 font-mono text-[11px] text-turd-cream-dim">
                {row('🏦 Bank', money(p.bank), 'text-turd-green')}
                {row('🪙 Gold', money(p.gold), 'text-turd-mustard-bright')}
                {row('💵 Cash', <span className="text-turd-cream-dim/40">inventory →</span>)}
                {row('⭐ Fame', p.fame != null ? Math.round(p.fame).toLocaleString() : '—', 'text-turd-mustard-bright')}
              </div>
              {/* Activity / location */}
              <div className="space-y-1.5 border-b border-turd-bronze/20 px-3 py-2 font-mono text-[11px] text-turd-cream-dim">
                {row('Play time', fmtDur(p.playTime))}
                {row('Last seen', p.online ? 'now' : fmtAgo(p.lastLogin))}
                {p.online && p.x != null && p.y != null && (
                  <>
                    {row('Sector', scumToGrid(p.x, p.y), 'text-turd-green')}
                    {row('X / Y', `${Math.round(p.x)} / ${Math.round(p.y)}`)}
                  </>
                )}
                {row('SteamID', <span className="text-[9px]">{p.steamId ?? '—'}</span>)}
              </div>
              {/* "Tech, Where's My Car?" — ring this player's vehicles + pan to one. */}
              <div className="border-b border-turd-bronze/20 px-3 py-2">
                <button
                  onClick={() => {
                    setLayers((L) => ({ ...L, vehicles: true }));
                    setFindCarFor(p.name);
                    const v = vehicles.find((vv) => vv.owner === p.name && vv.x != null && vv.y != null);
                    if (v && v.x != null && v.y != null) setOffset({ x: -zoom * worldToPx(v.x), y: -zoom * worldToPy(v.y) });
                    setActionResult(v && v.x != null && v.y != null
                      ? `Found ${p.name}'s ${vehicleMarker(v.class)} @ ${scumToGrid(v.x, v.y)}`
                      : `No vehicle on record for ${p.name} (turn Vehicles layer on + retry).`);
                  }}
                  className="w-full rounded border border-turd-green/40 px-2 py-1 font-mono text-[10px] text-turd-green hover:bg-turd-green/10"
                >🚗 Tech, Where's My Car?</button>
                {findCarFor === p.name && (
                  <p className="mt-1 text-center font-mono text-[9px] text-turd-cream-dim">{vehicles.filter((v) => v.owner === p.name).length} vehicle(s) ringed green on the map</p>
                )}
              </div>
              {/* Dossier — deaths + vehicles + last-known, aggregated client-side. */}
              {(() => {
                const myDeaths = deaths.filter((d) => d.victim === p.name);
                const myVehicles = vehicles.filter((v) => v.owner === p.name);
                const lk = lastKnown.find((l) => (p.steamId && l.steam === p.steamId) || l.name === p.name);
                return (
                  <div className="space-y-1.5 border-b border-turd-bronze/20 px-3 py-2 font-mono text-[11px] text-turd-cream-dim">
                    <div className="font-display text-[10px] uppercase tracking-wider text-turd-mustard">Dossier</div>
                    <div className="flex justify-between gap-3"><span>💀 Deaths</span><span className="truncate text-turd-cream">{myDeaths.length}{myDeaths.length > 0 ? ` · last ${fmtAgo(myDeaths[myDeaths.length - 1].at)}` : ''}</span></div>
                    <div className="flex justify-between gap-3"><span>🚗 Vehicles</span><span className="truncate text-turd-cream">{myVehicles.length}{myVehicles.length ? ` · ${myVehicles.map((v) => (v.x != null && v.y != null ? scumToGrid(v.x, v.y) : '?')).slice(0, 4).join(' ')}` : ''}</span></div>
                    {!p.online && lk && <div className="flex justify-between gap-3"><span>📍 Last seen</span><span className="truncate text-turd-green">{lk.sector} ({Math.round(lk.x)}, {Math.round(lk.y)})</span></div>}
                    {!p.online && !lk && lastKnownQuery.isFetched && <div className="flex justify-between gap-3"><span>📍 Last seen</span><span className="text-turd-cream-dim/50">no saved position</span></div>}
                  </div>
                );
              })()}
              {/* Vitals + attributes + skills + full inventory (live from the DB endpoints). */}
              {p.steamId && <PlayerSheet steam={p.steamId} target={server} online={p.online} />}
              {/* Live admin edit — fires SCUM admin commands at this player. Online only. */}
              {p.online && (() => {
                const inCls = 'w-full min-w-0 rounded border border-turd-bronze/30 bg-turd-bg-deep/60 px-1.5 py-0.5 font-mono text-[10px] text-turd-cream focus:border-turd-mustard-bright focus:outline-none';
                const setBtn = 'shrink-0 rounded border border-turd-bronze/40 px-2 py-0.5 text-[10px] text-turd-cream hover:bg-white/5 disabled:opacity-30';
                const editRow = (label: string, val: string, set: (v: string) => void, ph: number | undefined, onSet: (v: string) => void) => (
                  <div className="flex items-center gap-1.5">
                    <span className="w-12 shrink-0 font-mono text-[10px] text-turd-cream-dim">{label}</span>
                    <input value={val} onChange={(e) => set(e.target.value.replace(/[^0-9-]/g, ''))} placeholder={ph != null ? String(ph) : ''} className={inCls} />
                    <button disabled={!val} onClick={() => onSet(val)} className={setBtn}>Set</button>
                  </div>
                );
                return (
                  <div className="space-y-2 border-t border-turd-bronze/30 bg-turd-bg-mid/30 px-3 py-2">
                    <div className="font-display text-[10px] uppercase tracking-wider text-turd-mustard">Admin · edit live</div>
                    {editRow('Fame', editFame, setEditFame, p.fame, (v) => void setFame(p.name, v))}
                    {editRow('Bank', editMoney, setEditMoney, p.bank, (v) => void setCurrency(p.name, 1, v))}
                    {editRow('Gold', editGold, setEditGold, p.gold, (v) => void setCurrency(p.name, 2, v))}
                    <div className="flex flex-wrap gap-1.5">
                      <button onClick={() => void setGod(p.name, true)} className="rounded border border-turd-green/40 px-2 py-0.5 text-[10px] text-turd-green hover:bg-turd-green/10">God ON</button>
                      <button onClick={() => void setGod(p.name, false)} className="rounded border border-turd-bronze/40 px-2 py-0.5 text-[10px] text-turd-cream-dim hover:bg-white/5">God OFF</button>
                      <button onClick={() => void adminCmd('SetInfiniteStamina true', p.name, false)} className="rounded border border-turd-green/40 px-2 py-0.5 text-[10px] text-turd-green hover:bg-turd-green/10">∞ Stamina</button>
                      <button onClick={() => void adminCmd('SetInfiniteStamina false', p.name, false)} className="rounded border border-turd-bronze/40 px-2 py-0.5 text-[10px] text-turd-cream-dim hover:bg-white/5">Stam OFF</button>
                    </div>
                  </div>
                );
              })()}
              <p className="border-t border-turd-bronze/20 px-3 py-1.5 text-[9px] leading-tight text-turd-cream-dim/60">
                Live from SCUM.db — vitals, attributes, skills + full inventory.
              </p>
              </div>
            </div>
          );
        })()}
        {selectedList.length > 1 && (
          <div className="absolute right-3 top-3 z-50 w-56 rounded-lg border border-turd-bronze/50 bg-turd-bg-deep/95 px-3 py-2 shadow-2xl backdrop-blur-sm">
            <div className="flex items-center justify-between">
              <span className="font-display text-sm text-turd-mustard-bright">{selectedList.length} selected</span>
              <button onClick={clearSelection} className="text-xs text-turd-cream-dim hover:text-turd-cream">✕</button>
            </div>
            <p className="mt-1 font-mono text-[10px] text-turd-green">{selectedOnline} online · right-click map → teleport</p>
            <div className="mt-1.5 max-h-32 overflow-auto font-mono text-[10px] text-turd-cream-dim">
              {selectedList.map((r) => <div key={r.key} className="truncate">{r.online ? '● ' : '○ '}{r.name}</div>)}
            </div>
          </div>
        )}

        <div className="absolute bottom-3 left-3 flex items-center gap-3">
          <a
            href="https://www.scummymap.com"
            target="_blank"
            rel="noopener noreferrer"
            className="flex items-center gap-1.5 rounded bg-black/70 px-2 py-1 transition-colors hover:bg-black/90"
          >
            <span className="font-display text-[10px] text-turd-cream">www.ScummyMap.com</span>
            <span className="font-mono text-[9px] text-turd-cream-dim">Map Source</span>
          </a>
          <span className="font-mono text-[10px] text-turd-cream-dim/50">
            Scroll: zoom · Drag: pan · Right-click: actions
          </span>
          {!mapLoaded && (
            <span className="font-mono text-[10px] text-turd-mustard animate-pulse">Loading map...</span>
          )}
        </div>

        {spawnOpen && (
          <SpawnBrowser
            targetLabel={spawnTargetLabel}
            onClose={() => setSpawnOpen(false)}
            onSpawn={(cls, count, veh) => void handleSpawn(cls, count, veh)}
          />
        )}
      </div>
    </div>
  );
}
