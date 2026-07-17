import { z } from "zod";
import { network, persistence, getRuntime, type Disposable } from "@turdmod/turdmod-api";

const TpConfigSchema = z.object({
  tpCooldownSec: z.number().default(300),
  homeCooldownSec: z.number().default(1800),
  tpCost: z.number().default(500),
  enabled: z.boolean().default(true),
});
type TpConfig = z.infer<typeof TpConfigSchema>;

interface TpLocation { x: number; y: number; z: number; createdBy: string; public: boolean }
interface PlayerTpState { lastTp: number; lastHome: number; homePos: { x: number; y: number; z: number } | null }
interface ChatPayload { ts: string; channel: string; player: string; steam?: string; text: string }

const HANDLES: Disposable[] = [];
let cfg: TpConfig;

const log = (msg: string) => getRuntime().log("info", `[teleport] ${msg}`);

async function reply(playerName: string, message: string): Promise<void> {
  const engine = (globalThis as any).__turdmod_engine_client;
  if (!engine) return;
  try { await engine.call("sendChatLineToPlayer", { playerName, channel: "Admin", message: `[TP] ${message}` }); } catch {}
}

async function getLocations(): Promise<Record<string, TpLocation>> {
  return (await persistence.get<Record<string, TpLocation>>("teleport-locations")) ?? {};
}
async function saveLocations(locs: Record<string, TpLocation>): Promise<void> {
  await persistence.set("teleport-locations", locs);
}
async function getState(steam: string): Promise<PlayerTpState> {
  return (await persistence.getForPlayer<PlayerTpState>(steam, "tp-state")) ?? { lastTp: 0, lastHome: 0, homePos: null };
}
async function saveState(steam: string, state: PlayerTpState): Promise<void> {
  await persistence.setForPlayer(steam, "tp-state", state);
}
async function isAdmin(steam: string | undefined): Promise<boolean> {
  if (!steam) return false;
  const ids = await persistence.get<string[]>("admin_steam_ids");
  return Array.isArray(ids) && ids.includes(steam);
}

async function findPlayerPos(name: string): Promise<{ x: number; y: number; z: number } | null> {
  const engine = (globalThis as any).__turdmod_engine_client;
  if (!engine) return null;
  try {
    const res = await engine.call("getOnlinePlayers", {});
    const p = (res.players ?? []).find((p: any) => p.name.toLowerCase() === name.toLowerCase());
    return p?.pos ?? null;
  } catch { return null; }
}

async function teleport(playerName: string, pos: { x: number; y: number; z: number }): Promise<boolean> {
  const engine = (globalThis as any).__turdmod_engine_client;
  if (!engine) return false;
  try { await engine.call("teleportPlayer", { playerName, pos }); return true; } catch { return false; }
}

function cooldownLeft(lastUsed: number, cooldownSec: number): number {
  return Math.max(0, Math.ceil(cooldownSec - (Date.now() - lastUsed) / 1000));
}

async function cmdTpList(player: string): Promise<void> {
  const locs = await getLocations();
  const pub = Object.entries(locs).filter(([, v]) => v.public);
  if (pub.length === 0) { await reply(player, "No teleport locations set."); return; }
  await reply(player, `Locations: ${pub.map(([n]) => n).join(", ")}`);
}

async function cmdTp(player: string, steam: string, locName: string): Promise<void> {
  const locs = await getLocations();
  const loc = locs[locName.toLowerCase()];
  if (!loc || !loc.public) { await reply(player, `"${locName}" not found. Use !tp list.`); return; }
  const state = await getState(steam);
  const cd = cooldownLeft(state.lastTp, cfg.tpCooldownSec);
  if (cd > 0) { await reply(player, `Cooldown: ${cd}s remaining.`); return; }
  const ok = await teleport(player, loc);
  if (ok) { state.lastTp = Date.now(); await saveState(steam, state); await reply(player, `Teleported to ${locName}. (${cfg.tpCost} coins)`); }
  else { await reply(player, "Teleport failed."); }
}

async function cmdSetHome(player: string, steam: string): Promise<void> {
  const pos = await findPlayerPos(player);
  if (!pos) { await reply(player, "Could not find your position."); return; }
  const state = await getState(steam);
  state.homePos = pos;
  await saveState(steam, state);
  await reply(player, `Home set at (${Math.round(pos.x)}, ${Math.round(pos.y)}, ${Math.round(pos.z)}).`);
}

async function cmdHome(player: string, steam: string): Promise<void> {
  const state = await getState(steam);
  if (!state.homePos) { await reply(player, "No home set. Use !sethome first."); return; }
  const cd = cooldownLeft(state.lastHome, cfg.homeCooldownSec);
  if (cd > 0) { await reply(player, `Home cooldown: ${cd}s remaining.`); return; }
  const ok = await teleport(player, state.homePos);
  if (ok) { state.lastHome = Date.now(); await saveState(steam, state); await reply(player, "Teleported home."); }
  else { await reply(player, "Teleport failed."); }
}

async function cmdSetSpawnPoint(player: string, name: string): Promise<void> {
  const pos = await findPlayerPos(player);
  if (!pos) { await reply(player, "Could not find your position."); return; }
  const locs = await getLocations();
  locs[name.toLowerCase()] = { ...pos, createdBy: player, public: true };
  await saveLocations(locs);
  log(`Admin ${player} created spawn point "${name}"`);
  await reply(player, `Spawn point "${name}" created.`);
}

async function cmdRemoveSpawnPoint(player: string, name: string): Promise<void> {
  const locs = await getLocations();
  if (!locs[name.toLowerCase()]) { await reply(player, `"${name}" not found.`); return; }
  delete locs[name.toLowerCase()];
  await saveLocations(locs);
  log(`Admin ${player} removed "${name}"`);
  await reply(player, `"${name}" removed.`);
}

async function cmdListSpawnPoints(player: string): Promise<void> {
  const locs = await getLocations();
  const entries = Object.entries(locs);
  if (entries.length === 0) { await reply(player, "No spawn points."); return; }
  for (const [name, loc] of entries) {
    await reply(player, `${name}: (${Math.round(loc.x)}, ${Math.round(loc.y)}, ${Math.round(loc.z)}) [${loc.public ? "public" : "private"}]`);
  }
}

async function cmdTpPlayer(admin: string, target: string, locName: string): Promise<void> {
  const locs = await getLocations();
  const loc = locs[locName.toLowerCase()];
  if (!loc) { await reply(admin, `"${locName}" not found.`); return; }
  const ok = await teleport(target, loc);
  if (ok) { log(`Admin ${admin} tp'd ${target} to ${locName}`); await reply(admin, `Teleported ${target} to ${locName}.`); }
  else { await reply(admin, `Failed to teleport ${target}.`); }
}

async function cmdTpAll(admin: string, locName: string): Promise<void> {
  const locs = await getLocations();
  const loc = locs[locName.toLowerCase()];
  if (!loc) { await reply(admin, `"${locName}" not found.`); return; }
  const engine = (globalThis as any).__turdmod_engine_client;
  if (!engine) { await reply(admin, "Engine unavailable."); return; }
  try {
    const res = await engine.call("getOnlinePlayers", {});
    let count = 0;
    for (const p of res.players ?? []) { if (await teleport(p.name, loc)) count++; }
    log(`Admin ${admin} tp'd ${count} players to ${locName}`);
    await reply(admin, `Teleported ${count} players to ${locName}.`);
  } catch { await reply(admin, "Failed."); }
}

async function onChat(msg: { payload: ChatPayload }): Promise<void> {
  const { player, steam, text, channel } = msg.payload;
  if (channel !== "Local" || !text.startsWith("!")) return;
  const clean = player.replace(/\(\d+\)$/, "");
  const parts = text.trim().split(/\s+/);
  const cmd = parts[0].toLowerCase();

  if (cmd.startsWith("!!")) {
    if (!(await isAdmin(steam))) { await reply(clean, "Admin only."); return; }
    const sub = cmd.slice(2);
    if (sub === "setspawnpoint" && parts[1]) return cmdSetSpawnPoint(clean, parts[1]);
    if (sub === "removespawnpoint" && parts[1]) return cmdRemoveSpawnPoint(clean, parts[1]);
    if (sub === "listspawnpoints") return cmdListSpawnPoints(clean);
    if (sub === "tpplayer" && parts[1] && parts[2]) return cmdTpPlayer(clean, parts[1], parts[2]);
    if (sub === "tpall" && parts[1]) return cmdTpAll(clean, parts[1]);
    return;
  }

  if (!steam) return;
  if (cmd === "!tp") {
    const arg = parts.slice(1).join(" ");
    if (!arg || arg.toLowerCase() === "list") return cmdTpList(clean);
    return cmdTp(clean, steam, arg);
  }
  if (cmd === "!sethome") return cmdSetHome(clean, steam);
  if (cmd === "!home") return cmdHome(clean, steam);
}

export async function on_load(): Promise<void> {
  const raw = await persistence.get<TpConfig>("teleport-config");
  cfg = raw ? TpConfigSchema.parse(raw) : TpConfigSchema.parse({});
  if (!raw) await persistence.set("teleport-config", cfg);
  if (!(await persistence.get("teleport-locations"))) await persistence.set("teleport-locations", {});
  HANDLES.push(network.on<ChatPayload>("system.chat", onChat));
  log(`loaded — cooldowns: tp=${cfg.tpCooldownSec}s home=${cfg.homeCooldownSec}s cost=${cfg.tpCost}`);
}

export function on_unload(): void {
  for (const h of HANDLES) h.dispose();
  HANDLES.length = 0;
  log("unloaded");
}
