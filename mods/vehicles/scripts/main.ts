import { z } from "zod";
import { network, persistence, getRuntime, type Disposable } from "@turdmod/turdmod-api";

const ConfigSchema = z.object({
  maxPerPlayer: z.number().int().min(1).max(20).default(2),
  spawnCost: z.number().int().min(0).default(1000),
  insureCost: z.number().int().min(0).default(5000),
  enabled: z.boolean().default(true),
});
type VehicleConfig = z.infer<typeof ConfigSchema>;

const VEHICLE_MAP: Record<string, string> = {
  duster: "BPC_Kinglet_Duster_C", pickup: "BPC_Kinglet_Pickup_C", suv: "BPC_Kinglet_SUV_C",
  police: "BPC_Kinglet_Police_C", ambulance: "BPC_Kinglet_Ambulance_C", atv: "BPC_Kaine_ATV_C",
  rhib: "BPC_RHIB_C", jetski: "BPC_JetSki_C", motorcycle: "BPC_Motorcycle_C",
  golf: "BPC_Golf_Cart_C", tractor: "BPC_Tractor_C", bicycle: "BPC_Bicycle_C",
  firetruck: "BPC_Kinglet_FireTruck_C", military: "BPC_Kinglet_Military_C",
  sailboat: "BPC_Sailboat_C", speedboat: "BPC_Speedboat_C",
};
const AVAILABLE = Object.keys(VEHICLE_MAP).join(", ");

interface RegisteredVehicle {
  id: string; vehicleClass: string; friendlyName: string; nickname: string;
  insured: boolean; spawned: boolean; registeredAt: string;
}
interface ChatPayload { ts: string; channel: string; player: string; steam?: string; text: string }

const HANDLES: Disposable[] = [];
const log = (msg: string) => getRuntime().log("info", `[vehicles] ${msg}`);

function uuid(): string {
  return "xxxx-xxxx-4xxx-yxxx".replace(/[xy]/g, c => {
    const r = (Math.random() * 16) | 0;
    return (c === "x" ? r : (r & 0x3) | 0x8).toString(16);
  });
}

async function reply(player: string, message: string): Promise<void> {
  const engine = (globalThis as any).__turdmod_engine_client;
  if (!engine) return;
  try { await engine.call("sendChatLineToPlayer", { playerName: player, channel: "Admin", message: `[Vehicles] ${message}` }); } catch {}
}

async function getVehicles(steam: string): Promise<RegisteredVehicle[]> {
  return (await persistence.getForPlayer<RegisteredVehicle[]>(steam, "vehicles")) ?? [];
}
async function setVehicles(steam: string, v: RegisteredVehicle[]): Promise<void> {
  await persistence.setForPlayer(steam, "vehicles", v);
}
async function isAdmin(steam: string | undefined): Promise<boolean> {
  if (!steam) return false;
  const ids = await persistence.get<string[]>("admin_steam_ids");
  return Array.isArray(ids) && ids.includes(steam);
}

async function cmdRegister(steam: string, player: string, args: string[], cfg: VehicleConfig): Promise<void> {
  if (args.length < 1) { await reply(player, `Usage: !vregister <type> [name]. Types: ${AVAILABLE}`); return; }
  const cls = VEHICLE_MAP[args[0].toLowerCase()];
  if (!cls) { await reply(player, `Unknown type. Available: ${AVAILABLE}`); return; }
  const vehicles = await getVehicles(steam);
  if (vehicles.length >= cfg.maxPerPlayer) { await reply(player, `Max ${cfg.maxPerPlayer} vehicles reached.`); return; }
  const friendly = args[0].charAt(0).toUpperCase() + args[0].slice(1).toLowerCase();
  const nickname = args.slice(1).join(" ").trim() || friendly;
  vehicles.push({ id: uuid(), vehicleClass: cls, friendlyName: friendly, nickname, insured: false, spawned: false, registeredAt: new Date().toISOString() });
  await setVehicles(steam, vehicles);
  log(`${player} registered ${friendly} as "${nickname}"`);
  await reply(player, `Registered ${friendly} as "${nickname}" (${vehicles.length}/${cfg.maxPerPlayer}).`);
}

async function cmdSpawn(steam: string, player: string, args: string[], cfg: VehicleConfig): Promise<void> {
  const vehicles = await getVehicles(steam);
  if (vehicles.length === 0) { await reply(player, "No vehicles registered. Use !vregister <type>."); return; }
  const spawned = vehicles.find(v => v.spawned);
  if (spawned) { await reply(player, `"${spawned.nickname}" already spawned. !vdespawn first.`); return; }
  const query = args.join(" ").trim().toLowerCase();
  const target = query ? vehicles.find(v => v.nickname.toLowerCase() === query) : vehicles[0];
  if (!target) { await reply(player, `No vehicle named "${query}". Your vehicles: ${vehicles.map(v => v.nickname).join(", ")}`); return; }
  const engine = (globalThis as any).__turdmod_engine_client;
  if (!engine) { await reply(player, "Engine unavailable."); return; }
  try { await engine.call("spawnVehicle", { vehicleClass: target.vehicleClass, near: { playerName: player } }); }
  catch { await reply(player, "Spawn failed."); return; }
  target.spawned = true;
  await setVehicles(steam, vehicles);
  log(`${player} spawned "${target.nickname}"`);
  await reply(player, `"${target.nickname}" (${target.friendlyName}) spawned. Cost: ${cfg.spawnCost} coins.`);
}

async function cmdDespawn(steam: string, player: string): Promise<void> {
  const vehicles = await getVehicles(steam);
  const spawned = vehicles.find(v => v.spawned);
  if (!spawned) { await reply(player, "No vehicle spawned."); return; }
  const engine = (globalThis as any).__turdmod_engine_client;
  if (!engine) { await reply(player, "Engine unavailable."); return; }
  try { await engine.call("destroyVehicle", { vehicleClass: spawned.vehicleClass }); } catch {}
  spawned.spawned = false;
  await setVehicles(steam, vehicles);
  log(`${player} despawned "${spawned.nickname}"`);
  await reply(player, `"${spawned.nickname}" despawned and stored.`);
}

async function cmdList(steam: string, player: string): Promise<void> {
  const vehicles = await getVehicles(steam);
  if (vehicles.length === 0) { await reply(player, "No vehicles. Use !vregister <type>."); return; }
  const lines = vehicles.map(v => `${v.nickname}(${v.friendlyName}) ${v.spawned ? "SPAWNED" : "stored"}${v.insured ? " [insured]" : ""}`);
  await reply(player, lines.join(" | "));
}

async function cmdInsure(steam: string, player: string, args: string[], cfg: VehicleConfig): Promise<void> {
  const vehicles = await getVehicles(steam);
  if (vehicles.length === 0) { await reply(player, "No vehicles."); return; }
  const query = args.join(" ").trim().toLowerCase();
  const target = query ? vehicles.find(v => v.nickname.toLowerCase() === query) : vehicles[0];
  if (!target) { await reply(player, "Vehicle not found."); return; }
  if (target.insured) { await reply(player, `"${target.nickname}" already insured.`); return; }
  target.insured = true;
  await setVehicles(steam, vehicles);
  log(`${player} insured "${target.nickname}"`);
  await reply(player, `"${target.nickname}" insured. Cost: ${cfg.insureCost} coins.`);
}

async function cmdLicense(steam: string, player: string): Promise<void> {
  const vehicles = await getVehicles(steam);
  if (vehicles.length === 0) { await reply(player, "No vehicles."); return; }
  const v = vehicles[0];
  await reply(player, `${v.nickname} | ${v.friendlyName} | ${v.spawned ? "SPAWNED" : "Stored"} | Insured: ${v.insured ? "Yes" : "No"} | Since: ${v.registeredAt.slice(0, 10)}`);
}

async function onChat(msg: { payload: ChatPayload }): Promise<void> {
  const { player, steam, text, channel } = msg.payload;
  if (channel !== "Local" || !steam || !text.startsWith("!")) return;
  const clean = player.replace(/\(\d+\)$/, "");
  const parts = text.trim().split(/\s+/);
  const cmd = parts[0].toLowerCase();
  const args = parts.slice(1);
  const cfg = await persistence.get<VehicleConfig>("vehicle-config") ?? ConfigSchema.parse({});

  if (cmd.startsWith("!!")) {
    if (!(await isAdmin(steam))) { await reply(clean, "Admin only."); return; }
    const sub = cmd.slice(2);
    if (sub === "vehiclelist") {
      const engine = (globalThis as any).__turdmod_engine_client;
      if (!engine) return;
      const res = await engine.call("getOnlinePlayers", {});
      const lines: string[] = [];
      for (const p of res.players ?? []) {
        const s = p.steamId ?? p.steam ?? p.id;
        if (!s) continue;
        const vs = await getVehicles(s);
        for (const v of vs) lines.push(`${p.name}: ${v.nickname}(${v.friendlyName}) ${v.spawned ? "SPAWNED" : "stored"}`);
      }
      await reply(clean, lines.length > 0 ? lines.join(" | ") : "No registered vehicles.");
      return;
    }
    if (sub === "vehiclelimit" && args[0]) {
      const n = parseInt(args[0], 10);
      if (isNaN(n) || n < 1) return;
      const c = { ...cfg, maxPerPlayer: n };
      await persistence.set("vehicle-config", c);
      await reply(clean, `Max vehicles per player: ${n}`);
      return;
    }
    return;
  }

  if (!cfg.enabled) return;
  if (cmd === "!vregister") return cmdRegister(steam, clean, args, cfg);
  if (cmd === "!vspawn") return cmdSpawn(steam, clean, args, cfg);
  if (cmd === "!vdespawn") return cmdDespawn(steam, clean);
  if (cmd === "!vlist") return cmdList(steam, clean);
  if (cmd === "!vinsure") return cmdInsure(steam, clean, args, cfg);
  if (cmd === "!vlicense") return cmdLicense(steam, clean);
}

export async function on_load(): Promise<void> {
  const raw = await persistence.get<VehicleConfig>("vehicle-config");
  const cfg = raw ? ConfigSchema.parse(raw) : ConfigSchema.parse({});
  if (!raw) await persistence.set("vehicle-config", cfg);
  HANDLES.push(network.on<ChatPayload>("system.chat", onChat));
  log(`loaded — max ${cfg.maxPerPlayer}/player, spawn ${cfg.spawnCost}c, insure ${cfg.insureCost}c`);
}

export function on_unload(): void {
  for (const h of HANDLES) h.dispose();
  HANDLES.length = 0;
  log("unloaded");
}
