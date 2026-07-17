# turdmod-engine-bridge

In-process C++ bridge loaded by [UE4SS](https://github.com/UE4SS-RE/RE-UE4SS)
inside a UE4 game server. Exposes RPC handlers that let external tools
(Manager UI, companion apps) read engine state and drive actions via
UE4 reflection.

Links UE4SS's C++ reflection API on one side and the TurdMOD loader DLL
on the other via an in-process C ABI — direct function-pointer calls, no
IPC, no file polling.

## How it wires together

```
GameServer.exe (single process)
├── turdmod_server_loader.dll   (Rust)
│   ├── admin_api: named-pipe JSON-RPC server
│   ├── extern_api: C-ABI surface — register_handler, emit_event
│   └── exports: turdmod_engine_* (resolved via GetProcAddress)
└── UE4SS.dll                   (C++)
    └── UE4SS/Mods/TurdMODEngineBridge/dlls/main.dll  (THIS MOD)
        └── on_unreal_init():
            1. Mirror UE4SS resolved addresses (GUObjectArray, FName, etc.)
            2. GetModuleHandle for the loader DLL
            3. GetProcAddress for each turdmod_engine_* export
            4. Register RPC handlers
```

## Build

The bridge links UE4SS's C++ ABI and cannot build standalone. Place the
source under UE4SS's `cppmods/TurdMODEngineBridge/src/dllmain.cpp` and
build with cmake:

```powershell
# Sync canonical source into UE4SS tree
Copy-Item -Force `
  apps/turdmod-engine-bridge/src/TurdMODEngineBridge.cpp `
  <UE4SS_ROOT>/cppmods/TurdMODEngineBridge/src/dllmain.cpp

# Build (cmake configure must be done once first)
cmake --build <UE4SS_ROOT>/build --config Shipping --target TurdMODEngineBridge
```

Build env: C++20, MSVC, `/utf-8`, PolyHook2 for x64 detours.

## Deploy

Copy the built DLL to:
```
<GameServer>/Binaries/Win64/UE4SS/Mods/TurdMODEngineBridge/dlls/main.dll
```

Ensure `enabled.txt` exists in the mod's parent directory. The game
server must be stopped before copying (DLL is loaded in-process).

## Adapting for your game

The bridge ships as a generic UE4 modding framework. Game-specific
values are marked with `/* YOUR_GAME_* */` comments throughout the
source. You'll need to:

1. Replace class name filters (e.g., `YOUR_GAME_PC_CLASS`) with your
   game's PlayerController class name
2. Replace UFunction names in the event dispatch table with your game's
   chat/login/logout function names
3. Find `StaticConstructObject_Internal`'s RVA for your game build
   (use a sig-scanner or PDB symbols) and set `kStaticConstructObjectRVA`
4. Add game-specific handlers for your mod's features

## Registered handlers (core framework)

The `regs[]` array in the source registers all handlers. Core framework
handlers include:

```
ping                  broadcastChat         teleportPlayer
getOnlinePlayers      dumpUFunctions        findFunctions
dumpClasses           runAdminCommand       sendChat
dumpWidgets           describeWidget        readClassValues
readActorByPtr        findInstancesByClass  writeClassDefault
describeFunction      dumpAllClasses        dumpAllEnums
dumpAllStructs        createObject          readMemory
writeMemory           patchInstructions     unpatchInstructions
listPatches           readConfig            writeConfig
listConfigFiles       readConfigFile        writeConfigFile
getNearbyActors       writeActorProperty    callActorFunction
imageBase             findUInt64            listHandlers
```

## Key patterns

- **Mirror UE4SS globals** in `on_unreal_init` via
  `RC::UE4SSRuntime::GetResolvedAddresses()`. Skipping any silently
  no-ops reflection later.
- **ProcessEvent hook is deferred** — `GUObjectArray` is empty at init
  time. The global PE hook installs on first RPC via
  `ensure_hook_installed_once()`.
- **Serial RPC only.** Single named pipe + game-thread serialization.
  Never fire heavy handlers (`findFunctions`, `dump*`) in parallel.

## License

PolyForm Noncommercial 1.0.0
