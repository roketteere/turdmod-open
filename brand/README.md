# TurdMOD Brand

The **official TurdMOD logo** — the glossy smiling 💩 with cartoon flies buzzing
around it and a neon-cyan glow. This is the mark Joel chose: the launcher's turd
mascot elevated to the brand. Every visible TurdMOD surface (launcher, manager,
web, in-game menus, mods) should incorporate it.

## Files
| File | What |
|---|---|
| `turdmod-logo.svg` | Master mark, 512 viewBox, transparent. 3 flies. Use in menus/headers/splash. |
| `turdmod-icon.svg` | Bold app/tray glyph, 256 viewBox, ONE fly. Reads at 16–32px. |
| `turdmod-logo-1024.png` | Rasterized master (1024²). |
| `turdmod-icon-1024.png` | Rasterized glyph (1024²) — **source for `tauri icon`**. |
| `src-1f4a9.svg` / `src-1fab0.svg` | Source art: Twemoji 💩 (1f4a9) + 🪰 (1fab0), CC-BY 4.0. |

## Regenerate
```bash
# 1. (optional) re-author from emoji art — deterministic, no model needed
node tools/brand/compose-logo.mjs
# 2. rasterize SVG -> PNG (Chrome headless)
bash tools/brand/render.sh brand/turdmod-icon.svg brand/turdmod-icon-1024.png 256 4
bash tools/brand/render.sh brand/turdmod-logo.svg brand/turdmod-logo-1024.png 512 2
# 3. regenerate an app's full icon set (run from that app's dir)
pnpm tauri icon ../../brand/turdmod-icon-1024.png
```
`tools/brand/gen-logo.mjs` is the original DeepSeek freehand attempt (kept for
reference; the emoji-composite in `compose-logo.mjs` is what shipped).

Theme color: neon cyan `#00d4ff` (matches launcher `styles.css --neon`).
