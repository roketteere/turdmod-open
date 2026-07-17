// Elevated (dev-command) users panel — view who has dev access + add/remove. Changes queue and
// apply on the next server restart (the 6h scheduler rides it; no extra downtime). Mirrors the
// in-game !elevate command — both edit SCUM.db `elevated_users` via the service.
// @dep: tauri-remote.ts remoteElevatedGet/Set.
import { useState, useEffect, useCallback } from 'react';
import { remoteElevatedGet, remoteElevatedSet, type ElevatedInfo } from '../lib/tauri-remote';
import DraggablePanel from './DraggablePanel';

const hostLabel = (t: string) => (t === 'local' ? 'Local' : 'OVH');
const valid17 = (s: string) => /^\d{17}$/.test(s.trim());

const inp = {
  background: '#26262b',
  color: '#ddd',
  border: '1px solid #3a3a40',
  borderRadius: 4,
  padding: '3px 6px',
  fontSize: 12,
} as const;

export default function ElevatedUsers({ target }: { target: string }) {
  const [info, setInfo] = useState<ElevatedInfo | null>(null);
  const [unavailable, setUnavailable] = useState(false);
  const [busy, setBusy] = useState(false);
  const [entry, setEntry] = useState('');
  const [msg, setMsg] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setInfo(await remoteElevatedGet(target));
      setUnavailable(false);
    } catch {
      setInfo(null);
      setUnavailable(true);
    }
  }, [target]);

  useEffect(() => {
    refresh();
    const r = setInterval(refresh, 20000);
    return () => clearInterval(r);
  }, [refresh]);

  const mutate = async (body: { add?: string[]; remove?: string[] }) => {
    setBusy(true);
    setMsg(null);
    try {
      setInfo(await remoteElevatedSet(target, body));
      setMsg('✓ saved — applies next restart');
    } catch (e) {
      setMsg('✗ ' + String(e));
    } finally {
      setBusy(false);
      setTimeout(() => setMsg(null), 6000);
    }
  };

  const add = () => {
    const id = entry.trim();
    if (!valid17(id)) {
      setMsg('✗ need a 17-digit SteamID');
      return;
    }
    setEntry('');
    mutate({ add: [id] });
  };

  const host = hostLabel(target);
  const all = info ? Array.from(new Set([...info.effective, ...info.managed])) : [];

  return (
    <DraggablePanel
      title={`Elevated / Dev Users · ${host}`}
      defaultCorner="br"
      defaultWidth={330}
      defaultHeight={262}
      minH={170}
    >
      <div style={{ padding: 12, color: '#ddd', fontFamily: 'system-ui, sans-serif', fontSize: 12 }}>
        {unavailable ? (
          <div style={{ color: '#d2992b' }}>
            Elevated API not available on {host} (service not updated there yet).
          </div>
        ) : (
          <>
            <div style={{ display: 'flex', gap: 6, marginBottom: 8 }}>
              <input
                value={entry}
                placeholder="17-digit SteamID"
                onChange={(e) => setEntry(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') add();
                }}
                style={{ ...inp, flex: 1, minWidth: 0 }}
              />
              <button
                disabled={busy}
                onClick={add}
                style={{
                  background: '#238636',
                  color: '#fff',
                  border: '1px solid #2ea043',
                  borderRadius: 4,
                  padding: '3px 12px',
                  fontWeight: 700,
                  cursor: busy ? 'wait' : 'pointer',
                }}
              >
                + elevate
              </button>
            </div>
            <div style={{ maxHeight: 150, overflowY: 'auto' }}>
              {all.length === 0 ? (
                <div style={{ color: '#888' }}>No elevated users.</div>
              ) : (
                all.map((id) => {
                  const active = info!.effective.includes(id);
                  const pendingAdd = info!.pending_add.includes(id);
                  const pendingRem = info!.pending_remove.includes(id);
                  const label = pendingRem ? 'removing' : active ? 'active' : pendingAdd ? 'pending' : '—';
                  return (
                    <div key={id} style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 4 }}>
                      <span style={{ flex: 1, minWidth: 0, fontFamily: 'monospace', overflow: 'hidden', textOverflow: 'ellipsis' }}>
                        {id}
                      </span>
                      <span style={{ fontSize: 10, color: active && !pendingRem ? '#3fb950' : '#e8c45a' }}>{label}</span>
                      <button
                        disabled={busy}
                        title="remove"
                        onClick={() => mutate({ remove: [id] })}
                        style={{ background: '#5a1d1d', color: '#fff', border: 'none', borderRadius: 4, cursor: 'pointer', padding: '2px 7px' }}
                      >
                        ×
                      </button>
                    </div>
                  );
                })
              )}
            </div>
            <div style={{ marginTop: 8, color: '#777', fontSize: 11, lineHeight: 1.4 }}>
              Grants all SCUM <code>#</code> dev commands (SetAttributes, SetStamina, …). Changes apply on the next
              restart — the 6h auto-restart rides it. Admins in-game can also use <code>!elevate</code>.
            </div>
            {msg && (
              <div style={{ marginTop: 6, color: msg.startsWith('✓') ? '#3fb950' : '#f85149' }}>{msg}</div>
            )}
          </>
        )}
      </div>
    </DraggablePanel>
  );
}
