# turdmod-guard

The anti-cheat layer for TurdMOD-enabled SCUM private servers.

We turn BattlEye off so TurdMOD can work — that means **we** become the layer that keeps the server fair. This project is that layer: a server-side detection daemon that ingests events from the companion, runs them through pluggable detectors, and emits flags (warnings, kicks, ban-recommendations) to admins.

## Why this is a separate project

* **Different audience.** Companion is for mod authors + server-side mod hosting. Guard is for server admins worried about cheaters.
* **Different runtime profile.** Guard is long-running, low-throughput, alert-driven. Companion is burst-throughput log-tailing + script dispatching.
* **Different risk model.** Guard's bugs cause false bans (a player loses access). Companion's bugs cause a mod to misfire. Treat guard's correctness bar as higher.
* **Different distribution.** Many server admins will run guard who never write a single mod. Bundle separately so they can install one without the other.

## What guard detects (today + planned)

Detectors are pluggable. Each one has:

* a **name** (so reports are categorisable),
* a **subscribe list** of log/event channels,
* an **`evaluate(event, context)`** function that returns `Ok | Warn | Flag` with a reason.

Started detectors:

| Detector | What it catches | Status |
|---|---|---|
| `kill_distance` | A PvP kill where the reported distance exceeds the maximum effective range of the reported weapon (e.g. AK-47 hit @ 800m, suspicious; sniper hit @ 800m, fine) | filed `#196` |
| `speed` | Player position deltas across login/logout/admin events that imply velocity > sprint+vehicle | filed `#197` |
| `login_burst` | Single IP / steam-id-pool spawning multiple new accounts in a short window (alt-stuffing) | filed `#198` |
| `teleport` | Position jumps without a corresponding `Command: 'Teleport ...'` in the admin log | filed `#199` |

Planned later:

* `weapon_attack_rate` — fire-rate exceeds full-auto cyclic for the weapon
* `kill_streak_pattern` — many kills in rapid succession from the same player, all-headshot
* `inventory_anomaly` — items appear without trader / loot / craft origin events
* `client_integrity_mismatch` — TurdMOD loader's reported mod manifest doesn't match the server's authorized list
* `save_file_diff` — sqlite save-file edits between server stops (server admin tampered with the save manually)
* `network_objects_log_diff` — when `scum.EnableNetworkObjectLogging=True`, cross-validate spawn events vs. observed positions

## Architecture

```
┌──────────────────────┐         events            ┌─────────────────────┐
│  turdmod-companion   ├──────────────────────────►│   turdmod-guard     │
│  (log-tail + dispatch)        (HTTP / WS / unix)  │   (detection)       │
└──────────────────────┘                            └──────┬──────────────┘
                                                           │ flags
                                                           ▼
                                                    ┌─────────────────────┐
                                                    │   guard backend     │
                                                    │   (REST + DB)       │
                                                    │   ban-list, history │
                                                    └─────────┬───────────┘
                                                              │
                                                              ▼
                                                    ┌─────────────────────┐
                                                    │   admin dashboard   │
                                                    │   (apps/web)        │
                                                    └─────────────────────┘
```

* **Detection daemon** (this crate, Rust). Subscribes to the companion's event stream, applies detectors, emits flags. Runs as a Windows service or under tmux on Linux.
* **Backend** (`backend/`, Hono + Postgres). Receives flag reports, stores history, serves the ban-list cross-server. Servers can opt-in to a shared ban-list — a player banned on one TurdMOD server can be optionally checked against the shared list on others.
* **Admin dashboard** — lives under `apps/web/admin/guard/` (lands when guard reaches MVP). Read-only feed of flags, action buttons (warn / kick / ban), per-player history.

## Why not BattlEye?

Joel's call: TurdMOD's whole point is to be modder-friendly on private servers, and BE refuses to coexist with our scripting. We're not trying to replace BE for official servers — we're trying to give private-server admins a fair-play tool that **works alongside the mods they actually want to run**.

This is the same trade-off Rust modded servers make — they run alt-anti-cheat tooling like FacePunch's umod-friendly checks rather than EAC's hard-line stance. We're in that lineage.

## What guard explicitly DOESN'T do

* No memory scanning of the player's PC. We're not a kernel-level anti-cheat.
* No process enumeration on the client beyond what the loader's existing detection layer already does.
* No network packet inspection of the SCUM client's traffic.
* **No automatic bans.** Detectors emit flags; humans decide. False positives in this kind of detection are real, and a ban-by-bot is the fastest way to lose community trust.

## Repo layout

```
apps/turdmod-guard/
├── README.md          (this file)
├── Cargo.toml         <- the daemon (Rust)
├── src/
│   ├── main.rs        <- entrypoint; loads config, subscribes, dispatches
│   ├── ingest.rs      <- reads events from the companion
│   ├── reporter.rs    <- emits flags to the backend
│   ├── config.rs      <- parses guard.toml
│   └── detectors/
│       ├── mod.rs     <- detector trait + registry
│       ├── kill_distance.rs
│       ├── speed.rs
│       ├── login_burst.rs
│       └── teleport.rs
├── backend/           <- the central report + ban-list API (Hono)
│   ├── package.json
│   └── src/index.ts
└── docs/
    └── threat-model.md  (TBD)
```

## See also

* [docs/scum-internals/15-log-files.md](../../docs/scum-internals/15-log-files.md) — the events guard subscribes to.
* [docs/scum-internals/07-vehicles.md](../../docs/scum-internals/07-vehicles.md) — vehicle-related detector context.
* [apps/turdmod-companion/](../turdmod-companion/) — guard's upstream event source.
* `KTask #195` — parent task; sub-tasks `#196`-`#200`.
