import { z } from "zod";
import { network, persistence, getRuntime, type Disposable } from "@turdmod/turdmod-api";

const ConfigSchema = z.object({
  logAdminCommands: z.boolean().default(true),
  logChat: z.boolean().default(false),
  logChatChannels: z.array(z.string()).default(["Global", "Admin"]),
  logLogins: z.boolean().default(true),
  logLogouts: z.boolean().default(true),
  dangerousKeywords: z.array(z.string()).default(["kick", "ban", "teleport", "spawn", "give", "god", "immortal", "kill", "shutdown"]),
  adminColor: z.number().default(0xf59e0b),
  dangerColor: z.number().default(0xef4444),
  loginColor: z.number().default(0x22c55e),
  logoutColor: z.number().default(0x6b7280),
  chatColor: z.number().default(0x3b82f6),
  historyLimit: z.number().min(10).max(5000).default(500),
});

type AdminLogConfig = z.infer<typeof ConfigSchema>;

interface AdminEvent { ts: string; steam: string; player: string; cmd: string }
interface LoginPayload { ip: string; steam: string; player: string; pos: { x: number; y: number; z: number } }
interface LogoutPayload { steam: string; player: string }
interface ChatPayload { ts: string; channel: string; player: string; steam?: string; text: string }

const HANDLES: Disposable[] = [];
let cfg: AdminLogConfig;
let history: AdminEvent[] = [];

const log = (msg: string) => getRuntime().log("info", `[admin-log] ${msg}`);
const warn = (msg: string) => getRuntime().log("warn", `[admin-log] ${msg}`);

function isDangerous(cmd: string): boolean {
  const lower = cmd.toLowerCase();
  return cfg.dangerousKeywords.some((kw) => lower.includes(kw));
}

function broadcastEmbed(title: string, color: number, fields: Array<{ name: string; value: string; inline?: boolean }>, ts?: string) {
  network.broadcast("admin-log", {
    embed: { title, color, fields, timestamp: ts ?? new Date().toISOString(), footer: { text: "TurdMOD Admin Log" } },
  });
}

async function onAdminCommand(msg: { payload: AdminEvent }): Promise<void> {
  const ev = msg.payload;
  const dangerous = isDangerous(ev.cmd);
  const fields = [
    { name: "Player", value: ev.player, inline: true },
    { name: "SteamID", value: ev.steam, inline: true },
    { name: "Command", value: `\`${ev.cmd}\`` },
  ];
  if (dangerous) fields.push({ name: "Flagged", value: "Potentially dangerous command" });

  broadcastEmbed(dangerous ? "Admin Command (Dangerous)" : "Admin Command", dangerous ? cfg.dangerColor : cfg.adminColor, fields, ev.ts);

  history.push(ev);
  if (history.length > cfg.historyLimit) history = history.slice(-cfg.historyLimit);
  await persistence.set("admin-history", history);
}

function onChat(msg: { payload: ChatPayload }): void {
  const ev = msg.payload;
  if (!cfg.logChatChannels.includes(ev.channel)) return;
  const fields = [
    { name: "Player", value: ev.player, inline: true },
    { name: "Channel", value: ev.channel, inline: true },
    ...(ev.steam ? [{ name: "SteamID", value: ev.steam, inline: true }] : []),
    { name: "Message", value: ev.text },
  ];
  broadcastEmbed("Chat", cfg.chatColor, fields, ev.ts);
}

function onLogin(msg: { payload: LoginPayload }): void {
  const ev = msg.payload;
  broadcastEmbed("Player Joined", cfg.loginColor, [
    { name: "Player", value: ev.player, inline: true },
    { name: "SteamID", value: ev.steam, inline: true },
    { name: "Position", value: `${ev.pos.x.toFixed(0)}, ${ev.pos.y.toFixed(0)}, ${ev.pos.z.toFixed(0)}`, inline: true },
  ]);
}

function onLogout(msg: { payload: LogoutPayload }): void {
  const ev = msg.payload;
  broadcastEmbed("Player Left", cfg.logoutColor, [
    { name: "Player", value: ev.player, inline: true },
    { name: "SteamID", value: ev.steam, inline: true },
  ]);
}

async function loadConfig(): Promise<AdminLogConfig> {
  const raw = await persistence.get<unknown>("admin-log-config");
  if (raw) {
    const parsed = ConfigSchema.safeParse(raw);
    if (parsed.success) return parsed.data;
  }
  const defaults = ConfigSchema.parse({});
  await persistence.set("admin-log-config", defaults);
  log("seeded default config");
  return defaults;
}

export async function on_load(): Promise<void> {
  cfg = await loadConfig();
  const saved = await persistence.get<AdminEvent[]>("admin-history");
  if (Array.isArray(saved)) history = saved.slice(-cfg.historyLimit);

  if (cfg.logAdminCommands) { HANDLES.push(network.on<AdminEvent>("system.admin", onAdminCommand)); log("admin command logging enabled"); }
  if (cfg.logChat) { HANDLES.push(network.on<ChatPayload>("system.chat", onChat)); log(`chat logging enabled for: ${cfg.logChatChannels.join(", ")}`); }
  if (cfg.logLogins) { HANDLES.push(network.on<LoginPayload>("system.login", onLogin)); log("login logging enabled"); }
  if (cfg.logLogouts) { HANDLES.push(network.on<LogoutPayload>("system.logout", onLogout)); log("logout logging enabled"); }

  log(`loaded — ${history.length} historical entries`);
}

export async function on_unload(): Promise<void> {
  for (const h of HANDLES) h.dispose();
  HANDLES.length = 0;
  await persistence.set("admin-history", history);
  log("unloaded — history flushed");
}
