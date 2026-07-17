import { useEffect, useState, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';

// Live Monitor — consumes the turdmod-service /monitor + /control API for local or remote server.
// Per-mod 🟢🟡🔴 health + success/fail, the live activity feed (who/what/when/result), system
// CPU/load, and per-mod enable/maintenance/disable control. The control plane Joel asked for.

type ModView = {
  name: string;
  status: 'green' | 'yellow' | 'red';
  state: string;
  commands: string[];
  avg_ms: number;
  metric: {
    calls: number; handled: number; ignored: number; failed: number; timeouts: number;
    last_ms: number; last_fired_secs: number; last_error: string;
  };
};
type Activity = { at: number; event: string; from: string; mod_name: string; outcome: string; ms: number };
type SystemStats = {
  cpu_pct: number; mem_used_mb: number; mem_total_mb: number; svc_mem_mb: number;
  scum_cpu_pct: number; scum_mem_mb: number; scum_running: boolean; updated_secs: number;
};
type PlayerRow = { name: string; steam: string; online: boolean; sessions: number; playtime_secs: number; last_seen: number; is_admin: boolean };
type Players = { online_count: number; total_count: number; offline_count: number; players: PlayerRow[] };

const DOT: Record<string, string> = { green: '🟢', yellow: '🟡', red: '🔴' };

export function MonitorPage() {
  // persist the selected server across reloads/app restarts
  const [target, setTargetState] = useState<'local' | 'remote'>(
    () => ((localStorage.getItem('monitor.target') as 'local' | 'remote') || 'local'),
  );
  const setTarget = (t: 'local' | 'remote') => { localStorage.setItem('monitor.target', t); setTargetState(t); };
  const [countdown, setCountdown] = useState(4);
  const [mods, setMods] = useState<ModView[]>([]);
  const [activity, setActivity] = useState<Activity[]>([]);
  const [sys, setSys] = useState<SystemStats | null>(null);
  const [players, setPlayers] = useState<Players | null>(null);
  const [err, setErr] = useState<string>('');
  const [tunnelOn, setTunnelOn] = useState(false);

  const [auto, setAuto] = useState(false); // OFF by default — no background polling

  // Cheap, in-memory/local service stats (no bridge) — safe to auto-refresh.
  const refreshStats = useCallback(async () => {
    try {
      const [m, a, s] = await Promise.all([
        invoke<ModView[]>('monitor_mods', { target }),
        invoke<Activity[]>('monitor_activity', { target, limit: 80 }),
        invoke<SystemStats>('monitor_system', { target }),
      ]);
      setMods(m || []); setActivity(a || []); setSys(s); setErr('');
    } catch (e: any) { setErr(String(e)); }
  }, [target]);

  // Players come from the BRIDGE (getOnlinePlayers). On-demand ONLY — NEVER put
  // this on an interval: a steady bridge poll wedges the named-pipe relay
  // ("all pipe instances are busy") and takes the live server's admin RPC down.
  const refreshPlayers = useCallback(async () => {
    try { setPlayers(await invoke<Players>('monitor_players', { target })); setErr(''); }
    catch (e: any) { setErr(String(e)); }
  }, [target]);

  const refreshAll = useCallback(async () => { await Promise.all([refreshStats(), refreshPlayers()]); }, [refreshStats, refreshPlayers]);

  // Load once on mount / when the target changes. No auto-poll unless toggled.
  useEffect(() => { void refreshAll(); }, [refreshAll]);

  // Opt-in auto-refresh of the CHEAP stats only (never the bridge/player call).
  // Off by default; 10s when on.
  useEffect(() => {
    if (!auto) return;
    setCountdown(10);
    const refreshId = setInterval(() => { void refreshStats(); setCountdown(10); }, 10000);
    const tickId = setInterval(() => setCountdown((c) => (c > 1 ? c - 1 : 1)), 1000);
    return () => { clearInterval(refreshId); clearInterval(tickId); };
  }, [auto, refreshStats]);

  const setModState = async (name: string, state: string) => {
    try { await invoke('control_mod', { target, name, state }); void refreshStats(); }
    catch (e: any) { setErr(String(e)); }
  };

  useEffect(() => { invoke<boolean>('tunnel_status').then(setTunnelOn).catch(() => {}); }, [target]);

  const toggleTunnel = async () => {
    try {
      if (tunnelOn) { await invoke('tunnel_stop'); setTunnelOn(false); }
      else { await invoke('tunnel_start'); setTunnelOn(true); setTimeout(() => void refreshAll(), 1500); }
    } catch (e: any) { setErr(String(e)); }
  };

  const ago = (secs: number) => (secs ? `${Math.max(0, Math.floor(Date.now() / 1000 - secs))}s` : '—');

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 20 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
        <h1 style={{ fontSize: 20, fontWeight: 700, margin: 0 }}>Live Monitor</h1>
        <select value={target} onChange={(e) => setTarget(e.target.value as 'local' | 'remote')}
          style={{ background: '#222', color: '#eee', borderRadius: 6, padding: '4px 8px' }}>
          <option value="local">Local</option>
          <option value="remote">Remote</option>
        </select>
        <button onClick={() => void refreshAll()} style={btn(false)}>Refresh</button>
        <button onClick={() => setAuto((a) => !a)} style={btn(auto)} title="Auto-refreshes the cheap stats only (mods/activity/system). Player count comes from the bridge and stays on-demand — Refresh to update it.">
          {auto ? '⏸ Auto 10s (stats)' : '▶ Auto off'}
        </button>
        {target === 'remote' && (
          <button onClick={toggleTunnel} style={btn(tunnelOn)}>
            {tunnelOn ? '⛓ remote server Tunnel ON' : 'Start remote server Tunnel'}
          </button>
        )}
        {auto && <span style={{ color: '#999', fontSize: 12 }}>stats refresh in {countdown}s</span>}
        <span style={{ color: '#666', fontSize: 11 }}>· players on-demand (Refresh)</span>
      </div>
      {err && <div style={{ color: '#f87171', fontSize: 13 }}>⚠ {err}</div>}

      {players && (
        <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap', alignItems: 'center' }}>
          <div style={{ background: '#14321e', border: '1px solid #2c6e49', borderRadius: 10, padding: '10px 20px' }}>
            <div style={{ color: '#7fd8a0', fontSize: 11, fontWeight: 600 }}>🟢 ONLINE</div>
            <div style={{ fontSize: 30, fontWeight: 800, color: '#4ade80', lineHeight: 1.1 }}>{players.online_count}</div>
          </div>
          <div style={{ background: '#2a2a2a', borderRadius: 10, padding: '10px 20px' }}>
            <div style={{ color: '#999', fontSize: 11, fontWeight: 600 }}>⚫ OFFLINE</div>
            <div style={{ fontSize: 30, fontWeight: 800, lineHeight: 1.1 }}>{players.offline_count}</div>
          </div>
          <div style={{ background: '#2a2a2a', borderRadius: 10, padding: '10px 20px' }}>
            <div style={{ color: '#999', fontSize: 11, fontWeight: 600 }}>TOTAL TRACKED</div>
            <div style={{ fontSize: 30, fontWeight: 800, lineHeight: 1.1 }}>{players.total_count}</div>
          </div>
        </div>
      )}

      {sys && (
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: 12 }}>
          <Stat label="Box CPU" v={`${sys.cpu_pct.toFixed(0)}%`} />
          <Stat label="Box Mem" v={`${sys.mem_used_mb} / ${sys.mem_total_mb} MB`} />
          <Stat label="SCUM" v={sys.scum_running ? `${sys.scum_cpu_pct.toFixed(0)}% · ${sys.scum_mem_mb} MB` : 'DOWN'} />
          <Stat label="Service Mem" v={`${sys.svc_mem_mb} MB`} />
        </div>
      )}

      {players && players.players.length > 0 && (
        <div>
          <h2 style={{ fontSize: 15, fontWeight: 600, marginBottom: 8 }}>Players ({players.total_count})</h2>
          <div style={{ background: '#1c1c1c', borderRadius: 8, padding: 8, fontFamily: 'monospace', fontSize: 12, maxHeight: 280, overflow: 'auto' }}>
            <div style={{ display: 'flex', gap: 8, color: '#888', borderBottom: '1px solid #333', paddingBottom: 4, marginBottom: 4 }}>
              <span style={{ width: 16 }} />
              <span style={{ flex: 1 }}>name</span>
              <span style={{ width: 70, textAlign: 'right' }}>sessions</span>
              <span style={{ width: 90, textAlign: 'right' }}>playtime</span>
              <span style={{ width: 80, textAlign: 'right' }}>last seen</span>
            </div>
            {players.players.map((p) => (
              <div key={p.steam || p.name} style={{ display: 'flex', gap: 8, padding: '2px 0' }}>
                <span style={{ width: 16 }}>{p.online ? '🟢' : '⚫'}</span>
                <span style={{ flex: 1, color: p.online ? '#4ade80' : '#ccc' }}>{p.name}{p.is_admin ? ' 👑' : ''}</span>
                <span style={{ width: 70, textAlign: 'right', color: '#999' }}>{p.sessions}</span>
                <span style={{ width: 90, textAlign: 'right', color: '#999' }}>{fmtDur(p.playtime_secs)}</span>
                <span style={{ width: 80, textAlign: 'right', color: '#999' }}>{p.online ? 'now' : fmtAgo(p.last_seen)}</span>
              </div>
            ))}
          </div>
        </div>
      )}

      <div>
        <h2 style={{ fontSize: 15, fontWeight: 600, marginBottom: 8 }}>Mods ({mods.length})</h2>
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(2, 1fr)', gap: 8 }}>
          {mods.map((m) => (
            <div key={m.name} style={{ background: '#1c1c1c', borderRadius: 8, padding: 12, display: 'flex', justifyContent: 'space-between', gap: 8 }}>
              <div style={{ minWidth: 0 }}>
                <div style={{ fontFamily: 'monospace' }}>
                  {DOT[m.status] || '⚪'} {m.name} <span style={{ color: '#888', fontSize: 11 }}>{m.state}</span>
                </div>
                <div style={{ color: '#999', fontSize: 11 }}>
                  {m.metric.calls} calls · {m.metric.handled} ok · {m.metric.failed} fail · {m.metric.timeouts} t/o · {m.avg_ms}ms avg · {ago(m.metric.last_fired_secs)}
                  {m.commands.length > 0 ? ` · ${m.commands.join(' ')}` : ''}
                </div>
                {m.metric.last_error && <div style={{ color: '#f87171', fontSize: 11 }}>{m.metric.last_error}</div>}
              </div>
              <div style={{ display: 'flex', gap: 4, alignItems: 'flex-start' }}>
                <button onClick={() => setModState(m.name, 'enabled')} style={btn(m.state === 'enabled')}>On</button>
                <button onClick={() => setModState(m.name, 'maintenance')} style={btn(m.state === 'maintenance')}>Maint</button>
                <button onClick={() => setModState(m.name, 'disabled')} style={btn(m.state === 'disabled')}>Off</button>
              </div>
            </div>
          ))}
        </div>
      </div>

      <div>
        <h2 style={{ fontSize: 15, fontWeight: 600, marginBottom: 8 }}>Activity</h2>
        <div style={{ background: '#1c1c1c', borderRadius: 8, padding: 8, fontFamily: 'monospace', fontSize: 12, maxHeight: 320, overflow: 'auto' }}>
          {activity.length === 0 && <div style={{ color: '#888' }}>no activity yet</div>}
          {activity.map((a, i) => (
            <div key={i} style={{ display: 'flex', gap: 8 }}>
              <span style={{ color: '#888', width: 48 }}>{ago(a.at)}</span>
              <span style={{ color: a.outcome === 'failed' || a.outcome === 'timeout' ? '#f87171' : '#4ade80', width: 64 }}>{a.outcome}</span>
              <span style={{ width: 90 }}>{a.mod_name}</span>
              <span style={{ color: '#999' }}>{a.event}{a.from ? ` ← ${a.from}` : ''} ({a.ms}ms)</span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

function Stat({ label, v }: { label: string; v: string }) {
  return (
    <div style={{ background: '#1c1c1c', borderRadius: 8, padding: 12 }}>
      <div style={{ color: '#999', fontSize: 11 }}>{label}</div>
      <div style={{ fontSize: 18, fontWeight: 700 }}>{v}</div>
    </div>
  );
}

function fmtDur(secs: number): string {
  const h = Math.floor(secs / 3600), m = Math.floor((secs % 3600) / 60);
  return h > 0 ? `${h}h ${m}m` : `${m}m`;
}
function fmtAgo(epochSecs: number): string {
  if (!epochSecs) return '—';
  const d = Math.floor(Date.now() / 1000 - epochSecs);
  if (d < 60) return `${d}s`;
  if (d < 3600) return `${Math.floor(d / 60)}m`;
  if (d < 86400) return `${Math.floor(d / 3600)}h`;
  return `${Math.floor(d / 86400)}d`;
}

function btn(active: boolean): React.CSSProperties {
  return {
    fontSize: 11, padding: '4px 8px', borderRadius: 6, cursor: 'pointer', border: 'none',
    background: active ? '#d9a441' : '#2a2a2a', color: active ? '#000' : '#ccc',
  };
}
