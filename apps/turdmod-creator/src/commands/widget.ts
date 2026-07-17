import { existsSync, readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { ask, validators } from "../lib/prompt.js";
import { advancedMode, flag, info, ok, warn, logEvent, jsonMode } from "../lib/logger.js";

interface TemplateParam {
  name: string;
  type: "string" | "int" | "color" | "bool" | "enum" | "file";
  description: string;
  default?: string | number | boolean;
  enum?: string[];
  min?: number;
  max?: number;
}

interface TemplateManifest {
  name: string;
  version: string;
  description: string;
  category: string;
  parameters: TemplateParam[];
  baseClass?: string;       // e.g. "UTurdMODWidget"
  outputs?: string[];       // list of generated file paths under widgets/<name>/
}

function templatesDir(): string {
  const __filename = fileURLToPath(import.meta.url);
  const __dirname = dirname(__filename);
  return join(__dirname, "..", "..", "templates");
}

async function promptParam(p: TemplateParam, advanced: boolean): Promise<string> {
  const vs = validators();
  let validator: ((s: string) => string | null) | undefined;
  switch (p.type) {
    case "color":   validator = vs.hexColor; break;
    case "int":     validator = p.min !== undefined && p.max !== undefined ? vs.intRange(p.min, p.max) : undefined; break;
    case "enum":    validator = (s: string) => (p.enum ?? []).includes(s) ? null : `must be one of: ${(p.enum ?? []).join(", ")}`; break;
    case "bool":    validator = (s: string) => /^(true|false|yes|no|y|n|1|0)$/i.test(s) ? null : "expected true/false"; break;
    default:        validator = undefined;
  }
  const def = p.default === undefined ? undefined : String(p.default);
  const message = advanced ? `${p.name} (${p.type}) — ${p.description}` : `${p.description}`;
  const promptOpts: { message: string; validator?: (s: string) => string | null; default?: string } = { message };
  if (validator) promptOpts.validator = validator;
  if (def !== undefined) promptOpts.default = def;
  return await ask(promptOpts);
}

export async function cmdWidget(args: string[]): Promise<void> {
  const sub = args[0];
  if (!sub || sub === "list") return widgetList();
  if (sub === "add") return widgetAdd(args.slice(1));
  throw new Error(`unknown widget subcommand: ${sub}`);
}

async function widgetList(): Promise<void> {
  const proj = resolveProject();
  const manifestPath = join(proj.dir, "tmc.json");
  const m = JSON.parse(readFileSync(manifestPath, "utf8")) as { widgets: Array<{ name: string; template: string; parameters: Record<string, unknown> }> };
  if (jsonMode()) { console.log(JSON.stringify(m.widgets, null, 2)); return; }
  if (m.widgets.length === 0) {
    info(`no widgets yet. Try: tmc widget add notification --name welcome`);
    return;
  }
  info(`Widgets in ${proj.dir}:`);
  for (const w of m.widgets) {
    info(`  ${w.name.padEnd(20)} from template ${w.template}`);
  }
}

async function widgetAdd(args: string[]): Promise<void> {
  const templateName = args[0];
  if (!templateName) throw new Error("usage: tmc widget add <template> [--name X] [param=val ...]");

  const tplDir = join(templatesDir(), templateName);
  if (!existsSync(join(tplDir, "manifest.json"))) throw new Error(`template not found: ${templateName}`);
  const tpl = JSON.parse(readFileSync(join(tplDir, "manifest.json"), "utf8")) as TemplateManifest;

  const proj = resolveProject();
  const projManifestPath = join(proj.dir, "tmc.json");
  const projManifest = JSON.parse(readFileSync(projManifestPath, "utf8")) as { widgets: Array<{ name: string; template: string; parameters: Record<string, unknown>; createdAt: string }> };

  const widgetName = flag(args, "name") ?? await ask({
    message: "Widget instance name (used as filename + class suffix)",
    validator: validators().identifier,
  });
  if (projManifest.widgets.some(w => w.name === widgetName)) {
    warn(`widget "${widgetName}" already exists in project`);
    return;
  }

  // Gather parameters via interactive prompts (or --param=value overrides).
  const params: Record<string, unknown> = {};
  for (const p of tpl.parameters) {
    const cliVal = flag(args, p.name);
    if (cliVal !== undefined) {
      params[p.name] = parseValue(cliVal, p.type);
      continue;
    }
    // Hide "advanced" params in noob mode unless they have no default.
    const isAdvanced = (p as TemplateParam & { advanced?: boolean }).advanced === true;
    if (isAdvanced && !advancedMode() && p.default !== undefined) {
      params[p.name] = p.default;
      continue;
    }
    const raw = await promptParam(p, advancedMode());
    params[p.name] = parseValue(raw, p.type);
  }

  // Write the widget instance to widgets/<widgetName>/.
  const widgetDir = join(proj.dir, "widgets", widgetName);
  mkdirSync(widgetDir, { recursive: true });
  const instance = {
    name: widgetName,
    template: templateName,
    templateVersion: tpl.version,
    parameters: params,
    createdAt: new Date().toISOString(),
  };
  writeFileSync(join(widgetDir, "widget.json"), JSON.stringify(instance, null, 2), "utf8");

  // Copy any template static files (e.g. SVG previews, README).
  // For v1 we just reference the template's outputs by listing them.
  // Actual cooking happens at `tmc cook` which delegates to UnrealPak.

  projManifest.widgets.push(instance);
  writeFileSync(projManifestPath, JSON.stringify(projManifest, null, 2), "utf8");

  ok(`widget added: ${widgetName} (template: ${templateName})`);
  info(`  next: tmc cook   (build paks)`);
  logEvent({ kind: "widget.add", widget: widgetName, template: templateName });
}

function parseValue(raw: string, type: TemplateParam["type"]): unknown {
  switch (type) {
    case "int":   return parseInt(raw, 10);
    case "bool":  return /^(true|yes|y|1)$/i.test(raw);
    case "color": return raw.startsWith("#") ? raw : `#${raw}`;
    default:      return raw;
  }
}

interface ProjectInfo { dir: string }
function resolveProject(): ProjectInfo {
  const projectFlag = flag(process.argv.slice(2), "project");
  const dir = projectFlag ? resolve(projectFlag) : process.cwd();
  if (!existsSync(join(dir, "tmc.json"))) {
    throw new Error(`no turdmod-creator project here. Run \`tmc init <name>\` first.`);
  }
  return { dir };
}
