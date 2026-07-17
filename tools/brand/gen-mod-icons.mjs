#!/usr/bin/env node
// gen-mod-icons.mjs — one branded mod icon per TMM ModCategory. Each = a dark
// neon-cyan tile (TurdMOD theme) + the category's emoji + a tiny poop accent so
// every mod reads as "a TurdMOD mod" at a glance. Deterministic (no model).
// Out: apps/turdmod-manager/public/mod-icons/<category>.svg  (+ default.svg)
// @dep brand/mod-src/<cat>.svg (Twemoji, CC-BY) + brand/mod-src/poop.svg

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const SRC = resolve(ROOT, "brand", "mod-src");
const OUT = resolve(ROOT, "apps", "turdmod-manager", "public", "mod-icons");
mkdirSync(OUT, { recursive: true });
const NEON = "#00d4ff";

const inner = (f) => {
  const s = readFileSync(resolve(SRC, f), "utf8");
  return s.slice(s.indexOf(">", s.indexOf("<svg")) + 1, s.lastIndexOf("</svg>")).trim();
};
const POOP = inner("poop.svg"); // 36x36

// place a 36x36 emoji's inner markup, centered at (cx,cy) scaled.
const place = (markup, cx, cy, scale) =>
  `<g transform="translate(${cx - 18 * scale} ${cy - 18 * scale}) scale(${scale})">${markup}</g>`;

// category -> { emoji file (without ext) }, keyed by lowercase ModCategory.
const CATS = ["ui", "hud", "squad", "admin", "gameplay", "map", "vehicle", "npc"];

function tile(centerMarkup) {
  // 128 viewBox tile: rounded dark bg, neon border + glow, emoji center, poop accent.
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128" width="128" height="128">
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#121826"/><stop offset="1" stop-color="#080b12"/>
    </linearGradient>
    <filter id="glow" x="-40%" y="-40%" width="180%" height="180%">
      <feDropShadow dx="0" dy="0" stdDeviation="3.5" flood-color="${NEON}" flood-opacity="0.55"/>
    </filter>
  </defs>
  <rect x="6" y="6" width="116" height="116" rx="26" fill="url(#bg)" stroke="${NEON}" stroke-width="3" filter="url(#glow)"/>
  ${place(centerMarkup, 60, 58, 1.85)}
  ${place(POOP, 96, 96, 0.95)}
</svg>`;
}

for (const c of CATS) {
  writeFileSync(resolve(OUT, `${c}.svg`), tile(inner(`${c}.svg`)) + "\n");
}
// default icon: just the poop, centered, for any unmapped/unknown category.
writeFileSync(
  resolve(OUT, "default.svg"),
  tile(POOP) + "\n"
);
console.log("mod icons written:", [...CATS, "default"].map((c) => `${c}.svg`).join(" "));
