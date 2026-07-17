import { z } from "zod";
import { network, persistence, getRuntime, type Disposable } from "@turdmod/turdmod-api";

const ConfigSchema = z.object({
  enabled: z.boolean().default(true),
  autoKick: z.boolean().default(false),
  minDaysSinceLastBan: z.number().int().min(0).default(0),
  alertColor: z.number().int().default(0xef4444),
  cleanColor: z.number().int().default(0x22c55e),
  logCleanJoins: z.boolean().default(false),
});

type VacConfig = z.infer<typeof ConfigSchema>;

interface LoginPayload { ip: string; steam: string; player: string; pos: { x: number; y: number; z: number } }

interface SteamBanRecord {
  SteamId: string;
  CommunityBanned: boolean;
  VACBanned: boolean;
  NumberOfVACBans: number;
  DaysSinceLastBan: number;
  NumberOfGameBans: number;
  EconomyBan: string;
}

interface CachedBanCheck { checked: string; result: SteamBanRecord }

const HANDLES: Disposable[] = [];
const sessionCache = new Map<string, CachedBanCheck>();
let apiKey: string | undefined;
let cfg: VacConfig;
const CACHE_TTL_MS = 24 * 60 * 60 * 1000;

const log = (msg: string) => getRuntime().log("info", `[vac-screening] ${msg}`);
const warn = (msg: string) => getRuntime().log("warn", `[vac-screening] ${msg}`);

async function fetchBans(steamId: string): Promise<SteamBanRecord | null> {
  const url = `https://api.steampowered.com/ISteamUser/GetPlayerBans/v1/?key=${apiKey}&steamids=${steamId}`;
  try {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), 8000);
    const res = await fetch(url, { signal: controller.signal });
    clearTimeout(timer);
    if (!res.ok) {
      if (res.status === 403) warn("Steam API 403 — invalid API key");
      else if (res.status === 429) warn("Steam API rate limited (429)");
      else warn(`Steam API HTTP ${res.status} for ${steamId}`);
      return null;
    }
    const body = (await res.json()) as { players?: SteamBanRecord[] };
    return body.players?.[0] ?? null;
  } catch (err: unknown) {
    const msg = err instanceof Error ? err.message : String(err);
    warn(msg.includes("abort") ? `API timeout for ${steamId}` : `API error: ${msg}`);
    return null;
  }
}

async function getCached(steamId: string): Promise<SteamBanRecord | null> {
  const mem = sessionCache.get(steamId);
  if (mem && Date.now() - new Date(mem.checked).getTime() < CACHE_TTL_MS) return mem.result;
  const stored = await persistence.getForPlayer<CachedBanCheck>(steamId, "ban-check");
  if (stored && Date.now() - new Date(stored.checked).getTime() < CACHE_TTL_MS) {
    sessionCache.set(steamId, stored);
    return stored.result;
  }
  return null;
}

async function setCache(steamId: string, result: SteamBanRecord): Promise<void> {
  const entry: CachedBanCheck = { checked: new Date().toISOString(), result };
  sessionCache.set(steamId, entry);
  await persistence.setForPlayer(steamId, "ban-check", entry);
}

function isFlagged(record: SteamBanRecord): boolean {
  const hasBans = record.VACBanned || record.NumberOfGameBans > 0;
  if (!hasBans) return false;
  if (cfg.minDaysSinceLastBan > 0 && record.DaysSinceLastBan > cfg.minDaysSinceLastBan) return false;
  return true;
}

async function handleLogin(payload: LoginPayload): Promise<void> {
  if (!cfg.enabled) return;
  const { steam, player } = payload;

  const cached = await getCached(steam);
  let record: SteamBanRecord;
  if (cached) {
    record = cached;
  } else {
    const fetched = await fetchBans(steam);
    if (!fetched) return;
    record = fetched;
    await setCache(steam, record);
  }

  if (isFlagged(record)) {
    warn(`FLAGGED: ${player} (${steam}) — VAC=${record.NumberOfVACBans} Game=${record.NumberOfGameBans} Days=${record.DaysSinceLastBan}`);
    network.broadcast("vac-screening.flagged", {
      embed: {
        title: "VAC/Game Ban Detected",
        description: `**${player}** joined with active bans`,
        color: cfg.alertColor,
        fields: [
          { name: "Player", value: player, inline: true },
          { name: "SteamID", value: steam, inline: true },
          { name: "VAC Bans", value: String(record.NumberOfVACBans), inline: true },
          { name: "Game Bans", value: String(record.NumberOfGameBans), inline: true },
          { name: "Days Since Last Ban", value: String(record.DaysSinceLastBan), inline: true },
          { name: "Community Banned", value: record.CommunityBanned ? "Yes" : "No", inline: true },
        ],
        timestamp: new Date().toISOString(),
      },
    });
    if (cfg.autoKick) {
      network.broadcast("admin.kick", { steam, reason: "VAC/Game ban detected" });
      log(`Auto-kicked ${player} (${steam})`);
    }
  } else if (cfg.logCleanJoins) {
    log(`CLEAN: ${player} (${steam})`);
  }
}

async function loadConfig(): Promise<VacConfig> {
  const raw = await persistence.get<unknown>("vac_config");
  if (raw) {
    const parsed = ConfigSchema.safeParse(raw);
    if (parsed.success) return parsed.data;
  }
  const defaults = ConfigSchema.parse({});
  await persistence.set("vac_config", defaults);
  log("seeded default config");
  return defaults;
}

export async function on_load(): Promise<void> {
  apiKey = process.env.STEAM_WEB_API_KEY;
  if (!apiKey) {
    warn("STEAM_WEB_API_KEY not set — screening disabled. Set the env var and restart.");
    return;
  }
  cfg = await loadConfig();
  HANDLES.push(network.on<LoginPayload>("system.login", (msg) => {
    handleLogin(msg.payload).catch((err) => warn(`login handler error: ${err}`));
  }));
  log(`loaded — enabled=${cfg.enabled} autoKick=${cfg.autoKick} minDays=${cfg.minDaysSinceLastBan}`);
}

export function on_unload(): void {
  for (const h of HANDLES) h.dispose();
  HANDLES.length = 0;
  sessionCache.clear();
  log("unloaded");
}
