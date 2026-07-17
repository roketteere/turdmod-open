import { useState } from 'react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { engineRpc } from '../../lib/tauri-engine';
import { dispatchAdmin, dispatchNotification, fetchRoster, type RosterPlayer } from '../../lib/admin-dispatch';

// Players tab — live roster + per-player action panel. Per-player admin
// verbs fire through the shared dispatch (Force = zero-admin), so a single
// online player can be acted on with no admin account present.

const ROSTER_MS = 2000;

// Common per-player admin verbs. {p} is replaced with the selected name.
const QUICK_VERBS: { label: string; cmd: (p: string) => string }[] = [
  { label: 'God On', cmd: (p) => `SetGodMode ${p} 1` },
  { label: 'God Off', cmd: (p) => `SetGodMode ${p} 0` },
  { label: 'Heal', cmd: (p) => `Heal ${p}` },
  { label: 'Teleport Me→Them', cmd: (p) => `TeleportTo ${p}` },
];

export function PlayersTab() {
  const roster = useQuery({ queryKey: ['engine', 'getOnlinePlayers'], queryFn: fetchRoster, refetchInterval: ROSTER_MS, retry: false });
  const players = roster.data?.players ?? [];
  const [sel, setSel] = useState<RosterPlayer | null>(null);
  const [feedback, setFeedback] = useState<{ ok: boolean; text: string } | null>(null);

  const executor = players[0]?.name ?? null;
  const flash = (ok: boolean, text: string) => { setFeedback({ ok, text }); setTimeout(() => setFeedback(null), 4000); };

  const runVerb = async (cmd: string) => {
    const r = await dispatchAdmin({ command: cmd, force: true, executor });
    flash(!r.isError, r.text);
  };

  return (
    <div className="grid h-full min-h-0 grid-cols-[1fr_360px] gap-3">
      {/* Roster */}
      <section className="flex min-h-0 flex-col overflow-hidden glass rounded-xl">
        <header className="flex items-center justify-between border-b border-turd-bronze/30 p-3">
          <h2 className="font-display text-sm uppercase tracking-widest text-turd-mustard">Online Players</h2>
          <span className="font-mono text-[10px] text-turd-cream-dim">{roster.isLoading ? 'loading…' : `${roster.data?.count ?? 0} connected`}</span>
        </header>
        <div className="flex-1 overflow-auto">
          {players.length === 0 && !roster.isLoading ? (
            <p className="px-3 py-6 text-center text-xs text-turd-cream-dim">No players connected.</p>
          ) : (
            <table className="w-full font-mono text-[11px]">
              <thead className="sticky top-0 bg-turd-bg-mid/95">
                <tr className="border-b border-turd-bronze/30 text-left text-turd-cream-dim">
                  <th className="px-3 py-2 font-normal">Name</th>
                  <th className="px-3 py-2 font-normal">Class</th>
                </tr>
              </thead>
              <tbody>
                {players.map((p) => (
                  <tr key={p.ptr} onClick={() => setSel(p)} className={`cursor-pointer border-b border-turd-bronze/10 ${sel?.ptr === p.ptr ? 'bg-turd-mustard/20 text-turd-mustard-bright' : 'text-turd-cream hover:bg-white/5'}`}>
                    <td className="px-3 py-1.5">{p.name || <em className="text-turd-cream-dim">(no name)</em>}</td>
                    <td className="px-3 py-1.5 text-turd-cream-dim">{p.class}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      </section>

      {/* Action panel */}
      <section className="flex min-h-0 flex-col gap-3 overflow-auto">
        <div className="glass rounded-xl p-4">
          <h2 className="font-display text-sm uppercase tracking-widest text-turd-mustard">Selected</h2>
          {sel ? (
            <p className="mt-1 font-mono text-xs text-turd-cream">{sel.name || sel.controller}</p>
          ) : (
            <p className="mt-1 font-mono text-xs text-turd-cream-dim">Pick a player from the roster.</p>
          )}
        </div>

        {sel && (
          <>
            <div className="glass rounded-xl p-4">
              <h3 className="font-display text-xs uppercase tracking-widest text-turd-mustard">Quick Actions <span className="text-turd-cream-dim/60">(Force)</span></h3>
              <div className="mt-2 grid grid-cols-2 gap-2">
                {QUICK_VERBS.map((q) => (
                  <button key={q.label} type="button" onClick={() => void runVerb(q.cmd(sel.name))} disabled={!executor} className="rounded border border-turd-bronze/50 bg-turd-bg-soft px-2 py-1.5 font-mono text-[11px] text-turd-cream hover:border-turd-mustard hover:text-turd-mustard-bright disabled:opacity-30">{q.label}</button>
                ))}
              </div>
            </div>

            <TeleportPanel player={sel} onResult={flash} />
            <NotifyPanel playerName={sel.name} onResult={flash} />
          </>
        )}

        {feedback && <p className={`font-mono text-[11px] ${feedback.ok ? 'text-turd-green' : 'text-turd-red'}`}>{feedback.text}</p>}
      </section>
    </div>
  );
}

function TeleportPanel({ player, onResult }: { player: RosterPlayer; onResult: (ok: boolean, t: string) => void }) {
  const [x, setX] = useState(''); const [y, setY] = useState(''); const [z, setZ] = useState('');
  const m = useMutation({
    mutationFn: (p: { ptr: string; x: string; y: string; z: string }) => engineRpc<{ ok?: boolean; error?: string; x?: number; y?: number; z?: number }>('teleportPlayer', p),
  });
  const go = () => {
    if (!x || !y || !z) return;
    m.mutate({ ptr: player.ptr, x, y, z }, {
      onSuccess: (d) => onResult(!d?.error, d?.error ?? `teleported to (${d?.x}, ${d?.y}, ${d?.z})`),
      onError: (e) => onResult(false, String((e as Error)?.message ?? e)),
    });
  };
  const inp = 'mt-1 w-full rounded border border-turd-bronze/60 bg-turd-bg-soft px-2 py-1 font-mono text-xs text-turd-cream focus:border-turd-mustard focus:outline-none';
  return (
    <div className="glass rounded-xl p-4">
      <h3 className="font-display text-xs uppercase tracking-widest text-turd-mustard">Teleport</h3>
      <div className="mt-2 grid grid-cols-3 gap-2">
        <label className="font-mono text-[9px] uppercase text-turd-cream-dim">X<input value={x} onChange={(e) => setX(e.target.value)} className={inp} placeholder="100000" /></label>
        <label className="font-mono text-[9px] uppercase text-turd-cream-dim">Y<input value={y} onChange={(e) => setY(e.target.value)} className={inp} placeholder="-10000" /></label>
        <label className="font-mono text-[9px] uppercase text-turd-cream-dim">Z<input value={z} onChange={(e) => setZ(e.target.value)} className={inp} placeholder="300" /></label>
      </div>
      <button type="button" onClick={go} disabled={!x || !y || !z || m.isPending} className="mt-3 w-full rounded border border-turd-mustard/60 bg-turd-mustard/20 px-3 py-1.5 font-display text-[11px] uppercase tracking-wider text-turd-mustard-bright hover:bg-turd-mustard/30 disabled:opacity-30">{m.isPending ? 'Teleporting…' : 'Teleport'}</button>
    </div>
  );
}

function NotifyPanel({ playerName, onResult }: { playerName: string; onResult: (ok: boolean, t: string) => void }) {
  const [type, setType] = useState(1); const [msg, setMsg] = useState(''); const [busy, setBusy] = useState(false);
  const send = async () => {
    if (!msg.trim()) return;
    setBusy(true);
    const r = await dispatchNotification({ playerName, type, message: msg });
    onResult(!r.isError, r.text);
    if (!r.isError) setMsg('');
    setBusy(false);
  };
  return (
    <div className="glass rounded-xl p-4">
      <h3 className="font-display text-xs uppercase tracking-widest text-turd-mustard">Notify</h3>
      <div className="mt-2 flex gap-2">
        <select value={type} onChange={(e) => setType(Number(e.target.value))} className="rounded border border-turd-bronze/60 bg-turd-bg-soft px-2 py-1 font-mono text-xs text-turd-cream focus:border-turd-mustard focus:outline-none">
          {[1, 2, 3, 4, 5].map((t) => <option key={t} value={t}>Type {t}</option>)}
        </select>
        <input value={msg} onChange={(e) => setMsg(e.target.value)} onKeyDown={(e) => { if (e.key === 'Enter') { e.preventDefault(); void send(); } }} placeholder="Notification text" className="flex-1 rounded border border-turd-bronze/60 bg-turd-bg-soft px-2 py-1 font-mono text-xs text-turd-cream placeholder:text-turd-cream-dim/60 focus:border-turd-mustard focus:outline-none" />
      </div>
      <button type="button" onClick={() => void send()} disabled={!msg.trim() || busy} className="mt-3 w-full rounded border border-turd-mustard/60 bg-turd-mustard/20 px-3 py-1.5 font-display text-[11px] uppercase tracking-wider text-turd-mustard-bright hover:bg-turd-mustard/30 disabled:opacity-30">{busy ? 'Sending…' : 'Send Toast'}</button>
    </div>
  );
}
