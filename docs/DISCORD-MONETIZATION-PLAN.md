# ScummyMap Discord + Monetization System (DESIGN — 2026-06-08)

Researched by background agent over the turdmod tree. This is the durable plan for
Joel's ask: "register discord with ingame names and steam ids… reward verifying…
change titles/roles by donor level… P2W-safe perks (Gamepires-compliant)… clean
unlimited monetization." Status: **designed, not yet built.**

## What already exists (reuse, don't rebuild)
- **Donor-role engine** — `apps/turdmod-bot/src/role-sync.ts` (`addPremiumRole`/`removePremiumRole`
  via `member.roles.add/remove`, handles 50013/10007). Generalize to `setRole`/`setDonorTier`.
- **Signed webhook receiver** — `apps/turdmod-bot/src/webhook.ts` (Hono, HMAC-SHA256
  `x-turdmod-signature`). Template for a `POST /webhooks/donation`.
- **Kill-feed → Discord** — `apps/turdmod-bot/src/live-feed.ts` (SSE → embeds, batched, 429-aware).
  ALREADY BUILT — just point `DISCORD_FEED_WEBHOOK` at #kill-feed.
- **Two-way chat relay + admin slash cmds** — `apps/turdmod-bot/src/live-gateway.ts`
  (`/say /players /weather /time /kick /ban`, engine RPC via `:9090/engine/rpc`). ALREADY BUILT —
  point `DISCORD_RELAY_CHANNEL_ID` at #chat-relay. This is the host for the new community slash cmds.
- **In-game coin-grant hook** — `referral.rs::credit(steam, amount)` writes `economy.json`.
- **In-game private reply** — `bridge.rs::reply` / `fmt_reply` → `sendChatLineToPlayer` (yellow ch4).
- **SteamID resolution** — `scumdb.rs::profile_id_for_steam` / `profile_id_for_name` /
  `steam_for_profile_id` (SCUM.db `user_profile`); chat events often carry `ev.data["steam"]`.
- **Prior intent** — IDEAS.md ~253-265 already specifies the `/link` → `!link <code>` flow.

⚠ **economy.json key inconsistency (real bug):** `referral.rs` keys `players` by **SteamID**,
but `chat_cmds.rs` (`!bal`/`!top`) matches by **name** and `!claim daily` by `name.to_lowercase()`.
Any verify/donation grant MUST credit the key the player's `!bal` reads (name-keyed) or fix the
schema first. Reconcile before wiring grants.

## A. Verification: Discord ↔ in-game name ↔ SteamID
Bot-initiated code, redeemed in-game (matches IDEAS.md):
1. `/link` in Discord → bot makes a 6-char code, stores `link_pending.json` `{code→{discord_id,issued_at,ttl:600}}`, DMs it.
2. `!link <code>` in-game → new `discord_link.rs` (clone `referral.rs` loop) validates, resolves SteamID, writes link.
3. Storage **`C:\TurdMOD\data\discord_links.json`**, 3-way keyed: `by_steam{steam→{discord_id,name,linked_at,tier}}` + `by_discord{discord_id→steam}`.
4. **Reward (P2W-safe):** one-time "Verified" title (`title_system.rs`) + 250 coins (`credit()`, name-keyed wallet).

## B. Donor tiers → Discord roles
Bronze/Silver/Gold/Diamond (Joel creates 4 roles, IDs → bot config). `setDonorTier(discord_id,tier)`
removes the other 3 + adds target. Trigger: `POST /webhooks/donation {discord_id,tier,action}` (clone signed webhook).

## C. P2W-safe perks (Gamepires: cosmetic/convenience only, ZERO gameplay advantage)
✅ SAFE: Discord roles/colors/lounge, website badge/pin (static vanity, NOT live tactical data),
**priority queue / reserved slot** (canonical allowed convenience), in-game chat **title/color**
(`title_system.rs`), custom join broadcast (`welcome.rs`), cosmetic-only skins (verify zero
armor/storage/utility), name a cosmetic event, firework `!` command.
❌ P2W — NEVER gate behind donation: **selling coins** (economy buys insurance/taxi → advantage at scale),
**extra vehicle slots** beyond the 5-limit, `kits.rs`/`loot_multiplier.rs`/`skill_boost.rs`,
godmode / spawn-protection / fast-travel discounts. Rule: payment → Discord/website/cosmetic-text ONLY.

## D. Channels
#rules (static embed via `/postrules`), #announcements (mirror `scheduler.rs` restarts),
#kill-feed (live-feed.ts ✓), #chat-relay (live-gateway.ts ✓), #verify (run `/link`),
#live-map (pinned embed → https://www.scummymap.com/?map=official; phase-2: auto player-count / map screenshot),
#donor-lounge (role-gated), #commands (`/balance /daily /top /pay` — needs new economy endpoints).

## E. Discord #rules text — ready to post (see agent output / verbatim block); also trim `rules.rs` RULES array (currently stale 7 lines).

## F. Monetization (clean / unlimited, no P2W)
**Tebex (primary)** → `POST /webhooks/donation` → role-sync (cosmetic/convenience packages only, NO coins/gear).
**Ko-fi/Patreon** (Patreon native Discord role-sync). **Discord Server Subscriptions/Boosts** (config only).
**www.scummymap.com premium** (personal pins, ad-free, history — keep tactical data equal for all).
**Map-site ads. Merch (POD).** Marketplace Premium ($9.99/mo, already built, separate product).

## Build plan
**Code (parent can build):**
1. `discord_link.rs` — `!link <code>` → `discord_links.json` + Verified title + 250 coins (name-keyed).
2. `server.rs` — bearer routes: `POST /links/issue`, `GET /links/resolve`, `GET /economy/balance`, `POST /economy/grant`.
3. `turdmod-bot` — generalize `role-sync` → `setRole/setDonorTier`; add `/link /balance /daily /top /pay`; add `POST /webhooks/donation`; donor role IDs in config.
4. Point `live-feed`/`live-gateway` at #kill-feed/#chat-relay.
5. Replace `rules.rs` RULES; add `/postrules`.

**Manual (Joel / Discord-admin — CANNOT be automated):**
- Re-invite bot with **Manage Roles**: perms int **`275146409984`**
  (`https://discord.com/oauth2/authorize?client_id=1502882213857853580&permissions=275146409984&integration_type=0&scope=bot`).
- Create 4 donor roles + **drag the bot's role ABOVE them** (API can't self-elevate).
- Create the channels (D) + role-gate #donor-lounge.
- Tebex/Ko-fi store with cosmetic-only packages; paste the webhook secret.
- Confirm the canonical public live-map URL (the `?map=official` route — now LIVE as of the 2026-06-08 scummap deploy).
