import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { z } from "zod";
import {
  network,
  persistence,
  getRuntime,
  type Disposable,
} from "@turdmod/turdmod-api";

const ConfigSchema = z.object({
  coinsPerMinute: z.number().int().min(0).default(1),
  dailyBonus: z.number().int().min(0).default(100),
  streakBonuses: z.array(z.number().int()).default([10, 20, 30, 40, 50, 75, 100]),
  maxTransfer: z.number().int().min(1).default(10000),
  minTransfer: z.number().int().min(1).default(1),
  currency: z.string().default("coins"),
  embedColor: z.number().int().default(0xf59e0b),
});

type EconomyConfig = z.infer<typeof ConfigSchema>;

interface Wallet {
  balance: number;
  totalEarned: number;
  totalSpent: number;
  lastDailyClaim: string | null;
  loginStreak: number;
  lastLoginDate: string | null;
}

const EMPTY_WALLET: Wallet = {
  balance: 0, totalEarned: 0, totalSpent: 0,
  lastDailyClaim: null, loginStreak: 0, lastLoginDate: null,
};

interface LoginPayload { ip: string; steam: string; player: string; pos: { x: number; y: number; z: number } }
interface LogoutPayload { steam: string; player: string }
interface ChatPayload { ts: string; channel: string; player: string; steam?: string; text: string }

interface OnlineEntry { loginTime: number; playerName: string; lastAward: number }

const online = new Map<string, OnlineEntry>();
const HANDLES: Disposable[] = [];
let accrualTimer: ReturnType<typeof setInterval> | null = null;
let cfg: EconomyConfig;

const log = (msg: string) => getRuntime().log("info", `[economy] ${msg}`);
const warn = (msg: string) => getRuntime().log("warn", `[economy] ${msg}`);

async function replyToPlayer(playerName: string, message: string): Promise<void> {
  const engine = (globalThis as any).__turdmod_engine_client;
  if (!engine) { log(`[reply] engine not connected, Discord fallback: ${message}`); return; }
  try {
    await engine.call("sendChatLineToPlayer", { playerName, channel: "Admin", message: `[Economy] ${message}` });
  } catch (err) {
    warn(`reply to ${playerName} failed: ${err}`);
  }
}

async function loadWallet(steam: string): Promise<Wallet> {
  const raw = await persistence.getForPlayer<Wallet>(steam, "wallet");
  return raw ? { ...EMPTY_WALLET, ...raw } : { ...EMPTY_WALLET };
}

async function saveWallet(steam: string, w: Wallet): Promise<void> {
  await persistence.setForPlayer(steam, "wallet", w);
}

function todayDate(): string { return new Date().toISOString().slice(0, 10); }

function embed(title: string, description: string, color?: number) {
  return { embed: { title, description, color: color ?? cfg.embedColor, timestamp: new Date().toISOString() } };
}

function findOnlinePlayer(partial: string): { steam: string; entry: OnlineEntry } | null {
  const lower = partial.toLowerCase();
  for (const [steam, entry] of online) {
    if (entry.playerName.toLowerCase() === lower) return { steam, entry };
  }
  for (const [steam, entry] of online) {
    if (entry.playerName.toLowerCase().startsWith(lower)) return { steam, entry };
  }
  return null;
}

// ─── Item catalog ───
const __dirname = dirname(fileURLToPath(import.meta.url));
let catalog: any = null;

function loadCatalog(): void {
  const p = join(__dirname, "..", "..", "..", "data", "turdmod-companion", "economy", "item-catalog.json");
  try { catalog = JSON.parse(readFileSync(p, "utf-8")); log(`catalog loaded: ${catalog.totalItems} items`); }
  catch { warn("item catalog not found"); catalog = null; }
}

function checkPermission(steam: string, command: string): boolean {
  const check = (globalThis as any).__turdmod_checkPermission;
  if (typeof check === "function") return check(steam, command) as boolean;
  return true;
}

const SHOP_PAGE_SIZE = 10;

async function cmdShop(steam: string, player: string, parts: string[]): Promise<void> {
  if (!catalog) { await replyToPlayer(player, "Item catalog not loaded."); return; }
  if (parts.length < 2) {
    const cats = Object.entries(catalog.categories).map(([p, i]: [string, any]) => `${i.name}(${i.count}) [${p}]`).join(" | ");
    await replyToPlayer(player, `Shop: ${cats}`);
    return;
  }
  const catPrefix = parts[1].toLowerCase();
  const catInfo = catalog.categories[catPrefix];
  if (!catInfo) { await replyToPlayer(player, `Unknown category. Valid: ${Object.keys(catalog.categories).join(", ")}`); return; }
  const page = parts.length >= 3 ? Math.max(1, Math.floor(Number(parts[2]) || 1)) : 1;
  const catItems = Object.entries(catalog.items).filter(([c]: [string, any]) => c.startsWith(catPrefix)).sort(([a]: [string, any], [b]: [string, any]) => a.localeCompare(b));
  const totalPages = Math.ceil(catItems.length / SHOP_PAGE_SIZE);
  const start = (Math.min(page, totalPages) - 1) * SHOP_PAGE_SIZE;
  const pageItems = catItems.slice(start, start + SHOP_PAGE_SIZE);
  if (pageItems.length === 0) { await replyToPlayer(player, `No items in ${catInfo.name}.`); return; }
  const lines = pageItems.map(([code, item]: [string, any]) => `#${code} ${item.displayName}`);
  await replyToPlayer(player, `${catInfo.name} (${Math.min(page, totalPages)}/${totalPages}): ${lines.join(" | ")}`);
}

async function cmdPrice(steam: string, player: string, parts: string[]): Promise<void> {
  if (!catalog || parts.length < 2) { await replyToPlayer(player, "Usage: !price #<code>"); return; }
  const code = parts[1].replace(/^#/, "").toLowerCase();
  const item = catalog.items[code];
  if (!item) { await replyToPlayer(player, `Unknown: #${code}`); return; }
  await replyToPlayer(player, `${item.displayName} | ${item.category} | Spawn: ${item.spawnCode}`);
}

async function cmdBuy(steam: string, player: string, parts: string[]): Promise<void> {
  if (!catalog || parts.length < 2) { await replyToPlayer(player, "Usage: !buy #<code> [qty]"); return; }
  const code = parts[1].replace(/^#/, "").toLowerCase();
  const item = catalog.items[code];
  if (!item) { await replyToPlayer(player, `Unknown: #${code}`); return; }
  const qty = parts.length >= 3 ? Math.max(1, Math.floor(Number(parts[2]) || 1)) : 1;
  if (qty > 50) { await replyToPlayer(player, "Max 50 per purchase."); return; }
  const unitPrice = Math.max(1, Math.ceil(item.traderBuyPrice));
  const totalPrice = unitPrice * qty;
  const wallet = await loadWallet(steam);
  if (wallet.balance < totalPrice) { await replyToPlayer(player, `Need ${totalPrice}, have ${wallet.balance} ${cfg.currency}.`); return; }
  wallet.balance -= totalPrice;
  wallet.totalSpent += totalPrice;
  await saveWallet(steam, wallet);
  log(`PURCHASE ${player} bought ${qty}x ${item.displayName} (#${code}) for ${totalPrice} ${cfg.currency}`);
  await replyToPlayer(player, `Bought ${qty}x ${item.displayName} for ${totalPrice} ${cfg.currency}. Delivery pending. Balance: ${wallet.balance}`);
}

async function awardAccruedCoins(steam: string, entry: OnlineEntry): Promise<number> {
  const now = Date.now();
  const minutes = Math.floor((now - entry.lastAward) / 60_000);
  if (minutes < 1) return 0;
  const earned = minutes * cfg.coinsPerMinute;
  const wallet = await loadWallet(steam);
  wallet.balance += earned;
  wallet.totalEarned += earned;
  await saveWallet(steam, wallet);
  entry.lastAward += minutes * 60_000;
  return earned;
}

async function accrualTick(): Promise<void> {
  for (const [steam, entry] of online) {
    try { await awardAccruedCoins(steam, entry); } catch (err) { warn(`accrual failed ${steam}: ${err}`); }
  }
}

async function processLoginStreak(steam: string, playerName: string): Promise<void> {
  const wallet = await loadWallet(steam);
  const today = todayDate();
  if (wallet.lastLoginDate === today) return;

  let streakBroken = true;
  if (wallet.lastLoginDate) {
    const hoursGap = (Date.now() - new Date(wallet.lastLoginDate + "T00:00:00Z").getTime()) / 3_600_000;
    if (hoursGap <= 36) streakBroken = false;
  }

  wallet.loginStreak = streakBroken ? 1 : Math.min(wallet.loginStreak + 1, cfg.streakBonuses.length);
  const bonus = cfg.streakBonuses[Math.min(wallet.loginStreak, cfg.streakBonuses.length) - 1] ?? 0;
  wallet.balance += bonus;
  wallet.totalEarned += bonus;
  wallet.lastLoginDate = today;
  await saveWallet(steam, wallet);

  log(`${playerName} streak day ${wallet.loginStreak} +${bonus} ${cfg.currency}`);
  network.broadcast("economy.streak", embed("Login Streak", `**${playerName}** day **${wallet.loginStreak}** streak! +**${bonus}** ${cfg.currency}`));
}

function onLogin(msg: { payload: LoginPayload }): void {
  const { steam, player } = msg.payload;
  const cleanName = player.replace(/\(\d+\)$/, "");
  const now = Date.now();
  online.set(steam, { loginTime: now, playerName: cleanName, lastAward: now });
  processLoginStreak(steam, cleanName).catch((err) => warn(`streak error ${steam}: ${err}`));
}

async function onLogout(msg: { payload: LogoutPayload }): Promise<void> {
  const { steam } = msg.payload;
  const entry = online.get(steam);
  if (entry) {
    await awardAccruedCoins(steam, entry);
    online.delete(steam);
  }
}

async function onChat(msg: { payload: ChatPayload }): Promise<void> {
  const { player, steam, text } = msg.payload;
  if (!steam || !text.startsWith("!")) return;
  const { channel } = msg.payload;
  if (channel !== "Local") return;
  const cleanPlayer = player.replace(/\(\d+\)$/, "");
  const parts = text.trim().split(/\s+/);
  const cmd = parts[0].toLowerCase();

  // Admin commands bypass tier permissions
  if (cmd === "!!setbalance") return cmdAdminSetBalance(steam, cleanPlayer, parts);
  if (cmd === "!!addcoins") return cmdAdminAddCoins(steam, cleanPlayer, parts);

  // Player commands — check tier permission first
  if (!checkPermission(steam, cmd)) {
    await replyToPlayer(cleanPlayer, `${cmd} requires a higher tier. Contact admin.`);
    return;
  }

  if (cmd === "!balance" || cmd === "!bal") return cmdBalance(steam, cleanPlayer);
  if (cmd === "!pay") return cmdPay(steam, cleanPlayer, parts);
  if (cmd === "!top" || cmd === "!richest") return cmdTop(steam, cleanPlayer);
  if (cmd === "!daily") return cmdDaily(steam, cleanPlayer);
  if (cmd === "!shop") return cmdShop(steam, cleanPlayer, parts);
  if (cmd === "!price") return cmdPrice(steam, cleanPlayer, parts);
  if (cmd === "!buy") return cmdBuy(steam, cleanPlayer, parts);
}

async function cmdBalance(steam: string, player: string): Promise<void> {
  const wallet = await loadWallet(steam);
  await replyToPlayer(player, `Balance: ${wallet.balance.toLocaleString()} ${cfg.currency} | Earned: ${wallet.totalEarned.toLocaleString()} | Spent: ${wallet.totalSpent.toLocaleString()}`);
}

async function cmdPay(senderSteam: string, senderName: string, parts: string[]): Promise<void> {
  if (parts.length < 3) { await replyToPlayer(senderName, "Usage: !pay <player> <amount>"); return; }
  const amount = Math.floor(Number(parts[2]));
  if (!Number.isFinite(amount) || amount < cfg.minTransfer) { await replyToPlayer(senderName, `Min transfer: ${cfg.minTransfer} ${cfg.currency}`); return; }
  if (amount > cfg.maxTransfer) { await replyToPlayer(senderName, `Max transfer: ${cfg.maxTransfer.toLocaleString()} ${cfg.currency}`); return; }
  const target = findOnlinePlayer(parts[1]);
  if (!target) { await replyToPlayer(senderName, `${parts[1]} not online.`); return; }
  if (target.steam === senderSteam) { await replyToPlayer(senderName, "Can't pay yourself."); return; }
  const sw = await loadWallet(senderSteam);
  if (sw.balance < amount) { await replyToPlayer(senderName, `Insufficient funds (${sw.balance.toLocaleString()} ${cfg.currency}).`); return; }
  const rw = await loadWallet(target.steam);
  sw.balance -= amount; sw.totalSpent += amount;
  rw.balance += amount; rw.totalEarned += amount;
  await saveWallet(senderSteam, sw);
  await saveWallet(target.steam, rw);
  log(`${senderName} paid ${target.entry.playerName} ${amount} ${cfg.currency}`);
  await replyToPlayer(senderName, `Sent ${amount.toLocaleString()} ${cfg.currency} to ${target.entry.playerName}`);
  await replyToPlayer(target.entry.playerName, `Received ${amount.toLocaleString()} ${cfg.currency} from ${senderName}`);
}

async function cmdTop(steam: string, player: string): Promise<void> {
  const cached = await persistence.get<Record<string, { name: string; balance: number }>>("leaderboard_cache") ?? {};
  for (const [s, entry] of online) {
    const w = await loadWallet(s);
    cached[s] = { name: entry.playerName, balance: w.balance };
  }
  await persistence.set("leaderboard_cache", cached);
  const board = Object.values(cached).sort((a, b) => b.balance - a.balance).slice(0, 10);
  const lines = board.map((e, i) => `#${i + 1} ${e.name}: ${e.balance.toLocaleString()} ${cfg.currency}`);
  await replyToPlayer(player, lines.length > 0 ? `Top 10: ${lines.join(" | ")}` : "No data yet.");
}

async function cmdDaily(steam: string, player: string): Promise<void> {
  const wallet = await loadWallet(steam);
  if (wallet.lastDailyClaim) {
    const hoursSince = (Date.now() - new Date(wallet.lastDailyClaim).getTime()) / 3_600_000;
    if (hoursSince < 24) {
      await replyToPlayer(player, `Daily already claimed. Try again in ~${Math.ceil(24 - hoursSince)}h.`);
      return;
    }
  }
  wallet.balance += cfg.dailyBonus;
  wallet.totalEarned += cfg.dailyBonus;
  wallet.lastDailyClaim = new Date().toISOString();
  await saveWallet(steam, wallet);
  await replyToPlayer(player, `Daily bonus: +${cfg.dailyBonus} ${cfg.currency}! Balance: ${wallet.balance.toLocaleString()}`);
}

async function cmdAdminSetBalance(steam: string, player: string, parts: string[]): Promise<void> {
  if (parts.length < 3) return;
  const target = findOnlinePlayer(parts[1]);
  if (!target) return;
  const amount = Math.floor(Number(parts[2]));
  if (!Number.isFinite(amount) || amount < 0) return;
  const wallet = await loadWallet(target.steam);
  const prev = wallet.balance;
  wallet.balance = amount;
  await saveWallet(target.steam, wallet);
  log(`ADMIN ${player} set ${target.entry.playerName} balance: ${prev} -> ${amount}`);
  network.broadcast("economy.admin", embed("Admin — Set Balance", `**${player}** set **${target.entry.playerName}** balance to **${amount.toLocaleString()}** (was ${prev.toLocaleString()})`, 0xef4444));
}

async function cmdAdminAddCoins(steam: string, player: string, parts: string[]): Promise<void> {
  if (parts.length < 3) return;
  const target = findOnlinePlayer(parts[1]);
  if (!target) return;
  const amount = Math.floor(Number(parts[2]));
  if (!Number.isFinite(amount)) return;
  const wallet = await loadWallet(target.steam);
  wallet.balance += amount;
  if (amount > 0) wallet.totalEarned += amount;
  await saveWallet(target.steam, wallet);
  log(`ADMIN ${player} added ${amount} ${cfg.currency} to ${target.entry.playerName}`);
  network.broadcast("economy.admin", embed("Admin — Add Coins", `**${player}** added **${amount.toLocaleString()}** to **${target.entry.playerName}** (balance: **${wallet.balance.toLocaleString()}**)`, 0xef4444));
}

async function loadConfig(): Promise<EconomyConfig> {
  const raw = await persistence.get<unknown>("economy_config");
  if (raw) {
    const parsed = ConfigSchema.safeParse(raw);
    if (parsed.success) return parsed.data;
  }
  const defaults = ConfigSchema.parse({});
  await persistence.set("economy_config", defaults);
  log("seeded default config");
  return defaults;
}

export async function on_load(): Promise<void> {
  loadCatalog();
  cfg = await loadConfig();
  HANDLES.push(network.on<LoginPayload>("system.login", onLogin));
  HANDLES.push(network.on<LogoutPayload>("system.logout", onLogout));
  HANDLES.push(network.on<ChatPayload>("system.chat", onChat));
  accrualTimer = setInterval(() => { accrualTick().catch((err) => warn(`tick error: ${err}`)); }, 60_000);
  log(`loaded — ${cfg.coinsPerMinute} ${cfg.currency}/min, daily=${cfg.dailyBonus}`);
}

export function on_unload(): void {
  for (const h of HANDLES) h.dispose();
  HANDLES.length = 0;
  if (accrualTimer) { clearInterval(accrualTimer); accrualTimer = null; }
  log("unloaded");
}
