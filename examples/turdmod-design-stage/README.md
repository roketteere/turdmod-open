# TurdMOD design-stage examples

These are reference mods for delivery modes whose host runtime isn't yet
wired up end-to-end. They validate the manifest format and demonstrate
the API call shape; the runtime work needed to actually run them is
tracked on the kanban.

For mods you can run **today**, see [`../turdmod/`](../turdmod/) — the
production-shape `server-side` examples on the companion runtime.

## What's here

| Folder | Mode | Blocked on | Demonstrates |
|---|---|---|---|
| `better-tec1-loot/`     | `server-side` (Lua) | Lua-on-companion runtime (Fengari/embedded interpreter) | `content.setDataTableOverride` — bumps Memory Module spawn caps in TEC1 abandoned-bunker chests. The TS companion runtime is JS-only today; this example is for when we add a Lua interpreter or port the pattern to TS. |
| `radial-quick-actions/` | `offline-only` | Loader DLL Lua VM end-to-end (KTask #168 / #169) | `ui.bindHotkey` + `ui.showMenu` + `ui.toast` — F4-bound radial menu of common actions. Runs *inside* the SCUM game client via the loader DLL; refuses to inject when BattlEye is on. |
| `world-snapshot/`       | `offline-only` | Loader DLL Lua VM + `engine.actor-enumerate` capability | Hotkey to dump every actor in the running SCUM world to a JSON file — the first-ever complete world inventory. |

## Why they live here, not in `examples/turdmod/`

The companion silently skips mods it doesn't host (any mode other than
`server-side`), but for the contract-deliverable view we want
`examples/turdmod/` to be the *runs-today* set. Mods that depend on
runtime work still in flight stay here so the dossier-shape examples
dir doesn't carry skipped warnings or design-stage stubs.

When the loader DLL Lua runtime is verified end-to-end, the
`offline-only` examples graduate to the main directory.
