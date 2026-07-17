#!/usr/bin/env node
/**
 * Auto-generate the TurdMOD API reference from the .ts sources.
 *
 * Walks every namespace module (content / player / world / ui / persistence /
 * network), pulls exported functions + types + their JSDoc via the
 * TypeScript compiler API, and writes a single Markdown reference to
 * docs/turdmod/api-reference.md.
 *
 * Idempotent. Run from the repo root:
 *   pnpm --filter @turdmod/turdmod-api docs
 */

import * as path from "node:path";
import * as fs from "node:fs";
import * as ts from "typescript";

const PKG_ROOT = path.resolve(path.dirname(new URL(import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1")), "..");
const REPO_ROOT = path.resolve(PKG_ROOT, "..", "..");
const OUT_PATH = path.join(REPO_ROOT, "docs", "turdmod", "api-reference.md");

const NAMESPACES: Array<{ name: string; file: string; blurb: string }> = [
  { name: "content",     file: "content.ts",     blurb: "DataTable / Blueprint / locres overrides." },
  { name: "player",      file: "player.ts",      blurb: "Player lifecycle, inventory, stats hooks." },
  { name: "world",       file: "world.ts",       blurb: "Spawn / despawn entities, weather, time, custom POIs." },
  { name: "ui",          file: "ui.ts",          blurb: "HUD widgets, custom menus, hotkeys, toasts (Strategy C only)." },
  { name: "persistence", file: "persistence.ts", blurb: "Per-mod KV slot. Top-level + per-player scoped." },
  { name: "network",     file: "network.ts",     blurb: "Server-authoritative event dispatch. Private servers only." },
];

interface Doc {
  description: string;
  since?: string;
  params: { name: string; description: string }[];
  returns?: string;
  example?: string;
  raw: string;
}

interface ApiItem {
  kind: "function" | "type" | "interface" | "class";
  name: string;
  signature: string;
  doc: Doc;
}

function parseJSDoc(node: ts.Node, src: ts.SourceFile): Doc {
  const empty: Doc = { description: "", params: [], raw: "" };
  const ranges = ts.getLeadingCommentRanges(src.text, node.getFullStart());
  if (!ranges) return empty;
  const blocks = ranges
    .filter(r => src.text.slice(r.pos, r.pos + 3) === "/**")
    .map(r => src.text.slice(r.pos, r.end));
  if (!blocks.length) return empty;

  const raw = blocks[blocks.length - 1]!;
  const lines = raw
    .split("\n")
    .map(l => l.replace(/^\s*\/?\*+\/?/, "").replace(/\s*\*+\/$/, "").trim())
    .filter((l, i, arr) => !(i === 0 && l === "") && !(i === arr.length - 1 && l === ""));

  const doc: Doc = { description: "", params: [], raw };
  let mode: "desc" | "param" | "returns" | "example" | "since" = "desc";
  let buf: string[] = [];
  const flush = () => {
    const text = buf.join("\n").trim();
    if (mode === "desc" && text) doc.description = (doc.description ? doc.description + "\n\n" : "") + text;
    if (mode === "returns" && text) doc.returns = text;
    if (mode === "example" && text) doc.example = text;
    if (mode === "since" && text) doc.since = text;
    buf = [];
  };

  for (const line of lines) {
    const tag = line.match(/^@(\w+)\s*(.*)$/);
    if (!tag) {
      buf.push(line);
      continue;
    }
    flush();
    const [, name, rest] = tag;
    switch (name) {
      case "param": {
        const m = rest!.match(/^([A-Za-z_$][\w$.]*)\s*-?\s*(.*)$/);
        if (m) doc.params.push({ name: m[1]!, description: m[2] || "" });
        mode = "desc";
        buf = [];
        break;
      }
      case "returns":
      case "return":
        mode = "returns";
        buf = [rest!];
        break;
      case "example":
        mode = "example";
        buf = [];
        break;
      case "since":
        mode = "since";
        buf = [rest!];
        break;
      default:
        mode = "desc";
        buf = [`${rest!}`];
    }
  }
  flush();
  return doc;
}

function signatureOf(node: ts.Node): string {
  const text = node.getText().replace(/\r?\n/g, " ").replace(/\s+/g, " ").trim();
  if (text.length > 240) return text.slice(0, 237) + "...";
  return text;
}

function extractItems(filePath: string): ApiItem[] {
  const src = ts.createSourceFile(
    filePath,
    fs.readFileSync(filePath, "utf-8"),
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TS,
  );
  const out: ApiItem[] = [];
  function isExported(node: ts.Node): boolean {
    return (ts.canHaveModifiers(node) ? ts.getModifiers(node) : undefined)
      ?.some(m => m.kind === ts.SyntaxKind.ExportKeyword) ?? false;
  }
  ts.forEachChild(src, node => {
    if (ts.isFunctionDeclaration(node) && node.name && isExported(node)) {
      const sig = node.getText().split(/\{/)[0]!.trim();
      out.push({ kind: "function", name: node.name.text, signature: sig, doc: parseJSDoc(node, src) });
    } else if (ts.isVariableStatement(node) && isExported(node)) {
      for (const d of node.declarationList.declarations) {
        if (ts.isIdentifier(d.name) && d.initializer && ts.isArrowFunction(d.initializer)) {
          const sig = `export const ${d.name.text} = ${signatureOf(d.initializer)}`.split(/\{|=>/)[0]!.trim();
          out.push({ kind: "function", name: d.name.text, signature: sig, doc: parseJSDoc(node, src) });
        }
      }
    } else if (ts.isInterfaceDeclaration(node) && isExported(node)) {
      out.push({ kind: "interface", name: node.name.text, signature: signatureOf(node), doc: parseJSDoc(node, src) });
    } else if (ts.isTypeAliasDeclaration(node) && isExported(node)) {
      out.push({ kind: "type", name: node.name.text, signature: signatureOf(node), doc: parseJSDoc(node, src) });
    } else if (ts.isClassDeclaration(node) && node.name && isExported(node)) {
      out.push({ kind: "class", name: node.name.text, signature: `class ${node.name.text}`, doc: parseJSDoc(node, src) });
    }
  });
  return out;
}

function renderDoc(d: Doc): string {
  const lines: string[] = [];
  if (d.description) lines.push(d.description);
  if (d.params.length) {
    lines.push("");
    lines.push("**Parameters:**");
    for (const p of d.params) lines.push(`- \`${p.name}\` — ${p.description}`);
  }
  if (d.returns) {
    lines.push("");
    lines.push(`**Returns:** ${d.returns}`);
  }
  if (d.example) {
    lines.push("");
    lines.push("```ts");
    lines.push(d.example);
    lines.push("```");
  }
  if (d.since) {
    lines.push("");
    lines.push(`*${d.since.startsWith("@") ? d.since : "Since " + d.since}*`);
  }
  return lines.join("\n");
}

function renderItem(item: ApiItem): string {
  const heading = `### \`${item.name}\``;
  const sig = "```ts\n" + item.signature + "\n```";
  return [heading, "", sig, "", renderDoc(item.doc)].join("\n");
}

function main() {
  const out: string[] = [
    "# TurdMOD API reference",
    "",
    `*Generated ${new Date().toISOString()} from \`packages/turdmod-api/src/\` by \`scripts/gen-docs.ts\`. Do not edit by hand — re-run \`pnpm --filter @turdmod/turdmod-api docs\` after changing the API.*`,
    "",
    "Mod authors `import { content, player, world, ui, persistence, network } from \"@turdmod/turdmod-api\"` and call the namespaced functions documented below. Argument validation throws `TurdMODError` before the runtime is touched. The host (Strategy B/C loader) binds the runtime via `setRuntime()`.",
    "",
    "See also:",
    "- [Manifest spec v1](manifest-spec.md) — what authors write in `turdmod.json`",
    "- [Compatibility policy](compatibility-policy.md) — when each delivery mode is allowed",
    "- [BattlEye safety](battleye-safety.md) — Strategy C runtime gating",
    "- [Loader architecture](loader-architecture.md) — Strategy C internals",
    "",
  ];

  for (const ns of NAMESPACES) {
    out.push(`## \`turdmod.${ns.name}\``);
    out.push("");
    out.push(ns.blurb);
    out.push("");
    const items = extractItems(path.join(PKG_ROOT, "src", ns.file));
    if (!items.length) {
      out.push("_(no exported items)_");
      out.push("");
      continue;
    }
    const fns = items.filter(i => i.kind === "function");
    const types = items.filter(i => i.kind === "interface" || i.kind === "type");
    if (fns.length) {
      out.push("### Functions");
      out.push("");
      for (const f of fns) {
        out.push(renderItem(f));
        out.push("");
      }
    }
    if (types.length) {
      out.push("### Types");
      out.push("");
      for (const t of types) {
        out.push(renderItem(t));
        out.push("");
      }
    }
  }

  // Cross-cutting types from runtime.ts.
  out.push(`## Shared (\`@turdmod/turdmod-api\`)`);
  out.push("");
  const shared = extractItems(path.join(PKG_ROOT, "src", "runtime.ts"));
  for (const item of shared) {
    out.push(renderItem(item));
    out.push("");
  }

  fs.mkdirSync(path.dirname(OUT_PATH), { recursive: true });
  fs.writeFileSync(OUT_PATH, out.join("\n"), "utf-8");
  console.log(`wrote ${path.relative(REPO_ROOT, OUT_PATH)}  (${out.join("\n").length.toLocaleString()} chars, ${NAMESPACES.length} namespaces)`);
}

main();
