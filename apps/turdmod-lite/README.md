# TurdMOD Lite

> Free, open-source SCUM admin client for managed hosts.

## What this is

The Lite tier of TurdMOD. Works on any SCUM host that exposes
`/SCUM/Saved/` over FTP/SFTP and opens an RCON port — G-Portal,
Nitrado, Host Havoc, Survival Servers, GTX, PingPerfect, etc.

Lite gives you the **soft tier** of TurdMOD's surface (~70% of what
SCUM admins actually do):

- Edit + push `ServerSettings.ini` (~250 keys)
- Edit + push `Notifications.json` (banner mod)
- Edit + push `EconomyOverride.json` (trader pricing)
- Edit + push `RaidTimes.json` (PVP windows)
- Manage admin / ban / whitelist user lists
- Run RCON: `#announce`, `#listplayers`, `#kick`, `#ban`,
  `#setteleport`, `#spawnitem`, `#spawnvehicle`
- Tail server logs (chat events, kills, joins/leaves)

For the remaining 30% — custom widgets, real-time reflection,
UFunction calls, custom RPC — you need own-the-binaries hosts and
[TurdMOD Pro](https://turdmod.com/pro) (engine tier).

## Status

**Scaffold — not yet shippable.** 2026-05-18. Currently just the
empty Tauri shell + nav structure. Functionality is being ported
from the Admin codebase. Track progress in `IDEAS.md` at the repo
root.

## Stack

- Tauri 2 (Rust backend + WebView frontend)
- React 19 + Vite + TanStack Query + Tailwind
- `@turdmod/turdmod-core` for the `ServerAdapter` abstraction
- `russh-sftp` for SFTP, native TCP for RCON

## Develop

```sh
cd apps/turdmod-lite
pnpm install
pnpm tauri:dev
```

Vite dev server runs on port 5174 (Manager uses 5173).

## License

MIT. Source: <https://github.com/roketteere/turdmod>.

PRs welcome — see `CONTRIBUTING.md` at the repo root for the
contribution guidelines.
