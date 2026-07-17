# Server-side custom UI — unified plan

**Goal:** ship server-driven custom UI (toasts + full GUI panels) to vanilla SCUM clients with **no per-player install**. Joel's hard requirement (2026-05-19): everything server-side; no client DLL, no dxgi proxy, no loader.

This plan supersedes the parked client-side path in [custom-hud-panels-plan.md](./custom-hud-panels-plan.md) for production work. The client-side code stays on main for future Pro-tier optionality but is NOT what we're shipping.

---

## ⚠ 2026-05-19 PM status: Q2 probe came back NO. Plan narrowed to Pro tier + bypass dependency.

The Q2 probe (drop unsigned `_P.pak` into `Content/Paks/`, see if it mounts) crashed the server. SCUM emits a `LogSCUM: Error: Pak file or matching sig file integrity compromised` line and calls `RequestApplicationExit` — that's a **SCUM-specific hand-coded check**, not vanilla UE4. We can't sign as Gamepires (private key), so the "drop pak, it mounts" path is dead for everyone.

**Effect on this plan:**
- **Lite tier:** pak-shipping path dead (no in-process bypass available). Lite stays at FTP+RCON+JSON. Options 2 + 3 below are Lite-impossible.
- **Pro tier:** the plan still applies, BUT a P-1 phase is inserted before P0: **install a signature-check bypass via in-process hook.** Bridge/loader already hooks UE4 functions via PolyHook2; the target is the SCUM function that emits the crash line (or the `.sig` file lookup that triggers it). Timing is the open engineering question — pak enumeration runs very early, possibly before UE4SS finishes loading CppUserMods; we may need an earlier-load DLL (the `turdmod-server-loader` shape).
- Per [[pro-only-focus]] (Joel's 2026-05-19 directive), all engineering goes into the Pro path until in-process probing + wiring is complete.

Full Q2 crash evidence + bypass-investigation scaffold lives in [pak-mod-investigation-plan.md](./pak-mod-investigation-plan.md). Memory: [[scum-pak-signature-wall]]. The phased work order below is preserved as the target plan once the bypass is in place.

---

## What we learned 2026-05-19 (the architectural ceiling)

The bridge running inside `SCUMServer.exe` **cannot** invoke SCUM's admin parser to fire admin commands:

- `Chat_Server_ProcessAdminCommand` is a UE4 `Server` RPC. Calling it via `ProcessEvent` server-side silently no-ops because there's no NetConnection metadata for UE4's RPC dispatch to validate.
- `Test_ProcessAdminCommand` (the test/dev variant) also silently no-ops in shipped SCUM builds.
- `#SendNotification` specifically has a second-tier "Player must be developer" gate INSIDE the parser — even an admin in real chat can't run it.

What DOES work from the bridge:
- `BlueprintCallable` static UFunctions (e.g. `MiscStatics::BroadcastChatLine`)
- `NetMulticast` RPCs (server → all clients — designed to be called server-side)
- `Client` RPCs called from server-side (designed to push to specific clients)
- Direct property reads/writes on UObjects

The pattern for any new server-driven UI feature: **find the right primitive in the working categories above; never go through the admin parser.**

Full diagnosis in memory [`feedback_bridge_admin_rpc_blocker.md`](../../.claude/projects/C--Development-Claude-turdmod/memory/feedback_bridge_admin_rpc_blocker.md).

---

## The two outputs of this plan

### Option 2 — Custom notification toasts

**What it is:** the small `UI_NotificationWidget` toast SCUM uses for cargo drops, kill feed, server events. One-line text + icon + duration. Renders at top of screen.

**Architecture (validated 2026-05-19 live):**
- Renderable data lives in `BasicNotificationDescriptionData` data assets (56 bytes — Message FText, FontSize, Icon, Duration, Ping, Delay)
- `NotificationsManager::NetMulticast_RequestNotification(NotificationDescriptionReplicationHelper)` is the multicast that sends a notification to every client
- The `_C` BP class for `AdminCommand_SendNotification` references 5 pre-baked data assets — the "type 1..5" we kept asking about. Gamepires authored 5 fixed variants; the admin command just picks one.

**What we ship:**
1. A `.pak` containing N custom `BasicNotificationDescriptionData` assets we authored (our text, our icons, our durations)
2. A bridge handler `sendNotification(asset_path, [override_text])` that:
   - Resolves the data asset by path
   - Builds a `NotificationDescriptionReplicationHelper` struct around it
   - Calls `NetMulticast_RequestNotification` on the active `NotificationsManager` instance
3. Manager UI to pick + fire

**Use cases:** server restart warnings, raid alerts, admin messages, custom kill-feed, brief "Welcome <Name>" on join.

**Limitation:** single line of text + one icon. Layout is fixed. Won't render dialogs or panels.

### Option 3 — Full GUI surface (the real GUI Builder runtime)

**What it is:** anything UMG can render — multi-line panels, buttons, images, nested layouts, scroll views, input fields. Anything a UE4 game's HUD can show.

**Architecture:**
- A `.pak` containing custom UMG widget classes that we author (in UE4 Editor)
- The widgets have replicated/exposed properties: text strings, image refs, button labels, layout switches
- A custom `Multicast` or `Client` RPC we register (also in the pak) that takes "widget class to spawn + property values to set" and dispatches on the target client(s)
- Bridge handler invokes the RPC
- Client receives, instantiates the widget, sets props, adds to viewport

**What we ship:**
1. Pak with:
   - Custom UMG widget classes (`BP_TurdModPanel_C`, `BP_TurdModDialog_C`, etc.)
   - A custom subsystem class with the dispatch RPC (`UTurdModPanelSubsystem::Multicast_ShowPanel`)
2. Bridge handler `showPanel(target, widget_path, props)` that fires the RPC
3. Manager UI ([already exists](../apps/turdmod-manager/src/pages/GuiBuilderPage.tsx)) to compose the panel + fire send

**Use cases:** server admin panels, in-game stat displays, custom inventory mods, mini-games, info boards, anything.

**This IS the GUI Builder runtime.** The Manager's existing GuiBuilderPage was always going to need a runtime to display what was authored; this is it. Server-side, no client install.

---

## Why both options share one foundation

Both options require the bridge to dispatch RPCs that take **arguments referencing classes/assets that don't exist in vanilla SCUM**. We can't talk about "our notification asset" or "our panel widget" if vanilla SCUM doesn't have those classes in its `GUObjectArray`. So we have to ship them.

The ONLY way to ship new classes into `SCUMServer.exe`'s UObject registry without a per-player install is via a `.pak` mounted at the server's `Content/Paks/` directory. UE4 4.27's pak loader is the documented moddability path — same one Steam Workshop uses for ARK / Conan / Squad / Squad 44 / dozens of others.

So the dependency tree is one line:

```
Pak-mod investigation (docs/pak-mod-investigation-plan.md)
   ↓ (Q2: does SCUMServer mount unsigned _P.paks?)
Pak-mod toolchain (UE4 4.27.2 install, BP authoring shim, pak builder, mount detection, version pinning)
   ↓
[ Option 2 path ]                    [ Option 3 path ]
  Ship BasicNotification              Ship custom UMG widgets
  DescriptionData assets               + custom RPC subsystem
   ↓                                    ↓
  Bridge handler:                      Bridge handler:
  sendNotification(...)                 showPanel(target, widget, props)
   ↓                                    ↓
  Manager UI uses it                   Manager GuiBuilderPage uses it
```

**Both options light up roughly simultaneously** once the pak-mod toolchain is alive, because once you can put your own class in a pak, the rest is just "what shape did we author."

---

## Phased work order

Each phase has a clear go/no-go signal — don't invest in phase N+1 until N is green.

| Phase | What | Cost | Decision unlocked |
|---|---|---|---|
| **P0** | Pak-mod Q2 probe (local SCUMServer mount of an unsigned no-op `_P.pak`) | 30-60 min once UE4 install completes | If NO → all server-side custom UI is dead; pivot to FTP/RCON-only Lite. If YES → continue. |
| **P1** | Pak-mod Q3.a — Hello World admin verb BP class in a pak, fires `#TurdModPing` in chat | 1-2 days | Proves BP-class shipping works. Validates the toolchain. |
| **P2** | Author a single `BasicNotificationDescriptionData` asset + bridge handler `sendNotification` | 1-2 days | Option 2 ships. Server can send custom toasts. |
| **P3** | Author one custom UMG widget class + custom RPC subsystem + bridge handler `showPanel` | 3-5 days | Option 3 ships. GuiBuilderPage gets a real runtime. |
| **P4** | Manager wiring: ServerPanelsPage rewired to call `sendNotification` instead of broken `runAdminCommand`; GuiBuilderPage wired to `showPanel` | 1 day | The whole loop closes; admin authors a panel in the Manager → it appears in-game. |
| **P5** | Version pinning toolchain (per-SCUM-build pak variants, pre-flight schema check via scumdump) | 1-2 weeks | Shippable to other admins, not just Joel's box. |

---

## What changes in the Manager when each phase ships

- **P2 ships** → ServerPanelsPage's "Send notification" button calls `sendNotification` not `runAdminCommand`. The 5 vanilla "type 1..5" picker disappears, replaced by a dropdown of our shipped notification assets (or a free-text input that overrides the asset's message field if we set that up).
- **P3 ships** → CustomPanelsPage (currently parked behind the client-side loader, commit `f9428d4`) gets RE-WIRED to call `showPanel` instead of `loader_rpc`. The Custom Panels page comes back to life through the server-side path.
- **P4 ships** → GuiBuilderPage gains a "Push to player(s)" button that does live preview of authored widgets in-game.

---

## What we're NOT doing

- ❌ Trying again to make `runAdminCommand` or `runTestAdminCommand` work. They're dead — see memory [`bridge-admin-rpc-blocker`](../../.claude/projects/C--Development-Claude-turdmod/memory/feedback_bridge_admin_rpc_blocker.md).
- ❌ Using SCUM's vanilla `#SendNotification` — it's dev-gated even from real admin chat. The 5 pre-baked types are unreachable.
- ❌ Building a client-side install path. Joel made this a permanent rule (memory [`server-side-default`](../../.claude/projects/C--Development-Claude-turdmod/memory/feedback_server_side_default.md)).
- ❌ Mucking with vanilla SCUM widget classes. We ship our own; the vanilla ones stay untouched.

---

## What we ARE doing first

**Phase 0: the pak-mod Q2 probe.** Everything depends on it. The probe is testable on Joel's local SCUMServer today — no G-Portal needed for the gate question. See [pak-mod-investigation-plan.md](./pak-mod-investigation-plan.md) §"Q2 — Does SCUMServer.exe mount unsigned `_P.paks`?" for the exact 7-step sequence.

When Joel finishes the UE4 4.27.2 install (~30 GB Epic Games Launcher download), we run the probe in one focused session.

---

## See also

- [pak-mod-investigation-plan.md](./pak-mod-investigation-plan.md) — the foundation; answers whether server-side mod-shipping is viable at all
- [custom-hud-panels-plan.md](./custom-hud-panels-plan.md) — parked client-side path (Phase 1 + 2 shipped); kept for future Pro-tier optionality
- `docs/scum-internals/20-umg-server-driven-surfaces.md` — what vanilla SCUM exposes server-side (without pak mods)
- Memory `feedback_bridge_admin_rpc_blocker` — full diagnosis of why admin parser doesn't work from bridge
- Memory `feedback_server_side_default` — Joel's permanent rule
- Memory `feedback_scum_admin_auth_blocker` — older sibling finding that predicted this
