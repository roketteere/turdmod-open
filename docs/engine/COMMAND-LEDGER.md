# SCUM admin-command ledger (`#` commands) — records

The authoritative, **live** catalog of every SCUM admin verb + our tested-vs-not status. This is the
"we know exactly what we built and what's tested" record Joel asked for.

## Files
- **`admin-commands-catalog.json`** — the raw live catalog, pulled from the running OVH server via the
  bridge handler `dumpAdminCommands` (walks the game's `AdminCommand*` CDOs). **231 verbs**, each with
  `{verb, class, description, numRequired, args:[{name, description, dataType, completionClass,
  completionValues}]}`. This is the game's own truth — not a guessed list.
- **`command-ledger.json`** — generated from the catalog; one row per verb with a `status`.

## Status values
| status | meaning |
|---|---|
| `cataloged` | exists in the live game catalog (baseline for all 231) |
| `verified` | we ran it and **confirmed the effect in-game** |
| `blocked` | tried, **permission-tier rejected** — needs the bypass (below) or `elevated_users` |
| `deferred` | known/decided not to pursue |

Currently verified: `SetGodMode`, `SetTimeOfDay`. Everything else is `cataloged` (exists, untested by us).

## How to refresh the catalog (after a SCUM update — verbs can change)
On OVH (or local), with the engine running:
```
POST localhost:9090/engine/rpc   {"method":"dumpAdminCommands"}   (Bearer token from service.json)
```
Save the `.commands` array to `admin-commands-catalog.json`, then regenerate `command-ledger.json`.

## The permission-tier bypass (for `blocked` verbs)
Some verbs (e.g. `SetStamina`) reject at the **Admin** tier ("not authorized"). Two doors:
1. **`runTestAdminCommand`** bridge handler → `MiscStatics::Test_ProcessAdminCommand`, which **bypasses
   the production admin-auth check**. Needs a connected PlayerController for WorldContext.
2. **SCUM.db `elevated_users`** table — grants the Elevated tier (the real dev-command unlock).

See `~/.claude` memory `reference_turdmod_admin_catalog` + `reference_spa_and_developer_gate`.

## Next
Wire this ledger into turdmod-manager (a "Commands" view) so verifying a verb in-game flips its status
in the record — closing the loop between "what exists" and "what we've proven."
