#!/usr/bin/env node
// compose-logo.mjs — assemble the OFFICIAL TurdMOD logo from the real emoji art
// (Twemoji 💩 + 🪰, CC-BY 4.0) plus our neon-cyberpunk treatment: cyan rim-glow +
// dashed orbital arcs + flies buzzing around the poop. Deterministic — no model needed.
// Produces brand/turdmod-logo.svg (512 master) and brand/turdmod-icon.svg (256 glyph).
// @inv neon = #00d4ff (launcher styles.css --neon). @dep src-1f4a9.svg, src-1fab0.svg

import { readFileSync, writeFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const DIR = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..", "brand");
const NEON = "#00d4ff";

// Pull the inner markup (children of <svg>) so we can re-group + transform it.
const inner = (file) => {
  const s = readFileSync(resolve(DIR, file), "utf8");
  return s.slice(s.indexOf(">", s.indexOf("<svg")) + 1, s.lastIndexOf("</svg>")).trim();
};
const POOP = inner("src-1f4a9.svg"); // viewBox 0 0 36 36
const FLY = inner("src-1fab0.svg");  // viewBox 0 0 36 36

// A poop group scaled/placed; src space is 36x36, center (18,18).
const poopAt = (cx, cy, scale, filter = "") =>
  `<g ${filter}transform="translate(${cx - 18 * scale} ${cy - 18 * scale}) scale(${scale})">${POOP}</g>`;
const flyAt = (cx, cy, scale, rot = 0) =>
  `<g transform="translate(${cx} ${cy}) rotate(${rot}) scale(${scale}) translate(-18 -18)">${FLY}</g>`;
// dashed cyan orbital arc (a partial ellipse stroke) trailing a fly
const arc = (d) =>
  `<path d="${d}" fill="none" stroke="${NEON}" stroke-width="2.5" stroke-dasharray="6 7" stroke-linecap="round" opacity="0.55"/>`;

// Neon rim-glow filter: dilate the alpha, blur it, flood cyan, stack under the source.
const glowFilter = (id, dilate, blur) => `
  <filter id="${id}" x="-60%" y="-60%" width="220%" height="220%">
    <feMorphology in="SourceAlpha" operator="dilate" radius="${dilate}" result="d"/>
    <feGaussianBlur in="d" stdDeviation="${blur}" result="b"/>
    <feFlood flood-color="${NEON}" flood-opacity="0.9"/>
    <feComposite in2="b" operator="in" result="g"/>
    <feMerge><feMergeNode in="g"/><feMergeNode in="g"/><feMergeNode in="SourceGraphic"/></feMerge>
  </filter>`;

// ---- MASTER LOGO (512) ----
const master = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" width="512" height="512">
  <defs>${glowFilter("neon", 2, 7)}</defs>
  ${arc("M 120 150 q 40 -45 95 -30")}
  ${arc("M 400 210 q 35 35 5 90")}
  ${arc("M 150 400 q -30 -45 25 -70")}
  ${poopAt(256, 278, 9.2, 'filter="url(#neon)" ')}
  ${flyAt(150, 120, 1.7, -18)}
  ${flyAt(405, 235, 1.5, 22)}
  ${flyAt(150, 405, 1.4, 8)}
</svg>`;

// ---- ICON GLYPH (256) — bolder, ONE fly, fills the frame ----
const icon = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" width="256" height="256">
  <defs>${glowFilter("neonI", 2.4, 5)}</defs>
  ${arc("M 60 70 q 25 -28 60 -16")}
  ${poopAt(132, 148, 5.6, 'filter="url(#neonI)" ')}
  ${flyAt(78, 70, 1.25, -16)}
</svg>`;

writeFileSync(resolve(DIR, "turdmod-logo.svg"), master + "\n");
writeFileSync(resolve(DIR, "turdmod-icon.svg"), icon + "\n");
console.log("composed: turdmod-logo.svg + turdmod-icon.svg");
