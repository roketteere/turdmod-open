# CMakeLists.txt — TurdMOD Engine Bridge (UE4SS C++ Mod)
#
# This mod cannot build standalone — it links against UE4SS's C++ ABI,
# which is only available from a built UE4SS source tree. The standard
# pattern is to drop this directory into UE4SS's `CppUserMods/` folder
# and let UE4SS's own CMake driver build it as part of the wider build.
#
# Build instructions:
#   1. git clone --recurse-submodules https://github.com/UE4SS-RE/RE-UE4SS.git
#   2. Copy this entire `apps/turdmod-engine-bridge/` directory into
#      <ue4ss-clone>/CppUserMods/TurdMODEngineBridge/
#   3. Add `TurdMODEngineBridge` to <ue4ss-clone>/CppUserMods/mods.txt
#   4. Build UE4SS per its README (cmake --preset Game__Shipping__Win64).
#   5. Output: <ue4ss-build-dir>/Output/Game__Shipping__Win64/UE4SS/CppMods/
#               TurdMODEngineBridge/dlls/main.dll
#   6. Drop that into the game's
#      <game-install>/SCUM/Binaries/Win64/Mods/TurdMODEngineBridge/dlls/main.dll
#
# UE4SS's own `CppUserMods` driver supplies the include paths, link
# targets, and target-name convention. This file just declares the
# sources and target name in the form that driver expects.

cmake_minimum_required(VERSION 3.18)

set(TARGET TurdMODEngineBridge)
project(${TARGET})

# UE4SS's CppUserMods driver creates the target via `add_library(${TARGET} SHARED ...)`.
# We provide the source file list; the driver wires up include paths to
# UE4SS internals (Mod/, Unreal/, Helpers/, DynamicOutput/) and links to
# UE4SS.lib.
add_library(${TARGET} SHARED
    src/TurdMODEngineBridge.cpp
)

target_include_directories(${TARGET} PRIVATE
    src
    include
)

target_compile_features(${TARGET} PRIVATE cxx_std_20)

# UE4SS uses wchar_t string literals via the STR() macro; ensure /utf-8
# so wchar_t * literals are encoded correctly.
if(MSVC)
    target_compile_options(${TARGET} PRIVATE /utf-8)
endif()
