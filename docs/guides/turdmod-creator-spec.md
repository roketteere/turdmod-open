# turdmod-creator: Product + UX Specification

**Version:** 1.0.0  
**Status:** Draft  
**Target Release:** June 2026  
**Repository:** `github.com/roketteere/turdmod`  
**Path:** `apps/turdmod-creator/`

---

## 1. The "Zero-to-Hero" UX Promise

turdmod-creator is designed to serve three distinct tiers of users, with a seamless transition between them. The default mode is **Noob** — no UE Editor, no scripting, no prior modding experience required. A single CLI flag or config toggle escalates the experience to **Standard** or **Advanced**.

| Tier | Required Knowledge | UE Editor? | Blueprint Scripting? | AI Assistance? | Key Enabler |
|------|-------------------|------------|----------------------|----------------|-------------|
| **Noob** (default) | None – can read prompts | No | No | Yes (prompt-driven) | Interactive prompts + templates |
| **Standard** | Basic modding concepts (paks, layers) | Optional | Visual config only | Yes | `--advanced` flag expanded options |
| **Advanced** | Unreal Editor workflows, Blueprint nodes | Yes | Full Blueprint editing | Yes (raw Blueprint code generation) | Direct .uasset editing + UE launch |

**Transitions:**
- From Noob to Standard: Run any command with `--advanced` or set `"level": "standard"` in `~/.turdmod-creator/config.json`.
- From Standard to Advanced: Use `creator widget edit --advanced` or `creator project upgrade --advanced`. This copies the entire project directory to a UE-ready location and opens the Editor.

---

## 2. Project Structure

```
apps/turdmod-creator/
├── package.json
├── tsconfig.json
├── src/
│   ├── cli.ts                  # argv parser + subcommand router (yargs or commander)
│   ├── commands/
│   │   ├── init.ts             # create new project (3 templates: blank, persona, custom)
│   │   ├── template.ts         # list / show / install templates
│   │   ├── widget.ts           # author a single widget (add/edit/delete)
│   │   ├── cook.ts             # UnrealPak.exe invocation
│   │   ├── publish.ts          # upload to turdmod-marketplace
│   │   ├── ai.ts               # AI-pipe subcommand
│   │   ├── doctor.ts           # health-check
│   │   ├── serve.ts            # optional local web UI
│   │   └── key.ts              # manage author signing keys
│   ├── templates/
│   │   ├── notification/       # TurdMODNotification template files
│   │   ├── healing-wheel/
│   │   ├── kit-picker/
│   │   └── blank/              # Minimal UMG widget with one text block
│   ├── ai/
│   │   ├── providers.ts        # BYO API key handlers
│   │   ├── prompts/
│   │   │   ├── widget-author.ts
│   │   │   ├── widget-modify.ts
│   │   │   ├── widget-debug.ts
│   │   │   └── ux-review.ts
│   │   └── cost-estimator.ts   # estimates tokens and cost before call
│   ├── project/
│   │   ├── scaffold.ts         # generates .uproject + base BP class files
│   │   └── upgrade.ts          # standard → advanced upgrade
│   ├── lib/
│   │   ├── logger.ts           # structured NDJSON logging
│   │   ├── config.ts           # reads ~/.turdmod-creator/config.json
│   │   ├── unrealpak.ts        # wrapper around UnrealPak.exe
│   │   ├── marketplace.ts      # HTTP client for marketplace API
│   │   └── template-engine.ts  # parameter substitution in template files
│   └── types/
│       ├── template.ts
│       ├── project.ts
│       └── config.ts
├── bin/
│   └── turdmod-creator        # shebang entry point
└── README.md
```

**`package.json` key fields:**
```json
{
  "name": "@turdmod/creator",
  "version": "1.0.0",
  "bin": {
    "creator": "./bin/turdmod-creator"
  },
  "dependencies": {
    "yargs": "^17.7.2",
    "inquirer": "^9.2.12",
    "chalk": "^5.3.0",
    "openai": "^4.24.0",
    "@anthropic-ai/sdk": "^0.20.0",
    "figlet": "^1.7.0",
    "ora": "^8.0.1"
  }
}
```

**Global config file:** `~/.turdmod-creator/config.json`
```json
{
  "ue4Path": "C:/Program Files/Epic Games/UE_4.27",
  "unrealPakPath": null,
  "aiProvider": "ollama",
  "aiKey": "",
  "aiModel": "codellama",
  "authorKeyPath": "~/.turdmod-creator/author-keys/",
  "projectDefaultDir": "~/TurdMODProjects",
  "level": "noob",
  "telemetry": false
}
```

---

## 3. Subcommands — Detailed Specification

### `init <project-name>`
- **Usage:** `creator init my-first-widget`
- **Purpose:** Create a new turdmod-creator project directory with a .uproject file, Content folder, and default placeholder.
- **Beginner flow:** Interactive prompts for template selection (unless `--template` flag provided), project name, author name. After project created, prints success message and suggests next steps.
- **Advanced flow:** With `--advanced` flag, additionally creates full UE project structure with source code stubs for C++ (optional), and sets up Blueprint-only mode if user prefers.
- **Flags:** `--template <name>` (skip prompt), `--dir <path>`, `--advanced`, `--ue-only` (skip turdmod CLI files).

### `template list`
- **Usage:** `creator template list`
- **Purpose:** Show available templates with short description, thumbnail URL (if available), and turdmod-marketplace rating.
- **Beginner output:** Simple table with name + description.
- **Advanced output:** `--json` flag returns machine-readable array for piping into other tools.

### `template show <name>`
- Shows full parameter specification of a template.

### `widget add <template> [--name <widget-name>]`
- **Usage:** `creator widget add notification --name WelcomeBanner`
- **Purpose:** Create a new widget instance from a template, interactively filling its parameters.
- **Beginner:** Asks each parameter with validation. Example: `? Notification text (max 64 chars):` then `? Duration in milliseconds (1000-30000):` etc.
- **Advanced:** Accepts `--param` flags for non-interactive mode: `creator widget add notification --param text:"Hello" --param duration:5000`.

### `widget edit <widget-name> [--advanced]`
- **Usage:** `creator widget edit welcome`
- **Noob mode:** Opens interactive prompt to change the widget's parameters (same as add, but pre-filled with current values).
- **Advanced mode:** `--advanced` launches UE 4.27 Editor with the project and opens the widget's parent Blueprint. User can edit every property and save. CLI waits for UE exit.

### `cook [--flags ...]`
- **Usage:** `creator cook`
- **Purpose:** Build one or all widgets into a UE-ready .pak file using UnrealPak.exe.
- **Noob:** Cooks the entire project into one `Widgets.pak`. No flags configurable.
- **Standard:** With `--advanced`, allows selecting which widgets to include, output name, compression settings.
- **Advanced:** With `--advanced --flags raw`, exposes all UnrealPak flags (`--pak-encrypt`, `--no-fallback`, `--target platform`).

### `publish [widget-name]`
- **Usage:** `creator publish welcome`
- **Purpose:** Package widget, generate manifest, upload to turdmod-marketplace.
- Flow:
  1. Prompt for marketplace API key (stored to config after first time).
  2. Validate author signing key exists; if not, prompt to generate/import.
  3. Prompt for version bump (semver).
  4. Prompt for visibility: `free`, `premium-included`, `premium-exclusive`.
  5. Upload .pak + manifest + optional screenshots (from `widget-name/screenshots/`).
  6. Show URL preview: `https://marketplace.turdmod.app/widgets/your-author-tag/welcome`

### `ai prompt "<text>"`
- **Usage:** `creator ai prompt "Create a health bar widget with gradient fill"`
- **Purpose:** Send a natural-language description to the AI provider and receive structured JSON proposal for a widget.
- **Output:** A preview of what will be created, with parameter values. User confirms before applying.
- **Cost transparency:** Before calling, shows estimated cost: `Estimated cost: $0.0032 (based on 0.002/1k tokens). Proceed? [Y/n]`

### `ai chat`
- Interactive session. User sends messages, AI responds with suggestions that can be applied with `apply <id>`.

### `ai apply <suggestion-id>`
- Applies a previously generated AI proposal.

### `doctor`
- **Usage:** `creator doctor`
- **Purpose:** Health check. Checks:
  - UE 4.27 installed at configured path.
  - UnrealPak.exe found (or auto-detect).
  - AI API key valid (makes small test call).
  - Author signing key present (if intended to publish).
  - Network connectivity to marketplace.
  - File permissions for output directories.

### `serve`
- **Usage:** `creator serve --port 3000`
- **Purpose:** Start local web UI (built-in or from Manager integration) on localhost. This enables visual widget parameter editing in a browser.

### `key generate [--type rsa|ecdsa]`
### `key import <file>`
### `key list`
- Manage author signing keys for marketplace publishing.

---

## 4. AI Integration – BYO Key Model

**Architecture:**
- The CLI never stores API keys centrally; they are kept in the local config file (which is `.gitignore`d and excluded from `~/.turdmod-creator/` backups).
- Supported providers: `openai`, `anthropic`, `deepseek`, `ollama` (default), `gemini`.
- Environment variable override: `TURDMOD_AI_KEY`, `TURDMOD_AI_PROVIDER`.
- System prompts are stored in `src/ai/prompts/` and loaded dynamically.
- **AI NEVER auto-applies changes.** All proposals are presented as a structured JSON diff with parameter changes, file modifications, and a risk level (e.g., `"low"`, `"medium"`, `"high"`). User must explicitly confirm with `apply` before any disk writes.
- **Cost transparency:** Every AI call shows an estimated token count and dollar cost (if provider supports it). For Ollama (local), shows "Local inference – no cost."
- **Responsibility boundary:** Every prompt response includes a footer: *"This suggestion was generated by an AI model using your API key. turdmod does not have access to your key or the content of this exchange. You are solely responsible for the generated output."*

**AI Subcommand Flow (example):**
```
$ creator ai prompt "Make a circular health ring for a character"
 ───────────────────────────────────────────
 Using provider: openai (model gpt-4)
 Estimated cost: $0.0042
 ───────────────────────────────────────────
  
 AI Proposal "sugg-202605231234":
   Create new widget: `health-ring`
   Parameters:
     - radius: 150
     - segmentCount: 4
     - colors: ["#ff0000", "#00ff00"]
   Files to create:
     - Content/Widgets/HealthRing.uasset
     - Content/Widgets/HealthRing.uasset.json

 Risk level: medium (new widget creation)

 Apply? [y/N] y
```

---

## 5. Template Authoring Contract

**Manifest file** (`template.json` in each template directory):
```json
{
  "name": "notification",
  "version": "1.0.0",
  "description": "Top-of-screen banner notification widget",
  "author": "turdmod",
  "parameters": [
    {
      "id": "text",
      "label": "Notification text",
      "type": "string",
      "default": "Hello from TurdMOD!",
      "maxLength": 64,
      "required": true
    },
    {
      "id": "durationMs",
      "label": "Duration (ms)",
      "type": "int",
      "default": 10000,
      "min": 1000,
      "max": 30000
    },
    {
      "id": "fontSize",
      "label": "Font size",
      "type": "int",
      "default": 24,
      "min": 8,
      "max": 72
    },
    {
      "id": "bgColor",
      "label": "Background color",
      "type": "color",
      "default": "#222222"
    },
    {
      "id": "textColor",
      "label": "Text color",
      "type": "color",
      "default": "#ffffff"
    },
    {
      "id": "icon",
      "label": "Icon image (optional)",
      "type": "file",
      "optional": true,
      "filters": ["*.png", "*.jpg", "*.ico"]
    }
  ],
  "files": [
    {
      "source": "widget.uasset.json",
      "destination": "Content/Widgets/{{widget_name}}/{{widget_name}}-widget.uasset.json"
    },
    {
      "source": "blueprint.uasset.json",
      "destination": "Content/Widgets/{{widget_name}}/{{widget_name}}.uasset.json"
    }
  ]
}
```

**Parameter types:**
- `string` – free text
- `color` – hex color (`#ffffff`)
- `int` – integer with optional min/max
- `float` – floating point
- `enum` – dropdown selection (key-value pairs)
- `bool` – yes/no
- `file` – path to an image/asset, copied into the project
- `multiline` – longer text (for code, descriptions)

**Template files** contain placeholders `{{param_name}}`, `{{widget_name}}`, `{{project_name}}`. The template-engine replaces them at widget creation time.

---

## 6. The 3 Starter Templates (Detailed)

### 6.1 TurdMODNotification
- **Purpose:** A simple, animated banner that slides down from top of screen, displays text, auto-dismisses after duration.
- **Parameters:** (as shown in template manifest above)
- **Generated files:**
  - `ParentBlueprint.uasset` – UMG Widget Blueprint with text block, background border, animation timeline.
  - `_Config.uasset` – Data asset holding parameters (so modders can tweak later without recompiling).
  - `WidgetInstance.json` – Metadata for turdmod-loader.
- **Blueprint events:** `OnConstruct`, `OnShow`, `OnHide` (editable in advanced mode).
- **Default behavior:** Slide down in 300ms, hold for `durationMs`, slide up in 300ms.

### 6.2 TurdMODHealingWheel
- **Purpose:** A radial menu with 6 equally spaced segments, each representing a heal action. Activated by a hotkey.
- **Parameters:** `segmentLabels[6]` (array of 6 strings), `wheelRadius` (int, 100-400), `hotkey` (string, default "H"), `primaryColor` (color), `accentColor` (color).
- **Generated files:**
  - `RadialMenuWidget.uasset` – UMG widget with custom paint or buttons arranged radially.
  - `SegmentActionData.uasset` – Data table mapping segment index to Blueprint interface call.
- **Blueprint events:** `OnSegmentSelected(int32 segmentIndex)` (editable).
- **Default behavior:** On hotkey press, show wheel centered at mouse. Mouse over segment highlights; click triggers `OnSegmentSelected`.

### 6.3 TurdMODKitPicker
- **Purpose:** Grid of item slots for selecting loadout kits. Used by Quartermaster persona.
- **Parameters:** `gridCols` (int 1-6), `gridRows` (int 1-4), `slotSize` (int 32-128), `allowMultiselect` (bool), `title` (string).
- **Generated files:**
  - `KitGridWidget.uasset` – UMG wrapbox with dynamic tile buttons.
  - `KitItemSlot.uasset` – Reusable tile blueprint.
  - `InventoryData.uasset` – Struct storing item ID, name, icon, stack count.
- **Blueprint events:** `OnSelectionsChanged(TArray<int32> selectedIndices)` (editable).
- **Default behavior:** Click toggles selection; yellow highlight on selected slots. If `allowMultiselect` false, single-select only.

---

## 7. Advanced Mode – Unlocked Features

When a user runs any command with `--advanced` or sets `"level": "advanced"` in config, the following become available:

1. **Direct UE Editor Launch**
   - `creator widget edit --advanced` opens UE 4.27 Editor with the project, navigates to the widget's Blueprint.
   - After user closes UE, CLI detects changes and re-imports the .uasset files back into the turdmod project structure.

2. **Raw .uasset Editing via Plugin**
   - The turdmod-engine-bridge plugin includes an editor mode tool that allows editing widget parameters directly in UE's detail panel.
   - Changes are saved to a `.uasset.json` companion file that turdmod-creator can parse.

3. **Custom Blueprint Event Handlers**
   - Templates declare certain Blueprint functions as "editable" (e.g., `OnConstruct`, `OnSegmentSelected`).
   - In advanced mode, users can write custom Blueprint node logic. The CLI can even generate Blueprint code via AI using the `widget-modify` prompt with the existing blueprint's node graph serialization.

4. **Multiple-Pak Project Mode**
   - Projects can declare multiple output paks (e.g., `NotificationWidget.pak`, `HealingWheel.pak`).
   - Each widget can be assigned to a pak. Cooking produces separate paks.

5. **Cook Flags Exposed**
   - `--pak-encrypt` – encrypt pak with AES.
   - `--no-fallback` – disable fallback to loose files.
   - `--target Windows64Server` – specify platform.
   - `--compression zlib|none|oodle` – compression method.

6. **Cross-Pak Dependency Resolution**
   - Widgets can reference assets from other paks. The cook command resolves these and adds pakchunk references to the manifest.

7. **Author Signing Key Management**
   - `creator key generate` – creates RSA-2048 or ECDSA key pair.
   - `creator key import` – imports an existing private key.
   - `creator sign <pak>` – signs a pak with the author key (used during publish).

---

## 8. Manager UI Integration

The turdmod-manager desktop app will detect installed turdmod-creator projects in the user's project directory. The **Engine** page of Manager will have a dedicated section for creator users:

- **"Creator Projects"** panel: lists all projects found in `~/.turdmod-creator/projects/` with name, last modified, number of widgets, last cook status.
- **"Open in Creator"** button: launches `turdmod-creator serve --project <path>` which opens the local web UI for that project.
- **"Cook & Deploy"** button: runs `cook` on the selected project, then autonomously deploys the generated pak to the game's `~/TurdMOD/Content/Paks/` folder (if running in development mode with Layer 3 crack enabled).
- **Marketplace browser** embed: displays marketplace widgets, allows one-click import into the local project via `creator marketplace import <widget-id>`.

Manager uses the `creator --json` output for structured commands (e.g., `creator template list --json`).

---

## 9. Marketplace Publish Flow

The `publish` command automates the following steps:

1. **Validate project readiness:** Runs `doctor` checks, ensures all widgets are cooked, signing key exists.
2. **Bump version:** Interactive semver prompt: `? Current version is 1.0.0. New version: (patch/minor/major/custom)`
3. **Set visibility:** `? Widget visibility: (free / premium-included / premium-exclusive)`
4. **Generate manifest:** `publish-manifest.json` containing:
   - `widgetName`, `version`, `authorFingerprint`, `description`, `parameters` (schema), `dependencies`, `hash` of .pak, `filesize`, `platforms`, `screenshots` (URLs from `screenshots/`).
5. **Upload:** Uses REST API to `https://api.marketplace.turdmod.app/v1/publish`. Authenticates with `marketplaceApiKey` from config.
6. **Preview:** Shows a web URL: `https://marketplace.turdmod.app/widgets/{author-tag}/{widget-name}/{version}`. User confirms to finalize.
7. **Post-publish:** Prints sharing instructions and command to update metadata later.

---

## 10. Error Handling + Observability

- **Structured logging:** Every subcommand writes NDJSON lines to `~/.turdmod-creator/logs/YYYY-MM-DD.jsonl`. Each line has `timestamp`, `level`, `command`, `message`, `error` (optional), `durationMs`.
- **`creator audit` command:** Reads the latest log file and displays recent entries in a scrollable table. Supports `--tail N`, `--level error`, `--since`.
- **`doctor` command:** Comprehensive check:
  - UE 4.27 binary path (try `%UE4_ROOT%\Engine\Binaries\Win64\UE4Editor.exe`).
  - UnrealPak.exe from `%UE4_ROOT%\Engine\Binaries\Win64\UnrealPak.exe`.
  - AI key: performs a small test call (e.g., 10-token request) against the configured provider.
  - Author signing key: checks existence, expiration, permissions.
  - Network: pings `api.marketplace.turdmod.app`.
- **Nuanced error messages:** Instead of "Error: file not found", the CLI prints contextual advice. Example: `Could not find UnrealPak.exe. Create the environment variable UE4_ROOT or set 'unrealPakPath' in ~/.turdmod-creator/config.json`.

---

## 11. Pricing Implications

| Feature | Cost to User | turdmod Revenue |
|---------|-------------|------------------|
| CLI binary + source | Free (MIT) | None |
| All templates | Free | None |
| AI features | BYO API key – user pays AI provider directly | None |
| Marketplace hosting | Free for free widgets | 20% revenue share on premium widgets |
| Premium template packs (future) | One-time purchase or subscription | Partial (after platform cut) |

Important: **The CLI itself never charges.** All monetization flows through the marketplace. This aligns with the "protect-secrets boundary" – turdmod never touches user API keys or AI usage.

---

## 12. First-90-Minute UX Target

A brand new user with no modding experience should be able to go from zero to a working notification widget shipped to their game in 90 minutes:

| Step | Action | Estimated Time |
|------|--------|----------------|
| 1 | Install CLI: `npm install -g @turdmod/creator` | 2 min |
| 2 | Run `creator init my-first-widget` – interactive prompts | 2 min |
| 3 | Run `creator template list` – see 3 starters | 1 min |
| 4 | Run `creator widget add notification --name welcome` – fill 6 parameters | 5 min |
| 5 | Run `creator cook` – builds .pak (may take 2-5 min) | 5 min |
| 6 | Deploy: copy pak to `Content/Paks/` (or use Manager) | 1 min |
| 7 | Launch game, test notification | 3 min |
| 8 | Iterate with `creator widget edit welcome` | < 5 min |
|   | **Total:** | **~24 min** |

Remaining time can be used to experiment with other templates, test AI prompts, or publish to marketplace.

---

## 13. Followups & Non-Goals (v1)

**In Scope for v1:**
- All subcommands as specified.
- 3 starter templates fully functional.
- AI integration with 5 providers, BYO key.
- Basic marketplace publish (upload, manifest, signing).
- Doctor & audit commands.

**Deferred to v1.1+ (post-GA):**
- Voice integration.
- Real-time visual designer in browser (planned for Manager UI, v2).
- Multi-author collaboration (git integration).
- Custom physics/vehicle authoring (separate tool: turdmod-vehicle-creator).
- Live reload during UE development (hot-reload is editor-only).

**Non-goals (never):**
- Centralized AI proxying (will never hold user API keys).
- Mandatory paid features (CLI stays free forever).

---

## Appendix: Implementation Notes for Developers

### CLI Router (`src/cli.ts`) – Yargs Configuration

```typescript
#!/usr/bin/env node
import yargs from 'yargs';
import { hideBin } from 'yargs/helpers';

yargs(hideBin(process.argv))
  .scriptName('creator')
  .usage('$0 <command> [options]')
  .command(require('./commands/init'))
  .command(require('./commands/template'))
  .command(require('./commands/widget'))
  .command(require('./commands/cook'))
  .command(require('./commands/publish'))
  .command(require('./commands/ai'))
  .command(require('./commands/doctor'))
  .command(require('./commands/serve'))
  .command(require('./commands/key'))
  .demandCommand(1, 'You need at least one command before moving on')
  .option('advanced', {
    alias: 'a',
    type: 'boolean',
    description: 'Enable advanced mode (UE Editor integration, raw .uasset edit, etc.)'
  })
  .option('json', {
    type: 'boolean',
    description: 'Output as JSON (for programmatic consumption)'
  })
  .option('verbose', {
    alias: 'v',
    type: 'count',
    description: 'Increase verbosity'
  })
  .help()
  .argv;
```

### Template Engine (`src/lib/template-engine.ts`)

- Uses a simple Mustache-like substitution: `{{ param }}` replaced with user input (HTML-escaped for safety).
- Supports loops for array parameters (e.g., `{{#segmentLabels}}{{.}}{{/segmentLabels}}`).
- File copying: source -> destination, with directory creation.

### Cook Command (`src/commands/cook.ts`)

- Locates UnrealPak.exe (config -> env -> registry).
- Generates a response file (list of assets to pack) in a temp directory.
- Executes: `UnrealPak.exe <output>.pak -create=<response.txt> -compress`
- Returns exit code and parseable output.

### Marketplace API Client (`src/lib/marketplace.ts`)

- Endpoints:
  - `POST /v1/auth/login` (with API key) -> JWT token
  - `POST /v1/widgets/publish` (multipart: manifest, pak, screenshots)
  - `GET /v1/widgets/<id>` (for preview)
- Uses `axios` or native `fetch` (Node 18+).

---

*This document constitutes the complete product+UX specification for turdmod-creator v1.0.0. It is intended to be read by engineers, product managers, and designers. All questions of scope, behavior, and user experience are addressed herein.*