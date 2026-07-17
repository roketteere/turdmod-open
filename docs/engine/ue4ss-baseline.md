# UE4SS Baseline — Empirical Stage 2 Results

**Date:** 2026-05-15
**Verdict:** **GO** — UE4SS v3.0.1 runs cleanly on `SCUMServer.exe` (UE4 4.27, dedicated server) with zero graphics-init errors despite the binary's D3D/DXGI imports.

Captured UE4SS log: [`tools/engine-validation/ue4ss-bootstrap.log`](../../tools/engine-validation/ue4ss-bootstrap.log)
Verdict JSON: [`tools/engine-validation/ue4ss-verdict.json`](../../tools/engine-validation/ue4ss-verdict.json)

## Strategic implication

The original Sprint 2 plan was to derive our own AOB patterns for UE4 globals (`GWorld`, `FNamePool`, `FUObjectArray`, etc.) using `tools/engine-validation/sigscan_transfer.py`. That returned **PARTIAL** — only 2 of 5 canonical patterns matched SCUM's server build.

**Pivot:** instead of re-deriving patterns, **use UE4SS as the foundation**. UE4SS's `patternsleuth` module already finds everything we need in 152ms (per the captured log). The new Sprint 2 plan is:

1. **Ship `turdmod-server-loader.dll` alongside `UE4SS.dll`** — the launcher already supports `--dll` (primary) + `--extra-dll` (secondary). Operators inject both.
2. **Call UE4SS from our DLL** rather than re-implementing sig-scan. UE4SS exposes `GUObjectArray`, `FName::ToString`, `StaticConstructObject_Internal`, etc. as C++ symbols our DLL can resolve from `UE4SS.dll`'s exports.
3. **Use UE4SS's Lua API** as the primary scripting surface for engine-side mods rather than mlua-from-scratch — UE4SS already wires up `UObject` access, function hooks, and `ExecuteInGameThread()` (the game-thread marshalling we'd have to build ourselves).

This collapses ~80% of Sprint 2's planned work.

## Captured empirical data

### Process layout

- **Architecture:** x64 (AMD64), 118.6 MiB
- **UE4 EngineVersion:** 4.27 (matches client)
- **MainExe base:** `0x7ff6d67c0000`, size `0x79ac000`
- **Build configuration:** `Game__Shipping__Win64 (MSVC)`
- **patternsleuth scan time:** 152.886ms (fast — no concern for boot delay)

### Resolved globals (absolute addresses; subtract `0x7ff6d67c0000` for RVAs)

| Symbol | Absolute address | RVA |
|---|---|---|
| `GUObjectArray` | `0x7ff6dd8eed10` | `0x712ED10` |
| `GMalloc` | `0x7ff6dd869d20` | `0x70A9D20` |
| `FName::ToString` | `0x7ff6d8ffaa80` | `0x283AA80` |
| `FName::FName(wchar_t*)` | `0x7ff6d8fec070` | `0x282C070` |
| `StaticConstructObject_Internal` | `0x7ff6d9207e40` | `0x2A47E40` |
| `FText::FText(FString&&)` | `0x7ff6d8ee8a60` | `0x2728A60` |
| `ProcessInternal` | `0x7ff6d9200160` | `0x2A40160` |
| `ProcessLocalScriptFunction` | `0x7ff6d9200270` | `0x2A40270` |

Absolute addresses are ASLR-randomised per-launch; RVAs are stable until SCUM patches the binary.

### Confirmed UE4 type field offsets

Source: `MEMBER OFFSETS` block in [`ue4ss-bootstrap.log`](../../tools/engine-validation/ue4ss-bootstrap.log) (lines 87–360). Read directly from the log when implementing hooks — UE4SS flattens all dumped offsets under the `UObjectBase::` namespace prefix, but many actually belong to derived classes (e.g. `Tags`, `RootComponent`, `Owner` belong to `AActor`).

Highlights Sprint 2 needs:

```
UObjectBase::ClassPrivate     = 0x10   // every UObject's UClass pointer
UObjectBase::NamePrivate      = 0x18   // FName
UObjectBase::OuterPrivate     = 0x20
UObjectBase::InternalIndex    = 0x0C

UStruct::SuperStruct          = 0x40
UStruct::Children             = 0x48
UStruct::ChildProperties      = 0x50
UStruct::PropertiesSize       = 0x58

UFunction::FunctionFlags      = 0xB0
UFunction::NumParms           = 0xB4
UFunction::Func               = 0xD8   // the trampoline we hook for UFunction calls

UClass::ClassConstructor      = 0xB0
UClass::ClassDefaultObject    = 0x118
UClass::FuncMap               = 0x130  // function dispatch table

UWorld::AuthorityGameMode     = 0x118
UWorld::TimeSeconds           = 0x5A0
UWorld::DeltaTimeSeconds      = 0x5B0
UWorld::PlayerNum             = 0x558
```

Full table in the log.

## Sprint 2 revised plan

1. **(no rust work)** Document operator deployment: drop `UE4SS.dll` + `UE4SS-settings.ini` + `Mods/` next to `SCUMServer.exe`, then launch via `turdmod-launcher --dll turdmod_server_loader.dll --extra-dll UE4SS.dll`.
2. **(small rust work)** Make `turdmod-server-loader` detect whether `UE4SS.dll` is loaded in-process. If yes, use UE4SS's C++ ABI (via `GetProcAddress` on the loaded module) to access `UObject`s. If no, fall back to our own sig-scan path.
3. **(medium rust work)** Implement `EngineApi` methods (`broadcastChat`, `teleportPlayer`, `getOnlinePlayers`, `spawnVehicle`) on top of UE4SS's `UObjectGlobals::FindFirstOf`, `UFunction::ProcessEvent`, and friends.
4. **(small rust work)** Bridge UE4SS events (chat, login) into our `admin_api::EventBroadcaster` so the companion gets them over the named pipe.

The pure-Rust sig-scan path in `server_hooks.rs` becomes a fallback rather than the primary code path. The `server-hooks-DESIGN.md` document still applies for the fallback case.

## Known gotchas observed in the log

- `BPModLoaderMod` fails to load: `"UE4SS does not support loading mods for this game"`. This is the **Blueprint mod loader** sub-mod and depends on `<RootGamePath>/Game/Binaries/Win64` layout, which SCUM doesn't have (it uses `SCUM/Binaries/Win64`). Irrelevant — UE4SS core still works. We don't need BP mod loading; we use the Lua and C++ APIs.
- `[Lua] ConsoleClass, GameViewport, or ViewportConsole is invalid` — expected on a dedicated server (no viewport). UE4SS gracefully handles it.
- `dwmapi.dll` is the proxy DLL UE4SS ships for the "drop-next-to-EXE" install path. We're not using it (we're injecting via CreateRemoteThread through the launcher), so this proxy is unused in our deployment.

## Re-running

```powershell
# (run from elevated PowerShell)
& 'C:\Development\Claude\turdmod\tools\engine-validation\ue4ss_test.ps1'
```

The script now has working defaults for Joel's install. To re-extract a verdict from the existing log without spawning the server:

```powershell
& 'C:\Development\Claude\turdmod\tools\engine-validation\ue4ss_test.ps1' -SkipInjection -UE4SSPath 'C:\Tools\UE4SS'
```
