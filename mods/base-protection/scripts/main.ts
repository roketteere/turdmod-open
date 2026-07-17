import { z } from "zod";
import { network, persistence, getRuntime, type Disposable } from "@turdmod/turdmod-api";

const ConfigSchema = z.object({
  offlineHoursBeforeUnlock: z.number().default(48),
  checkIntervalMinutes: z.number().default(5),
  warnBeforeUnlockHours: z.number().default(6),
  enabled: z.boolean().default(true),
  exemptSteamIds: z.array(z.string()).default([]),
});
type Config = z.infer<typeof ConfigSchema>;

interface LoginPayload { ip: string; steam: string; player: string; pos: { x: number; y: number; z: number } }
interface LogoutPayload { steam: string; player: string }
interface ChatPayload { ts: string; channel: string; player: string; steam?: string; text: string }

interface PlayerActivity {
  steamId: string;
  playerName: string;
  lastSeen: string; // ISO timestamp
  isOnline: boolean;
  baseProtected: boolean;
}

const HANDLES: Disposable[] = [];
let cfg: Config;
let checkTimer: ReturnType<typeof setInterval> | null = null;
const onlinePlayers = new Map<string, { name: string; loginTime: number }>();

const log = (msg: string) => getRuntime().log("info", `[base-protection] ${msg}`);
const warn = (msg: string) => getRuntime().log("warn", `[base-protection] ${msg}`);

async function reply(player: string, message: string): Promise<void> {
  const engine = (globalThis as any).__turdmod_engine_client;
  if (!engine) return;
  try { await engine.call("sendChatLineToPlayer", { playerName: player, channel: "Admin", message: `[Zilla] ${message}` }); } catch {}
}

async function getActivity(steam: string): Promise<PlayerActivity | null> {
  return persistence.getForPlayer<PlayerActivity>(steam, "activity");
}

async function saveActivity(steam: string, activity: PlayerActivity): Promise<void> {
  await persistence.setForPlayer(steam, "activity", activity);
}

function onLogin(msg: { payload: LoginPayload }): void {
  const { steam, player } = msg.payload;
  const cleanName = player.replace(/\(\d+\)$/, "");
  onlinePlayers.set(steam, { name: cleanName, loginTime: Date.now() });

  const now = new Date().toISOString();
  saveActivity(steam, {
    steamId: steam,
    playerName: cleanName,
    lastSeen: now,
    isOnline: true,
    baseProtected: true,
  }).catch(err => warn(`login activity save failed: ${err}`));

  log(`${cleanName} online — base protection active`);
}

function onLogout(msg: { payload: LogoutPayload }): void {
  const { steam, player } = msg.payload;
  const cleanName = player.replace(/\(\d+\)$/, "");
  onlinePlayers.delete(steam);

  const now = new Date().toISOString();
  saveActivity(steam, {
    steamId: steam,
    playerName: cleanName,
    lastSeen: now,
    isOnline: false,
    baseProtected: true,
  }).catch(err => warn(`logout activity save failed: ${err}`));

  log(`${cleanName} offline — protection timer starts (${cfg.offlineHoursBeforeUnlock}h until unlock)`);
}

async function protectionCheck(): Promise<void> {
  if (!cfg.enabled) return;

  // Check all known players' last-seen times
  // Note: without a full player registry, we can only check players we've
  // seen this session. A production version would scan all persistence entries.
  const now = Date.now();

  for (const [steam, entry] of onlinePlayers) {
    // Online players are always protected
    const activity = await getActivity(steam);
    if (activity && !activity.baseProtected) {
      activity.baseProtected = true;
      await saveActivity(steam, activity);
    }
  }

  // For this V1, we broadcast warnings but can't actually toggle locks
  // (needs bridge handler for flag manipulation — RE work pending).
  // The activity tracking is the foundation; the lock toggle wires in later.
}

async function cmdProtection(player: string, steam: string | undefined): Promise<void> {
  if (!steam) { await reply(player, "Cannot identify your SteamID."); return; }
  const activity = await getActivity(steam);
  if (!activity) {
    await reply(player, "No activity data yet. Play for a while first.");
    return;
  }

  const lastSeen = new Date(activity.lastSeen);
  const hoursAgo = Math.round((Date.now() - lastSeen.getTime()) / 3600000);
  const hoursLeft = Math.max(0, cfg.offlineHoursBeforeUnlock - hoursAgo);

  await reply(player, `Base protection: ${activity.baseProtected ? "ACTIVE" : "EXPIRED"} | Last seen: ${hoursAgo}h ago | Unlocks in: ${hoursLeft}h of offline time`);
}

async function onChat(msg: { payload: ChatPayload }): Promise<void> {
  const { player, steam, text, channel } = msg.payload;
  if (channel !== "Local" || !text.startsWith("!")) return;
  const clean = player.replace(/\(\d+\)$/, "");
  const cmd = text.trim().toLowerCase();

  if (cmd === "!protection" || cmd === "!base") {
    return cmdProtection(clean, steam);
  }
}

export async function on_load(): Promise<void> {
  const raw = await persistence.get<Config>("base-protection-config");
  cfg = raw ? ConfigSchema.parse(raw) : ConfigSchema.parse({});
  if (!raw) { await persistence.set("base-protection-config", cfg); log("seeded default config"); }

  HANDLES.push(network.on<LoginPayload>("system.login", onLogin));
  HANDLES.push(network.on<LogoutPayload>("system.logout", onLogout));
  HANDLES.push(network.on<ChatPayload>("system.chat", onChat));

  checkTimer = setInterval(() => { protectionCheck().catch(err => warn(`check error: ${err}`)); }, cfg.checkIntervalMinutes * 60_000);

  log(`loaded — unlock after ${cfg.offlineHoursBeforeUnlock}h offline, check every ${cfg.checkIntervalMinutes}m`);
}

export function on_unload(): void {
  for (const h of HANDLES) h.dispose();
  HANDLES.length = 0;
  if (checkTimer) { clearInterval(checkTimer); checkTimer = null; }
  log("unloaded");
}
