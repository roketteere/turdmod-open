// Server lifecycle controls for the Admin Map, in a movable/resizable panel.
// Wires the (already-existing) remote_server_* service plumbing to actual buttons.
// Acts on the host passed in `target` ("remote", "local"). @dep: tauri-remote.ts.
import { useState, useEffect, useCallback, type CSSProperties } from 'react';
import {
  remoteServerStart,
  remoteServerStop,
  remoteServerRestart,
  remoteServerStatus,
  remoteServerBattleye,
  type ServerStatus,
} from '../lib/tauri-remote';
import DraggablePanel from './DraggablePanel';

const hostLabel = (t: string) => (t === 'local' ? 'Local' : 'Remote');

function fmtUptime(s?: number): string {
  if (!s || s < 0) return '—';
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return h > 0 ? `${h}h ${m}m` : `${m}m`;
}

const btn = (bg: string, disabled?: boolean): CSSProperties => ({
  background: disabled ? '#3a3a3a' : bg,
  color: disabled ? '#888' : '#fff',
  border: 'none',
  borderRadius: 6,
  padding: '6px 12px',
  fontSize: 13,
  fontWeight: 600,
  cursor: disabled ? 'not-allowed' : 'pointer',
});

export default function ServerControls({ target }: { target: string }) {
  const [status, setStatus] = useState<ServerStatus | null>(null);
  const [statusErr, setStatusErr] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [countdown, setCountdown] = useState(60);

  const refresh = useCallback(async () => {
    try {
      setStatus(await remoteServerStatus(target));
      setStatusErr(null);
    } catch (e) {
      // Don't fail silently — a down tunnel / unreachable host reads as "broken
      // controls". Surface why so it's never a mystery.
      setStatus(null);
      setStatusErr(String(e));
    }
  }, [target]);

  useEffect(() => {
    refresh();
    const id = setInterval(refresh, 5000);
    return () => clearInterval(id);
  }, [refresh]);

  const run = async (label: string, fn: () => Promise<unknown>, confirmMsg?: string) => {
    if (confirmMsg && !window.confirm(confirmMsg)) return;
    setBusy(label);
    setMsg(null);
    try {
      const r = await fn();
      setMsg(`✓ ${label} — ${typeof r === 'object' ? JSON.stringify(r) : String(r)}`);
      setTimeout(refresh, 1500);
    } catch (e) {
      setMsg(`✗ ${label} failed: ${String(e)}`);
    } finally {
      setBusy(null);
      setTimeout(() => setMsg(null), 9000);
    }
  };

  const host = hostLabel(target);
  const running = status?.server_running;
  const be = status?.battleye;
  const dotColor = running == null ? '#888' : running ? '#3fb950' : '#f85149';
  const dotText = running == null ? 'unknown' : running ? 'running' : 'stopped';

  const headerRight = (
    <span style={{ display: 'flex', alignItems: 'center', gap: 5, fontSize: 11, color: '#bbb' }}>
      <span style={{ width: 9, height: 9, borderRadius: '50%', background: dotColor, display: 'inline-block' }} />
      {dotText}
    </span>
  );

  return (
    <DraggablePanel
      title={`Server Controls · ${host}`}
      headerRight={headerRight}
      defaultCorner="br"
      defaultWidth={290}
      defaultHeight={222}
      minH={150}
    >
      <div style={{ padding: 12, color: '#ddd', fontFamily: 'system-ui, sans-serif' }}>
        <div style={{ fontSize: 12, color: '#999', marginBottom: 10, display: 'flex', gap: 14, flexWrap: 'wrap' }}>
          <span>pid: {status?.pid ?? '—'}</span>
          <span>uptime: {fmtUptime(status?.uptime_secs)}</span>
          <span>restarts: {status?.restarts ?? '—'}</span>
        </div>

        {statusErr && (
          <div
            style={{
              marginBottom: 10,
              fontSize: 11,
              color: '#f0883e',
              background: '#3a2a14',
              border: '1px solid #6a4a1a',
              borderRadius: 6,
              padding: '6px 8px',
              wordBreak: 'break-word',
            }}
          >
            ⚠ Can’t reach {host}: {statusErr}
            {target !== 'local' && (
              <div style={{ color: '#bbb', marginTop: 3 }}>
                The remote server SSH tunnel may be down. It auto-starts on app launch — reopen the
                Manager, or start it on the Engine page.
              </div>
            )}
          </div>
        )}

        <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap', alignItems: 'center' }}>
          <button
            style={btn('#238636', !!busy || running === true)}
            disabled={!!busy || running === true}
            onClick={() => run('Start', () => remoteServerStart(target))}
          >
            {busy === 'Start' ? '…' : 'Start'}
          </button>
          <button
            style={btn('#da3633', !!busy || running === false)}
            disabled={!!busy || running === false}
            onClick={() =>
              run('Stop', () => remoteServerStop(target), `Stop the ${host} server now? Players will be disconnected.`)
            }
          >
            {busy === 'Stop' ? '…' : 'Stop'}
          </button>
          <button
            style={btn('#9e6a03', !!busy)}
            disabled={!!busy}
            onClick={() =>
              run(
                'Restart',
                () => remoteServerRestart(target, countdown),
                `Restart the ${host} server with a ${countdown}s warning banner?`,
              )
            }
          >
            {busy === 'Restart' ? '…' : 'Restart'}
          </button>
          <label style={{ fontSize: 12, color: '#999', display: 'flex', alignItems: 'center', gap: 4 }}>
            warn
            <select
              value={countdown}
              onChange={(e) => setCountdown(Number(e.target.value))}
              style={{ background: '#26262b', color: '#ddd', border: '1px solid #3a3a40', borderRadius: 4, padding: '2px 4px' }}
            >
              <option value={0}>0s</option>
              <option value={30}>30s</option>
              <option value={60}>60s</option>
              <option value={120}>2m</option>
              <option value={300}>5m</option>
            </select>
          </label>
        </div>

        <div style={{ marginTop: 11, display: 'flex', alignItems: 'center', gap: 8, fontSize: 12 }}>
          <span style={{ color: '#999' }}>BattlEye</span>
          <button
            disabled={!!busy || be == null}
            onClick={() =>
              run(
                `BattlEye ${be ? 'off' : 'on'}`,
                () => remoteServerBattleye(target, !be),
                `Stage BattlEye ${be ? 'OFF' : 'ON'} on ${host}? Applies on the next restart.`,
              )
            }
            style={{
              background: be == null ? '#3a3a3a' : be ? '#238636' : '#5a1d1d',
              color: be == null ? '#888' : '#fff',
              border: '1px solid ' + (be ? '#2ea043' : '#7a2a2a'),
              borderRadius: 12,
              padding: '3px 12px',
              fontSize: 12,
              fontWeight: 700,
              cursor: !!busy || be == null ? 'not-allowed' : 'pointer',
              minWidth: 46,
            }}
          >
            {be == null ? '—' : be ? 'ON' : 'OFF'}
          </button>
          <span style={{ color: '#777', fontSize: 11 }}>applies on restart</span>
        </div>

        {msg && (
          <div
            style={{
              marginTop: 9,
              fontSize: 12,
              color: msg.startsWith('✓') ? '#3fb950' : '#f85149',
              wordBreak: 'break-word',
            }}
          >
            {msg}
          </div>
        )}
      </div>
    </DraggablePanel>
  );
}
