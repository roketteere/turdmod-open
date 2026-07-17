# TurdMOD — architecture & tier model

> Decided 2026-05-17 PM after the G-Portal FTP probe confirmed
> managed SCUM hosts gate `/SCUM/Binaries/Win64/` behind their
> control panel (no write access), making UE4SS + the engine bridge
> impossible on those hosts.

## TL;DR

TurdMOD ships **three frontend apps** plus shared infrastructure, all in
one pnpm monorepo. Each app targets a different user. The open foundation
builds trust + ecosystem; the closed products + services capture revenue.
This is the **open-core model** — same shape as GitLab, Sentry,
Mattermost, Supabase, n8n.

## The three frontends

| App | Audience | Route to server | Open/closed | Price |
|---|---|---|---|---|
| **TurdMOD Admin** (`apps/turdmod-manager/`) | Joel only — internal Super Admin | Everything | Closed | n/a |
| **TurdMOD Pro** (`apps/turdmod-pro/`, planned) | Paying customers, modded community owners | UE4SS + engine bridge (in-process RPC) | Closed | ~$9.99/mo (Stripe) |
| **TurdMOD Lite** (`apps/turdmod-lite/`, planned) | Managed-host server owners | FTP + RCON (no engine) | Open (MIT) | Free / freemium |

A single CLI (`apps/turdmod-cli/`, open) covers the same operations
as the GUI apps in script form, with tier-aware gating.

## The two technical tiers

### Soft tier (Lite + managed hosts)

```
TurdMOD Lite (admin's box)
     │
     ├── FTP → /SCUM/Saved/Config/  (ServerSettings.ini, Notifications.json,
     │                                EconomyOverride.json, RaidTimes.json,
     │                                AdminUsers.ini, BannedUsers.ini, ...)
     │
     ├── FTP read → /SCUM/Saved/Logs/  (chat events, deaths, joins)
     │
     └── TCP → RCON port  (#announce, #listplayers, #kick, #ban,
                            #setteleport, #spawnitem, #spawnvehicle)
```

**Coverage:** ~70% of practical mod surface.

**Works on:** G-Portal, Nitrado, Host Havoc, Survival Servers, GTX,
PingPerfect, any managed SCUM host that exposes `/SCUM/Saved/` over FTP
and opens an RCON port. Also works against engine-tier servers
(it's a strict subset of what Pro can do).

### Engine tier (Pro + own-the-binaries hosts)

```
TurdMOD Pro (admin's box, anywhere)
     │
     └── Named pipe over TCP-tunneled SSH (or LAN)
            │
            ▼
   ┌───────────────────────────────────────┐
   │ Remote VPS / dedicated (admin owns)   │
   │                                       │
   │  GameServer.exe                       │
   │   └── UE4SS injected at startup       │
   │        └── TurdMODEngineBridge.dll    │
   │             ├── named pipe RPC server │
   │             ├── PolyHook2 ProcessEvent│
   │             ├── handler registry      │
   │             │    (broadcastChat,      │
   │             │     getOnlinePlayers,   │
   │             │     dumpAllClasses,     │
   │             │     describeWidget,     │
   │             │     ~14 handlers today) │
   │             └── live event emitter    │
   └───────────────────────────────────────┘
```

**Coverage:** 100% of mod surface.

**Requires:** A host where the admin owns `Binaries/Win64/` —
self-managed VPS, dedicated server, or bare metal. **Cannot be
installed on G-Portal, Nitrado, or any other managed SCUM host.**

**Engine-only features (the 30% Lite can't reach):**
- Custom UMG widget overlays drawn directly inside the running game
- Real-time reflection queries (live class/property inspection)
- Per-frame `ProcessEvent` hooks (subscribe to in-game events)
- Custom RPC / UFunction invocations from the app
- Runtime Survival Tips replacement
- Live event streams (chat hooks, kill events, vehicle spawns)

## The shared packages

All three apps compose the same building blocks. Open-sourcing these
is how third-party tools can interop with TurdMOD.

| Package | Open/closed | What it provides |
|---|---|---|
| `packages/turdmod-core/` (planned) | Open | `ServerAdapter` interface with three implementations: `LocalFsAdapter`, `RemoteFtpAdapter`, `EngineRpcAdapter`. Types, RCON client, FTP client. All three apps consume this. |
| `packages/turdmod-ui/` (planned) | Open | Shared React components — Server Settings editor (250 keys), Notifications editor, Admin/Ban list editor, etc. Each app composes them; tier gating applies at the page level. |
| `packages/turdmod-api/` | Open | RPC wire protocol spec — the JSON shape every bridge handler accepts and returns. TypeScript + Rust bindings. |
| `packages/turdmod-manifest/` | Open | Mod manifest format spec — what a `turdmod-mod.json` looks like. Used by the marketplace + all three apps for install/update flows. |

## The engine bridge

`apps/turdmod-engine-bridge/` is the C++ DLL that lives inside
`GameServer.exe` (loaded by UE4SS). It is the **only piece that
actually has access to SCUM's running game state** — everything Pro
does ultimately calls a handler in this DLL.

**Open by design.** Server admins won't run a closed C++ DLL that
hooks engine internals in their game process (anti-cheat concerns,
trust deficit). Opening it is non-negotiable for adoption.

Current handler surface (as of 2026-05-17):
- `ping`, `bridgeReady` event
- `broadcastChat`, `sendChat`, `runAdminCommand`,
  `runTestAdminCommand`
- `teleportPlayer`, `spawnVehicle`, `getOnlinePlayers`
- `dumpUFunctions`, `findFunctions`, `dumpClasses`, `describeFunction`
- `dumpWidgets`, `describeWidget`
- `dumpAllClasses`, `dumpAllEnums`, `dumpAllStructs` (Phase A — scumdump pipeline)

Adding handlers is the most common way to expand the engine's surface.
PRs welcome (and there's a clear pattern — see `handle_dump_widgets`
as the template).

## scumdump (separate repo)

`C:/Development/Claude/scumdump/` — extraction pipeline that runs
once per SCUM build to produce a **~610 MB on-disk database** of
SCUM's complete type system + game content:

- Phase A: reflection (classes/enums/structs) via the bridge — 15 MB JSON
- Phase B: full C++ SDK via Dumper-7 — 543 MB
- Phase C: pak content (widgets/datatables/locres) via CUE4Parse — 51 MB

Used by Pro + Admin for offline class lookups, mod authoring,
type-checked code generation. Open source (MIT).

## Closed: marketplace + hosted services

`apps/turdmod-web/` (turdmod.com) handles:
- Stripe-backed Pro subscription billing
- Paid mod marketplace (commission on author sales)
- Discord/GitHub OAuth login
- Cloudflare R2 mod artifact distribution

Hosted services (cloud sync, log search, analytics) live behind
turdmod.com. These are TurdMOD's recurring revenue streams.

## Revenue model

1. **TurdMOD Pro subscription** (~$9.99/mo) — the paid app
2. **Marketplace commission** on third-party paid mods
3. **Hosted services** (cloud sync, log search, multi-admin tools)
4. **Premium first-party mods** Joel authors and sells directly
5. **Enterprise / community-host white-labeling** licenses

## Recommended deployment for Engine-tier servers

See [`docs/turdmod/server-sizing.md`](./turdmod/server-sizing.md) (TBD)
or the project memory `[[turdmod-server-sizing]]`. Short version:

- **Recommended pop cap: 48 players**
- **Recommended host: OVH RISE-1** (Xeon-E 2386G @ 4.7 GHz, 32 GB ECC,
  2× 512 GB NVMe, Hillsboro/HIL1), ~$85/mo with Windows Server 2022
- Engine-tier overhead leaves ~25% CPU/RAM headroom for bridge work
  + future mod expansion
- 60+ players causes SCUM's UE4 server to struggle regardless of
  hardware

## Why this works

- **Open foundation drives adoption** — SCUM modding subreddit /
  Discord shares + recommends an MIT-licensed bridge. Free marketing.
- **Closed product captures revenue** — Pro is what people pay for.
  Premium feel, multi-server, real-time everything.
- **Ecosystem lock-in via open protocol** — once third-party tools
  depend on `turdmod-api` and `turdmod-manifest`, TurdMOD becomes
  the standard. We're "Stripe for SCUM modding."
