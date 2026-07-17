# TurdMOD: UE 4.27 Content Pak Recipe (SCUM v23128915)

**Target:** Ship a Blueprint‑callable function (`BP_TurdMODQuartermaster::SpawnAndGiveItem`) inside a signed patch‑pak that loads into SCUM via the existing UE4SS bridge bypass.  
**Total time:** ~4–8 hours for a working proof‑of‑concept.

---

## 1. Project Setup

### 1.1. Create the UE 4.27 C++ Project

1. Launch **Unreal Engine 4.27.2** (Epic Launcher build).  
2. **New Project** → **C++** tab → **Blank** template (not Third Person, not Blueprint only).  
   - **Why C++?** We need to compile a Blueprint Function Library (BFL) that uses `ProcessEvent` to call SCUM functions by name. Pure Blueprint cannot reference SCUM types at compile time.
3. **Project Name:** `TurdMODContent`  
   - Keep default settings: no starter content, target platform Windows.
4. Choose a folder outside the Steam directory (e.g., `D:\UE_projects\`).

### 1.2. Configure Project for Pak Output

- Open **Project Settings** → **Packaging**  
  - *Use Pak File (for shipped content)*: ☑ Enable  
  - *Use Io Store*: **Disable** (SCUM does not use Io Store)  
  - *Encrypt Pak*: **Disable** (our pak will be unencrypted; the bypass handles SCUM’s encryption on the game side)  
  - *Sign Pak*: **Disable** (we rely on the v3.1/v4 bypass; see Section 4)

- Open **Project Settings** → **File Rules**  
  - Ensure *Use Pak File Rules* is disabled (we want a simple cooked pak).

- Open your `.uproject` file in a text editor and **add the following block** inside `"Modules"` if missing (the C++ wizard should have added it):
```json
"Modules": [
  {
    "Name": "TurdMODContent",
    "Type": "Runtime",
    "LoadingPhase": "Default"
  }
]
```

- **No encryption key needed** for our own pak. The game’s AES key is already handled by the bridge bypass.

---

## 2. Authoring the C++ Blueprint Function Library + Blueprint

### 2.1. Create the C++ BFL Class

In UE Editor:  
1. **File** → **New C++ Class** → **Parent Class:** `Blueprint Function Library` → **Next**  
2. **Name:** `TurdMODBFL` (header will be `TurdMODBFL.h`, implementation `TurdMODBFL.cpp`)  
3. **Public** (check “Show All Classes” if needed). Click **Create Class**.

### 2.2. Implement `TurdMODBFL.h`

Replace the content with:

```cpp
#pragma once

#include "CoreMinimal.h"
#include "Kismet/BlueprintFunctionLibrary.h"
#include "TurdMODBFL.generated.h"

UCLASS()
class TURDMODCONTENT_API UTurdMODBFL : public UBlueprintFunctionLibrary
{
    GENERATED_BODY()

public:
    /**
     * Spawns 'Count' instances of the item class 'ItemClassName'
     * and places them into the inventory of the Prisoner named 'PlayerName'.
     * Returns false if player not found or item class not loaded.
     */
    UFUNCTION(BlueprintCallable, Category = "TurdMOD")
    static bool SpawnAndGiveItem(const FString& PlayerName, const FString& ItemClassName, int32 Count);

    /**
     * Returns the Prisoner object whose name matches 'PlayerName'.
     * Names are the in‑game display names (e.g., "TechyRican").
     */
    UFUNCTION(BlueprintCallable, Category = "TurdMOD")
    static UObject* FindPrisonerByName(const FString& PlayerName);
};
```

### 2.3. Implement `TurdMODBFL.cpp`

This is the critical part. We use **reflection** – no SCUM header includes needed.

```cpp
#include "TurdMODBFL.h"
#include "Engine/World.h"
#include "UObject/Class.h"
#include "UObject/UObjectIterator.h"

// --------------------------------------------------------------------------------
// Helper – search all UObjects of a given class name (fast path using GetObjectsOfClass)
// --------------------------------------------------------------------------------
static UClass* FindSCUMClass(const TCHAR* ClassName)
{
    // e.g. ClassName = "/Script/SCUM.Prisoner"
    return FindObject<UClass>(nullptr, ClassName);
    // If that fails, try StaticLoadObject with the same string.
}

static UObject* FindSCUMObjectByName(const FString& TargetName)
{
    // Walk all Prisoner instances (use the class from the reflection dump)
    UClass* PrisonerClass = FindSCUMClass(TEXT("/Script/SCUM.Prisoner"));
    if (!PrisonerClass) return nullptr;

    TArray<UObject*> Prisoners;
    GetObjectsOfClass(PrisonerClass, Prisoners, false, RF_NoFlags);
    for (UObject* P : Prisoners)
    {
        // The 'Name' property might be string or FName; typical SCUM uses a 'Name' FName property.
        // Adjust based on scumpdump: e.g. "Name" as FString property.
        FString DisplayName;
        if (FProperty* NameProp = P->GetClass()->FindPropertyByName(FName("Name")))
        {
            // Handle both FName and FString properties
            if (FStrProperty* StrProp = CastField<FStrProperty>(NameProp))
                DisplayName = StrProp->GetPropertyValue_InContainer(P);
            else if (FNameProperty* NameProp2 = CastField<FNameProperty>(NameProp))
                DisplayName = NameProp2->GetPropertyValue_InContainer(P).ToString();
        }
        if (DisplayName.Equals(TargetName, ESearchCase::IgnoreCase))
            return P;
    }
    return nullptr;
}

// --------------------------------------------------------------------------------
// Public functions
// --------------------------------------------------------------------------------
bool UTurdMODBFL::SpawnAndGiveItem(const FString& PlayerName,
                                   const FString& ItemClassName,
                                   int32 Count)
{
    // 1. Find player
    UObject* Player = FindPrisonerByName(PlayerName);
    if (!Player) return false;

    // 2. Load item class
    FString ItemClassPath = FString::Printf(TEXT("/Script/SCUM.%s"), *ItemClassName);
    UClass* ItemClass = FindObject<UClass>(nullptr, *ItemClassPath);
    if (!ItemClass) return false;

    // 3. Get the World from the Player
    UWorld* World = Player->GetWorld();
    if (!World) return false;

    // 4. Create items using NewObject (UWorld as outer)
    //    For non‑Actor items this is the correct primitive.
    for (int32 i = 0; i < Count; ++i)
    {
        UObject* NewItem = NewObject<UObject>(World, ItemClass);
        if (!NewItem) return false;

        // 5. Call Prisoner::PlaceItemInInventoryOrHolster(NewItem, true)
        //    Use ProcessEvent on the Prisoner object.
        UFunction* PlaceFunc = Player->GetClass()->FindFunctionByName(
            FName("PlaceItemInInventoryOrHolster"));
        if (!PlaceFunc) return false;

        struct FParams
        {
            UObject* Item;
            bool  tryToJoinItems;
        };
        FParams Params;
        Params.Item = NewItem;
        Params.tryToJoinItems = true;
        Player->ProcessEvent(PlaceFunc, &Params);
    }
    return true;
}

UObject* UTurdMODBFL::FindPrisonerByName(const FString& PlayerName)
{
    return FindSCUMObjectByName(PlayerName);
}
```

> **Important:** The class path `"/Script/SCUM.Prisoner"` must match exactly what `scumpdump` shows. Verify by grepping the dump:  
> `grep -i '"Prisoner"' scumdump/data/extracted/v23128915/ClassNames.txt`  
> Adjust if the path includes `_C` (e.g., `Prisoner_C`), then change to `"/Script/SCUM.Prisoner_C"`.  
> The property name `"Name"` may also differ – check the reflection dump for the correct FName.

### 2.4. Compile the C++ Code

- Click **Build** (or **Ctrl+Shift+F5**).  
- If any errors, double-check the class path strings and property names. The project must compile successfully.

### 2.5. Create the Blueprint Class `BP_TurdMODQuartermaster`

1. In **Content Browser**, right‑click → **Blueprint Class** → **Choose Parent Class** → **Object** (since we want a pure UObject, not an Actor).  
2. Name it `BP_TurdMODQuartermaster`.  
3. Open the Blueprint Editor and create a **Function** named `SpawnAndGiveItem`:
   - **Inputs:** `playerName` (FString), `itemClassName` (FString), `count` (int)  
   - **Output:** `returnValue` (bool)  
4. Drag a **Call Function** node into the graph, search for `SpawnAndGiveItem` from **TurdMODBFL**. Wire the inputs.  
5. Connect the return bool to the function’s return.  
6. Compile and save.  

> **Why a separate Blueprint?** The bridge calls functions on a Blueprint class (as shown in `BP_HelloWorld`). The C++ BFL is just a helper – the BP remains the public API.

---

## 3. Cooking the Pak

### 3.1. Package with UAT (Recommended)

Open a **Developer Command Prompt** (Visual Studio tools) and run:

```cmd
cd "<UE_4.27_Engine>/Engine/Build/BatchFiles"
RunUAT.bat BuildCookRun -project="D:/UE_projects/TurdMODContent/TurdMODContent.uproject" -platform=Win64 -targetplatform=WindowsServer -cook -build -stage -pak -archive -archivedirectory="D:/UE_projects/TurdMODContent/PakOutput"
```

- **`-targetplatform=WindowsServer`** – SCUM’s dedicated server runs on the same binary as the client, but using `WindowsServer` ensures we get the proper subdirectories.
- **`-build`** ensures the C++ module is compiled.
- **`-pak`** enables pak creation.
- **`-archive`** copies the cooked content to a clean folder.

### 3.2. Locate the Cooked Pak

After the command completes, find:

```
D:/UE_projects/TurdMODContent/PakOutput/WindowsServer/TurdMODContent/Content/Paks/TurdMODContent-WindowsServer.pak
```

Rename it to reflect the **patch‑pak** convention:

```cmd
move "TurdMODContent-WindowsServer.pak" "001_TurdMODQuartermaster_P.pak"
```

- **`_P` suffix** is mandatory for UE4 patch‑pak ordering.  
- **`001`** ensures load priority (higher number = later load; 001 loads early, fine for asset overrides).

---

## 4. Pak Signing / Bypass

Choose **Option A** (matches the probe‑pak flow).

### Option A – No Signature File (Recommended for v1)

- Do **not** create a `.sig` file.  
- The team’s signature bypass (commits `5eea8e6`, `3c6562a`) suppresses the missing‑signature failure. This works exactly like the `BP_HelloWorld` probe‑pak.

### Option B – Generate a Structurally Valid `.sig` (Experimental)

If bypass for signature files is being investigated, you can create a dummy signature:

```cmd
openssl genrsa -out private.pem 2048
openssl rsa -in private.pem -pubout -out public.pem
<UE Engine>\Engine\Binaries\Win64\UnrealPak.exe Sign "001_TurdMODQuartermaster_P.pak" -PRIVATE=private.pem -PUBLIC=public.pem -OUTPUT_SIGNATURE="001_TurdMODQuartermaster_P.sig"
```

Place both the `.pak` and `.sig` into the target `Content/Paks/` folder. The bypass may still need to suppress integrity checks.

---

## 5. Deploy + Smoke Test

### 5.1. Deploy to SCUM Server

Copy the pak file:

```
Source: D:/UE_projects/TurdMODContent/PakOutput/WindowsServer/TurdMODContent/Content/Paks/001_TurdMODQuartermaster_P.pak
Destination: C:/Program Files (x86)/Steam/steamapps/common/SCUM Server/SCUM/Content/Paks/001_TurdMODQuartermaster_P.pak
```

If the server is running, restart it.

### 5.2. Verify the Function is Reachable via Bridge

Run the bridge’s RPC test tool:

```cmd
node tools/engine-rpc-test.mjs findFunctions --grep SpawnAndGiveItem
```

Expected output (partial):
```
Function: SpawnAndGiveItem (BlueprintCallable) on class BP_TurdMODQuartermaster_C
```

If found, the pak has mounted and the BP is registered.

### 5.3. Call the Function

```cmd
node tools/engine-rpc-test.mjs runQuartermasterSpawn --playerName TechyRican --itemClass BPC_MP5 --count 1
```

> **Note:** The bridge must have a handler `runQuartermasterSpawn` that calls `BP_TurdMODQuartermaster::SpawnAndGiveItem` via `ProcessEvent`. If the handler does not exist yet, the recipe assumes the team will add it in a follow‑up. For the current smoke test, just verifying reachability is sufficient.

---

## 6. Debug Checklist (Pak Not Mounting)

Look in `SCUM/Saved/Logs/SCUM.log` for these patterns:

| What to grep | Meaning | Action |
|--------------|---------|--------|
| `LogPakFile: ... mounted` | Pak loaded | Good – proceed |
| `LogPakFile: ... signature` | Bypass not suppressing signature error | Check bypass commits are applied and active |
| `LogAssetRegistry` | Assets registered | Good – your classes should appear here |
| `Fatal Error` | Crash | Possible pak corruption or incompatible asset format |
| `LogCompiledIn: Failed to load` | Pak file not found | Check filename and path |
| `No matching PakFile` | Pak signature or hash rejected | Re‑examine bypass layer 3 |

Also check if the bridge itself logs errors related to `BP_TurdMODQuartermaster_C` not being found.

---

## 7. Open Questions (Resolved by Experiment)

1. **Does the probe‑pak signature path accept Blueprint‑only content paks with full logic?**  
   The HelloWorld pak contained only a trivial class. This experiment will test whether a BP with complex calls (even if via BFL) loads without triggering Layer 3.

2. **Does cooking introduce metadata that triggers Layer 3?**  
   Probe‑paks were likely cooked with minimal settings. Our BFL and BP may add dependencies (e.g., `Engine` module). The log will show if `FatalError` occurs.

3. **Is `NewObject<UObject>(World, ItemClass)` sufficient for SCUM items?**  
   Some items may require a specific outer (e.g., `UInventory`), or have a custom `GetDefaultObject` pattern. If spawn fails, we will need to identify the correct static creation function from the scumpdump reflection dump (e.g., `UItem::Create` or `UInventory::AddItem`).

4. **Can BPs cooked against vanilla UE 4.27 reference SCUM types (like `Prisoner`)?**  
   Our BFL uses `FindObject` and `GetObjectsOfClass` – no header bindings. That should work as long as the SCUM DLLs are already loaded in the process. The test will confirm.

---

## Shortest Path to Smoke‑Testable Result

Given 4–8 hours, follow this order:

1. **Set up the C++ project** – 30 min  
2. **Write the BFL** (copy the code above, adjust class paths after grepping `scumpdump`) – 1 h  
3. **Create the Blueprint** – 15 min  
4. **Build & Cook** (UAT command) – 30 min (first time may take longer)  
5. **Deploy + Sign** (Option A) – 5 min  
6. **Restart server + Verify** via bridge `findFunctions` – 10 min  
7. If reachable, celebrate. If not, debug logs (Section 6).  

**Total hands‑on focus:** ~2.5 hours. The remaining time can be spent refining the BFL logic or investigating Layer 3 if it fails.

---

**TurdMOD Team** – You now have a complete, executable recipe. Proceed step‑by‑step, and when in doubt, check the scumpdump JSON for exact class and property names. Good luck.