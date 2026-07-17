import { z } from "zod";
import { network, persistence, getRuntime, type Disposable } from "@turdmod/turdmod-api";

const Vec3Schema = z.object({ x: z.number(), y: z.number(), z: z.number() });

const ScheduleDefSchema = z.object({
  kind: z.enum(["interval", "daily", "cron_like"]),
  intervalMinutes: z.number().positive().optional(),
  dailyAtHour: z.number().int().min(0).max(23).optional(),
  daysOfWeek: z.array(z.number().int().min(0).max(6)).optional(),
});

const EventDefSchema = z.object({
  id: z.string().min(1),
  name: z.string().min(1),
  type: z.enum(["announcement", "warzone", "loot_drop"]),
  enabled: z.boolean(),
  schedule: ScheduleDefSchema,
  config: z.record(z.unknown()),
});

type Vec3 = z.infer<typeof Vec3Schema>;
type EventDef = z.infer<typeof EventDefSchema>;

interface LoginPayload { ip: string; steam: string; player: string; pos: Vec3 }
interface LogoutPayload { steam: string; player: string }
interface SchedulerState { lastFired: Record<string, number> }
interface ActiveWarzone { eventId: string; zoneName: string; center: Vec3; radiusM: number; coinMultiplier: number; endsAt: number }
interface TrackedPlayer { steam: string; player: string; pos: Vec3 }

const TICK_MS = 60_000;
const HANDLES: Disposable[] = [];
let tickHandle: ReturnType<typeof setInterval> | null = null;
const onlinePlayers = new Map<string, TrackedPlayer>();
const activeWarzones = new Map<string, ActiveWarzone>();

const log = (msg: string) => getRuntime().log("info", `[events] ${msg}`);
const warn = (msg: string) => getRuntime().log("warn", `[events] ${msg}`);

const LOOT_EMOJI: Record<string, string> = { common: "box", rare: "gem", legendary: "trophy" };

const DEFAULT_EVENTS: EventDef[] = [
  {
    id: "hourly-announce", name: "Hourly Server Message", type: "announcement", enabled: true,
    schedule: { kind: "interval", intervalMinutes: 60 },
    config: { message: "Welcome to the server! Type !help for commands.", embedColor: 0x3b82f6 },
  },
  {
    id: "daily-warzone", name: "Daily Warzone", type: "warzone", enabled: false,
    schedule: { kind: "daily", dailyAtHour: 18 },
    config: { zoneName: "Airfield", center: { x: 0, y: 0, z: 0 }, radiusM: 500, coinMultiplier: 3, durationMinutes: 120 },
  },
  {
    id: "loot-airdrop", name: "Loot Airdrop", type: "loot_drop", enabled: false,
    schedule: { kind: "interval", intervalMinutes: 25 },
    config: { locationName: "Random Sector", pos: { x: 0, y: 0, z: 0 }, description: "Supply crate incoming!", lootTier: "rare" },
  },
];

function broadcastEmbed(title: string, description: string, color: number, fields?: Array<{ name: string; value: string; inline?: boolean }>) {
  network.broadcast("events.announce", { embed: { title, description, color, fields: fields ?? [], timestamp: new Date().toISOString() } });
}

function dist2d(a: Vec3, b: Vec3): number {
  return Math.sqrt((a.x - b.x) ** 2 + (a.y - b.y) ** 2);
}

function shouldFire(schedule: z.infer<typeof ScheduleDefSchema>, lastFiredMs: number | undefined, now: Date): boolean {
  const elapsed = now.getTime() - (lastFiredMs ?? 0);
  if (schedule.kind === "interval") return elapsed >= (schedule.intervalMinutes ?? 60) * 60_000;
  if (schedule.kind === "daily") {
    if (now.getUTCHours() !== (schedule.dailyAtHour ?? 0)) return false;
    return elapsed >= 59 * 60_000;
  }
  if (schedule.kind === "cron_like") {
    if (!(schedule.daysOfWeek ?? []).includes(now.getUTCDay())) return false;
    if (now.getUTCHours() !== (schedule.dailyAtHour ?? 0)) return false;
    return elapsed >= 59 * 60_000;
  }
  return false;
}

function fireAnnouncement(event: EventDef): void {
  const c = event.config as { message: string; embedColor: number };
  log(`announcement: ${event.name}`);
  broadcastEmbed("Server Announcement", c.message, c.embedColor);
}

function fireWarzone(event: EventDef): void {
  const c = event.config as { zoneName: string; center: Vec3; radiusM: number; coinMultiplier: number; durationMinutes: number };
  const endsAt = Date.now() + c.durationMinutes * 60_000;
  activeWarzones.set(event.id, { eventId: event.id, zoneName: c.zoneName, center: c.center, radiusM: c.radiusM, coinMultiplier: c.coinMultiplier, endsAt });
  log(`warzone activated: ${c.zoneName} for ${c.durationMinutes}m`);
  broadcastEmbed(`WARZONE ACTIVE - ${c.zoneName}`, `Head to the zone for ${c.coinMultiplier}x coin rewards!`, 0xef4444, [
    { name: "Zone", value: c.zoneName, inline: true },
    { name: "Radius", value: `${c.radiusM}m`, inline: true },
    { name: "Multiplier", value: `${c.coinMultiplier}x`, inline: true },
    { name: "Duration", value: `${c.durationMinutes} min`, inline: true },
  ]);
  network.broadcast("events.warzone-active", { eventId: event.id, zoneName: c.zoneName, center: c.center, radiusM: c.radiusM, coinMultiplier: c.coinMultiplier, endsAt });
}

function endWarzone(eventId: string, zoneName: string): void {
  activeWarzones.delete(eventId);
  log(`warzone ended: ${zoneName}`);
  broadcastEmbed(`Warzone Ended - ${zoneName}`, "Coin multiplier deactivated.", 0x6b7280);
  network.broadcast("events.warzone-ended", { eventId, zoneName });
}

function fireLootDrop(event: EventDef): void {
  const c = event.config as { locationName: string; pos: Vec3; description: string; lootTier: string };
  log(`loot drop: ${c.locationName} (${c.lootTier})`);
  broadcastEmbed(`LOOT DROP - ${c.locationName}`, c.description, 0xf59e0b, [
    { name: "Location", value: c.locationName, inline: true },
    { name: "Coordinates", value: `(${c.pos.x}, ${c.pos.y}, ${c.pos.z})`, inline: true },
    { name: "Tier", value: c.lootTier, inline: true },
  ]);
  network.broadcast("events.loot-drop", { eventId: event.id, locationName: c.locationName, pos: c.pos, lootTier: c.lootTier });
}

function checkWarzoneOccupants(): void {
  for (const [eventId, wz] of activeWarzones) {
    for (const [steam, p] of onlinePlayers) {
      if (dist2d(p.pos, wz.center) <= wz.radiusM) {
        network.broadcast("events.coin-multiplier", { steam, player: p.player, multiplier: wz.coinMultiplier, zoneName: wz.zoneName, eventId });
      }
    }
  }
}

async function tick(): Promise<void> {
  const now = new Date();
  const events = await persistence.get<EventDef[]>("events") ?? [];
  const state = await persistence.get<SchedulerState>("scheduler-state") ?? { lastFired: {} };
  let changed = false;

  for (const event of events) {
    if (!event.enabled) continue;
    if (!shouldFire(event.schedule, state.lastFired[event.id], now)) continue;
    if (event.type === "announcement") fireAnnouncement(event);
    else if (event.type === "warzone") fireWarzone(event);
    else if (event.type === "loot_drop") fireLootDrop(event);
    state.lastFired[event.id] = now.getTime();
    changed = true;
  }

  for (const [eventId, wz] of activeWarzones) {
    if (now.getTime() >= wz.endsAt) endWarzone(eventId, wz.zoneName);
  }
  checkWarzoneOccupants();

  if (changed) await persistence.set("scheduler-state", state);
}

function onLogin(msg: { payload: LoginPayload }): void {
  const { steam, player, pos } = msg.payload;
  onlinePlayers.set(steam, { steam, player, pos });
  for (const [eventId, wz] of activeWarzones) {
    if (dist2d(pos, wz.center) <= wz.radiusM) {
      network.broadcast("events.coin-multiplier", { steam, player, multiplier: wz.coinMultiplier, zoneName: wz.zoneName, eventId });
    }
  }
}

function onLogout(msg: { payload: LogoutPayload }): void {
  onlinePlayers.delete(msg.payload.steam);
}

export async function on_load(): Promise<void> {
  let events = await persistence.get<EventDef[]>("events");
  if (!events || events.length === 0) {
    events = DEFAULT_EVENTS;
    await persistence.set("events", events);
    log("seeded default events");
  }
  log(`loaded ${events.length} event(s), ${events.filter(e => e.enabled).length} enabled`);

  HANDLES.push(network.on<LoginPayload>("system.login", onLogin));
  HANDLES.push(network.on<LogoutPayload>("system.logout", onLogout));

  tickHandle = setInterval(() => { tick().catch((err) => warn(`tick error: ${err}`)); }, TICK_MS);
  tick().catch((err) => warn(`initial tick error: ${err}`));
}

export function on_unload(): void {
  if (tickHandle) { clearInterval(tickHandle); tickHandle = null; }
  for (const [eventId, wz] of activeWarzones) endWarzone(eventId, wz.zoneName);
  for (const h of HANDLES) h.dispose();
  HANDLES.length = 0;
  onlinePlayers.clear();
  log("unloaded");
}
