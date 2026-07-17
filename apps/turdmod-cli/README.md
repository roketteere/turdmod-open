# `@turdmod/turdmod-cli` — `turdmod`

TurdMOD CLI for managing **pak content mods** (Strategy A in the TurdMOD engine).
Drop a `.pak` into the SCUM `~mods/` folder; this CLI handles install /
enable / disable / vanilla-mode launches, plus a metadata sidecar so the
manager UI knows what each pak is.

For the full TurdMOD architecture, see:

- `docs/turdmod/battleye-safety.md` — BE-safe design + detection layer
- `docs/turdmod/compatibility-policy.md` — when each strategy is allowed

## Install

From the repo root:

```bash
pnpm install
pnpm --filter @turdmod/turdmod-cli build
```

The binary is published to `dist/index.js`. To run during development:

```bash
pnpm --filter @turdmod/turdmod-cli dev -- <command> [...args]
# or, equivalently:
cd apps/turdmod-cli && npx tsx src/index.ts <command> [...args]
```

## Configuration

The CLI needs to know where SCUM's `Content/Paks` directory lives. In order
of precedence:

1. `--paks <path>` flag on any command
2. `TURDMOD_PAKS_DIR` env var
3. `SCUM_PAKS_DIR` env var (shared with the extractor pipeline)
4. Auto-detection of the standard Steam install path on Windows

If none of these resolve, the CLI exits with a clear error.

## Commands

The CLI is namespaced by delivery strategy. Today only `pak` (Strategy A) is
implemented; `scripting` (C), `server` (B), and `tool` (D) are reserved
top-level slots that print a "not implemented yet" stub.

```
turdmod pak list                        # show installed mods (active + disabled)
turdmod pak install <pak> [--id ...]    # copy a .pak into ~mods/ + write sidecar
turdmod pak uninstall <id> --force      # remove pak + sidecar
turdmod pak enable <id>                 # move from ~mods.disabled/ to ~mods/
turdmod pak disable <id>                # move from ~mods/ to ~mods.disabled/
turdmod pak vanilla                     # move ALL active mods aside (safe for official servers)
turdmod pak modded                      # restore previously disabled mods
turdmod pak status                      # current mode (Vanilla/Modded) + counts
turdmod pak validate <id-or-path>       # validate a sidecar / manifest JSON
turdmod pak watch [--json]              # tail add/remove/enable/disable events
```

The pre-namespace forms (`turdmod install`, `turdmod status`, etc.) still
work as hidden aliases — they print a one-line deprecation note that points
at the new shape and then run.

`turdmod pak install` accepts these metadata flags (all optional, recorded
in the sidecar):

```
--id <id>            override the mod id (default: pak filename without .pak)
--name <name>        display name
--version <ver>      semver string (default: 0.0.1)
--author <author>    author handle
--description <desc>
--min-build <id>     min compatible SCUM buildId
--max-build <id>     max compatible SCUM buildId
```

## On-disk layout

```
<SCUM>/Content/Paks/
  ~mods/                      # active mods (UE 4.27 auto-loads at startup)
    my-loot-tweak.pak
    my-loot-tweak.turdmod.json
  ~mods.disabled/             # disabled mods (parked here, ignored by the game)
    cosmetic-pack.pak
    cosmetic-pack.turdmod.json
```

The sidecar is a small JSON file:

```json
{
  "id": "sample-mod",
  "name": "Sample Mod",
  "version": "1.0.0",
  "author": "YOUR_OWNER_NAME",
  "mode": "pak-content",
  "description": "...",
  "minBuild": "23128448",
  "installedAt": "2026-05-09T00:23:35.868Z",
  "sourcePath": "C:\\path\\to\\original.pak"
}
```

## Vanilla vs Modded launch

Per TurdMOD's compatibility policy, pak mods MUST NOT be active when
connecting to an official Gamepires server. The CLI provides two bulk-move
commands:

- `turdmod pak vanilla` — moves every active mod into `~mods.disabled/`.
  Safe to connect to official servers afterward.
- `turdmod pak modded` — restores everything in `~mods.disabled/` back to
  `~mods/`.

A future launcher (the manager UI, KTask #150) will wrap these into a
single-click "Launch Vanilla" vs "Launch Modded" choice and warn at three
points (before activation / at launch / post-connect detection).
