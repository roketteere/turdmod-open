#!/usr/bin/env node
// gen-logo.mjs — DeepSeek-authored official TurdMOD logo (turd + orbiting flies).
// Token-saver: the creative SVG authoring runs on DeepSeek, not on the Opus session.
// Output: brand/turdmod-logo.svg (full) + brand/turdmod-icon.svg (32px-legible glyph).
// @inv neon-cyberpunk theme must match launcher styles.css (--neon #00d4ff on dark).
// @dep DEEPSEEK_API_KEY in env. Run: node tools/brand/gen-logo.mjs

import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const OUT = resolve(ROOT, "brand");
mkdirSync(OUT, { recursive: true });

const KEY = process.env.DEEPSEEK_API_KEY;
if (!KEY) { console.error("FATAL: DEEPSEEK_API_KEY not set"); process.exit(1); }

// Brand DNA shared by both prompts so the two SVGs feel like one family.
const BRAND = `
TurdMOD is a tongue-in-cheek SCUM game modding brand. The mascot is a glossy,
cartoon swirl of poop (the classic emoji silhouette: a soft-serve coil, 3 stacked
tiers tapering to a point on top) with cute cartoon houseflies buzzing around it.
Visual theme = neon cyberpunk: deep near-black background, the poop in warm brown
gradients (#6b3f1d shadow -> #8b5a2b mid -> #b9803f highlight) with a glossy specular
shine, and a NEON CYAN (#00d4ff) rim-light / glow outline so it pops on dark UI.
Flies are small dark bodies with translucent cyan-tinted wings and tiny dashed
motion arcs showing their orbit. Bold, confident, readable. Premium logo quality,
NOT clip-art. Clean vector shapes, smooth gradients, subtle drop shadow.`;

async function ask(prompt) {
  const res = await fetch("https://api.deepseek.com/chat/completions", {
    method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${KEY}` },
    body: JSON.stringify({
      model: "deepseek-chat",
      temperature: 1.0,
      messages: [
        { role: "system", content:
          "You are a senior brand/vector illustrator who outputs production SVG by hand. " +
          "You return ONLY a single valid <svg>...</svg> document with no markdown, no prose, " +
          "no code fences. Use gradients (<linearGradient>/<radialGradient>), <filter> for glow/" +
          "shadow (feGaussianBlur/feDropShadow), and clean <path> geometry. Must render correctly " +
          "in a browser and rasterize cleanly." },
        { role: "user", content: prompt },
      ],
    }),
  });
  if (!res.ok) throw new Error(`DeepSeek ${res.status}: ${await res.text()}`);
  const j = await res.json();
  let s = j.choices?.[0]?.message?.content ?? "";
  // Strip any stray markdown fences, then slice to the <svg>...</svg> envelope.
  s = s.replace(/```[a-z]*\n?/gi, "").trim();
  const a = s.indexOf("<svg"); const b = s.lastIndexOf("</svg>");
  if (a < 0 || b < 0) throw new Error("no <svg> in response:\n" + s.slice(0, 400));
  return s.slice(a, b + 6);
}

async function gen(name, prompt, tries = 2) {
  for (let i = 1; i <= tries; i++) {
    try {
      const svg = await ask(prompt);
      const p = resolve(OUT, name);
      writeFileSync(p, svg + "\n", "utf8");
      console.log(`OK  ${name}  (${svg.length} bytes)`);
      return;
    } catch (e) {
      console.error(`try ${i}/${tries} ${name}: ${e.message}`);
      if (i === tries) throw e;
    }
  }
}

const FULL = `${BRAND}
Create the OFFICIAL TurdMOD master logo as an SVG, viewBox="0 0 512 512", with a
TRANSPARENT background (no background rect). Center the glossy poop mascot, large,
filling most of the canvas. Add THREE cartoon flies at different positions orbiting
it (upper-left, right, lower) each with a faint dashed orbital arc. Give the whole
mascot a soft neon-cyan outer glow so it reads on dark UI. Do NOT include any text or
letters — this is the icon mark only. Make it iconic and instantly recognizable.`;

const ICON = `${BRAND}
Create a SIMPLIFIED app/tray ICON version as an SVG, viewBox="0 0 256 256",
TRANSPARENT background. It must stay legible and punchy when scaled down to 32x32
and even 16x16: bolder shapes, fewer details, thicker neon-cyan outline, ONE single
prominent fly (not three). The poop fills ~80% of the frame, centered. No text.
Heavier contrast than the master logo so it survives tiny sizes in a Windows tray.`;

console.log("=== TurdMOD logo gen via DeepSeek ===");
await gen("turdmod-logo.svg", FULL);
await gen("turdmod-icon.svg", ICON);
console.log("=== DONE — both SVGs written to brand/ ===");
