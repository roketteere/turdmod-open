import { readdirSync, readFileSync, existsSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { info, jsonMode, warn } from "../lib/logger.js";

interface TemplateManifest {
  name: string;
  version: string;
  description: string;
  category: "ui" | "vehicle" | "item" | "audio" | "other";
  parameters: Array<{
    name: string;
    type: "string" | "int" | "color" | "bool" | "enum" | "file";
    description: string;
    default?: string | number | boolean;
    enum?: string[];
    min?: number;
    max?: number;
  }>;
}

function templatesDir(): string {
  // ESM-safe __dirname replacement.
  const __filename = fileURLToPath(import.meta.url);
  const __dirname = dirname(__filename);
  // Walk up to package root, then into templates/
  return join(__dirname, "..", "..", "templates");
}

function listTemplates(): TemplateManifest[] {
  const dir = templatesDir();
  if (!existsSync(dir)) return [];
  const out: TemplateManifest[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    const manifestPath = join(dir, entry.name, "manifest.json");
    if (!existsSync(manifestPath)) continue;
    try {
      const m = JSON.parse(readFileSync(manifestPath, "utf8")) as TemplateManifest;
      out.push(m);
    } catch {
      warn(`bad manifest in ${entry.name}`);
    }
  }
  return out;
}

export async function cmdTemplate(args: string[]): Promise<void> {
  const sub = args[0] || "list";
  if (sub === "list") {
    const items = listTemplates();
    if (jsonMode()) {
      console.log(JSON.stringify(items, null, 2));
      return;
    }
    if (items.length === 0) {
      warn("no templates found");
      return;
    }
    info("Available templates:\n");
    for (const t of items) {
      info(`  ${t.name.padEnd(20)} [${t.category}]  v${t.version}`);
      info(`    ${t.description}`);
    }
    info("\nUsage: tmc widget add <name>");
    return;
  }
  if (sub === "show") {
    const name = args[1];
    if (!name) throw new Error("usage: tmc template show <name>");
    const items = listTemplates();
    const t = items.find(x => x.name === name);
    if (!t) throw new Error(`template not found: ${name}`);
    if (jsonMode()) { console.log(JSON.stringify(t, null, 2)); return; }
    info(`# ${t.name} (${t.category})  v${t.version}`);
    info(t.description);
    info("\nParameters:");
    for (const p of t.parameters) {
      const def = p.default !== undefined ? ` [default: ${p.default}]` : "";
      const rng = p.min !== undefined && p.max !== undefined ? ` (${p.min}..${p.max})` : "";
      const enm = p.enum ? ` { ${p.enum.join(" | ")} }` : "";
      info(`  ${p.name} (${p.type}${rng}${enm})${def}`);
      info(`    ${p.description}`);
    }
    return;
  }
  throw new Error(`unknown template subcommand: ${sub}`);
}
