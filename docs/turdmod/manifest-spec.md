# TurdMOD manifest spec v1

The canonical schema every mod author writes (`turdmod.json`) and every loader /
registry / CLI reads. Implemented in `packages/turdmod-manifest/src/index.ts`
with Zod; that module is the source of truth — this doc is the human-readable
companion.

## Files

A mod has two related shapes:

- **Manifest** (`turdmod.json`) — what the author writes. Lives at the root of a
  mod project; published into a built pak as the asset metadata.
- **Sidecar** (`<id>.turdmod.json`) — what `turdmod install` writes next to a pak in
  `<SCUM>/Content/Paks/~mods/`. A sidecar is a manifest plus install-time
  fields (`installedAt`, `sourcePath`).

The CLI accepts either where it makes sense; the registry expects manifests;
the loader checks sidecars at load time.

## Schema (v1)

```jsonc
{
  // Schema version. Starts at 1; bumped only on breaking changes.
  "schema": 1,

  // Required identity ----------------------------------------------------
  "id":      "my-loot-tweak",          // kebab-case, [a-z][a-z0-9-]*[a-z0-9]
  "name":    "Better TEC1 Loot",
  "version": "1.0.0",                  // X.Y.Z[-tag]
  "mode":    "pak-content",            // pak-content | server-side | offline-only | external-tool

  // Optional metadata ----------------------------------------------------
  "author":      "TechyRican",
  "description": "Higher Memory Module drop rates in TEC1 abandoned bunkers.",
  "homepage":    "https://github.com/me/my-loot-tweak",
  "tags":        ["loot", "tec1"],

  // Compatibility window. Empty maxBuild = no upper bound.
  "minBuild": "23128448",
  "maxBuild": "",

  // Other mods this one depends on (id -> version range string).
  "dependencies": {},

  // Sandbox capabilities (see compatibility-policy.md).
  "capabilities": [
    { "filesystem": "read" },
    { "network": "localhost" }
  ],

  // Mode-specific entrypoint hint (Lua file path, server module, etc.).
  "entrypoint": "scripts/main.lua"
}
```

A **sidecar** is the same shape with two additional install-time fields:

```jsonc
{
  // ...all manifest fields above, plus:
  "installedAt": "2026-05-09T00:23:35.868Z",
  "sourcePath":  "C:\\path\\to\\original.pak"
}
```

## Field reference

| Field | Type | Required | Notes |
|---|---|---|---|
| `schema` | `1` | yes (defaults to 1) | Bump only on breaking changes |
| `id` | string | yes | `^[a-z][a-z0-9-]*[a-z0-9]$`, length 2–64 |
| `name` | string | yes | Display name; 1–120 chars |
| `version` | string | yes | `\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?` |
| `mode` | enum | yes | `pak-content` / `server-side` / `offline-only` / `external-tool` |
| `author` | string | no | 1–120 chars |
| `description` | string | no | up to 2000 chars |
| `homepage` | URL | no | Standard URL validation |
| `tags` | string[] | no | Up to 16, each ≤40 chars |
| `minBuild` / `maxBuild` | string | no | SCUM buildId (`\d{4,12}`) or empty |
| `dependencies` | object | no | `{ "<other-id>": "<version>" }` |
| `capabilities` | array | no | See "Capabilities" below |
| `entrypoint` | string | no | Mode-specific path; ≤512 chars |
| `installedAt` | ISO datetime | sidecar only | Stamped by `turdmod install` |
| `sourcePath` | string | sidecar only | Pak source path for audit |

Unknown top-level fields are **rejected** (strict schema). Add new fields by
bumping `schema` to 2 and shipping a migrator.

## Capabilities

Declared up front so the runtime sandbox knows what to allow. Currently:

```jsonc
[
  { "filesystem": "none" | "read" | "readwrite" },
  { "network":    "none" | "localhost" | "any" },
  { "process":    "none" }
]
```

`pak-content` and `external-tool` mods don't need capabilities (they don't
run in the TurdMOD sandbox). `server-side` and `offline-only` mods MUST declare
their capabilities — the loader denies any operation outside the declared
set.

## Compatibility helpers

Implemented as plain functions in the package:

```ts
import { isCompatibleWithBuild, isModeAllowedInEnvironment } from "@turdmod/turdmod-manifest";

isCompatibleWithBuild({ minBuild: "23128448", maxBuild: "" }, "23200000"); // true
isModeAllowedInEnvironment("offline-only", "private-be-on");               // false
```

`Environment` values: `solo` / `private-be-off` / `private-be-on` /
`official`. Mirrors the matrix in `docs/turdmod/compatibility-policy.md`.

## CLI

Validate any sidecar or manifest from the command line:

```bash
turdmod validate <mod-id>                  # validates the installed sidecar
turdmod validate path/to/turdmod.json         # validates an arbitrary file
```

The CLI tries the sidecar schema first (because installed mods always have
`installedAt`); if that fails it tries the plain manifest schema (a published
mod's `turdmod.json`).

## Versioning policy

- v1 is **frozen** as of 2026-05-08. Adding optional fields without changing
  semantics is allowed within v1 (no schema bump).
- Breaking changes (renaming a field, tightening validation, changing
  semantics of an existing field) → bump `schema` to 2 + migrator.
- The loader/CLI must accept any `schema` value it understands and refuse
  any value it doesn't, with a clear error pointing at this doc.

## See also

- `packages/turdmod-manifest/` — implementation + tests
- `docs/turdmod/compatibility-policy.md` — when each `mode` is allowed
- `docs/turdmod/battleye-safety.md` — runtime safety for `offline-only`
