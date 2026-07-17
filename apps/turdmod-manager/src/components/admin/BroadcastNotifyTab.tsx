import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { dispatchBroadcast, dispatchNotification, fetchRoster } from '../../lib/admin-dispatch';
import { WelcomeModSection } from '../../pages/AdminPage';
import type { WelcomeModConfig, WelcomeModStatus } from '../../hooks/useWelcomeMod';

// Broadcast & Notify tab — server-wide chat broadcast + per-player toast.
// Single notification path (sendNotification handler); the old
// #SendNotification admin-verb route with the leading-'#' bug is retired.

const ROSTER_MS = 2000;
const CHAT_TYPES = [
  { value: '0', label: 'Local (0)' },
  { value: '1', label: 'Squad (1)' },
  { value: '2', label: 'Global (2)' },
  { value: '3', label: 'Admin (3)' },
];

export interface BroadcastNotifyTabProps {
  welcomeMod?: {
    config: WelcomeModConfig;
    setConfig: (update: Partial<WelcomeModConfig>) => void;
    status: WelcomeModStatus;
  };
}

export function BroadcastNotifyTab({ welcomeMod }: BroadcastNotifyTabProps) {
  return (
    <div className="flex h-full min-h-0 flex-col gap-3 overflow-auto">
      <BroadcastSection />
      <NotifySection />
      {welcomeMod && (
        <WelcomeModSection config={welcomeMod.config} setConfig={welcomeMod.setConfig} status={welcomeMod.status} />
      )}
    </div>
  );
}

function BroadcastSection() {
  const [text, setText] = useState(''); const [chatType, setChatType] = useState('2');
  const [fb, setFb] = useState<{ ok: boolean; text: string } | null>(null); const [busy, setBusy] = useState(false);
  const send = async () => {
    if (!text.trim()) return;
    setBusy(true);
    const r = await dispatchBroadcast({ text, chatType });
    setFb({ ok: !r.isError, text: r.text }); setTimeout(() => setFb(null), 4000);
    if (!r.isError) setText('');
    setBusy(false);
  };
  return (
    <section className="glass rounded-xl p-4">
      <h2 className="font-display text-sm uppercase tracking-widest text-turd-mustard">Broadcast</h2>
      <p className="mt-0.5 text-[11px] text-turd-cream-dim">Server-side chat line to every player.</p>
      <div className="mt-3 flex gap-2">
        <input value={text} onChange={(e) => setText(e.target.value)} onKeyDown={(e) => { if (e.key === 'Enter') { e.preventDefault(); void send(); } }} placeholder="Message to broadcast…" className="flex-1 rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-1.5 font-mono text-xs text-turd-cream placeholder:text-turd-cream-dim/60 focus:border-turd-mustard focus:outline-none" />
        <select value={chatType} onChange={(e) => setChatType(e.target.value)} className="rounded border border-turd-bronze/60 bg-turd-bg-soft px-2 py-1.5 font-mono text-xs text-turd-cream focus:border-turd-mustard focus:outline-none">
          {CHAT_TYPES.map((c) => <option key={c.value} value={c.value}>{c.label}</option>)}
        </select>
        <button type="button" onClick={() => void send()} disabled={!text.trim() || busy} className="rounded border border-turd-mustard/60 bg-turd-mustard/20 px-4 py-1.5 font-display text-xs uppercase tracking-wider text-turd-mustard-bright hover:bg-turd-mustard/30 disabled:opacity-30">{busy ? 'Sending…' : 'Send'}</button>
      </div>
      {fb && <p className={`mt-2 font-mono text-[11px] ${fb.ok ? 'text-turd-green' : 'text-turd-red'}`}>{fb.text}</p>}
    </section>
  );
}

function NotifySection() {
  const roster = useQuery({ queryKey: ['engine', 'getOnlinePlayers'], queryFn: fetchRoster, refetchInterval: ROSTER_MS, retry: false });
  const players = roster.data?.players ?? [];
  const [playerName, setPlayerName] = useState(''); const [type, setType] = useState(1); const [msg, setMsg] = useState('');
  const [fb, setFb] = useState<{ ok: boolean; text: string } | null>(null); const [busy, setBusy] = useState(false);
  const send = async () => {
    if (!playerName || !msg.trim()) return;
    setBusy(true);
    const r = await dispatchNotification({ playerName, type, message: msg });
    setFb({ ok: !r.isError, text: r.text }); setTimeout(() => setFb(null), 4000);
    if (!r.isError) setMsg('');
    setBusy(false);
  };
  return (
    <section className="glass rounded-xl p-4">
      <h2 className="font-display text-sm uppercase tracking-widest text-turd-mustard">Notification Toast</h2>
      <p className="mt-0.5 text-[11px] text-turd-cream-dim">Per-player UI_NotificationWidget toast (5 styles). Fires via ProcessEvent — no admin auth required.</p>
      <div className="mt-3 flex gap-2">
        <select value={playerName} onChange={(e) => setPlayerName(e.target.value)} disabled={players.length === 0} className="rounded border border-turd-bronze/60 bg-turd-bg-soft px-2 py-1.5 font-mono text-xs text-turd-cream focus:border-turd-mustard focus:outline-none disabled:opacity-40">
          <option value="">{players.length === 0 ? '(no players online)' : '— select player —'}</option>
          {players.map((p) => <option key={p.ptr} value={p.name}>{p.name || '(no name)'}</option>)}
        </select>
        <select value={type} onChange={(e) => setType(Number(e.target.value))} className="rounded border border-turd-bronze/60 bg-turd-bg-soft px-2 py-1.5 font-mono text-xs text-turd-cream focus:border-turd-mustard focus:outline-none">
          {[1, 2, 3, 4, 5].map((t) => <option key={t} value={t}>Type {t}</option>)}
        </select>
        <input value={msg} onChange={(e) => setMsg(e.target.value)} onKeyDown={(e) => { if (e.key === 'Enter') { e.preventDefault(); void send(); } }} placeholder="Notification text" className="flex-1 rounded border border-turd-bronze/60 bg-turd-bg-soft px-3 py-1.5 font-mono text-xs text-turd-cream placeholder:text-turd-cream-dim/60 focus:border-turd-mustard focus:outline-none" />
        <button type="button" onClick={() => void send()} disabled={!playerName || !msg.trim() || busy} className="rounded border border-turd-mustard/60 bg-turd-mustard/20 px-4 py-1.5 font-display text-xs uppercase tracking-wider text-turd-mustard-bright hover:bg-turd-mustard/30 disabled:opacity-30">{busy ? 'Sending…' : 'Send'}</button>
      </div>
      {fb && <p className={`mt-2 font-mono text-[11px] ${fb.ok ? 'text-turd-green' : 'text-turd-red'}`}>{fb.text}</p>}
    </section>
  );
}
