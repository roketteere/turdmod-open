# TurdMOD decorator DLL — implementation spec

> **Pivot 2026-05-09 (post-Q3 review):** original two-file pak+DLL plan
> simplified to **all-DLL via function detours**. Joel's "private and
> modded only" framing mooted the pak overlay's BE-on advantage; the
> remaining advantage (pak ships layout / asset replacements) collided
> with a real tooling constraint (no UE4 editor / UAssetAPI workflow
> set up yet). Function-detouring the engine-stock decorator classes
> from a small Rust DLL delivers the same end-state (full Rust-CUI
> panel rendered by SCUM's own UMG) with one-file install and zero
> asset authoring. Original section retained below for posterity.



**Goal.** Ship a Rust-CUI-quality in-game welcome panel to TurdMOD
players via a **two-file install** (one `.pak`, one `.dll`), with no
sideloaded UE4 hooks or ImGui. Renders inside SCUM's own UMG widget
system. Independent of Gamepires's release schedule.

**Non-goal.** This is *not* the zero-install Gamepires demo — that path
needs Gamepires to ship the engine extension specified in
[`gamepires-cui-extension-spec.md`](./gamepires-cui-extension-spec.md).
The pak+DLL path is what TurdMOD ships in the meantime, and what
private-server admins use whether or not Gamepires ever ships
their patch.

**Scope summary.** Status as of 2026-05-09:

| Layer | Delivers | Install | BattlEye |
|---|---|---|---|
| **Pak** (Phase 1) | Rich-styled text + DataTable-keyed images | `.pak` drop into `Content/Paks/~mods/` | Documented BE-on safe (passive content load) [<sup>1</sup>](#fn1) |
| **DLL** (Phase 2) | URL-driven images, link buttons, dismiss bindings | `.dll` drop into `Content/Paks/~mods/` (or via `turdmod-launcher.exe`) | BE-off only |
| **Wire-up** (Phase 3) | welcome-screen mod publishes the markup; live-test | n/a (server-side) | n/a |

<sup id="fn1">1.</sup> Per `docs/scum-internals/21-custom-content.md` —
the pak overlay path "works even with BE on (passive content load is
not flagged)". Conservatively we ship the pak+DLL combo as BE-off only
since the DLL portion is the BE-sensitive part. Pak-only could be
extended to BE-on later if validated against the live Gamepires
Official server.

---

## Architecture overview

```
┌─────────────────┐     vehicle.notify.<steam>           ┌─────────────────┐
│ TurdMOD         │     panel.welcome.<steam>            │ SCUM game       │
│ companion       │     kill-feed                        │  (vanilla        │
│ (server-side)   │     squad.<id>.position              │   client)       │
│                 │              │                       │                 │
│  formats        │              ▼                       │  reads          │
│  markup         │     #PushCustomUI <user> <id> <text> │  replicated     │
│  string +       │  (server-side hook → existing chat-  │  string         │
│  pushes via     │   broadcast machinery)               │  property       │
│  existing       │              │                       │                 │
│  RPC            │              ▼                       │      ▼          │
└─────────────────┘     ServerSettings.WelcomeMessage    │ ┌─────────────┐ │
                        ServerSettings.MessageOfTheDay   │ │ chat panel  │ │
                        (existing replicated FString)    │ │             │ │
                                                         │ │  Replaced   │ │
                                                         │ │  by our     │ │
                                                         │ │  pak so     │ │
                                                         │ │  message    │ │
                                                         │ │  text is a  │ │
                                                         │ │  RichText   │ │
                                                         │ │  Block      │ │
                                                         │ │  instead    │ │
                                                         │ │  of plain   │ │
                                                         │ │  TextBlock  │ │
                                                         │ └──────┬──────┘ │
                                                         │        │        │
                                                         │        ▼        │
                                                         │  decorator      │
                                                         │  registry       │
                                                         │  (DLL          │
                                                         │   injects new   │
                                                         │   classes:      │
                                                         │   <img>,        │
                                                         │   <a>,          │
                                                         │   <dismiss>)    │
                                                         └─────────────────┘
```

**Key point.** No new replicated property is added. We reuse SCUM's
existing `WelcomeMessage` / `MessageOfTheDay` server→client wire — the
text it already broadcasts is reinterpreted as markup by our replaced
widget. The wire format is unchanged; only the *rendering* changes.

---

## Phase 1 — the pak

### 1.1 Asset replacements

The pak ships under `Content/Paks/~mods/turdmod-ui.pak` (UE4
auto-loads). Replaces these vanilla assets:

| Vanilla asset | Replacement intent |
|---|---|
| `SCUM/Content/ConZ_Files/UI/UI_ChatMessageInteractive.uasset` | `_messageText` widget changes from `UTextBlock` → `URichTextBlock`. `URichTextBlock.TextStyleSet` set to `MinigameInstructionsRichTextTable` (existing pak datatable, already wired for `<Style.Important>` styling) plus a new `TurdMODStyleSet` we ship. Decorator class list includes the engine-stock decorators plus the new ones the DLL registers (referenced by class path; class is registered at runtime). |
| `SCUM/Content/ConZ_Files/UI/UI_Chat.uasset` | Possibly *no change*. Container widget; only swapped if needed to fit the panel layout. |

**Why these two.** Per the dossier
([`20-umg-server-driven-surfaces.md`](../scum-internals/20-umg-server-driven-surfaces.md)),
the chat panel + chat-row widgets are the rendering destination for
the existing `WelcomeMessage` / `MOTD` / `#Announce` flows. Replacing
the row widget changes *all three* surfaces' appearance simultaneously.

### 1.2 New assets

The pak also ships these **new** assets that the replaced widgets
reference:

| New asset | Purpose |
|---|---|
| `SCUM/Content/_TurdMOD/UI/DT_TurdMODImageRegistry.uasset` | DataTable for `URichTextBlockImageDecorator`. Maps `<img id="...">` keys to brushes baked into the pak. Initial entries: `turdmod_logo`, `turdmod_banner_pve_pvp`, `discord_glyph`, `website_glyph`. |
| `SCUM/Content/_TurdMOD/UI/DT_TurdMODStyleSet.uasset` | Rich-text style table used in addition to `MinigameInstructionsRichTextTable`. Defines `Style.PlayerName`, `Style.HeroTagline`, `Style.RuleNumber`, etc. — the colour/font palette TurdMOD branding uses. |
| `SCUM/Content/_TurdMOD/UI/Brushes/turdmod_logo.uasset` (and friends) | Brush wrappers for the actual textures. |
| `SCUM/Content/_TurdMOD/UI/Textures/turdmod_logo.uasset` (and friends) | The actual texture data — pre-cooked from `examples/turdmod/welcome-screen/assets/banner.png` and similar. |
| `SCUM/Content/_TurdMOD/UI/WBP_TurdMODServerCustomPanel.uasset` | Optional new panel widget — TitleBar + BannerImage + RichTextBlock body + dismiss area. Used when the DLL is also installed (without the DLL its dismiss decorator no-ops, so the chat-row replacement still gives a usable result). |

The `_TurdMOD/` namespace is non-conflicting (vanilla SCUM has no
`_TurdMOD/` directory in `Content/`). New assets only — no override
risk for that path.

### 1.3 Cooking pipeline

Built via the existing `apps/extractor` infrastructure plus a new
`apps/turdmod-cli pak build` subcommand:

```bash
turdmod-cli pak build \
  --src   examples/turdmod/turdmod-ui-pak/      # source dir with .uasset copies
  --out   data/build/v23128448/turdmod-ui.pak   # versioned output
  --build 23128448                              # SCUM build to target
  --aes   "$SCUM_AES_KEY"                       # if signing requested
```

Internally:

1. Locates `UnrealPak.exe` from the SCUM Server install
   (`SCUM Server/SCUM/Binaries/Win64/`) or a pinned UE 4.27 SDK.
2. Builds the pak with the standard `-Compress` flag (~30% size win
   for our text-heavy assets).
3. SCUM doesn't enforce pak signing on private servers, so unsigned
   is fine. Signing path documented but not required.
4. Writes the result + a per-build manifest (`.pak.manifest.json`)
   listing every replaced and new asset path so admins can audit.

### 1.4 Build-diff integration

Every SCUM patch potentially renames a widget GUID or moves a
property. The existing `scripts/diff-builds.py` already runs in the
extract pipeline (per `feedback_log_game_build_diffs.md`). We add a
post-step:

```bash
turdmod-cli pak diff \
  --src-build 23128448 \
  --dst-build <new>
```

Reads the manifest from the previously-cooked pak, checks whether
each referenced vanilla asset path still exists in the new build with
the same GUID, and flags drift before any player updates. Output
goes to `data/version-diffs/v<old>-v<new>/turdmod-ui-pak-impact.md`,
committed alongside the build diff.

**On break:** human (us) updates the source widget assets to match
the new vanilla, recooks, releases. Players auto-update via the
TurdMOD CLI's pak-version check.

### 1.5 What pak-only (no DLL) delivers

Even if the player only installs the pak (not the DLL), they get a
real upgrade:

* Server messages render via `URichTextBlock` instead of `UTextBlock` —
  `<Style.Important>red</>` markup actually styles.
* DataTable image lookups via `<img id="turdmod_logo"/>` work (engine-
  stock `URichTextBlockImageDecorator` does this with no DLL).
* Existing `<KeyPrompt/>` / `<Important>` decorators keep working.

**What pak-only does NOT deliver:**

* `<img src="https://...">` URL-driven images. (DLL provides this.)
* `<a href="...">Join Discord</a>` clickable links. (DLL.)
* `<dismiss key="...">` keybind to close. (DLL.)
* Layout slots beyond what `WBP_TurdMODServerCustomPanel` ships. (Pak
  controls layout but can't add new C++ Slate widgets.)

So pak-only = **branded MOTD with embedded logo**. Pak + DLL = **full
clickable panel**.

---

## Phase 2 — the minimal decorator DLL

### 2.1 What it is

A small Rust DLL — call it `turdmod-rich-decorators.dll` — that
registers three new `URichTextBlockDecorator` subclasses with UE4's
UClass system at load time. It does **nothing else**. No DXGI hook,
no ImGui, no UE4 game-object hooks, no Lua runtime.

This is intentionally **not** the existing `turdmod-loader.dll` (which
ships hudhook + ImGui + Lua VM + IPC subscriber). Two reasons to keep
it separate:

1. **Smaller install + smaller trust ask.** A few hundred lines of
   Rust registering UClasses is auditable; players can read the
   source. The hudhook DLL has ~50× more surface area.
2. **Different target audience.** The decorator DLL goes to every
   TurdMOD player who wants the polish. The hudhook DLL goes to
   server admins running their own client mods (the dev tier).

### 2.2 Decorators it registers

Three decorator classes, all subclasses of `URichTextBlockDecorator`:

| Class | Markup tag | Behaviour |
|---|---|---|
| `UTurdMODImageURLDecorator` | `<img src="https://..." alt="..."/>` | Fetch image bytes from URL. Whitelist hosts via `~/.scummy-map/turdmod-image-hosts.txt` (default: `cdn.scummap.com`, `placehold.co`, `i.imgur.com`). Non-whitelisted hosts render `alt` text. Cache by URL hash for the session. Per-image cap: 512 KB / 3 s timeout. |
| `UTurdMODHyperlinkDecorator` | `<a href="https://..." key="...">label</a>` | Render `label` styled as a link. On click (or keybind via `key`), open URL in Steam Overlay browser via existing UE4 `FPlatformProcess::LaunchURL`. |
| `UTurdMODDismissDecorator` | `<dismiss key="Escape" label="Got it"/>` | Render the label as a button at the bottom-right of the parent rich-text run. Bind `key` to dismiss the parent panel widget. No-op (blank) if the parent isn't a TurdMOD panel. |

### 2.3 How the DLL registers UClasses

**The hard part.** UE4's UClass system uses C++ static initializers
generated by UnrealHeaderTool to register classes at module load time.
Adding new UClasses from an *external* DLL not built via the UE4
toolchain requires manually replicating what UHT generates.

Our path:

1. **Locate the relevant engine internals via sigscan** at DLL load
   time:
   * `GUObjectArray` (already located by the existing TurdMOD loader's
     `sigscan.rs` for build 23128448).
   * `URichTextBlockDecorator::StaticClass()` — the parent UClass our
     subclasses derive from.
   * `UClass::GetDefaultObject<>()` and the `UClassRegister` write path.
2. **Allocate a UClass struct** matching UE 4.27's binary layout for
   each of our three decorators. Set the parent class to
   `URichTextBlockDecorator`'s UClass.
3. **Register each UClass** via the same path UHT-generated code uses
   (`GetPrivateStaticClass` + `UClass::AddObject` patterns).
4. **Implement each decorator's behaviour** in Rust — bind the vtable
   entry for `CreateDecorator()` (returns `TSharedPtr<ITextDecorator>`)
   to a Rust function. The Rust function constructs the right Slate
   widget (FSlateBrush for img, hyperlink decorator for `<a>`, etc.)
   and returns it.

**Why we believe this is feasible.** The existing TurdMOD loader DLL
(`apps/turdmod-loader/src/sigscan.rs`) already locates UE4 4.27 globals
in build 23128448. The hudhook + DXGI Present hook proved the DLL can
operate inside SCUM.exe's process space without crashes. UClass
registration uses public-but-undocumented UE4 internals — the patterns
are reverse-engineered in projects like UE4SS and Dumper-7, which both
operate in vanilla UE4 4.27.

**Risk.** UClass binary layout is UE-version-specific. Every SCUM
patch needs a sigscan re-validation; a layout drift would break us.
The build-diff pipeline catches this (existing tooling flags
sigscan-relevant changes per `16-memory-signatures.md`).

### 2.4 Why NOT use the existing turdmod-loader DLL

The existing `turdmod-loader.dll` is the kitchen-sink: hudhook + ImGui
+ Lua VM + sigscan + IPC. Combining adds:

* Larger DLL → bigger install, slower load.
* Larger trust surface → more "what does this DLL do" code players
  must trust.
* Couples the decorator-registration concern to the entire mod-loader
  lifecycle. A bug in hudhook brings down the decorators.

The minimal-decorator DLL stays minimal — same crate workspace, same
sigscan code reused via a shared Rust lib, but a separate `cdylib`
target. ~few hundred lines.

Compile target: `apps/turdmod-loader/decorators/Cargo.toml`,
output `apps/turdmod-loader/decorators/target/release/turdmod-rich-decorators.dll`.

---

## Phase 3 — wire-up

### 3.1 welcome-screen mod update

The mod's `panel.welcome.<steamId>` payload already carries the right
shape (`server_name`, `tagline`, `banner`, `sections`, `actions`). We
add a markup formatter that converts that payload into a markup string
acceptable by the new decorators, then route the string through one of
the existing single-string surfaces.

```ts
// new in welcome-screen v0.2.0
function formatPanelMarkup(cfg: WelcomeConfig, playerName: string): string {
  const sections = [
    cfg.rules?.length    && `<title>${cfg.rulesTitle}</title>\n${cfg.rules.map((r,i)=>`${i+1}. ${r}`).join("\n")}`,
    cfg.modBrag?.length  && `<title>Powered by TurdMOD</title>\n${cfg.modBrag.map(m=>`${m.emoji ?? ""} <Style.ModName>${m.name}</> — ${m.tagline}`).join("\n")}`,
    cfg.commands?.length && `<title>${cfg.commandsTitle}</title>\n${cfg.commands.map(c=>`<Style.Cmd>\`${c.name}\`</> — ${c.description}`).join("\n")}`,
  ].filter(Boolean);

  const actions = [
    cfg.discord  && `<a href="${cfg.discord}">Join Discord</a>`,
    cfg.website  && `<a href="${cfg.website}">Live Map</a>`,
    `<dismiss key="${cfg.dismissKey ?? "Escape"}" label="Got it"/>`,
  ].filter(Boolean).join("  ");

  return `
<title>${cfg.serverName}</title>
${cfg.banner ? `<img src="${cfg.banner}" alt="banner"/>` : `<img id="turdmod_banner_pve_pvp"/>`}

<Style.HeroTagline>${cfg.tagline ?? ""}</>
Welcome, <Style.PlayerName>${playerName}</>.

${sections.join("\n\n")}

${actions}
  `.trim();
}
```

The mod publishes via the existing `WelcomeMessage` mechanism — that
single-string field now carries our markup, the replaced chat-row
widget renders it, decorators handle the rich tags.

For per-player targeting (welcome panels are per-player), the mod
falls back to the existing `#SendNotification 1 <userid>` flow (which
already targets specific users) using the same markup string. The
notification widget gets the same chat-row replacement treatment so
both surfaces share decorator support.

### 3.2 Live test

Same pattern as the welcome-screen Step-2 live test:

1. Cook the pak via `turdmod-cli pak build`. Drop in `Content/Paks/~mods/`.
2. Drop the decorator DLL alongside.
3. Restart SCUM client.
4. Connect to the test server (BE-off, our local instance).
5. Companion fires `welcome-screen` on login; the markup is broadcast
   via `WelcomeMessage` into chat; replaced chat-row widget renders
   the panel; decorators paint the banner image, links, dismiss.
6. Capture: companion log line + screenshot of the in-game panel.

Drops into the existing `examples/turdmod/welcome-screen/EVIDENCE.md`
as a third evidence section: §3 already exists for the Discord
embed; new §8 captures the in-game panel render.

---

## Open questions for Phase 1 prep

1. **Does `URichTextBlockImageDecorator` ship in vanilla SCUM?** Engine-
   stock UE 4.27 includes it; SCUM is built on 4.27; default UMG module
   is unlikely stripped. Confirm with a one-shot test pak that uses it
   and a synthetic `<img id="..."/>` markup — reads OK = ship the pak
   path; reads as raw text = pak path Tier 2 collapses, only Tier 1
   (rich-styled text) is available without DLL.
2. **Does the chat-row widget Cast<UTextBlock>(_messageText) anywhere
   in the C++ render path?** If yes, swapping to `URichTextBlock`
   crashes (failed cast). Mitigation: test on a local server first;
   if cast-failure observed, ship a side-by-side replacement widget
   (`UI_ChatMessageInteractive_RichText`) and hook the chat panel to
   pick it for TurdMOD-aware messages.
3. **What's the exact AssetReference path for the decorator class
   list inside the .uasset?** Standard UE4 widget assets list decorator
   classes by `BlueprintGeneratedClass'/Game/.../Decorator_C'`. The
   pak references our DLL-registered C++ classes by their UE4 class
   path. Need to verify the registration path the DLL uses produces
   classes findable by FAssetRegistry on the asset side.
4. **Pak signing / hash check.** The dossier
   ([`21-custom-content.md`](../scum-internals/21-custom-content.md))
   says SCUM doesn't enforce pak signing on private servers. Verify
   on the live BE-off test server that an unsigned pak loads.
5. **Discord-Overlay vs in-game URL launch.** `<a href>` opens the URL
   how? UE4's `FPlatformProcess::LaunchURL` uses the platform default
   browser, which on Steam invokes the Steam Overlay browser if Steam
   is running. Confirm that's the user-visible behaviour on a real
   game session.

These five questions are answered in Phase 1 day-one — no engineering
weight behind any of them; just paktest-and-observe runs.

---

## Build artefact summary

After all three phases ship:

| Artefact | Path | Phase |
|---|---|---|
| `turdmod-ui.pak`                 | `data/build/v<scumBuild>/turdmod-ui.pak`              | 1 |
| `turdmod-rich-decorators.dll`    | `apps/turdmod-loader/decorators/target/release/...`   | 2 |
| Updated welcome-screen mod       | `examples/turdmod/welcome-screen/scripts/main.ts`     | 3 |
| Markup-formatter helper lib      | `packages/turdmod-markup/`                            | 3 |
| Pak-build CLI subcommand         | `apps/turdmod-cli/src/pak.ts`                         | 1 |
| Pak-diff CLI subcommand          | `apps/turdmod-cli/src/pak-diff.ts`                    | 1 |
| Manifest pretty-printer          | `apps/turdmod-cli/src/pak-manifest.ts`                | 1 |
| Image-host whitelist file        | `~/.scummy-map/turdmod-image-hosts.txt` (per player)  | 2 |
| Live evidence capture            | `examples/turdmod/welcome-screen/EVIDENCE.md` §8       | 3 |

## Cross-references

* [`gamepires-cui-extension-spec.md`](./gamepires-cui-extension-spec.md)
  — the engine-side path. Pak+DLL is the in-the-meantime alternative.
* [`docs/scum-internals/20-umg-server-driven-surfaces.md`](../scum-internals/20-umg-server-driven-surfaces.md)
  — dossier section answering "what surfaces exist" with evidence.
* [`docs/scum-internals/21-custom-content.md`](../scum-internals/21-custom-content.md)
  — pak overlay basics; UnrealPak path; signing notes.
* [`docs/scum-internals/16-memory-signatures.md`](../scum-internals/16-memory-signatures.md)
  — UE 4.27 class-layout + global-pointer signatures the DLL relies on.
* [`apps/turdmod-loader/src/sigscan.rs`](../../apps/turdmod-loader/src/sigscan.rs)
  — existing reverse-engineered Rust sigscan; the decorator DLL reuses
  this crate.
* [`examples/turdmod/welcome-screen/PAYLOAD-SCHEMA.md`](../../examples/turdmod/welcome-screen/PAYLOAD-SCHEMA.md)
  — payload the markup formatter consumes.
