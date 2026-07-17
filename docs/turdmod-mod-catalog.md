# TurdMOD — Mod Catalog

The curated list of mods worth building. Two filters applied:

1. **Available + working today (or buildable on our current loader).** Drop any idea that depends on tech we don't have AND can't build.
2. **Not duplicated in spirit.** If two ideas converge on the same payoff, keep the better-shaped one. Exception: if one is genuinely "one-of-a-kind" enough to be worth shipping alongside, keep both.

Curated against what's actually possible on:
- **Companion (server-side, log-tail driven, no in-game UI)** — works today, BE-agnostic.
- **Loader Layer 2 Lua runtime** — works today, BE-off only.
- **Loader Layer 3 UE4 hooks (#169)** — when this lands, in-game panels + Slate UI become viable.
- **Pak overlay** — see [scum-internals/21-custom-content.md](./scum-internals/21-custom-content.md).

Status legend: `[idea]` = curated, not started · `[building]` = in flight · `[shipped]` = live · `[parked]` = deferred · `[rejected]` = considered and dropped

---

## Server-side / Companion (works today)

These run in the companion process via log-tail. No client install needed for players. Fastest to ship.

* `[shipped]` **KillFeed** — Rust-style death feed. PvP / PvE / suicide / fall classification. → `examples/turdmod/kill-feed/`
* `[shipped]` **MySquad** — squad position tracking, surfaces on the scummap map.
* `[shipped]` **WelcomeScreen v0** — first-join welcome message broadcast (currently to console; Discord webhook + in-game panel pending).
* `[shipped]` **VehicleManager** — vehicle ownership registry, license-plate naming, owner notifications.
* `[idea]` **Server stats panel** — connected players, top killers, vehicle counts, bunker timers — pushed every 5 min to Discord webhook + scummap web tab.
* `[idea]` **Bunker timer bot** — `[LogBunkerLock]` events fan out to a Discord channel: "C4 active in 23 min", "B5 unlocked".
* `[idea]` **AdminAudit** — every admin command from `admin_*.log` posts to a private audit channel. Scrubs accidentally leaked passwords.
* `[idea]` **PlaytimeLeaderboard** — login/logout deltas → per-player hours/week. Slash command `/playtime me` and `/playtime top`.
* `[idea]` **WipeReporter** — when a wipe runs, generate a "season X recap" with kills, vehicles destroyed, bunkers raided.
* `[idea]` **AntiGriefAlerts** — heuristics on login churn / kill streaks / chat keywords flag suspected griefers, ping admins.
* `[idea]` **Fame economy** — points for kills/raids/quests, redeemable in-game via admin-spawned items (server companion drives the spawn via SendInput admin chat).
* `[idea]` **Casino** — `/spin`, `/roulette`, `/blackjack` chat commands; payouts via the existing /balance economy.
* `[idea]` **VoteKick / VoteKill** — `/vote kick <player>` ↔ chat-aggregated; auto-runs admin command if threshold reached.
* `[idea]` **EventScheduler** — companion runs scheduled in-game events (zombie waves, vehicle drops) by issuing admin commands at server-time.
* `[idea]` **MarketBot v2** — already shipped foundation; expand with auto-pricing, alert-on-deal, "I'll buy if anyone lists Y".
* `[idea]` **DiscordPresence** — `/whois <discord-name>` returns linked SCUM player; `/whois <steam>` returns Discord. Identity bridge.

## Client-side / Lua mods on the loader (works today, BE-off)

Run inside SCUM.exe via the loader. Per-player install. No UI yet (waiting on #169) but Lua-side logic is live.

* `[idea]` **WelcomeScreen v1 (in-game)** — once #169 lands, render the welcome panel as a Slate widget with a Dismiss button. Currently logs panel content as a stub.
* `[idea]` **HUD overhaul** — replace vanilla HUD widgets with a denser, configurable variant. Per-player colour theme, per-element opacity.
* `[idea]` **Compass + waypoint** — Rust-style compass at top of screen with N/S/E/W markers and waypoint arrows.
* `[idea]` **Loot scanner ring** — radius display around player showing nearby loot containers (filtered by item type). Toggle hotkey.
* `[idea]` **Vehicle HUD** — speed/fuel/damage gauges in a clean overlay; replaces vanilla's tiny corner widgets.
* `[idea]` **Safezone bubble visualization** — render trader safezones as visible gradient circles (not just the 1-line "you are leaving the safezone" warning).
* `[idea]` **NoTreeFog** — local-only render hack: clip tree-density fog so the player sees further. Cosmetic; doesn't change network state.
* `[idea]` **Inventory tag colours** — items get coloured borders by category (food/medical/weapon/junk). Pure UI overlay.
* `[idea]` **Wide-screen FOV** — push beyond vanilla's FOV slider cap. Settings-only mod, no asset changes.

## Hybrid (companion broadcasts, loader renders)

These are the killer apps once IPC is wired. Companion knows the world state (logs); loader puts it on the player's screen.

* `[idea]` **Live kill ticker** — KillFeed but rendered as an in-game ticker, not a Discord post.
* `[idea]` **Squad map panel** — open with hotkey, shows squad-mates' positions on a sub-map widget. Driven by MySquad's existing data.
* `[idea]` **Bunker countdown HUD** — small clock overlay with the next bunker activation time. Companion has the bunker state; loader paints it.
* `[idea]` **Server announcements** — admins type `/announce <msg>` in companion-side console; appears as a centered, fade-in panel for every connected player who has the loader installed.
* `[idea]` **Vehicle "your car was destroyed" toast** — already a Discord notification via VehicleManager; promote to an in-game toast.

## Content mods — the big swings (need pak overlay + Lua glue)

See [scum-internals/21-custom-content.md](./scum-internals/21-custom-content.md) for how to build these.

* `[idea]` **Puppet Pal — tame puppets into perma-companions** *(KTask #201, flagship)*. Feed a puppet to tame, complete a bonding quest to make it permanent, auto-spawns with you on login. Command wheel (Follow/Stay/Guard/Attack/Scout/Fetch/Sleep), search tasks (`fetch food` → wanders to loot containers and brings back), shared pack-mule inventory, level/bond progression, naming + cosmetic outfit, multiplayer-aware (only the owner sees them as friendly). Tech: UE4 AI-controller hook for behaviour swap, DataTable injection for the tamed-puppet variant, per-player save state, command-wheel widget, chat-command parser. Open design questions: animation set (do tamed puppets keep zombie locomotion or get cleaner walks?), death model (knockdown-revivable vs permadeath toggle), trade interaction (can other players tip your companion?). Worth building because no other SCUM mod has done this and the screenshot/clip potential is unmatched — it's our Skyrim-follower moment.
* `[idea]` **Helicopter** — the headline feature. Custom flight physics, custom mesh, DataTable injection, server config. Detailed walkthrough in the dossier. Long horizon but iconic.
* `[idea]` **Custom Map Builder ladder** *(KTask #206, flagship-tier)*. Four tiers: (1) `turdmod build map` CLI that cooks + paks UE4 projects for SCUM correctly, (2) UE4 editor template with SCUM-specific actors prewired + lint rules, (3) visual web/desktop builder (no UE4 needed), (4) collaborative live builder. Each tier is its own product; ship them in order as community demand justifies the complexity. Tier 1 is the obvious next move once we have any custom-map work shipped.
* `[idea]` **Custom small map (private PvP arena)** — easier than replacing Island; build as a separate `.umap` with hand-placed spawn points and bunkers. Ship as the "TurdMOD Arena" map.
* `[idea]` **Sniper rifle pack** — add 3-5 high-end rifles missing from vanilla. Mostly mesh + DataTable work.
* `[idea]` **Realistic firearm sounds** — replace vanilla weapon SFX with field recordings. Pure asset swap.
* `[idea]` **Vehicle pack — pickup trucks** — Toyota Hilux, Ford F-150 analogues. Mesh-heavy but no flight physics, no new game systems.
* `[idea]` **Crafting expansion** — new craftable items (improvised armour, traps, lockpicks) with recipe DataTable additions and existing gameplay loops.
* `[idea]` **Trader expansion** — new trader NPCs at additional compounds, with custom inventories. Combines NPC blueprint + DataTable injection.
* `[idea]` **Skin pack** — character skins, weapon camos, vehicle paint jobs. Cosmetic-only, easy first-content-mod for new authors.
* `[idea]` **Custom event — Black Market truck** — periodic event where a truck spawns with high-tier loot but draws zombie hordes. Uses EventScheduler + custom NPCs + custom drop tables.

## Quality-of-life — small but loved

* `[idea]` **InventorySearch** — type to filter inventory by item name. Text input widget overlay.
* `[idea]` **AutoSort** — chest auto-sort hotkey. Sorts by category, weight, alphabet.
* `[idea]` **QuickStash** — hotkey: deposit all matching items in nearby chest in one keypress.
* `[idea]` **BetterMap** — map zoom, custom markers, route plotting. Replaces vanilla M-key.
* `[idea]` **TextChatBetter** — bigger chat history, name colours, ping highlight.
* `[idea]` **DeathNotificationLog** — your own deaths with cause + timestamp, accessible from a UI panel.

## Admin / RCON tools (overlap with the existing premium overlay)

* `[idea]` **AdminPanel GUI** — comprehensive in-game admin surface: every SCUM admin command behind buttons + forms (#190 in KTask, expansion). User explicitly wants this big.
* `[idea]` **Quick teleport** — saved waypoints + 1-click teleport to player / coords / bunker.
* `[idea]` **Loot spawner UI** — search items, click to spawn at cursor.
* `[idea]` **Player inspector** — click a player on the map, see inventory / position / kill count / playtime.
* `[idea]` **Time / weather slider** — drag to set time of day / weather. Already exists as command; want a UI.

## Rejected / parked

* `[rejected]` **NoBattlEye launcher** — let players join official servers without BE. Not happening; account-ban risk and we've explicitly scoped to private servers.
* `[rejected]` **AutoAim / radar / wallhack** — cheating tools. Out of scope.
* `[parked]` **Speedrun mod** — global "race to the centre" leaderboard. Cool but niche; revisit after the core surface is stable.
* `[parked]` **VR support** — UE4 VR pipeline is a separate beast; the player count justifies revisiting later.
* `[parked]` **Linux server / Mac client** — out of platform scope.

---

## How to add to this list

1. Append under the right section (don't reorder; date your addition).
2. Status `[idea]` until someone files a kanban task and starts.
3. If the idea overlaps with an existing one, decide: merge, drop, or keep both with notes on what makes them distinct.
4. Don't delete rejected entries — leave them so future-us doesn't re-pitch them.
