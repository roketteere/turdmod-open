# scummap Discord Bot — /shop Command Integration

## Overview

The scummap Discord bot at `C:\Development\claude\scummap\apps\bot\` needs a
`/shop` command that lets players browse and buy items from Discord, using the
same wallet and catalog as the in-game `!shop`/`!buy` commands.

## New Discord Slash Commands

### /shop browse [category]
- Lists items from the item catalog with short codes and prices
- Paginated embeds (10 items per page, with Next/Prev buttons)
- Category filter: wep, mel, amm, clo, med, fod, etc.
- Shows item icon from `marketplace_items.icon_url` if available

### /shop buy <code> [quantity]
- Calls `POST /api/internal/shop/purchase`
- Same business logic as in-game `!buy`
- Deducts from the player's scummap wallet
- Creates a delivery order (pending until spawnItem ships)
- Requires Steam link (player must have linked via `/link-steam` first)

### /shop price <code>
- Shows item detail embed: name, category, price, spawn code, rarity

### /shop orders
- Lists pending deliveries for the player
- Status: queued / spawned / failed

### /shop search <query>
- Fuzzy search by item name across all categories
- Returns top 10 matches with short codes

### /link-steam <steam64>
- Links the player's Discord account to their Steam ID
- One-time operation; stored in `steam_user_links` table
- Required before `/shop buy` or in-game economy features work

## API Endpoints Needed (scummap api server)

These go in `C:\Development\claude\scummap\apps\api\src\routes\internal\`:

### steam-links.ts
- `GET /api/internal/steam-links/:steamId` → `{ userId, discordId }`
- `POST /api/internal/steam-links` → `{ steamId, discordUserId }`
- `DELETE /api/internal/steam-links/:steamId`

### shop.ts
- `GET /api/internal/shop/catalog?guildId=...&category=...&limit=25&offset=0`
- `GET /api/internal/shop/item/:shortCode?guildId=...`
- `POST /api/internal/shop/purchase` → `{ guildId, userId, shortCode, quantity }`
- `GET /api/internal/shop/orders?guildId=...&userId=...&status=pending`

## Database Changes Needed

### Migration: add short_code to marketplace_items
```sql
ALTER TABLE marketplace_items ADD COLUMN short_code text;
CREATE UNIQUE INDEX marketplace_items_short_code_idx ON marketplace_items(short_code);
```

### Migration: steam_user_links table
```sql
CREATE TABLE steam_user_links (
  steam_id text PRIMARY KEY,
  user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  linked_at timestamptz NOT NULL DEFAULT now(),
  linked_via text NOT NULL DEFAULT 'manual'
);
CREATE INDEX steam_user_links_user_idx ON steam_user_links(user_id);
```

## Implementation Notes

- The item catalog JSON (`data/turdmod-companion/economy/item-catalog.json`)
  is the source for short codes. Run `scripts/build-item-catalog.mjs` to
  regenerate after SCUM updates.
- The same catalog should seed the `marketplace_items.short_code` column
  in Postgres via `scripts/assign-short-codes.mjs` (to be created in scummap).
- The bot command files go in `scummap/apps/bot/src/commands/shop.ts` and
  `scummap/apps/bot/src/commands/link-steam.ts`.
- The API routes reuse existing wallet transaction patterns from
  `scummap/apps/api/src/routes/internal/wallet.ts`.
