# UE4 .uproject cook + pak recipe (Phase C onward)

**Purpose.** Take a UE4 4.27.2 project with a single BP class, cook it
for `WindowsServer`, wrap the cooked assets as a `.pak`, deploy to
SCUMServer's `Content/Paks/`, and verify the BP shows up in
`dumpClasses`. This is the executable recipe for Phase C of
`docs/plans/priorities-1-7.md`.

**Status:** Recipe based on the proven Q2 probe-pak path
(`scripts/pak-probe/build-probe-pak.ps1`) extended for BP authoring.
**Not yet run end-to-end with a BP class** — Phase C is gated on the
UE4 4.27.2 install completing.

---

## Prerequisites (one-time setup)

### 1. UE 4.27.2 install — ✅ ALREADY DONE

Verified 2026-05-22: `C:\Program Files\Epic Games\UE_4.27\` is
present (16.8 GB). `UnrealPak.exe` confirmed at the path expected
by `scripts/pak-probe/build-probe-pak.ps1`. Originally installed
for the 2026-05-19 Q2 probe pak work. Both `UE4Editor.exe` and
`UE4Editor-Cmd.exe` are present.

If for some reason it's NOT installed on a fresh machine: Epic
Games Launcher → Unreal Engine → Library → Engine Versions → `+` →
4.27.2 → Install. Uncheck Android/iOS/HTML5 to save ~10 GB. Default
path `C:\Program Files\Epic Games\UE_4.27\` — keep it; every recipe
below assumes that path.

### 2. Set up BP authoring shim (asset-borrowing route)

Per `docs/pak-mod-investigation-plan.md` recommendation, use route 2
(asset-borrowing from Dumper-7 output) rather than route 1 (header
reconstruction). It's simpler and avoids C++ recompile loops.

- Source `.uasset` files: `scumdump/data/extracted/v23128915/sdk/` —
  Dumper-7 has already produced ~9000 headers + the matching uassets.
- We don't need the whole SDK; just the base classes the BP_HelloWorld
  will subclass + their immediate dependencies.

(The exact uasset-extraction step is TBD in Phase C; Dumper-7's
output may need post-processing to be Editor-importable. Worst case
fall back to route 1 per the investigation plan.)

### 3. Create the helloworld-pak project

The scaffold lives at `apps/turdmod-helloworld-pak/` (placeholder
exists; flesh out during Phase C).

```
apps/turdmod-helloworld-pak/
├── README.md             # this file's mirror — phase-c instructions
├── TurdmodHelloWorld.uproject
├── Source/
│   └── TurdmodHelloWorld/
│       ├── TurdmodHelloWorld.cpp           # empty module entry
│       ├── TurdmodHelloWorld.h
│       └── TurdmodHelloWorld.Build.cs
├── Content/
│   └── SCUM/                                # borrowed assets go here
│   └── TurdMOD/
│       └── BP_HelloWorld.uasset             # the BP we'll cook
└── Config/
    └── DefaultEngine.ini                    # ProjectSettings target Win64
```

---

## Cook + pak recipe (per iteration)

### Step 1 — Open UE4 Editor against the project

```powershell
& "C:\Program Files\Epic Games\UE_4.27\Engine\Binaries\Win64\UE4Editor.exe" `
    "C:\Development\Claude\turdmod\apps\turdmod-helloworld-pak\TurdmodHelloWorld.uproject"
```

First open may take 10-15 min (compiling shaders + loading borrowed
assets). Subsequent opens are seconds.

### Step 2 — Author / edit BP_HelloWorld

In the Editor's Content Browser:
- Open `BP_HelloWorld` (or create it: right-click → Blueprint Class →
  pick the SCUM base class to inherit from — typically a Blueprint-
  exposed admin verb or notification dispatcher).
- Add a UFunction `BroadcastHelloWorld(message: FString)` that
  calls `MiscStatics::BroadcastChatLine(self, message, 1)` (Squad
  channel for visibility).
- Save.

### Step 3 — Cook for WindowsServer

From the Editor: **File → Cook Content → Windows Server**.

Or via command-line:

```powershell
& "C:\Program Files\Epic Games\UE_4.27\Engine\Binaries\Win64\UE4Editor-Cmd.exe" `
    "C:\Development\Claude\turdmod\apps\turdmod-helloworld-pak\TurdmodHelloWorld.uproject" `
    -run=cook -targetplatform=WindowsServer -unversioned `
    -OutputDir="C:\Development\Claude\turdmod\apps\turdmod-helloworld-pak\Saved\Cooked"
```

Cooked output lands at:
`apps/turdmod-helloworld-pak/Saved/Cooked/WindowsServer/`.

### Step 4 — Pak the cooked content

UE4's `UnrealPak.exe` takes a "response file" mapping source paths to
in-pak paths. Critical: the in-pak path MUST mirror SCUM's pak
layout (`../../../SCUM/Content/...`) so SCUM's mount logic finds the
assets at expected paths.

```powershell
$cookDir   = "C:\Development\Claude\turdmod\apps\turdmod-helloworld-pak\Saved\Cooked\WindowsServer\TurdmodHelloWorld\Content"
$tempDir   = "C:\Development\Claude\turdmod\tmp\helloworld-pak"
$response  = "$tempDir\response.txt"
$outputPak = "$tempDir\TurdMODHelloWorld_P.pak"

New-Item -ItemType Directory -Path $tempDir -Force | Out-Null

# Build response file: each line is "<absolute source>" "<in-pak path>"
# The in-pak path uses SCUM's convention so SCUM's asset registry finds them.
$lines = @()
Get-ChildItem -Path $cookDir -Recurse -File | ForEach-Object {
    $rel = $_.FullName.Substring($cookDir.Length + 1)
    $inPak = "../../../SCUM/Content/TurdMOD/$rel"
    $lines += "`"$($_.FullName)`" `"$inPak`""
}
Set-Content -Path $response -Value $lines -Encoding UTF8

# UnrealPak invocation
$unrealPak = "C:\Program Files\Epic Games\UE_4.27\Engine\Binaries\Win64\UnrealPak.exe"
& $unrealPak $outputPak "-Create=$response" -compress
```

**Naming convention:** filename suffix `_P` (`TurdMODHelloWorld_P.pak`)
is required — it sets mount priority. Without it, SCUM won't mount
the pak. See `scripts/pak-probe/build-probe-pak.ps1` for the proven
working pattern.

### Step 5 — Deploy to SCUM Server

```powershell
$src = "C:\Development\Claude\turdmod\tmp\helloworld-pak\TurdMODHelloWorld_P.pak"
$dst = "C:\Program Files (x86)\Steam\steamapps\common\SCUM Server\SCUM\Content\Paks\TurdMODHelloWorld_P.pak"

# Stop SCUMServer first — paks can't hot-reload
Get-Process SCUMServer -ErrorAction SilentlyContinue | ForEach-Object {
  Start-Process powershell -ArgumentList '-NoProfile','-Command',"Stop-Process -Id $($_.Id) -Force" `
    -Verb RunAs -Wait -WindowStyle Hidden
}
Start-Sleep -Seconds 2

Copy-Item -Path $src -Destination $dst -Force
```

### Step 6 — Enable the pak-bypass + restart

```powershell
# CRITICAL: TURDMOD_PAK_BYPASS=1 in the env tree Manager will spawn from.
# The simplest way: kill Manager, set env in current shell, restart Manager,
# then click Engine → Start. See feedback_env_var_propagation_to_manager memory.

$env:TURDMOD_PAK_BYPASS = '1'
# kill + relaunch Manager via `pnpm tauri dev` from a shell that has the env
```

Then click **Manager → Engine → Start**. Expect the modal — DON'T
dismiss it. The 60s ExitProcess guard catches the modal-driven exit;
bridge keeps running long enough for verification.

### Step 7 — Verify the BP class mounted

```powershell
cd C:\Development\Claude\turdmod
node tools/engine-rpc-test.mjs listClassInstances --pattern HelloWorld
```

Expected: `scanned: 967265`, matches > 0 with `BP_HelloWorld_C` (or
similar) in the `found` list.

Then fire the UFunction:

```powershell
node tools/engine-rpc-test.mjs runHelloWorld --message "hello from pak"
```

(Bridge handler `runHelloWorld` must be added — straightforward
extension of the existing setTimeOfDay / setEconomy pattern. Locates
the BP class, finds the UFunction, ProcessEvents with the message
FString.)

Expected outcome: a chat line appears in-game on any connected client
saying "hello from pak". That's the Phase C pass criterion per the
plan.

---

## The one Joel-driven step (BP authoring)

Everything except this is automated (UE project scaffold +
cook + deploy scripts + bridge handler + Manager UI smoke card
+ bridge build all done 2026-05-22 ~06:00 PDT, commit forthcoming).
The ONLY remaining manual action:

1. **Open the UE Editor against the project:**
   ```powershell
   & "C:\Program Files\Epic Games\UE_4.27\Engine\Binaries\Win64\UE4Editor.exe" `
       "C:\Development\Claude\turdmod\apps\turdmod-helloworld-pak\turdmod-helloworld-pak.uproject"
   ```
   First open compiles the `HelloWorldPak` C++ module via UBT.
   Wait ~5 min cold. May prompt to "rebuild missing modules" — say
   Yes.

2. **In Content Browser** (bottom of Editor):
   - Right-click in the empty Content pane → **Blueprint Class**
   - Parent class: pick **Actor** (simplest; we'll subclass SCUM
     classes properly in Phase D)
   - Name the asset: `BP_HelloWorld` (exact name; the bridge
     handler matches on this prefix)
   - Save (Ctrl+S).

3. **Open BP_HelloWorld** (double-click):
   - Switch to the **Event Graph** tab (top center)
   - Right-click empty graph → **Add Custom Event** → name it
     `BroadcastHelloWorld`
   - On the new node, click "+" next to "Inputs" → add a pin:
     - Name: `message`
     - Type: **String** (which is FString under the hood)
   - From the Exec output pin, drag → "Print String" node (cheap
     stand-in for `MiscStatics::BroadcastChatLine` until Phase D
     adds the SCUM-class subclass version)
   - Connect the `message` pin → Print String's "In String" input
   - **Compile** (top-left button) → **Save**

4. **Close the Editor.**

5. **Cook + pak:**
   ```powershell
   & C:\Development\Claude\turdmod\scripts\helloworld-pak\cook.ps1
   ```
   Expect ~30 s on first cook, faster on iterations.

6. **Deploy:**
   ```powershell
   & C:\Development\Claude\turdmod\scripts\helloworld-pak\deploy.ps1
   ```

7. **Enable pak bypass + restart Manager:**
   ```powershell
   $env:TURDMOD_PAK_BYPASS = '1'
   # Kill + relaunch Manager via pnpm tauri dev (see
   # feedback_env_var_propagation_to_manager memory).
   ```

8. **Manager → Engine → Start.**
   Modal "Failed to open descriptor file ../../../SCUM/SCUM.uproject"
   will appear (pak-bypass v3 not yet shipped; caller-aware filter
   is a separate session). **LEAVE the modal alone** — clicking
   OK triggers a non-suppressed exit path that kills the server.

9. **Verify (CLI):**
   ```powershell
   cd C:\Development\Claude\turdmod
   node tools\engine-rpc-test.mjs listClassInstances --pattern HelloWorld
   # Expect matches > 0 with BP_HelloWorld_C in the found list

   node tools\engine-rpc-test.mjs runHelloWorld --message "first pak ship"
   # Expect: { ok: true, message: "first pak ship", classFound: "BP_HelloWorld_C", function: "BroadcastHelloWorld" }
   # Server log should show the Print String output.
   ```

10. **Verify (Manager UI):**
    Bridge Smoke page → scroll to "runHelloWorld (Phase C P1)"
    card → set a message → Fire. Same response should appear
    inline.

## Gotchas (collected as we hit them)

(Empty for now — fill in as Phase C session encounters them.)

---

## See also

- `docs/pak-mod-investigation-plan.md` — Q2 outcome that necessitates
  the pak signature bypass + tooling recommendation
- `docs/server-side-custom-ui-plan.md` — P1/P2/P3 sequence that this
  runbook gates
- `docs/plans/priorities-1-7.md` — Phase C entry referencing this
  runbook
- `scripts/pak-probe/build-probe-pak.ps1` — proven minimal pak build
  (txt file only; no BP)
- Memory `pak-bypass-blocks-reflection` — the TURDMOD_PAK_BYPASS env
  var contract
- Memory `env-var-propagation-to-manager` — how to get the env var
  into Manager's process tree
