# SCUM Server-Driven Custom UI — Engineering Specification

**Audience:** Gamepires engineering team or contracted developers.

**Authors:** TurdMOD team (this codebase). Contact via the email on the
contract cover sheet.

**Status:** Proposed. The spec stands ready for implementation against
SCUM build `23128448`. Implementation effort is on Gamepires; the
TurdMOD side delivers the server-authoring layer + reference mods +
demo content immediately on receipt of an experimental client build.

## TL;DR

Vanilla SCUM today has no primitive for a server to push a structured
custom GUI panel to a player's client. The closest existing surfaces
are single-line text (welcome message, MOTD, admin announce, toast
notifications). This spec proposes a small, surgical extension: hook
SCUM's existing `<Style>text</>` markup parser (already running in
`WBP_SurvivalTip` and 3 other widgets) into a new server-replicated
`RichTextBlock` widget plus an `<img src="..."/>` decorator with a
whitelisted host list. With that one change shipped, server admins can
author Rust-CUI-quality welcome panels, kill announcements, branded
notifications, and arbitrary in-game GUI without any client install.

The TurdMOD welcome-screen mod already produces a stable
`welcome-panel` payload schema (see PAYLOAD-SCHEMA.md in the mod's
directory) — once the new widget exists in vanilla SCUM, the mod
publishes that payload, the client renders it, demo lands.

## Problem statement

The complete reverse-engineered analysis of vanilla SCUM's UMG widget
tree is in
[`docs/scum-internals/20-umg-server-driven-surfaces.md`](../scum-internals/20-umg-server-driven-surfaces.md).
117 widget assets dumped from the shipped `.pak`. The findings:

- **No widget in the vanilla client is server-driven AND uses
  `RichTextBlock`.** Only 4 of 117 widgets use `RichTextBlock` at all
  — `WBP_SurvivalTip`, the Quest Book minigame, the Notice Board
  minigame, and two HUD action prompts. All four read **fixed authored
  markup** from a pak datatable; none reads from a server-replicated
  string property.
- **Every server→client UI surface is single-string.** `WelcomeMessage`,
  `MessageOfTheDay`, `#Announce <text>`, `#SendNotification <type>
  <userid> <text>` — all carry one plain-text string. The richest of
  these (`#SendNotification`) renders to `UI_NotificationWidget` which
  is exactly one `TextBlock` + one `Image` icon, with the icon
  selected from a hardcoded set of 5 styled variants. **Not a panel.**
- **`#Widget add/remove` is not a CUI primitive.** Despite the
  promising name, the command's CDO hardcodes a list of two widgets
  (`PD_MainWidget` debug panel, `BodyHighlightDemo`) and runs only on
  the admin's *own* client (`_shouldExecuteOnServer: false,
  _shouldExecuteOnClient: true`). It does not replicate, and it
  cannot drive arbitrary widget names.
- **`scum.ServerBannerUrl` works** — the existing server-driven image
  URL renders in the server browser. Useful for branding, but it's a
  single fixed image slot and only visible pre-connect.

The result: a server admin can author *text* notifications today, but
the moment they want to render a paneled GUI (banner image + multiple
text regions + dismiss action), they hit a wall. Rust ships
`CuiHelper.AddUi(player, json)` for exactly this — a server plugin
serializes a widget tree to JSON and the vanilla client renders it.
SCUM has no equivalent.

## Proposed solution

Add a **server-replicated `RichTextBlock` widget** to vanilla SCUM
that:

1. Receives a markup string + a per-player target list via a new
   replicated property (server → client).
2. Parses the markup using SCUM's **existing** `<Style>text</>` syntax
   (the parser running in `WBP_SurvivalTip`).
3. Adds **one new decorator** to that parser: `<img src="..."/>`,
   matched against an admin-configured whitelist of hosts. Unmatched
   hosts render as a placeholder text token; no untrusted image
   fetches.
4. Lives inside a wrapper `WBP_ServerCustomPanel` widget — a simple
   container with a vertical box that hosts the rich-text block, a
   title bar, and a dismiss button. Optional banner-image slot above.

The existing markup syntax (e.g. `<Important>Hold ENTER</>`) keeps
working unchanged; the addition is one new decorator (`<img>`) and
one new server-replicated input source. Nothing else in the client
changes.

This is deliberately the **smallest viable change** that unblocks
panel-shaped server-driven UI. A bigger overhaul (full Rust-CUI-style
arbitrary widget-tree spec) is also valuable, but this spec keeps
scope tight enough to ship in one patch cycle.

## Implementation surface

### 1. Server-side RPC

A new admin command (or, preferably, a public server-side hook):

```
#PushCustomUI <userId|"-1"> <stringId> <markupBase64>
```

* `userId`: target player's `SCUM:UserId` (matches `#SendNotification`'s
  user-id semantics). `-1` broadcasts to all connected clients.
* `stringId`: an admin-chosen short identifier (32 chars max,
  `[a-z0-9_-]+`). Lets the server later push an *update* with the same
  `stringId` to mutate the visible panel, or push an empty markup to
  clear it. Multiple distinct `stringId`s coexist on the client (think
  one for welcome panel, one for kill feed banner, etc.).
* `markupBase64`: base64-encoded UTF-8 markup string (so the existing
  admin-command tokenizer doesn't need to handle inline newlines or
  quotes).

The command's CDO sets:
* `_requiredExecutorLevel: EExecutorStatus::Admin`
* `_shouldExecuteOnServer: true, _shouldExecuteOnClient: true`
  (server validates target + persists the active set; client renders).

A non-admin server-side hook is preferable so server-side scripts /
companion processes can invoke this without authenticating as a
specific admin user. Either:
* A new replicated function on `ASCUMGameMode` callable via the
  existing server-side scripting surface, or
* A `_requiredExecutorLevel: EExecutorStatus::Game` fallback so the
  game itself can self-trigger from server-side `BlueprintCallable`
  hooks.

### 2. Replication wire format

The client maintains a `TMap<FString, FCustomUIEntry>` — `stringId` →
entry — replicated per-player. `FCustomUIEntry`:

```cpp
USTRUCT()
struct FCustomUIEntry {
    GENERATED_BODY()
    UPROPERTY(Replicated) FString StringId;        // matches the server-side ID
    UPROPERTY(Replicated) FString Markup;          // raw markup text
    UPROPERTY(Replicated) int32   Priority;        // z-ordering, default 0
    UPROPERTY(Replicated) float   ExpiresInSeconds; // 0 = no expiry
};
```

Empty `Markup` removes the entry. Existing entries are replaced atomically
when `StringId` collides.

### 3. Client-side widget — `WBP_ServerCustomPanel`

New UMG widget asset:
`SCUM/Content/ConZ_Files/UI/UI_ServerCustomPanel.uasset`

Tree:
```
WBP_ServerCustomPanel (UUserWidget)
├── Overlay
│   ├── BackgroundImage           (UImage; brush set from theme)
│   └── VerticalBox
│       ├── TitleBar              (UHorizontalBox)
│       │   ├── TitleText         (URichTextBlock)
│       │   └── CloseButton       (UButton)
│       ├── BannerImage           (UImage; URL-driven; visibility off if URL empty)
│       └── BodyText              (URichTextBlock)
```

The widget reads the active `FCustomUIEntry` set from
`ASCUMPlayerController` and renders one panel per entry, stacked by
`Priority`. The widget tree is fixed; the **content** (title, banner,
body) is parsed from the markup string by the rich-text decorator
chain.

### 4. Markup grammar

Parsed by the existing rich-text parser plus three new tags:

| Tag                                | Purpose |
|---|---|
| `<Style.Name>text</>`              | Existing — references `TextStyleSet` rows. Keep. |
| `<title>text</title>`              | New — text inside is hoisted to `TitleText`. First match wins. |
| `<banner src="https://…"/>`        | New — URL is hoisted to `BannerImage` brush; only fires when the URL host is on the whitelist. |
| `<img src="https://…" alt="…"/>`   | New — inline image inside the body rich-text block. Same whitelist. |
| `<dismiss key="Escape"/>`          | New — sets the close-button keyboard binding. Optional; defaults to `Escape`. |
| `<section title="…">…</section>`   | New — vertical box of grouped content (rules / mods / commands). Renders title in the body region's accent style. |

The existing decorators (`<KeyPrompt/>`, `<Important>`, etc.) stay
working. The new tags above piggyback on the same parser; add four
decorator classes (`UTitleHoistDecorator`, `UBannerHoistDecorator`,
`UInlineImageDecorator`, `UDismissBindingDecorator`) and one section
container.

Rendering rules:

* `<title>`, `<banner>`, `<dismiss>` are **hoist tags** — they pull
  out of the body flow and assign to the structural slots. If absent,
  defaults apply (no title, no banner, Escape dismisses).
* Unrecognized tags render as their `alt` attribute or empty text;
  never raw markup.
* Markup string max length: **8 KiB**. Larger payloads truncate with
  `[…truncated]` appended.

### 5. Image decorator + whitelist

`<banner>` and `<img>` URL hosts must match the new
`scum.CustomUIImageHostWhitelist` setting in `ServerSettings.ini`:

```ini
[CustomUI]
scum.CustomUIImageHostWhitelist=cdn.scummap.com,placehold.co,i.imgur.com
scum.CustomUIImageMaxBytes=512000
scum.CustomUIImageTimeoutMs=3000
```

* Comma-separated list of allowed hosts. No wildcards in v1 (keep
  trust model simple); subdomains must be listed explicitly.
* Per-image fetch capped at `CustomUIImageMaxBytes` and
  `CustomUIImageTimeoutMs`.
* Fetched images are cached on the client by URL hash for the
  duration of the session.
* On host mismatch / fetch failure / timeout, the image renders as
  a 1×1 transparent placeholder (no error toast — admins can see the
  failure server-side via existing fetch logs).

The whitelist exists because community-server admins can be
adversarial — without it, a malicious admin could push tracking-pixel
URLs to every connected player.

### 6. Sandbox / security

* **No script execution.** Markup is rendered, never evaluated.
* **No HTML JavaScript / no link click navigates outside the game.**
  `<a href="...">` (if added later) would launch the URL in the
  player's default browser via the existing Steam Overlay path,
  identical to how the `Discord` field in the server browser already
  works.
* **No pak overrides.** The new widget is a fixed pak asset; markup
  cannot reference arbitrary `.uasset` paths.
* **Bandwidth cap per player.** Replicated entry set capped at 16
  active entries per player; new entries past the cap evict
  lowest-priority oldest first.

### 7. Backward compatibility

Pure additive. The existing single-string surfaces
(`scum.WelcomeMessage`, `scum.MessageOfTheDay`, `#Announce`,
`#SendNotification`) keep working unchanged. Servers that don't push
custom UI see no client behaviour change. Old clients connecting to
new servers ignore the unknown `FCustomUIEntry` replication updates
gracefully (UE's NetSerialize ignores unknown structs).

## Authoring API — what a server-side mod looks like

The TurdMOD welcome-screen mod (server-side, no client install) today
publishes a `panel.welcome.<steamId>` payload with structured fields
(`server_name`, `tagline`, `banner`, `sections`, `actions`). With the
new client widget, the same mod renders that payload by formatting it
as a markup string and calling the new RPC:

```ts
async function onLogin(p: LoginPayload) {
  const cfg = await getConfig();
  if (!cfg.alwaysShow && await hasSeen(p.steam)) return;
  await markSeen(p.steam);

  // Already in the wire — wraps the configured panel content.
  const markup = formatPanelMarkup(cfg, p.player);

  // New RPC the spec adds — pushes the markup to one specific player.
  await scum.pushCustomUI(p.steam, "welcome-panel", markup);
}

function formatPanelMarkup(cfg, player) {
  return `
<title>${cfg.serverName}</title>
<banner src="${cfg.banner}"/>
<dismiss key="${cfg.dismissKey}"/>

<Style.HeroTagline>${cfg.tagline}</>
Welcome, <Style.PlayerName>${player}</>.

<section title="${cfg.rulesTitle}">
${cfg.rules.map((r, i) => `${i+1}. ${r}`).join("\n")}
</section>

<section title="Powered by TurdMOD">
${cfg.modBrag.map(m => `${m.emoji} <Style.ModName>${m.name}</> — ${m.tagline}`).join("\n")}
</section>

<section title="${cfg.commandsTitle}">
${cfg.commands.map(c => `<Style.Cmd>\`${c.name}\`</> — ${c.description}`).join("\n")}
</section>

${cfg.discord ? `<a href="${cfg.discord}">Join Discord</a>` : ""}
${cfg.website ? `<a href="${cfg.website}">Live Map</a>` : ""}
  `.trim();
}
```

That mod ships today on the TurdMOD companion runtime; the only piece
missing is the client renderer. As soon as `pushCustomUI` exists,
welcome-screen renders the panel without any further code changes on
our side.

The same authoring surface drives kill-feed (push a single-line markup
on every PvP kill), vehicle-manager (push a notification panel to the
owner when their vehicle is destroyed), and any future mod that wants
a custom in-game UI.

## Test plan / acceptance criteria

A patch is acceptance-passing when all of the following hold on a
fresh SCUM install:

1. **Hello-world panel.** Admin runs `#PushCustomUI <self> hello
   <base64("<title>Hi</title>Hello, world.")>`. The admin's client
   renders a panel with title "Hi" and body "Hello, world." Dismiss
   key Escape closes it.
2. **Per-player targeting.** Admin runs the same command targeting
   another player's user id. Only that player sees the panel.
3. **Broadcast.** Admin targets `-1`. Every connected client sees the
   panel.
4. **Markup styling.** Markup containing `<Style.Important>x</>`
   renders with the existing TextStyleSet style — proves the parser
   chain still works.
5. **Inline image.** Markup containing
   `<img src="https://placehold.co/200x100"/>` renders the image
   inline (assuming placehold.co is in the whitelist).
6. **Whitelist enforcement.** Same markup with a non-whitelisted host
   renders a 1×1 transparent placeholder; no fetch is attempted.
7. **Update by stringId.** Pushing a new markup with the same
   `stringId` replaces the visible panel atomically.
8. **Clear.** Pushing empty markup with the same `stringId` removes
   the panel.
9. **Cap.** Pushing a 17th distinct `stringId` evicts the
   lowest-priority oldest entry.
10. **Truncation.** A markup string > 8 KiB renders with
    `[…truncated]` appended at the cap; no panic, no crash.
11. **Old client compat.** Connecting an unpatched 23128448 client to
    a patched server: client receives the replication updates without
    error and ignores them. Existing `#Announce` / `#SendNotification`
    flows still work.
12. **No regression.** All four widgets that already use
    `RichTextBlock` (`WBP_SurvivalTip`, the two minigames, the two
    HUD action prompts) render identically before and after the patch.

The TurdMOD welcome-screen mod's `panel.welcome.<steamId>` payload,
formatted via `formatPanelMarkup` above, lands as the demo capture —
recorded video, sent on patch acceptance, ends the demo arc.

## What TurdMOD ships on patch receipt

* The full `formatPanelMarkup` implementation as part of the
  welcome-screen mod's next release.
* A markup-authoring helper library (`@turdmod/turdmod-markup`) so
  third-party mod authors don't reimplement the formatter per mod.
* Seven additional reference mods using the new primitive:
  kill-feed banner, vehicle-manager owner alert, marketplace listing
  card, server stats overlay, weather warning, event countdown,
  admin broadcast.
* The recorded demo video Gamepires asked for. (Captured against the
  TurdMOD Official server with our existing branding pack.)
* This spec, updated to reflect any changes the implementing team
  made, as the engineering reference document for future maintainers.

## Open questions

* **Replication channel cost.** Should the active-entry set live on
  the player's `PlayerState`, the `PlayerController`, or a new
  `ASCUMCustomUIComponent`? Cost analysis depends on Gamepires's
  current replication graph configuration; the implementing team
  picks.
* **Markup parser performance under churn.** Pushing a new entry per
  second to many players is plausible (live event countdown). The
  existing parser caches by string content; we don't see a concern
  but a 60 Hz stress test should validate before shipping.
* **Localization.** SCUM's existing rich-text parses against the
  active locres-based TextStyleSet. Markup pushed by the server
  carries no locale awareness today; future enhancement adds an
  optional `<lang code="es">…</lang>` switch tag.
* **Mobile / controller dismiss.** Default dismiss is keyboard
  Escape; controller players need an equivalent. The existing input
  remap surface should suffice but verify against current bindings.

## Versioning + future evolution

This spec is **v1**. Forward-compatible additions (additional
hoist tags, new decorators, button widgets, input fields) ship in
**v1.x** without breaking subscribers. A v2 (full arbitrary widget-tree
spec — Rust-CUI parity for layout, not just rich-text content)
remains an option, but the experience of authoring against v1 in
production should inform v2's design rather than rushing it.

## Cross-references

* [`docs/scum-internals/20-umg-server-driven-surfaces.md`](../scum-internals/20-umg-server-driven-surfaces.md)
  — the dossier section answering "what server-driven UI surfaces
  vanilla SCUM has today" with evidence per widget.
* [`docs/scum-internals/14-server-admin.md`](../scum-internals/14-server-admin.md)
  — existing native server-driven UI primitives (`ServerBannerUrl`,
  `WelcomeMessage`, `MessageOfTheDay`).
* [`examples/turdmod/welcome-screen/PAYLOAD-SCHEMA.md`](../../examples/turdmod/welcome-screen/PAYLOAD-SCHEMA.md)
  — the panel payload TurdMOD's welcome-screen mod publishes today;
  the markup formatter consumes the same shape.
* [`docs/scum-internals/16-memory-signatures.md`](../scum-internals/16-memory-signatures.md)
  — UE 4.27 memory layout reference for the implementing team.
