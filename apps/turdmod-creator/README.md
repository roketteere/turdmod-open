# turdmod-creator

Zero-to-hero content authoring CLI for TurdMOD / SCUM / UE 4.27.

## What this is

A small CLI that lets you build custom in-game widgets, vehicles, and items
WITHOUT writing C++ or opening the Unreal Editor (most of the time).
Noobs use 3-5 prompts to ship a working widget. Power users add `--advanced`
and unlock raw UE Editor integration, custom Blueprint scripting, multi-pak
projects, and signing keys.

Plus: pipe ANY AI you want (your key, your billing) to generate or modify
widgets with natural language. We never see your API key. We never charge
for AI usage.

## Quickstart (60 seconds, noob mode)

```bash
# install
npm install -g @turdmod/creator
# (or use directly: npx @turdmod/creator)

# diagnose your setup
tmc doctor

# create a project
tmc init my-first-widget
cd my-first-widget

# add a notification banner widget (interactive prompts)
tmc widget add notification --name welcome

# see what's in the project
tmc widget list

# build it (cook into a .pak — requires UE 4.27 installed)
tmc cook
```

## Templates (v1)

| Template | What it is |
|---|---|
| `notification` | Top-of-screen banner. Auto-dismisses. Pairs with Storyteller persona. |
| `healing-wheel` | 6-segment radial selector. Pairs with Doctor persona. |
| `kit-picker` | Grid of item slots. Pairs with Quartermaster persona. |
| `blank` | Empty widget for advanced UE 4.27 Editor authoring. |

```bash
tmc template list                    # see all
tmc template show notification       # see params
```

## AI piping (BYO key)

We DO NOT host AI for you. You bring your own API key from your favourite
provider, and the CLI talks directly to that provider over HTTPS. We never
see, store, or proxy your key. Your billing, your responsibility.

Supported providers:
- `openai` — gpt-4.1, gpt-4.1-mini, gpt-5
- `anthropic` — claude-haiku-4-5, claude-sonnet-4-6, claude-opus-4-7
- `deepseek` — deepseek-chat, deepseek-reasoner
- `ollama` — local, no key needed (default)
- `gemini` — gemini-1.5-flash, gemini-1.5-pro

### Setup

```bash
# Set your key (one-time, per provider). Windows:
setx TURDMOD_AI_KEY "<your-api-key>"

# Or use a provider-specific env var:
setx ANTHROPIC_API_KEY "<key>"
setx OPENAI_API_KEY "<key>"
# etc.
```

### Use

```bash
# One-shot prompt
tmc ai prompt "make me a healing wheel with 8 segments, gold theme" --provider deepseek

# Interactive chat
tmc ai chat --provider anthropic --model claude-haiku-4-5-20251001

# Apply a saved proposal
tmc ai apply proposal-2026-05-23T03-15-22.json
```

AI proposals are **always preview-then-confirm**. We never auto-apply.

## Noob mode vs Advanced mode

By default, every command runs in noob mode — 3-5 prompts max, sensible
defaults, friendly errors.

Add `--advanced` to any command to unlock:
- Raw UnrealPak.exe flag exposure (`--pak-encrypt`, `--target`, etc.)
- Direct UE 4.27 Editor integration (`--open-editor`)
- Custom Blueprint event handlers
- Multi-pak projects with dependency resolution
- Author signing keys (`tmc key generate / import / sign`)
- Cross-pak references (use widgets from other projects)

Noob mode hides every "advanced" parameter that has a sensible default.
Power users see them all.

## Config

`~/.turdmod-creator/config.json` (gitignored). Set via prompts or hand-edit.

```json
{
  "uePath": "C:/Program Files/Epic Games/UE_4.27",
  "aiProvider": "deepseek",
  "aiModel": "deepseek-chat",
  "keyEnvVar": "TURDMOD_AI_KEY",
  "ollamaHost": "http://127.0.0.1:11434"
}
```

## Logs

`~/.turdmod-creator/logs/<date>.jsonl` — every command emits a structured
NDJSON log entry. Use this for post-hoc review of what AI proposals you
applied and when.

## Status (v0.1.0)

This is the **MVP scaffold**. Shipped:
- Project init + widget instance creation
- 4 starter templates with parameter schemas
- Interactive prompts (noob mode) + flag-based params (advanced/scripted)
- AI piping for 5 providers with BYO key
- Cost estimates on AI calls
- Doctor health-check
- NDJSON command log

**Not yet shipped (v0.2.0+):**
- Actual UE 4.27 `.uasset` generation (templates currently ship JSON
  manifests; UE-side cooking is manual via UE Editor following the recipe
  doc at `docs/guides/ue4-content-pak-recipe.md`)
- Marketplace publish (`tmc publish` is a stub)
- Author signing keys (`tmc key` is planned)
- Custom vehicle + physics templates (depend on Phase 2.3 + L3 unlock)

## License

MIT. Use it, fork it, build with it. Sell what you make.

## Read more

- `docs/guides/turdmod-creator-spec.md` — full UX + architecture spec
- `docs/guides/custom-gui-maker.md` — UMG widget pipeline (Phase 2.2)
- `docs/guides/ue4-content-pak-recipe.md` — manual cook steps until v0.2.0
- `docs/strategy/community-and-launch.md` — what we're building, why
