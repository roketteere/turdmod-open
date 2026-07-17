# @turdmod/turdmod-core

ServerAdapter abstraction over the three TurdMOD deployment routes.

## What this package is

A `ServerAdapter` interface plus three implementations:

| Adapter | Used by | Talks to |
|---|---|---|
| `LocalFsAdapter` | Admin / Pro (local mode) | Local filesystem + optional RCON |
| `RemoteFtpAdapter` | Lite | Remote SCUM host via FTP/SFTP + RCON |
| `EngineRpcAdapter` | Admin / Pro (engine mode) | Named pipe → UE4SS engine bridge |

All three apps consume the same interface. UI code calls
`adapter.readFile(...)`, `adapter.runRcon(...)`, etc., and the right
underlying transport is chosen by the configured adapter.

## What this package is NOT

- **It is not a Tauri plugin.** Each app's Tauri Rust backend provides
  the actual implementations of the operations the adapters dispatch
  to. This package just defines the TS-side contract.
- **It is not Tauri-version-pinned.** Each adapter takes the host
  app's `invoke` and `listen` functions in its config so this package
  stays compatible across Tauri 1 / 2 / future.
- **It does not yet replace Manager's existing wiring.** Manager's
  current callsites in `src-tauri/src/*` still talk directly to FTP /
  RCON / engine_rpc. Migrating them to consume `ServerAdapter` is a
  separate task — this package is the scaffolding the migration lands
  against.

## Tier model

Two technical tiers — see `docs/architecture.md` in the repo root.

- **Lite** — FTP for config push, RCON for live admin. Works on any
  managed SCUM host that exposes `/SCUM/Saved/`. ~70% of mod surface.
- **Engine** — UE4SS + `TurdMODEngineBridge.dll` in-process via named
  pipe. Requires own-the-binaries hosts (VPS / dedicated). ~100% of
  mod surface.

Check `adapter.capabilities` before calling engine-only methods
(`engineRpc`, `subscribeEvents`). Lite adapters throw
`UnsupportedOperationError` on those.

## Status

Scaffolded 2026-05-18. Interface stable; implementations have TODO
markers for Tauri commands that don't exist in Manager yet (e.g.
`manager_server_list_remote_files`). Adding those is part of the
migration work.

## License

MIT — same as the rest of the open-source TurdMOD core. PRs welcome.
