# Contributing to TurdMOD

Thanks for your interest. TurdMOD is a mixed open / proprietary
monorepo — please read this before opening a PR.

## What's open (PRs welcome)

| Path | What |
|---|---|
| `apps/turdmod-engine-bridge/` | C++ UE4SS cppmod — the engine itself |
| `apps/turdmod-cli/` | Unified command-line interface |
| `apps/turdmod-bot/` | Discord bot reference implementation |
| `apps/turdmod-loader/` | Client-side UE4SS proxy loader |
| `apps/turdmod-server-loader/` | Server-side loader DLL |
| `packages/turdmod-api/` | RPC wire protocol + bindings |
| `packages/turdmod-manifest/` | Mod manifest format spec |

Sibling project [`scumdump`](https://github.com/roketteere/scumdump)
(separate repo, when published) is also open and welcomes PRs.

## What's NOT open (PRs declined)

| Path | What |
|---|---|
| `apps/turdmod-manager/` | TurdMOD Admin — Joel's private Super Admin tool |
| `apps/turdmod-pro/` (planned) | TurdMOD Pro — commercial paid app |
| `apps/turdmod-web/` | turdmod.com marketplace + payments |

Bug reports and feature suggestions on the closed projects are
welcome via Issues — we just can't accept code PRs against them.

## What's TBD

These directories haven't been licensed yet. PRs are paused until a
LICENSE file lands:

- `apps/turdmod-companion/`
- `apps/turdmod-guard/`
- `apps/turdmod-registry/`

## How to contribute

1. **File an Issue first** for non-trivial changes — saves you time if
   the direction isn't a fit.
2. **Fork + branch** off `main`. Use a descriptive branch name like
   `bridge/add-getplayer-inventory-handler`.
3. **Match existing style.** Run the project's formatter / typecheck
   before pushing:
   - TypeScript: `pnpm typecheck` (Manager / packages)
   - C++ bridge: build clean with the existing CMake setup (see
     `apps/turdmod-engine-bridge/README.md`)
   - Rust loader: `cargo fmt && cargo clippy`
4. **One concern per PR.** Easier to review, easier to revert.
5. **Open a PR** against `main`. Describe what + why. Reference the
   Issue if there is one. CI must pass.

## Bridge handlers — the most common contribution

Adding a new RPC handler to `apps/turdmod-engine-bridge/` is the
most common contribution path. The pattern is well-established —
clone `handle_dump_widgets` (in `src/TurdMODEngineBridge.cpp` as
the template):

1. Declare a `static thread_local std::string s_<name>_result;`
   buffer near the others at the top of the file.
2. Write `static int32_t handle_<name>(const char* params_json,
   const char** result_out, const char**)`.
3. Use the existing helpers — `extract_json_str`,
   `fname_to_wstring`, `fname_to_json_string`,
   `UObjectGlobals::ForEachUObject`.
4. Register the handler in the `regs[]` array near the bottom of
   `on_unreal_init()`.
5. Test against a running SCUM server before opening the PR.

## Code of conduct

Be kind. Assume good faith. Modded-gaming communities have a wide
mix of skill levels — onboarding new contributors helps more than
gatekeeping.

## License

Contributions to open-source paths are licensed under MIT — the
same license as the receiving subdirectory. By submitting a PR you
agree to license your contribution under MIT.
