# turdmod-rich-decorators

Minimal Rust DLL that upgrades SCUM's engine-stock UMG `URichTextBlock`
decorator classes via runtime function-detour, adding three new markup
tag behaviours that the vanilla client's compiled C++ doesn't ship:

| Tag                                    | What it renders |
|---|---|
| `<img src="https://..."/>`             | URL-fetched inline image (whitelisted hosts only) |
| `<a href="..." key="...">label</a>`    | Clickable hyperlink → Steam Overlay browser |
| `<dismiss key="Escape" label="..."/>`  | Keybind that closes the parent panel widget |

Sibling crate to `apps/turdmod-loader/`. **Intentionally minimal** — no
DXGI hook, no ImGui, no UE4 game-object hooks, no Lua runtime. Few-
hundred-line surface; auditable independently from the kitchen-sink
loader DLL.

## Status

**Phase A — boot-and-log.** Today the DLL only logs its attach + version
to `%LOCALAPPDATA%/TurdMOD/decorators.log`. Validates that the DLL loads
cleanly into `SCUM.exe` via the existing `turdmod-launcher.exe`
injector before any engine-internal sigscan / detour work starts.

**Phase B — engine-stock detour (next).** Plan:

1. Sigscan UE 4.27 globals — reuse pattern catalog from
   `../src/sigscan.rs` (the loader's existing scanner). Need
   `URichTextBlockImageDecorator::CreateDecorator()`,
   `URichTextBlockKeyPromptDecorator::CreateDecorator()`,
   `URichTextBlockActionPromptDecorator::CreateDecorator()`.
2. Install function-detours via vendored MinHook (already vendored in
   the loader crate; share via path-dep or duplicate the small wrapper).
3. Detour bodies parse the markup tag attributes (UE4 passes them
   through to `CreateDecorator`), do the upgraded behaviour
   (URL fetch / launch / keybind), return the appropriate `ITextDecorator`
   Slate widget.

**Phase C — wire-up.** The TurdMOD welcome-screen mod's markup formatter
publishes via existing `WelcomeMessage` / `MOTD` / `#SendNotification`
surfaces; replaced rendering paints the panel.

## Build

```bash
cd apps/turdmod-loader/decorators
cargo build --release
# Output: target/release/turdmod_rich_decorators.dll
```

The release profile inherits the loader's settings (LTO fat, single
codegen unit, panic abort, stripped symbols) so the resulting DLL is
small and predictable.

## Install (manual, until the CLI lands a `turdmod-cli decorators install`
flow)

The DLL needs to be loaded into the SCUM game process. Two paths:

### Via the existing turdmod-launcher (preferred)

```powershell
# from a PowerShell window, after closing the SCUM game:
& "C:\Development\Claude\turdmod\apps\turdmod-loader\launcher\target\release\turdmod-launcher.exe" `
    --extra-dll "C:\Development\Claude\turdmod\apps\turdmod-loader\decorators\target\release\turdmod_rich_decorators.dll"
```

The launcher injects both the kitchen-sink loader and the decorators
DLL together. Each reports its own attach line in its own log file.

### As a standalone dxgi.dll proxy (Strategy C)

Rename the built DLL to `dxgi.dll` and drop next to `SCUM.exe`. Windows
loader resolves the import. Less flexible (one DLL at a time) but works
when the launcher isn't available.

## Logs

`%LOCALAPPDATA%/TurdMOD/decorators.log` — append-only NDJSON-ish lines.
Distinct from `loader.log` so the two DLLs' logs don't interleave when
both are loaded together.

## See also

* [`docs/turdmod/pak-plus-decorator-dll-spec.md`](../../../docs/turdmod/pak-plus-decorator-dll-spec.md)
  — implementation spec (carrying the all-DLL pivot).
* [`docs/scum-internals/20-umg-server-driven-surfaces.md`](../../../docs/scum-internals/20-umg-server-driven-surfaces.md)
  — research finding "no server-driven RichTextBlock exists in vanilla";
  this DLL fills that gap for private/modded servers.
* [`../src/sigscan.rs`](../src/sigscan.rs)
  — UE 4.27 sigscan patterns the loader already uses; phase B reuses
  them for the decorator class globals.
