import { z } from "zod";
import { network, persistence, getRuntime, type Disposable } from "@turdmod/turdmod-api";

const TierSchema = z.object({
  name: z.string().min(1),
  commands: z.array(z.string()),
  limits: z.record(z.number()).default({}),
});

const TierConfigSchema = z.object({
  tiers: z.record(TierSchema),
  playerTiers: z.record(z.string()),
  defaultTier: z.string(),
});
type TierConfig = z.infer<typeof TierConfigSchema>;

interface ChatPayload { ts: string; channel: string; player: string; steam?: string; text: string }

const OWNER_STEAM_ID = process.env.OWNER_STEAM_ID || "YOUR_STEAM_ID_1";

const DEFAULT_CONFIG: TierConfig = {
  tiers: {
    player: { name: "Player", commands: ["!balance", "!bal", "!daily", "!top", "!richest", "!shop", "!price", "!buy", "!tp", "!home", "!sethome", "!stats", "!protection"], limits: { homeCooldown: 300, tpCooldown: 60 } },
    vip: { name: "VIP", commands: ["!balance", "!bal", "!daily", "!top", "!richest", "!shop", "!price", "!buy", "!tp", "!home", "!sethome", "!stats", "!protection", "!vspawn", "!vdespawn", "!vlist", "!vregister"], limits: { homeCooldown: 120, tpCooldown: 30, maxHomes: 5 } },
    moderator: { name: "Moderator", commands: ["*"], limits: {} },
    admin: { name: "Admin", commands: ["*"], limits: {} },
    owner: { name: "Owner", commands: ["*"], limits: {} },
  },
  playerTiers: { [OWNER_STEAM_ID]: "owner" },
  defaultTier: "player",
};

const HANDLES: Disposable[] = [];
let tierCfg: TierConfig;
const log = (msg: string) => getRuntime().log("info", `[permissions] ${msg}`);

async function reply(player: string, message: string): Promise<void> {
  const engine = (globalThis as any).__turdmod_engine_client;
  if (!engine) return;
  try { await engine.call("sendChatLineToPlayer", { playerName: player, channel: "Admin", message: `[Perms] ${message}` }); } catch {}
}

function publishToGlobal(): void {
  (globalThis as any).__turdmod_tiers = { tiers: tierCfg.tiers, playerTiers: tierCfg.playerTiers, defaultTier: tierCfg.defaultTier };
  (globalThis as any).__turdmod_checkPermission = (steamId: string, command: string): boolean => {
    const d = (globalThis as any).__turdmod_tiers;
    if (!d) return true;
    const tierName = d.playerTiers[steamId] || d.defaultTier;
    const tier = d.tiers[tierName];
    if (!tier) return true;
    if (tier.commands.includes("*")) return true;
    return tier.commands.includes(command);
  };
}

async function save(): Promise<void> {
  await persistence.set("tier-config", tierCfg);
  publishToGlobal();
}

function isOwnerOrAdmin(steam: string): boolean {
  if (steam === OWNER_STEAM_ID) return true;
  const tier = tierCfg.playerTiers[steam] || tierCfg.defaultTier;
  return tier === "owner" || tier === "admin";
}

function isOwner(steam: string): boolean {
  return steam === OWNER_STEAM_ID || tierCfg.playerTiers[steam] === "owner";
}

async function onChat(msg: { payload: ChatPayload }): Promise<void> {
  const { player, steam, text, channel } = msg.payload;
  if (channel !== "Local" || !steam || !text.startsWith("!!")) return;
  const clean = player.replace(/\(\d+\)$/, "");
  const parts = text.trim().split(/\s+/);
  const cmd = parts[0].toLowerCase().slice(2);

  // Owner-only commands: SetTier, CreateTier, TierAddCmd, TierRemoveCmd
  const ownerOnly = ["settier", "createtier", "tieraddcmd", "tierremovecmd"];
  if (ownerOnly.includes(cmd) && !isOwner(steam)) {
    await reply(clean, "Owner only.");
    return;
  }
  // Admin+ commands: PlayerTier, ListTiers
  if (!isOwnerOrAdmin(steam)) {
    await reply(clean, "Access denied.");
    return;
  }

  if (cmd === "settier") {
    if (parts.length < 3) { await reply(clean, "Usage: !!SetTier <steamId> <tier>"); return; }
    const target = parts[1], tier = parts[2].toLowerCase();
    if (!tierCfg.tiers[tier]) { await reply(clean, `Unknown tier: ${tier}. !!ListTiers`); return; }
    tierCfg.playerTiers[target] = tier;
    await save();
    log(`Admin ${clean} set ${target} -> ${tier}`);
    await reply(clean, `${target} assigned to ${tierCfg.tiers[tier].name}`);
  } else if (cmd === "createtier") {
    if (parts.length < 2) { await reply(clean, "Usage: !!CreateTier <name>"); return; }
    const name = parts[1].toLowerCase();
    if (tierCfg.tiers[name]) { await reply(clean, `Tier "${name}" exists.`); return; }
    tierCfg.tiers[name] = { name: parts[1], commands: [], limits: {} };
    await save();
    log(`Admin ${clean} created tier ${name}`);
    await reply(clean, `Tier "${parts[1]}" created.`);
  } else if (cmd === "tieraddcmd") {
    if (parts.length < 3) { await reply(clean, "Usage: !!TierAddCmd <tier> <cmd>"); return; }
    const tier = tierCfg.tiers[parts[1].toLowerCase()];
    if (!tier) { await reply(clean, `Unknown tier.`); return; }
    const c = parts[2].toLowerCase();
    if (!tier.commands.includes(c)) { tier.commands.push(c); await save(); }
    await reply(clean, `${tier.name} commands: ${tier.commands.join(", ")}`);
  } else if (cmd === "tierremovecmd") {
    if (parts.length < 3) { await reply(clean, "Usage: !!TierRemoveCmd <tier> <cmd>"); return; }
    const tier = tierCfg.tiers[parts[1].toLowerCase()];
    if (!tier) { await reply(clean, `Unknown tier.`); return; }
    const c = parts[2].toLowerCase();
    const idx = tier.commands.indexOf(c);
    if (idx >= 0) { tier.commands.splice(idx, 1); await save(); }
    await reply(clean, `${tier.name} commands: ${tier.commands.join(", ")}`);
  } else if (cmd === "listtiers") {
    const lines = Object.entries(tierCfg.tiers).map(([k, t]) => `${t.name}[${k}](${t.commands.length} cmds)`);
    await reply(clean, `Tiers: ${lines.join(" | ")} | Default: ${tierCfg.defaultTier}`);
  } else if (cmd === "playertier") {
    if (parts.length < 2) { await reply(clean, "Usage: !!PlayerTier <steamId>"); return; }
    const t = tierCfg.playerTiers[parts[1]] || tierCfg.defaultTier;
    await reply(clean, `${parts[1]} -> ${t}`);
  }
}

export async function on_load(): Promise<void> {
  const raw = await persistence.get<unknown>("tier-config");
  if (raw) {
    const parsed = TierConfigSchema.safeParse(raw);
    tierCfg = parsed.success ? parsed.data : DEFAULT_CONFIG;
  } else {
    tierCfg = DEFAULT_CONFIG;
    await persistence.set("tier-config", DEFAULT_CONFIG);
    log("seeded default tier config");
  }
  publishToGlobal();
  HANDLES.push(network.on<ChatPayload>("system.chat", onChat));
  log(`loaded — ${Object.keys(tierCfg.tiers).length} tiers, ${Object.keys(tierCfg.playerTiers).length} assigned, default=${tierCfg.defaultTier}`);
}

export function on_unload(): void {
  for (const h of HANDLES) h.dispose();
  HANDLES.length = 0;
  delete (globalThis as any).__turdmod_tiers;
  delete (globalThis as any).__turdmod_checkPermission;
  log("unloaded");
}
