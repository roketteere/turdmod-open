# `@turdmod/turdmod-companion`

Log-tail-based server-side mod host. Watches SCUM server logs, fires
mod hooks on chat / login / death / kick events, and proxies admin
actions back into the server via RCON. **Stopgap until the engine
loader ships** — the engine-tier bridge (`turdmod-engine-bridge`) is
the long-term path; Companion remains useful for managed hosts where
in-process modding isn't an option.

## Run locally

```powershell
cd apps\turdmod-companion
pnpm dev
```

For a quick smoke test against a running SCUM server:

```powershell
pnpm demo
```

## Build / start / test

```powershell
pnpm build       # tsc → dist/
pnpm start       # node dist/...
pnpm test        # vitest
pnpm typecheck   # tsc --noEmit
pnpm verify      # full pre-flight (typecheck + test + smoke)
```

## Preconditions

- pnpm installed (workspace).
- A reachable SCUM server (local or remote) with:
  - Read access to its log files (`SCUM.log`, `RCON.log`, etc.).
  - RCON enabled + credentials available.
- Configuration via env vars or local config file — see
  `src/config.ts` for the schema.

## How it fits

- Companion talks to the bridge over named-pipe RPC when running
  alongside the engine loader.
- For Lite-tier (managed-host) deployments where in-process injection
  isn't possible, Companion stands alone and uses RCON only.

## License

MIT.
