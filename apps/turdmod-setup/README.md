# TurdMOD Setup

The guided installer. It finds your SCUM server, tells you honestly what your hosting can
actually run, installs everything, and verifies it worked — with a built-in AI assistant that
can perform the whole install for you.

Built because "read the docs, copy four DLLs into three folders, hand-edit `service.json`" was
losing people. Half of the support load turned out to be operators on rented FTP-only hosts who
*cannot* run the engine at all — this app tells them that in 30 seconds instead of after an hour.

## What it does

1. **Asks where the server lives** — this PC, your own VPS, rented from a game host, or not sure.
2. **Finds it** — Steam registry + `libraryfolders.vdf`, with manual browse as fallback.
3. **Reports capability honestly** — engine mods / pak mods / config tuning / dashboard, each
   marked yes/no/maybe with a plain-language reason. Nothing is promised that can't be delivered.
4. **Generates the config** — paths and access token, no hand-editing.
5. **Installs** — places the artifacts, writes `service.json`, installs and starts the Windows
   Service. Stops before the service step if any file copy failed.
6. **Verifies** — service `/health` → game server running → engine bridge ping. Each failing
   check comes with a specific fix; downstream checks report "skipped", never a false green.

### The hard constraint

The engine runs *inside* the game server process (Windows Service + DLL injection). So:

| Host | Engine mods | What Setup does |
|---|---|---|
| This PC | Yes | Full install |
| Your own VPS / dedicated | Yes | Tells you to run Setup on that box (see Limitations) |
| Rented game host (FTP + web panel) | **No** | Explains why, and what still works |

## The AI assistant

Optional side panel. Pick a provider, paste a key, and it can drive the same Tauri commands the
UI buttons do — there is no separate "agent path" that could drift from a manual install.

- **Providers**: Anthropic, OpenAI, DeepSeek, Gemini, or local **Ollama** (free, no key).
- **Billing**: yours. The key is stored in the Tauri store on your machine and is sent only to
  the provider you picked — nothing is proxied through TurdMOD.
- **Safety**: every destructive tool shows a confirm card before it runs. Turn on "let it install
  without asking me" for a hands-off install; it's off by default.

## Run it locally

```powershell
cd apps/turdmod-setup
pnpm install
pnpm tauri dev
```

Frontend-only (no Tauri backend, useful for styling work):

```powershell
pnpm dev          # vite on http://localhost:5190
```

## Build

```powershell
cd apps/turdmod-setup
pnpm tauri build
# → src-tauri/target/release/TurdMOD-Setup.exe
# → src-tauri/target/release/bundle/  (msi + nsis installers)
```

Or as part of a release, which also drops it at the root of the Server Pack zip:

```powershell
.\scripts\package-release.ps1 -WithSetup
```

## Tests / typecheck

```powershell
cd apps/turdmod-setup/src-tauri
cargo test          # detect, capability, install_local, verify

cd apps/turdmod-setup
pnpm build          # tsc typecheck + vite build
```

## Preconditions

- **Windows** — installs a Windows Service and injects DLLs.
- **Rust toolchain** + **MSVC C++ build tools** + **Node 20/pnpm** to build. End users need none
  of that; they run the packaged exe.
- **Run as Administrator** when installing — service installation requires it.
- **Stop the SCUM server** before installing; files can't be replaced while in use.
- The **Server Pack** must be extracted next to `TurdMOD-Setup.exe` (or at `C:\TurdMOD`) so the
  artifacts can be found.

## Limitations (current)

- **Install is local-only.** For a VPS, copy Setup + the Server Pack onto that box and run it
  there. The wizard says so explicitly rather than silently installing onto the wrong machine.
  SSH/SCP and FTP install paths are designed but not built.
- No modded-client install yet — server side only.

## Layout

```
src/
  App.tsx              step router + rail + AI panel toggle
  steps/               Welcome, Detect, Capability, Configure, Install, Verify
  ai/
    providers.ts       multi-provider client with tool-use (Anthropic / OpenAI-compat / Gemini)
    agent.ts           conversation + tool loop, confirm gating, friendly error mapping
    tools.ts           tool defs → the same Tauri commands the UI uses
    AiPanel.tsx        provider sign-in, chat, confirm cards
  lib/
    api.ts             typed invoke bindings
    setup-state.ts     wizard state machine + state summary for the assistant
src-tauri/src/
  detect.rs            Steam scan (ported from turdmod-manager/scum_paths.rs)
  capability.rs        host kind → honest capability matrix
  install_local.rs     token gen, config build, artifact placement, service install
  verify.rs            dependency-ordered health checks with fixes
  lib.rs               Tauri commands
```
