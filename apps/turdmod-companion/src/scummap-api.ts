/**
 * scummap-api.ts — HTTP client for scummap's internal API.
 *
 * The companion calls scummap's API server (localhost) to read/write
 * wallets, look up Steam→Discord links, and process shop purchases.
 * Auth via X-Internal-Token header (same as the Discord bot uses).
 *
 * Env vars:
 *   SCUMMAP_API_URL — e.g. http://localhost:3001 (default)
 *   SCUMMAP_API_INTERNAL_TOKEN — shared secret
 */

const API_URL = process.env.SCUMMAP_API_URL || "http://localhost:3001";
const TOKEN = process.env.SCUMMAP_API_INTERNAL_TOKEN || "";

interface WalletBalance {
  available: number;
  locked: number;
}

interface SteamLink {
  userId: string;
  discordId: string;
}

interface ShopPurchaseResult {
  ok: boolean;
  orderId?: string;
  balanceAfter?: number;
  error?: string;
}

async function apiCall<T>(path: string, method = "GET", body?: unknown): Promise<T> {
  const opts: RequestInit = {
    method,
    headers: {
      "Content-Type": "application/json",
      "X-Internal-Token": TOKEN,
    },
  };
  if (body) opts.body = JSON.stringify(body);

  const res = await fetch(`${API_URL}${path}`, opts);
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(`scummap API ${method} ${path} → ${res.status}: ${text.slice(0, 200)}`);
  }
  return res.json() as Promise<T>;
}

export async function getWallet(guildId: string, discordUserId: string): Promise<WalletBalance | null> {
  try {
    return await apiCall<WalletBalance>(`/api/internal/wallet/me?guildId=${guildId}&userId=${discordUserId}`);
  } catch {
    return null;
  }
}

export async function walletGrant(guildId: string, discordUserId: string, amount: number, memo: string): Promise<boolean> {
  try {
    await apiCall(`/api/internal/wallet/grant`, "POST", { guildId, userId: discordUserId, amount, memo });
    return true;
  } catch {
    return false;
  }
}

export async function walletSend(guildId: string, senderDiscordId: string, recipientDiscordId: string, amount: number, memo: string): Promise<boolean> {
  try {
    await apiCall(`/api/internal/wallet/send`, "POST", { guildId, senderId: senderDiscordId, recipientId: recipientDiscordId, amount, memo });
    return true;
  } catch {
    return false;
  }
}

export async function lookupSteamLink(steamId: string): Promise<SteamLink | null> {
  try {
    return await apiCall<SteamLink>(`/api/internal/steam-links/${steamId}`);
  } catch {
    return null;
  }
}

export async function createSteamLink(steamId: string, discordUserId: string): Promise<boolean> {
  try {
    await apiCall(`/api/internal/steam-links`, "POST", { steamId, discordUserId });
    return true;
  } catch {
    return false;
  }
}

export async function shopPurchase(guildId: string, discordUserId: string, shortCode: string, quantity: number): Promise<ShopPurchaseResult> {
  try {
    return await apiCall<ShopPurchaseResult>(`/api/internal/shop/purchase`, "POST", { guildId, userId: discordUserId, shortCode, quantity });
  } catch (err) {
    return { ok: false, error: String(err) };
  }
}

export function isConfigured(): boolean {
  return !!TOKEN;
}
