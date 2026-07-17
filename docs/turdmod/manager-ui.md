# TurdMOD manager UI — design

How the player-facing mod manager looks and behaves. Two surfaces:

- **Web browser** (read-only) at `apps/web/src/app/turdmod/page.tsx` — live
  today against the registry (mock data when `TURDMOD_REGISTRY_URL` isn't
  set). Shipped as KTask turdmod-cli #182's first deliverable.
- **Tauri overlay tab** (read+write) inside `apps/overlay` — the actual
  install / enable / disable UI. Lives behind the `userFeatures.feature='premium_admin'`
  gate same as the rest of the premium overlay (per scummap memory
  `project_premium_overlay.md`).

This doc covers both, plus the **Vanilla / Modded launcher** that the
overlay needs in order to honor the three-warning policy from
`docs/turdmod/compatibility-policy.md`.

---

## Information architecture

```
TurdMOD tab
├── Browse                ← search / filter / install
├── Installed             ← active + disabled (with toggle)
├── Conflicts             ← mods that override the same row
├── Compat check          ← latest build-diff vs installed mods
├── Launcher              ← Vanilla / Modded mode + "Launch SCUM"
└── Settings              ← paks dir override, audit-log path, BE policy
```

Web browser version: only **Browse** + a read-only mirror of **Installed**
(it can't actually toggle without local CLI access).

## Browse — wireframe

```
┌─ TurdMOD ─────────────────────────────── ● live registry ─┐
│ [search...]  [mode▼]  [tag▼]                             │
│ ⚠ Reminder: never use mods on official Gamepires servers │
│                                                          │
│ ┌──────────────────────────────────────────────────────┐ │
│ │ Better TEC1 Loot                       [SERVER] ✓142 │ │
│ │ better-tec1-loot · by YOUR_OWNER_NAME · v1.0.0 · ★4.6     │ │
│ │ Boosts Gold Memory Module drop rate in Depository... │ │
│ │ [loot] [tec1] [server-side]              [INSTALL]   │ │
│ └──────────────────────────────────────────────────────┘ │
│ ┌──────────────────────────────────────────────────────┐ │
│ │ Radial Quick Actions                  [OFFLINE] ✓76 │ │
│ │ radial-quick-actions · v1.0.0 · ★4.8                 │ │
│ │ Hold F4 for a heal/eat/drink/repair radial menu.     │ │
│ │ [ui] [qol] [offline-only]                [INSTALL]   │ │
│ └──────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────┘
```

Mode badge colors (green / indigo / amber / fuchsia) match the four
delivery strategies in `docs/turdmod/compatibility-policy.md`.

## Installed — wireframe

```
┌─ Installed mods ─────────────────────────────────────────┐
│ ✓ better-tec1-loot  Better TEC1 Loot v1.0.0  [server]   │
│   3.2 KB · installed 2026-05-09T01:00 · author TechyR   │
│   [▼ details]  [disable]  [uninstall]                   │
│ ─                                                        │
│ ✓ radial-quick-actions  Radial Quick Actions v1.0.0     │
│   1.1 KB · [offline]                                     │
│   [▼ details]  [disable]  [uninstall]                   │
│ ─                                                        │
│ · cosmetic-icon-pack  Cosmetic Icon Pack v1.2.0          │
│   8.4 MB · [content] · DISABLED                          │
│   [▼ details]  [enable]   [uninstall]                   │
│                                                          │
│ Mode: MODDED · 2 active / 1 disabled                     │
│ ⚠ Run 'turdmod pak vanilla' before connecting to        │
│   official Gamepires servers.                            │
└──────────────────────────────────────────────────────────┘
```

The toggle / uninstall buttons run the corresponding `turdmod pak ...`
commands via Tauri's command bridge. The mode badge under the list is
the same data the CLI's `turdmod pak status` returns.

## Conflicts panel

When two mods override the same DataTable row or BP class, the manager
flags it. Source: cross-reference each installed mod's
`content.listOverrides()` (#171) at runtime — for v1, fall back to
heuristic match against sidecar tokens (same approach as
`turdmod pak compat-check` from #184).

```
┌─ Conflicts (1) ──────────────────────────────────────────┐
│ ItemSpawningParameters :: MemoryModule_Level4            │
│   ✓ better-tec1-loot v1.0.0     ← LAST LOADED, WINS     │
│   ✓ harder-mode v0.3.0                                  │
│   [resolve]: load order  ·  disable harder-mode         │
└──────────────────────────────────────────────────────────┘
```

## Compat check panel

Reads the result of `turdmod pak compat-check --json` (already shipped in
#184). Surfaces flagged mods with the diff summary inline.

```
┌─ Compat check vs build v23128448 ────────────────────────┐
│ Diff: data/version-diffs/v23009297_to_v23128448.md       │
│ Scanned: 3 mods · Flagged: 0                             │
│ ✓ All mods in range; no at-risk overrides detected.     │
└──────────────────────────────────────────────────────────┘
```

When a flag fires, expand inline:

```
│ ⚠ better-tec1-loot v1.0.0 — at-risk                     │
│   Latest diff touched 2 tokens this mod references:      │
│   - MemoryModule_Level4                                  │
│   - ItemSpawningParameters                               │
│   [open diff]   [pin to current build]                  │
```

## Launcher — Vanilla vs Modded

This is the part of the policy that absolutely needs UI (the CLI commands
exist already — `turdmod pak vanilla` / `turdmod pak modded` — but a
player launching SCUM the normal way through Steam never sees them).

Two big buttons replace the standard "Play SCUM" action when TurdMOD is
installed:

```
┌──────────────────────────────────────────────────────────┐
│   ┌────────────────┐          ┌────────────────┐        │
│   │   ▶ VANILLA     │          │  🔧 MODDED     │        │
│   │   (clean)       │          │  (3 active)    │        │
│   └────────────────┘          └────────────────┘        │
│                                                          │
│   Vanilla: moves ~mods/ aside, launches SCUM with        │
│   anti-cheat-safe state. Use for official servers.       │
│                                                          │
│   Modded: launches with active mods. PRIVATE SERVERS    │
│   AND SOLO ONLY. Do NOT join an official server while   │
│   modded.                                                │
└──────────────────────────────────────────────────────────┘
```

The Modded button is **amber** if any mods are active; the Vanilla button
is **emerald**. After the player clicks, the manager runs the appropriate
CLI command and then either launches SCUM (`steam://run/513710`) or
returns control if launch is the player's responsibility.

**Post-connect detection** — once the game starts, the existing overlay's
window-capture + screen-OCR pipeline (the Auto-PIN F8 vector documented in
`MEMORY/feedback_no_battleye_risk.md`) reads the server name from the
join screen. If it matches a known official Gamepires server pattern AND
`turdmod pak status` says MODDED, the overlay shows a top-of-screen
warning:

```
┌──────────────────────────────────────────────────────────┐
│ ⚠ TURDMOD: You connected to what looks like an official │
│   Gamepires server with mods active. Disconnect now and  │
│   relaunch in Vanilla mode.                              │
│                                              [dismiss]   │
└──────────────────────────────────────────────────────────┘
```

UMOD never force-disconnects — that's the player's call. It just makes
sure they can't claim they didn't know.

## Settings panel

```
┌─ Settings ───────────────────────────────────────────────┐
│ Paks directory     [C:/Steam/.../Content/Paks  ] [browse]│
│ Audit log path     [%LOCALAPPDATA%/TurdMOD/loader.log  ] │
│ Auto-vanilla       [☐] move mods aside before any launch │
│ Server-list watch  [☑] check server name post-connect    │
│ BE policy          (read-only) Strict — never inject     │
│                    while BattlEye is active              │
└──────────────────────────────────────────────────────────┘
```

The BE policy is intentionally read-only. The detection layer in
`apps/turdmod-cli/src/detect.ts` is load-bearing safety — it should never
be configurable from the UI, only inspectable.

## Data flow

```
┌─────────────────┐      ┌──────────────────┐
│  Browse         │ ───► │ turdmod-registry │  (HTTP/JSON; remote)
│  (web + tauri)  │      └──────────────────┘
└─────────────────┘
                          ┌──────────────────┐
┌─────────────────┐ ───►  │ turdmod-cli      │  (subprocess)
│  Tauri tab:     │       │  pak install     │
│  Installed,     │       │  pak enable      │
│  Conflicts,     │       │  pak status      │
│  Launcher,      │       │  scripting check │
│  Settings       │       │  pak compat-check│
└─────────────────┘       └──────────────────┘
                                   │
                                   ▼
                          ┌──────────────────┐
                          │ ~/SCUM/Content/  │
                          │   Paks/~mods/    │
                          └──────────────────┘
```

Every action the Tauri tab exposes maps to one CLI command. No new
backend logic — the tab is a GUI on top of the CLI we already shipped.
That keeps the CLI as the source of truth and makes the tab trivial to
test (mock the subprocess shell).

## Implementation plan (when this gets built)

1. **Web preview** (shipped as part of #180): `apps/web/src/app/turdmod/page.tsx`
   — read-only browse against the registry. Demonstrates the visual
   language; copy lives in this doc.
2. **Tauri command bridge** in `apps/overlay/src-tauri/`: a Rust module
   that shells out to `turdmod` and returns stdout. ~150 LOC.
3. **Tauri tab** in `apps/overlay/src/`: React component implementing the
   wireframes above. Reuses the existing premium-overlay tab framework.
4. **Launcher** is a Tauri-only feature (the web version doesn't launch
   SCUM). Wraps `turdmod pak vanilla` / `turdmod pak modded` + a Steam
   protocol URL.
5. **Server-list watcher** runs in Tauri's main process — uses the same
   `xcap` / window capture path the existing Auto-PIN feature uses.

That's a full sub-board of work; KTask issue is intentionally a "design"
task today. Implementation gets sub-tasks when we're ready to build it.
