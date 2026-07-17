```markdown
# Custom GUI Maker Pipeline — Technical Specification

**Version:** 1.0  
**Target Engine:** Unreal Engine 4.27.2  
**Target Game:** SCUM (using turdmod UE4SS bridge)  
**Authors:** turdmod team  
**Status:** Planning — Wave A  

---

## 1. Pipeline Overview

The turdmod Custom GUI Maker enables mod authors to design and ship custom UMG widgets to SCUM players.  
The pipeline consists of five stages:

1. **Authoring** — Using UE4.27 Editor with a turdmod-provided project template (`TurdMODContent`), authors create `UserWidget` subclasses following a strict contract.
2. **Cooking & Packing** — The editor packages the widget assets into a `.pak` file using Unreal Engine’s cooking pipeline.
3. **Distribution** — The pak is uploaded to `turdmod-marketplace.com` and optionally encrypted with AES.
4. **Client Installation** — Players download the pak and place it in `SCUM\Content\Paks\` or use the turdmod-manager.
5. **Server Invocation** — The turdmod bridge (via UE4SS) calls a server-side C++ function that triggers a `NetMulticast_*` RPC, causing all clients with the pak to instantiate the widget.

**Key insight:** SCUM widgets are client-rendered. The server cannot directly create UMG widgets—it must replicate instructions to clients.  
The bridge uses a **NetMulticast BlueprintImplementableEvent** defined in a base class that lives inside a mandatory `TurdMODLoader` pak (shipped with every turdmod-aware server).  
When the server wants to show a custom widget, it calls `NetMulticast_ShowCustomWidget(WidgetClass, PayloadJson)` which is implemented by the mod’s widget subclass.

---

## 2. UMG Widget Contract

For a widget to be usable with the turdmod system, it **must** conform to the following requirements:

### 2.1 Base Class

Every custom widget **must** inherit from **`UTurdMODUserWidget`**, which itself inherits from `UUserWidget`.  
This base class provides:

- A `NetMulticast_ShowCustomWidget` function (BlueprintNativeEvent, implemented on the server-side bridge but overridden in the widget class itself via a `BlueprintImplementableEvent` node).
- A `WidgetName` (FName) string metadata field.
- A `PersonaNamespace` (FString) metadata field.
- A `WidgetVersion` (int32) metadata.

**Usage Instructions in UE4.27 Editor:**  
1. Create a new Blueprint Class → Search `TurdMODUserWidget` under “All Classes”.  
2. Name it `BP_MyWidget`.  
3. In the Class Defaults → "Class Metadata" template (we provide a blueprint function library to fill these).

### 2.2 Required Blueprint Functions

Any custom widget **must** implement the following event:

| Event Signature | Description |
|----------------|-------------|
| `Event OnShowFromServer(string PayloadJson)` | Called when the server pushes a payload to this widget. Inside this event, the widget should deserialize the JSON (using the turdmod `JsonObject` library) and populate its UI elements. |

Additionally, the widget class **must** call the parent’s `NetMulticast_ShowCustomWidget_Implementation()` in the **Construct** event (to register with the bridge). This is provided as a default node in the template.

### 2.3 Metadata Requirements

Set these in the **Class Defaults → Class Metadata** fields:

| Key | Value Example |
|-----|---------------|
| `PersonaNamespace` | `Doctor` |
| `WidgetName` | `HealingWheel` |
| `WidgetVersion` | `1` |

These are used by the bridge to resolve which widget to instantiate.

### 2.4 Asset Path Convention

Place your widget Blueprint asset at:  
`/TurdMODContent/Widgets/<PersonaNamespace>/<WidgetName>.uasset`

For example: `/TurdMODContent/Widgets/Doctor/HealingWheel.uasset`

This ensures consistent mounting when the pak is loaded.

---

## 3. Cooking & Packing

After authoring the widget in the turdmod content project, you need to cook it for the **WindowsNoEditor** platform and pack it into a `.pak`.

### 3.1 Prerequisites

- UE4.27 Editor with the turdmod content plugin installed (see section 8).
- UnrealPak.exe (located in `Engine\Binaries\Win64\`).

### 3.2 Cooking

In the Editor:  
1. Open the **Project Settings → Packaging**.  
2. Set “Use Pak File” to True.  
3. Set “Cook only maps” to False (we need all assets).  
4. Run **File → Package Project → Windows (64-bit)**.  
5. Wait for cooking to finish. The output folder will contain a `WindowsNoEditor` directory with a `Content\Paks` subdirectory. Inside you will find a `pakchunk0-WindowsNoEditor.pak`.

### 3.3 Manual Cooking via Command Line

Alternatively, use this command (adjust paths to your project):

```bat
"C:\UE_4.27\Engine\Binaries\Win64\UE4Editor-Cmd.exe" "C:\TurdMOD\TurdMODContent.uproject" -run=Cook -targetplatform=WindowsNoEditor -fileopenlog -unversioned
```

This produces cooked assets in `Saved\Cooked\WindowsNoEditor`.

### 3.4 Packing with UnrealPak

To create a clean, single-pak file that contains only your widget assets:

```bat
"C:\UE_4.27\Engine\Binaries\Win64\UnrealPak.exe" "C:\Output\MyWidget_P.pak" -Create=C:\TurdMOD\Saved\Cooked\WindowsNoEditor\TurdMODContent\Content\TurdMODContent\Widgets\ -compress -crypto="C:\TurdMOD\Config\crypto.json"
```

**Important:** The `crypto.json` file must contain the **same AES key** that SCUM uses for its base game paks. If you do not use encryption, omit the `-crypto` flag. (See section 4.)

### 3.5 Mount Priority

The pak must be mounted with a **lower priority** than the base game paks but **higher** than any mod paks that depend on it. The turdmod loader pak (`TurdMODLoader.pak`) is mounted with priority 0.  
Custom widget paks should use priority 10.  
This is set inside the pak file’s `mount.ini` (embedded via the turdmod cook step). We provide a helper script.

---

## 4. Distribution

### 4.1 Pak Format

- **File:** `MyWidget_V1.pak` (naming convention: `<WidgetName>_V<Version>.pak`)
- **Encryption:** Optional but recommended. The AES key must match the one used by SCUM. We distribute the key via the turdmod‑api to verified servers/players only.
- **Integrity:** SHA‑256 hash provided on the marketplace page.

### 4.2 Marketplace Hosting

All paks are hosted on `turdmod-marketplace.com` under the mod author’s profile.  
The site provides:

- Download link (plain HTTP/HTTPS)  
- Direct download to manager clients  
- Version history

---

## 5. Client Installation

### 5.1 Manual Installation

1. Download the `.pak` file.  
2. Place it into `[SCUM Installation]\SCUM\Content\Paks\` (alongside the existing `pakchunk0.pak`).  
3. Restart SCUM (or reconnect to the server).

The game will automatically mount all `.pak` files in that directory on launch.

### 5.2 Automatic Installation via turdmod-manager

The turdmod-manager (a separate electron app) can:

- Browse the marketplace, click “Install”.
- Download the pak, verify hash, place in `Paks\`.
- Optionally add a `[TurdMOD Widgets]` section in `Game.ini` for easier management.

### 5.3 Future: Server‑Push (Steam Workshop Style)

Long term, we could embed download tokens in the server’s response. Not in v1.

---

## 6. Server‑Side Invocation

The bridge (in C++) exposes the following static functions that can be called from Blueprint (via Pattern A):

### 6.1 Bridge API (BlueprintCallable, in `UTurdMODBridge`)

```cpp
UFUNCTION(BlueprintCallable, Category = "TurdMOD|Widgets")
static void ShowWidgetToPlayer(APlayerController* Target, const FString& WidgetClassName, const FString& PayloadJson);

UFUNCTION(BlueprintCallable, Category = "TurdMOD|Widgets")
static void ShowWidgetToAll(const FString& WidgetClassName, const FString& PayloadJson);

UFUNCTION(BlueprintCallable, Category = "TurdMOD|Widgets")
static void HideWidget(APlayerController* Target, const FString& WidgetClassName);

UFUNCTION(BlueprintCallable, Category = "TurdMOD|Widgets")
static void UpdateWidget(APlayerController* Target, const FString& WidgetClassName, const FString& PayloadJson);
```

### 6.2 Underlying Mechanism

The C++ implementation of these functions looks up the requested widget class (by `PersonaNamespace` and `WidgetName` → resolved from metadata) and calls a **NetMulticast RPC** on the server's `TurdMODRouter` actor (a persistent actor spawned on listen servers or dedicated servers).

**Router Actor Blueprint:**  
- Class: `BP_TurdMODRouter` (part of `TurdMODLoader` pak)  
- Contains a `NetMulticast_HandleWidgetCommand` function:

```plain
// Server → Clients
UFUNCTION(NetMulticast, Reliable, BlueprintCallable, Category = "TurdMOD|Widgets")
void NetMulticast_HandleWidgetCommand(const FString& Command, const FString& WidgetClassPath, const FString& Payload);
```

Where `Command` is one of `"Show"`, `"Hide"`, `"Update"`.

On the client side, the `BP_TurdMODRouter` instance (spawned at game start) receives the multicast and uses the WidgetClassPath (e.g., `/TurdMODContent/Widgets/Doctor/HealingWheel.HealingWheel_C`) to create the widget via `CreateWidget<UUserWidget>()`.

### 6.3 Registration

Each widget class **must** register itself with the router in its *Construct* event by calling:

```
RegisterCustomWidget(self);
```

This is provided by the turdmod widget template.

---

## 7. Persona‑Driven Widget Composition

The turdmod system allows administrators to enable “personas” for players. Each persona defines a set of widgets that should be available.  
Personas are configured in the server’s `Personas.json` (a turdmod config file). Example:

```json
{
  "Doctor": {
    "Active": true,
    "Widgets": ["HealingWheel", "PatientMonitor"]
  },
  "Quartermaster": {
    "Active": true,
    "Widgets": ["KitPicker", "SupplyRequest"]
  }
}
```

When a player is assigned a persona (via `setPersona <player> <persona>`), the bridge automatically calls `ShowWidgetToPlayer` for each widget in the persona’s list with a default payload (JSON describing allowed items, etc.).  
The widgets are then rendered and can be toggled via keybinds (e.g., `B` for Doctor wheel).

Each persona’s widgets are **independent mods** — they are authored separately and can be versioned individually.

---

## 8. Mod Author UX — The turdmod Content Project

To lower the barrier, we distribute a UE4.27 project template called `TurdMODContent`:

### 8.1 Bundled Content

- **Blueprint base class:** `TurdMODUserWidget` (inside `/TurdMODContent/Blueprints/BaseClasses/`)  
- **Common UI assets:** shared textures, font references, icons for SCUM-style UI elements.  
- **Blueprint Function Libraries:** `TurdMODJsonLibrary`, `TurdMODHUDLibrary` (for sending HUD messages).  
- **Example Widget:** `BP_WelcomeBanner` — a simple animated text banner.

### 8.2 Editor Setup

1. Download `TurdMODContent_v1.0.zip` from `turdmod-marketplace.com/tools`.  
2. Extract to `C:\TurdMOD\`.  
3. Right‑click `TurdMODContent.uproject` → **Generate Visual Studio project files** (for C++ if needed).  
4. Open the project.  
5. Enable the **TurdMODContent** plugin (should be auto-enabled).  
6. Accept the warning about missing modules? → Yes (the plugin is Blueprint-only).

### 8.3 Cook Button & Publish

We provide a custom **Editor Utility Widget** (a toolbar button) that:

- Runs the cooking command with the correct target platform and compression.  
- Invokes UnrealPak with the correct crypto.json.  
- Asks for a version number and metadata.  
- Opens a dialog to upload to `turdmod-marketplace.com` (requires API token).

---

## 9. Marketplace Presence

The turdmod‑web marketplace shows:

- Widget preview image (author provides a screenshot).  
- Live demo: a screen‑captured video or a browser‑based simulator (future).  
- Author name, rating (via thumbs up/down), version number, changelog.  
- **Install** button (detects if turdmod-manager is installed and opens a download dialogue).

APIs:

| Endpoint | Description |
|----------|-------------|
| `GET /api/widgets/?namespace=` | List widgets for a persona. |
| `POST /api/widgets/` | Upload new widget pak (requires token). |
| `GET /api/widgets/<id>/download` | Returns direct download URL. |

---

## 10. Visual Style & Theming

The turdmod content project ships with a default style asset `TurdMODDefaultStyle` (UDataAsset) that mimics SCUM’s UI:

- **Font:** `Roboto Condensed` (Regular 18, Bold 20, Light 16). We bundle a `.uasset` redirect to the game’s own font file (since SCUM already uses Roboto Condensed).  
- **Color Palette:**  
  - Background: `#2B2B2B` (dark grey)  
  - Primary text: `#F5F5F5`  
  - Accent: `#E6A11D` (amber)  
  - Danger: `#C0392B`  
  - Success: `#27AE60`  
- **Border style:** 2px solid `#4A4A4A`, no rounded corners.  
- **Button style:** Flat, no shadows, pressed state = darken by 20%.

Authors are encouraged to override these via style overrides in their widgets, but the default gives a cohesive look.

---

## 11. Wave Plan

| Wave | Deliverable | Estimated Effort |
|------|-------------|------------------|
| **A** | This spec + bridge C++ stubs in `TurdMODBridge.cpp` (the four endpoint functions) | 4h (planning + coding) |
| **B** | Ship `TurdMODLoader.pak` (base router actor + `TurdMODUserWidget` base class) + first widget `BP_WelcomeBanner` that shows a “Welcome to TurdMOD” text. **Requires Layer 3 to be functional.** | 8–12h |
| **C** | Persona widgets: Doctor healing wheel, Quartermaster kit picker. Each as separate paks. | 16h per persona |
| **D** | Marketplace integration: upload API, web preview, automatic install | 24h |
| **E** | In‑Manager widget preview/editor (browser‑based widget rendering) | 40h (future) |

---

## 12. Risk / Limit Catalog

| Risk | Impact | Mitigation |
|------|--------|------------|
| **Layer 3 still blocking content paks** | Wave B cannot start. No custom widgets. | Continue RE on Layer 3; use `sendHudMessage` for minimal UI. |
| **Widget class not found on client** | `CreateWidget` returns null → widget fails silently. | The router logs missing class warnings; we can fallback to stock HUD message. |
| **Widget state sync** | Per‑player updates require individual Multicast RPCs. | Use reliable channel; limit updates to <5 per second. |
| **Style conflicts** | Two mods define same color variable → visual clash. | Enforce namespace for all assets; no global overrides. |
| **Performance** | 10+ heavily animated widgets on low‑end PCs | Provide a `bDisableWhenLowFps` option; widget authors can throttle rendering using `OnTick` checks. |
| **AES key distribution** | Leaked key could allow pirated widgets. | Use server‑side key rotation; store keys in encrypted bridge memory. (v2) |

---

## Appendix A: Example Blueprint Node Graph for a Simple Widget

```plain
Event Construct -> 
    Call Parent’s RegisterCustomWidget (provided by base class)

Event OnShowFromServer (PayloadJson) ->
    Deserialize PayloadJson (Using ParseJSON node from TurdMODJsonLibrary)
    Get field "title" as string
    Set TextBlock_Title.Text = title
```

## Appendix B: UnrealPak Command Template

Use the following batch file to repack your cooked folder:

```bat
@echo off
set PROJECT_DIR=C:\TurdMOD
set OUTPUT=C:\Output\MyWidget.pak
set COOKED=%PROJECT_DIR%\Saved\Cooked\WindowsNoEditor\TurdMODContent\Content\TurdMODContent\Widgets
"%UE4_DIR%\Engine\Binaries\Win64\UnrealPak.exe" "%OUTPUT%" -Create="%COOKED%" -compress -crypto="%PROJECT_DIR%\Config\crypto.json"
echo Pak created: %OUTPUT%
```

**Note:** The `crypto.json` must be obtained from the SCUM file decryption process (we provide a tool `scum-key-extractor.exe` that dumps the AES key from a running game instance). This is a one‑time setup per installation.

---

*End of specification.*  
*Next step: Implement bridge stubs in `TurdMODBridge.cpp` and create `TurdMODLoader` pak (Wave A).*
```