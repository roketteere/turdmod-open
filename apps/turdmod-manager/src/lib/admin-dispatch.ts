import { engineRpc } from './tauri-engine';

// Single source of truth for firing admin actions from the Manager.
// Replaces the THREE divergent command paths that used to live in
// TurdComTerminalPage (bare runAdminCommand), ChatConsolePage
// (runTestAdminCommand) and AdminCommandsPage (runAdminCommand + bypass) —
// plus the two notification senders (clean sendNotification vs. the buggy
// #SendNotification admin-verb path in ServerPanelsPage).
//
// @inv: ProcessAdminCommand wants the BARE verb — a leading '#' yields
//       "Unrecognized command." (live-confirmed 2026-06-10). Always strip.
// @inv: Force (zero-admin) needs gameThread/bypass as STRING "1" — the
//       bridge's extract_json_str ignores JSON numbers (confirmed 2026-06-18).
// @inv: Force needs >=1 online player as the executor channel.
// @dep: bridge handlers runAdminCommand / broadcastChat / sendNotification.

export type InputMode = 'admin' | 'mod' | 'broadcast';

export interface ClassifiedInput {
  mode: InputMode;   // '#' -> admin verb, '!' -> mod command, else broadcast
  verb: string;      // first token (admin/mod); '' for broadcast
  args: string[];    // tokens after the verb (admin/mod)
  text: string;      // full payload sans sigil (broadcast uses this verbatim)
}

// Classify a raw input line by its leading sigil. The '#'/'!' sigils are
// the same convention the in-game chat parser uses, so muscle memory carries.
export function classifyInput(raw: string): ClassifiedInput {
  const trimmed = raw.trim();
  if (trimmed.startsWith('#')) {
    const body = trimmed.replace(/^#+\s*/, '');
    const [verb = '', ...args] = body.split(/\s+/);
    return { mode: 'admin', verb, args, text: body };
  }
  if (trimmed.startsWith('!')) {
    const body = trimmed.replace(/^!+\s*/, '');
    const [verb = '', ...args] = body.split(/\s+/);
    return { mode: 'mod', verb, args, text: body };
  }
  return { mode: 'broadcast', verb: '', args: [], text: trimmed };
}

export interface AdminResult {
  ok: boolean;
  text: string;          // human-readable outcome line for history
  isError: boolean;
  gatePatched?: boolean; // zero-admin gate flip succeeded (Force only)
}

interface AdminRpcResponse {
  ok?: boolean;
  error?: string;
  command?: string;
  gatePatched?: boolean;
  seh?: string;
}

// Fire a SCUM admin verb through the real admin parser. When `force` is set,
// flips the CanExecute auth gate so it runs with ZERO admins online, using
// `executor` (an online player name) as the dispatch channel.
export async function dispatchAdmin(opts: {
  command: string;            // verb + args, with or without leading '#'
  force?: boolean;
  executor?: string | null;   // required when force is true
}): Promise<AdminResult> {
  const bare = opts.command.replace(/^#+\s*/, '').trim();
  if (!bare) return { ok: false, isError: true, text: 'empty command' };

  const params: Record<string, string> = { command: bare };
  if (opts.force) {
    if (!opts.executor) {
      return {
        ok: false,
        isError: true,
        text: 'Force needs ≥1 player online (an executor channel). Have someone join, or turn off Force.',
      };
    }
    params.playerName = opts.executor;
    params.gameThread = '1';
    params.bypass = '1';
  }

  try {
    const resp = await engineRpc<AdminRpcResponse>('runAdminCommand', params);
    if (resp?.error) return { ok: false, isError: true, text: resp.error };
    if (opts.force) {
      if (resp?.gatePatched) {
        return { ok: true, isError: false, gatePatched: true, text: `dispatched (forced, as ${opts.executor})` };
      }
      return {
        ok: true,
        isError: false,
        gatePatched: false,
        text: `dispatched, but auth gate NOT patched (seh ${resp?.seh ?? '?'}) — only ran if the executor is already admin; gate signature may need re-deriving.`,
      };
    }
    return { ok: true, isError: false, text: resp?.command ? `dispatched: ${resp.command}` : 'dispatched' };
  } catch (err) {
    return { ok: false, isError: true, text: String((err as Error)?.message ?? err) };
  }
}

interface BroadcastResponse { ok?: boolean; sent?: string; error?: string }

// Server-side chat broadcast. chatType: '0' local, '1' squad, '2' global, '3' admin.
export async function dispatchBroadcast(opts: { text: string; chatType: string }): Promise<AdminResult> {
  const text = opts.text.trim();
  if (!text) return { ok: false, isError: true, text: 'empty message' };
  try {
    const resp = await engineRpc<BroadcastResponse>('broadcastChat', { text, chatType: opts.chatType });
    if (resp?.error) return { ok: false, isError: true, text: resp.error };
    return { ok: true, isError: false, text: `broadcast: ${resp?.sent ?? text}` };
  } catch (err) {
    return { ok: false, isError: true, text: String((err as Error)?.message ?? err) };
  }
}

interface NotificationResponse {
  ok?: boolean;
  playerName?: string;
  type?: number;
  calledFunction?: string;
  error?: string;
}

// Per-player UI_NotificationWidget toast (5 fixed styles). Fires via
// ProcessEvent — no admin auth required, no '#' parser involved. This is the
// ONLY notification path now; the #SendNotification admin-verb route (which
// still carried the leading-'#' bug in ServerPanelsPage) is retired.
export async function dispatchNotification(opts: {
  playerName: string;
  type: number;
  message: string;
}): Promise<AdminResult> {
  const playerName = opts.playerName.trim();
  const message = opts.message.trim();
  if (!playerName || !message) return { ok: false, isError: true, text: 'player and message required' };
  try {
    const resp = await engineRpc<NotificationResponse>('sendNotification', { playerName, type: opts.type, message });
    if (resp?.ok) {
      return { ok: true, isError: false, text: `notified ${resp.playerName} via ${resp.calledFunction ?? 'toast'}` };
    }
    return { ok: false, isError: true, text: resp?.error ?? 'notification failed' };
  } catch (err) {
    return { ok: false, isError: true, text: String((err as Error)?.message ?? err) };
  }
}

// Mod (!) command. True manager-side execution of TurdMOD chat verbs still
// needs the /chat/simulate inject (not yet built); for now we echo the line
// as an admin-channel broadcast so it is at least visible in-game.
// @brk: replace with the chat-simulate inject RPC once it lands.
export async function dispatchMod(opts: { command: string }): Promise<AdminResult> {
  const bare = opts.command.replace(/^!+\s*/, '').trim();
  if (!bare) return { ok: false, isError: true, text: 'empty mod command' };
  const r = await dispatchBroadcast({ text: `!${bare}`, chatType: '3' });
  return r.ok ? { ...r, text: `mod cmd echoed (true exec pending chat-simulate inject): !${bare}` } : r;
}

export interface RosterPlayer { name: string; controller: string; class: string; ptr: string }
interface RosterResponse { count: number; players: RosterPlayer[] }

export async function fetchRoster(): Promise<RosterResponse> {
  return engineRpc<RosterResponse>('getOnlinePlayers', {});
}

// ── Command-catalog sync ────────────────────────────────────────────────
// runAdminCommand arms the bridge's chat-capture buffer; getAdminOutput
// drains it. So firing the in-game "list commands" verb and then reading
// the output back gives us the AUTHORITATIVE verb list straight from SCUM —
// the complete set the static catalog + BP dump both miss (e.g. SetWeather).
// Re-runnable after every SCUM update to stay current.

export async function getAdminOutput(clear = true): Promise<string[]> {
  try {
    const r = await engineRpc<{ ok?: boolean; lines?: string[] }>('getAdminOutput', { clear });
    return r?.lines ?? [];
  } catch {
    return [];
  }
}

// Pull command-name tokens out of captured output. Verbs are PascalCase and
// may be '#'-prefixed; prose words (lowercase) and short tokens are dropped.
export function parseVerbs(lines: string[]): string[] {
  const out = new Set<string>();
  for (const line of lines) {
    for (const tok of line.match(/#?[A-Za-z][A-Za-z0-9]{2,}/g) ?? []) {
      const v = tok.replace(/^#/, '');
      if (/^[A-Z]/.test(v)) out.add(v);
    }
  }
  return Array.from(out).sort();
}

// Refresh the authoritative command catalog from the live game. SCUM exposes
// no chat "list commands" verb (Help/Commands/? all return "Unrecognized"),
// so the real source is the bridge's dumpAdminCommands — a walk of every
// AdminCommand_* Blueprint CDO (~231 verbs). Re-run after a SCUM update to
// pick up BP-set changes. Native non-BP commands (e.g. SetWeather) never
// appear here — the learn-on-use path covers those.
// @inv: returns the FULL set or an empty array on failure; never partial noise.
export async function dumpCommandCatalog(): Promise<{ used: string; verbs: string[] }> {
  try {
    const r = await engineRpc<{ commands?: { verb: string }[] }>('dumpAdminCommands', {});
    const verbs = (r?.commands ?? [])
      .map((c) => c.verb)
      .filter((v): v is string => !!v)
      .sort();
    return { used: 'dumpAdminCommands', verbs };
  } catch {
    return { used: 'dumpAdminCommands', verbs: [] };
  }
}
