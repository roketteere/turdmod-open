# TurdMOD Cookbook 🍳

Living recipe book for the TurdMOD content + engine pipeline. Each recipe is a
proven, repeatable procedure with exact commands. Status tags: ✅ proven on this
machine · 🟡 partial / needs a live target · ⏭ designed, not yet run.

Paths assume repo root `C:\Development\Claude\turdmod`. UE: `UE_4.27`.
Shorthand: `UECMD = C:\Program Files\Epic Games\UE_4.27\Engine\Binaries\Win64\UE4Editor-Cmd.exe`.

---

## 🧩 The cook-and-bake bridge (headless UE authoring)

The big one: author **native UE assets with zero editor GUI**. UE4.27 Python can
*create / import / cook / pak / load* but **cannot author visual graphs** (no
`WidgetTree` / `WidgetBlueprintLibrary` bindings — verified). For widget trees and
BP graphs we use a **C++ commandlet** (UE4.27 C++ has the full API). That commandlet
IS the bridge.

### Recipe 1 — Create an empty asset (Python) ✅
`apps/turdmod-helloworld-pak/` is a C++ uproject with an Editor target.
```bash
UECMD "<...>/turdmod-helloworld-pak.uproject" -run=pythonscript \
  -script="<abs path to make_*.py>" -unattended -nopause -nosplash -stdout
```
`make_welcome_widget.py` pattern: `AssetToolsHelpers.get_asset_tools().create_asset(NAME, "/Game/TurdMOD", unreal.WidgetBlueprint, unreal.WidgetBlueprintFactory())` then save via `EditorLoadingAndSavingUtils.save_packages([pkg], False)` (NOT `EditorAssetLibrary` — unavailable in `-run` commandlet mode).

### Recipe 2 — Author a UMG widget tree (C++ commandlet) ✅  ⭐ the breakthrough
Source: `apps/turdmod-helloworld-pak/Source/HelloWorldPak/TurdMODAuthorWidgetCommandlet.{h,cpp}`.
Build deps are **editor-gated** in `HelloWorldPak.Build.cs` (`if (Target.bBuildEditor)` →
UMG, UMGEditor, UnrealEd, Kismet, KismetCompiler) so the **server/game cook never links UnrealEd**.

Build the editor target (recompiles only our module, ~30s warm):
```powershell
& "C:\Program Files\Epic Games\UE_4.27\Engine\Build\BatchFiles\Build.bat" `
  HelloWorldPakEditor Win64 Development `
  -project="<...>\turdmod-helloworld-pak.uproject" -waitmutex
```
Run it (builds the tree: CanvasPanel root → Border → VerticalBox → TextBlocks):
```bash
UECMD "<...>/turdmod-helloworld-pak.uproject" -run=TurdMODAuthorWidget \
  -unattended -nopause -nosplash -stdout
# -> "[TurdMOD] === WELCOME WIDGET AUTHORED: saved=1 root=RootCanvas children=8 ==="
```
To author a **new** widget: copy the commandlet, change the package path + the
`AddLine(...)` calls, rebuild, run. To author **other** graph assets (vehicle BPs,
materials-by-graph), add sibling commandlets the same way — this is the general tool.

### Recipe 3 — Cook a uproject headless ✅
The wrapper `scripts/cook/cook-uproject.ps1` has had PS-5.1 bugs; the raw command is
reliable:
```bash
UECMD "<...>/turdmod-helloworld-pak.uproject" -run=Cook \
  -targetplatform=WindowsServer -unversioned -compressed -unattended -nopause -stdout
# cooks to apps/.../Saved/Cooked/WindowsServer/.../Content/TurdMOD/*.uasset(+.uexp)
```
Cook **twice** for distributable content: `WindowsServer` (server) and `WindowsClient`
(full mesh/material for client render).

### Recipe 4 — Pak + sign-bypass 🟡
Pak the cooked folder with `UnrealPak.exe`, forge/skip the `.sig`. The 6-patch
server bypass (`game-specific hooks`, flag `C:\TurdMOD\pak_bypass.enabled`) lets the
unsigned pak mount. See `scripts/pak-probe/*` and the bypass memories. S2 proved an
unsigned custom pak loads server-side.

### Recipe 5 — Load + run server-side via the bridge ✅ (for BP)
`loadAsset {packagePath:"/Game/TurdMOD/<Name>"}` → `ok:true`; dispatch with the
matching verb (`runHelloWorld` etc.).

### Recipe 5b — Show a native UMG widget on the CLIENT screen ✅ (DONE 2026-06-14)
Renders a cooked WBP in-game with zero ImGui — the engine's own UMG. Loader:
`apps/turdmod-loader/src/native_ui.rs`. Steps:
1. **Deploy the pak as a SCUM chunk** — SCUM only mounts its own `pakchunk*` files, so a
   `*_P.pak` is ignored. Build with `scripts/pak-probe/build-welcome-pak.ps1`, then deploy as
   `<SCUM>\Content\Paks\pakchunk0_s21-WindowsNoEditor.pak` + a dummy `.sig` (copy a vanilla
   chunk's). The 6/6 client pak-bypass passes the unsigned sig. Relaunch (paks mount at boot).
2. **Launch modded client** — BE off (`rename BattlEye`), `turdmod-launcher.exe --scum <exe> --dll <dll>`.
3. **Trigger in-world** — 4-byte LE len + `{"id":"1","method":"showWelcome"}` to `\\.\pipe\turdmod-loader`.
   The loader (on the game thread via a `UGameEngine::Tick` detour) force-loads the package
   (`LoadPackage` AOB), then `UWidgetBlueprintLibrary::Create` + `UUserWidget::AddToViewport`
   via `ProcessEvent` (GEngine vtable[68]). See [[reference_client_umg_panel_re]] for RVAs/AOBs.
@inv UMG calls are game-thread-only (off-thread = crash). Iterating loader code = close client
(DLL locked) → rebuild → relaunch → reload sandbox.

---

## 🎨 Brand & icons

### Recipe 6 — Regenerate the official logo ✅
```bash
node tools/brand/compose-logo.mjs          # emoji art -> brand/turdmod-{logo,icon}.svg
bash tools/brand/render.sh brand/turdmod-icon.svg brand/turdmod-icon-1024.png 256 4
```
Source art = Twemoji 💩 (1f4a9) + 🪰 (1fab0), CC-BY. Theme cyan `#00d4ff`.

### Recipe 7 — Regenerate an app's icon set (tray/taskbar) ✅
From the app dir: `pnpm tauri icon ../../brand/turdmod-icon-1024.png`
(regenerates icon.ico/.png/.icns + all sizes). Launcher tray code: `src-tauri/src/lib.rs`.

### Recipe 8 — Regenerate TMM mod icons ✅
```bash
node tools/brand/gen-mod-icons.mjs   # -> apps/turdmod-manager/public/mod-icons/<cat>.svg
```
One per `ModCategory` (UI/HUD/Squad/Admin/Gameplay/Map/Vehicle/NPC) + default.
`ModCard.tsx` resolves `mod.category` → icon.

### Recipe 9 — Rasterize any SVG (headless Chrome) ✅
```bash
bash tools/brand/render.sh <in.svg> <out.png> <intrinsic_viewbox_size> <scale>
```
Gotchas: needs an **isolated** `--user-data-dir` (a running Chrome locks the default
profile → exit 2); avoid `--virtual-time-budget` (hangs the screenshot); pass the SVG
file path directly, not an HTML wrapper.

---

## ⚙️ Engine binaries

### Recipe 10 — Build the server bridge DLL (Ninja, no xmake) ✅
`vcvars64` then `cmake --build C:\Development\RE-UE4SS\build --config Game__Shipping__Win64
--target TurdMODEngineBridge`. Canonical source overlay: `overlay/cppmods/TurdMODEngineBridge/`.

### Recipe 11 — Build the client loader DLL ✅
`cargo build --release` in `apps/turdmod-loader` (close the running SCUM client first —
it locks `turdmod_loader.dll`).

---

## Notes
- **Never** add UnrealEd/editor deps to a module without the `bBuildEditor` guard — the
  server cook will fail to link.
- Keep this cookbook current: when a recipe changes or a new one is proven, add/update it
  here in the same commit.
