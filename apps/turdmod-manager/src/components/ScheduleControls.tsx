// Recurring restart schedule, in a movable/resizable panel. Service works in
// epochs (no timezone); we render local clocks + live countdowns client-side.
// Acts on the host in `target`. @dep: tauri-remote.ts schedule wrappers.
import { useState, useEffect, useCallback, useRef } from 'react';
import {
  remoteServerScheduleGet,
  remoteServerScheduleSet,
  type RestartScheduleInfo,
  type WarnBanner,
} from '../lib/tauri-remote';
import DraggablePanel from './DraggablePanel';

const hostLabel = (t: string) => (t === 'local' ? 'Local' : 'OVH');

function fmtDur(secs: number): string {
  if (!isFinite(secs)) return '—';
  const neg = secs < 0;
  let s = Math.abs(Math.floor(secs));
  const d = Math.floor(s / 86400);
  s %= 86400;
  const h = Math.floor(s / 3600);
  s %= 3600;
  const m = Math.floor(s / 60);
  const sec = s % 60;
  const out = d > 0 ? `${d}d ${h}h` : h > 0 ? `${h}h ${m}m` : m > 0 ? `${m}m ${sec}s` : `${sec}s`;
  return (neg ? '-' : '') + out;
}

function fmtClock(epoch?: number | null): string {
  if (!epoch) return '—';
  return new Date(epoch * 1000).toLocaleString(undefined, {
    weekday: 'short',
    hour: '2-digit',
    minute: '2-digit',
  });
}

const INTERVALS = [1, 2, 3, 4, 6, 8, 12, 24];
const WARNS = [
  { v: 30, l: '30s' },
  { v: 60, l: '1m' },
  { v: 120, l: '2m' },
  { v: 300, l: '5m' },
  { v: 600, l: '10m' },
];

const selStyle = {
  background: '#26262b',
  color: '#ddd',
  border: '1px solid #3a3a40',
  borderRadius: 4,
  padding: '2px 4px',
  fontSize: 12,
} as const;

export default function ScheduleControls({ target }: { target: string }) {
  const [info, setInfo] = useState<RestartScheduleInfo | null>(null);
  const [unavailable, setUnavailable] = useState(false);
  const fetchedAt = useRef(0);
  const [, setTick] = useState(0);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [showMsgs, setShowMsgs] = useState(false);
  const [draft, setDraft] = useState<WarnBanner[] | null>(null);

  const refresh = useCallback(async () => {
    try {
      const d = await remoteServerScheduleGet(target);
      setInfo(d);
      setUnavailable(false);
      fetchedAt.current = Date.now();
    } catch {
      setInfo(null);
      setUnavailable(true);
    }
  }, [target]);

  useEffect(() => {
    refresh();
    const r = setInterval(refresh, 15000);
    const t = setInterval(() => setTick((x) => x + 1), 1000);
    return () => {
      clearInterval(r);
      clearInterval(t);
    };
  }, [refresh]);

  const apply = async (patch: {
    enabled?: boolean;
    interval_hours?: number;
    warn_secs?: number;
    messages?: WarnBanner[];
  }) => {
    setBusy(true);
    setMsg(null);
    try {
      const d = await remoteServerScheduleSet(target, patch);
      setInfo(d);
      fetchedAt.current = Date.now();
      setMsg('✓ saved');
    } catch (e) {
      setMsg('✗ ' + String(e));
    } finally {
      setBusy(false);
      setTimeout(() => setMsg(null), 6000);
    }
  };

  const host = hostLabel(target);
  const serverNow = info ? info.now + (Date.now() - fetchedAt.current) / 1000 : 0;
  const nextIn = info?.next_restart_at ? info.next_restart_at - serverNow : null;
  const lastAgo = info?.last_restart_at ? serverNow - info.last_restart_at : null;
  const en = info?.enabled ?? false;

  const headerRight = (
    <span style={{ display: 'flex', alignItems: 'center', gap: 5, fontSize: 11, color: '#bbb' }}>
      <span
        style={{
          width: 9,
          height: 9,
          borderRadius: '50%',
          background: en ? '#3fb950' : '#888',
          display: 'inline-block',
        }}
      />
      {en ? 'on' : 'off'}
    </span>
  );

  return (
    <DraggablePanel
      title={`Restart Schedule · ${host}`}
      headerRight={headerRight}
      defaultCorner="bl"
      defaultWidth={320}
      defaultHeight={210}
      minH={150}
    >
      <div style={{ padding: 12, color: '#ddd', fontFamily: 'system-ui, sans-serif', fontSize: 12 }}>
        {unavailable ? (
          <div style={{ color: '#d2992b' }}>
            Schedule API not available on {host} (service not updated there yet).
          </div>
        ) : (
          <>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
              <button
                disabled={busy || !info}
                onClick={() => apply({ enabled: !en })}
                style={{
                  background: en ? '#238636' : '#5a1d1d',
                  color: '#fff',
                  border: '1px solid ' + (en ? '#2ea043' : '#7a2a2a'),
                  borderRadius: 12,
                  padding: '3px 12px',
                  fontWeight: 700,
                  cursor: busy ? 'wait' : 'pointer',
                  minWidth: 46,
                }}
              >
                {en ? 'ON' : 'OFF'}
              </button>
              <label style={{ color: '#999', display: 'flex', alignItems: 'center', gap: 4 }}>
                every
                <select
                  value={info?.interval_hours ?? 6}
                  disabled={busy || !info}
                  onChange={(e) => apply({ interval_hours: Number(e.target.value) })}
                  style={selStyle}
                >
                  {INTERVALS.map((h) => (
                    <option key={h} value={h}>
                      {h}h
                    </option>
                  ))}
                </select>
              </label>
              <label style={{ color: '#999', display: 'flex', alignItems: 'center', gap: 4 }}>
                warn
                <select
                  value={info?.warn_secs ?? 300}
                  disabled={busy || !info}
                  onChange={(e) => apply({ warn_secs: Number(e.target.value) })}
                  style={selStyle}
                >
                  {WARNS.map((w) => (
                    <option key={w.v} value={w.v}>
                      {w.l}
                    </option>
                  ))}
                </select>
              </label>
            </div>

            <div style={{ marginTop: 11, lineHeight: 1.7 }}>
              <div>
                <span style={{ color: '#999' }}>Next restart: </span>
                {en ? (
                  <>
                    <span style={{ color: '#e8c45a' }}>{fmtClock(info?.next_restart_at)}</span>
                    <span style={{ color: '#7fb5ff' }}> · in {fmtDur(nextIn ?? NaN)}</span>
                  </>
                ) : (
                  <span style={{ color: '#888' }}>disabled</span>
                )}
              </div>
              <div>
                <span style={{ color: '#999' }}>Last restart: </span>
                {info?.last_restart_at ? (
                  <span style={{ color: '#bbb' }}>
                    {fmtClock(info.last_restart_at)} · {fmtDur(lastAgo ?? NaN)} ago
                  </span>
                ) : (
                  <span style={{ color: '#888' }}>—</span>
                )}
              </div>
            </div>

            <div style={{ marginTop: 10, borderTop: '1px solid #2c2c30', paddingTop: 8 }}>
              <button
                onClick={() => {
                  if (!showMsgs) setDraft(info?.messages ? info.messages.map((m) => ({ ...m })) : []);
                  setShowMsgs((v) => !v);
                }}
                style={{ background: 'none', border: 'none', color: '#9ab', cursor: 'pointer', fontSize: 12, padding: 0 }}
              >
                {showMsgs ? '▾' : '▸'} Warning messages ({info?.messages?.length ?? 0})
              </button>
              {showMsgs && draft && (
                <div style={{ marginTop: 6, maxHeight: 170, overflowY: 'auto' }}>
                  {draft.map((b, i) => (
                    <div key={i} style={{ display: 'flex', gap: 4, marginBottom: 4, alignItems: 'center' }}>
                      <input
                        type="number"
                        value={b.at_secs}
                        title="seconds before restart"
                        onChange={(e) =>
                          setDraft((d) => d!.map((x, j) => (j === i ? { ...x, at_secs: Number(e.target.value) } : x)))
                        }
                        style={{ ...selStyle, width: 48 }}
                      />
                      <input
                        value={b.color}
                        title="R-G-B color"
                        onChange={(e) =>
                          setDraft((d) => d!.map((x, j) => (j === i ? { ...x, color: e.target.value } : x)))
                        }
                        style={{ ...selStyle, width: 66 }}
                      />
                      <input
                        value={b.text}
                        onChange={(e) =>
                          setDraft((d) => d!.map((x, j) => (j === i ? { ...x, text: e.target.value } : x)))
                        }
                        style={{ ...selStyle, flex: 1, minWidth: 0 }}
                      />
                      <button
                        title="remove"
                        onClick={() => setDraft((d) => d!.filter((_, j) => j !== i))}
                        style={{ background: '#5a1d1d', color: '#fff', border: 'none', borderRadius: 4, cursor: 'pointer', padding: '2px 6px' }}
                      >
                        ×
                      </button>
                    </div>
                  ))}
                  <div style={{ display: 'flex', gap: 6, marginTop: 6 }}>
                    <button
                      onClick={() =>
                        setDraft((d) => [...(d ?? []), { at_secs: 60, color: '255-0-0', text: 'Server restart soon' }])
                      }
                      style={{ background: '#26262b', color: '#ddd', border: '1px solid #3a3a40', borderRadius: 4, cursor: 'pointer', padding: '2px 8px' }}
                    >
                      + add
                    </button>
                    <button
                      disabled={busy || !draft}
                      onClick={() => draft && apply({ messages: draft })}
                      style={{ background: '#238636', color: '#fff', border: '1px solid #2ea043', borderRadius: 4, cursor: busy ? 'wait' : 'pointer', padding: '2px 10px', fontWeight: 700 }}
                    >
                      Save messages
                    </button>
                  </div>
                  <div style={{ marginTop: 4, color: '#777', fontSize: 11, lineHeight: 1.4 }}>
                    Each banner fires at its “seconds before restart” mark (must be under the warn lead). Disabled
                    schedule = no restart and no warnings.
                  </div>
                </div>
              )}
            </div>

            {msg && (
              <div style={{ marginTop: 8, color: msg.startsWith('✓') ? '#3fb950' : '#f85149' }}>{msg}</div>
            )}
          </>
        )}
      </div>
    </DraggablePanel>
  );
}
