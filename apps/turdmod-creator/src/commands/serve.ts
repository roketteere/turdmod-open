/**
 * `tmc serve` — local web GUI for turdmod-creator.
 *
 * Joel 2026-05-23: "now the gui maker needs a gui version as well."
 *
 * Cross-platform browser UI. No frontend framework — single HTML file
 * with vanilla JS + CSS. Backend mirrors the CLI commands via JSON HTTP.
 *
 * Usage:
 *   tmc serve                 — http://localhost:5179 (default)
 *   tmc serve --port 8080     — custom port
 *   tmc serve --no-open       — don't auto-open browser
 *   tmc serve --project DIR   — open straight into a project
 */
import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import { readFileSync, existsSync, readdirSync, mkdirSync, writeFileSync, statSync } from "node:fs";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";
import { flag, info, ok, warn, err, logEvent } from "../lib/logger.js";
import { loadConfig, saveConfig } from "../lib/config.js";
import { callAi, type ProviderName } from "../ai/providers.js";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const PACKAGE_ROOT = resolve(__dirname, "..", "..");
const UI_DIR = join(PACKAGE_ROOT, "src", "ui");
const TEMPLATES_DIR = join(PACKAGE_ROOT, "templates");

interface TemplateManifest {
  name: string;
  version: string;
  description: string;
  category: string;
  baseClass?: string;
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

interface ProjectManifest {
  name: string;
  version: string;
  author: string;
  type: string;
  createdAt: string;
  ueVersion?: string;
  ueProjectPath?: string | null;
  widgets: Array<{
    name: string;
    template: string;
    templateVersion?: string;
    parameters: Record<string, unknown>;
    createdAt: string;
  }>;
}

function readTemplates(): TemplateManifest[] {
  if (!existsSync(TEMPLATES_DIR)) return [];
  const out: TemplateManifest[] = [];
  for (const entry of readdirSync(TEMPLATES_DIR, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue;
    const manifestPath = join(TEMPLATES_DIR, entry.name, "manifest.json");
    if (!existsSync(manifestPath)) continue;
    try {
      out.push(JSON.parse(readFileSync(manifestPath, "utf8")) as TemplateManifest);
    } catch { /* skip */ }
  }
  return out;
}

function readProject(dir: string): ProjectManifest | null {
  const p = join(dir, "tmc.json");
  if (!existsSync(p)) return null;
  try {
    return JSON.parse(readFileSync(p, "utf8")) as ProjectManifest;
  } catch {
    return null;
  }
}

async function readJsonBody(req: IncomingMessage): Promise<Record<string, unknown>> {
  const chunks: Buffer[] = [];
  for await (const chunk of req) chunks.push(chunk as Buffer);
  const text = Buffer.concat(chunks).toString("utf8");
  if (!text) return {};
  try {
    return JSON.parse(text) as Record<string, unknown>;
  } catch {
    throw new Error("invalid JSON body");
  }
}

function sendJson(res: ServerResponse, status: number, body: unknown): void {
  res.writeHead(status, {
    "Content-Type": "application/json",
    "Access-Control-Allow-Origin": "*",
  });
  res.end(JSON.stringify(body));
}

function sendFile(res: ServerResponse, path: string, contentType: string): void {
  try {
    const buf = readFileSync(path);
    res.writeHead(200, { "Content-Type": contentType, "Cache-Control": "no-store" });
    res.end(buf);
  } catch {
    res.writeHead(404);
    res.end("not found");
  }
}

function safeJoin(root: string, candidate: string): string | null {
  const full = resolve(root, candidate);
  if (!full.startsWith(resolve(root))) return null;
  return full;
}

export async function cmdServe(args: string[]): Promise<void> {
  const port = parseInt(flag(args, "port") ?? "5179", 10);
  const noOpen = args.includes("--no-open");
  const projectDir = flag(args, "project") ?? process.cwd();

  const server = createServer(async (req, res) => {
    const url = new URL(req.url ?? "/", `http://localhost:${port}`);
    const path = url.pathname;
    try {
      // CORS preflight
      if (req.method === "OPTIONS") {
        res.writeHead(204, {
          "Access-Control-Allow-Origin": "*",
          "Access-Control-Allow-Methods": "GET,POST,OPTIONS",
          "Access-Control-Allow-Headers": "Content-Type",
        });
        return res.end();
      }

      // ── Static UI ─────────────────────────────────────────────
      if (path === "/" || path === "/index.html") {
        return sendFile(res, join(UI_DIR, "index.html"), "text/html; charset=utf-8");
      }
      if (path === "/app.js") return sendFile(res, join(UI_DIR, "app.js"), "application/javascript; charset=utf-8");
      if (path === "/style.css") return sendFile(res, join(UI_DIR, "style.css"), "text/css; charset=utf-8");
      if (path === "/favicon.ico") {
        res.writeHead(204); return res.end();
      }

      // ── API ───────────────────────────────────────────────────
      if (path === "/api/templates" && req.method === "GET") {
        return sendJson(res, 200, { templates: readTemplates() });
      }
      if (path.startsWith("/api/template/") && req.method === "GET") {
        const name = path.substring("/api/template/".length);
        const t = readTemplates().find(x => x.name === name);
        if (!t) return sendJson(res, 404, { error: "template not found" });
        return sendJson(res, 200, t);
      }
      if (path === "/api/project" && req.method === "GET") {
        const dir = url.searchParams.get("dir") ?? projectDir;
        const p = readProject(dir);
        return sendJson(res, 200, { project: p, dir });
      }
      if (path === "/api/init" && req.method === "POST") {
        const body = await readJsonBody(req);
        const name = String(body.name ?? "");
        const author = String(body.author ?? "Anonymous");
        const parentDir = String(body.dir ?? projectDir);
        if (!/^[a-zA-Z][a-zA-Z0-9_-]*$/.test(name)) {
          return sendJson(res, 400, { error: "invalid name; use letters, digits, _ or -" });
        }
        const dir = join(parentDir, name);
        if (existsSync(dir)) return sendJson(res, 409, { error: "directory exists" });
        mkdirSync(dir, { recursive: true });
        mkdirSync(join(dir, ".tmc"), { recursive: true });
        mkdirSync(join(dir, "widgets"), { recursive: true });
        const manifest: ProjectManifest = {
          name, author,
          version: "0.1.0",
          type: "turdmod-creator-project",
          createdAt: new Date().toISOString(),
          ueVersion: "4.27.2",
          ueProjectPath: null,
          widgets: [],
        };
        writeFileSync(join(dir, "tmc.json"), JSON.stringify(manifest, null, 2), "utf8");
        logEvent({ kind: "gui.init", project: name, dir });
        return sendJson(res, 200, { ok: true, dir, project: manifest });
      }
      if (path === "/api/widget" && req.method === "POST") {
        const body = await readJsonBody(req);
        const dir = String(body.dir ?? projectDir);
        const template = String(body.template ?? "");
        const name = String(body.name ?? "");
        const parameters = (body.parameters ?? {}) as Record<string, unknown>;
        if (!/^[a-zA-Z][a-zA-Z0-9_-]*$/.test(name)) {
          return sendJson(res, 400, { error: "invalid widget name" });
        }
        const project = readProject(dir);
        if (!project) return sendJson(res, 404, { error: "no project at dir" });
        if (project.widgets.some(w => w.name === name)) {
          return sendJson(res, 409, { error: "widget exists" });
        }
        const tpl = readTemplates().find(t => t.name === template);
        if (!tpl) return sendJson(res, 404, { error: "template not found" });
        const widgetDir = join(dir, "widgets", name);
        mkdirSync(widgetDir, { recursive: true });
        const widget = {
          name,
          template,
          templateVersion: tpl.version,
          parameters,
          createdAt: new Date().toISOString(),
        };
        writeFileSync(join(widgetDir, "widget.json"), JSON.stringify(widget, null, 2), "utf8");
        project.widgets.push(widget);
        writeFileSync(join(dir, "tmc.json"), JSON.stringify(project, null, 2), "utf8");
        logEvent({ kind: "gui.widget.add", widget: name, template, dir });
        return sendJson(res, 200, { ok: true, widget, project });
      }
      if (path === "/api/widget/delete" && req.method === "POST") {
        const body = await readJsonBody(req);
        const dir = String(body.dir ?? projectDir);
        const name = String(body.name ?? "");
        const project = readProject(dir);
        if (!project) return sendJson(res, 404, { error: "no project at dir" });
        project.widgets = project.widgets.filter(w => w.name !== name);
        writeFileSync(join(dir, "tmc.json"), JSON.stringify(project, null, 2), "utf8");
        logEvent({ kind: "gui.widget.delete", widget: name, dir });
        return sendJson(res, 200, { ok: true, project });
      }
      if (path === "/api/ai" && req.method === "POST") {
        const body = await readJsonBody(req);
        const provider = String(body.provider ?? "deepseek") as ProviderName;
        const model = String(body.model ?? "deepseek-chat");
        const keyEnv = body.keyEnv as string | undefined;
        const prompt = String(body.prompt ?? "");
        if (!prompt) return sendJson(res, 400, { error: "prompt required" });
        // Build a schema-aware system prompt — inject the real template
        // schemas so the AI sticks to actual params instead of inventing them.
        const tpls = readTemplates();
        const schemaBlock = tpls.map(t => {
          const params = t.parameters.map(p => {
            const range = (p.min !== undefined && p.max !== undefined) ? ` ${p.min}..${p.max}` : "";
            const enm = p.enum ? ` {${p.enum.join("|")}}` : "";
            const def = p.default !== undefined ? ` default=${JSON.stringify(p.default)}` : "";
            return `    - ${p.name} (${p.type}${range}${enm})${def} — ${p.description}`;
          }).join("\n");
          return `  ${t.name}  [${t.category}]  v${t.version}\n    ${t.description}\n${params}`;
        }).join("\n\n");
        const systemPrompt = `You are TurdMOD's widget-authoring assistant. The user is editing in a web GUI.

You may ONLY propose actions that use these templates and their EXACT parameter names. Do not invent parameters that don't exist.

AVAILABLE TEMPLATES:

${schemaBlock}

When the user describes what they want, respond with a STRUCTURED JSON proposal wrapped in a \`\`\`json code block:

{
  "kind": "widget.proposal",
  "rationale": "<one short paragraph>",
  "actions": [
    { "type": "widget.add", "template": "<exact-template-name>", "name": "<widget-name>", "parameters": { "<exact-param-name>": <value>, ... } }
  ]
}

Rules:
- ALL parameter names must match exactly what the template schema lists. If a behavior you want needs a parameter that doesn't exist, mention it in rationale and pick the closest existing param OR pick a different template.
- Widget names must match /^[a-zA-Z][a-zA-Z0-9_-]*$/.
- For colors, use #RRGGBB hex format.
- For string params that hold JSON (like tabsJson, buttonsJson), emit valid JSON-as-string.
- Be concise — the user is paying for tokens.`;
        try {
          const result = await callAi({
            provider, model,
            ...(keyEnv ? { keyEnv } : {}),
            systemPrompt,
            userPrompt: prompt,
          });
          logEvent({
            kind: "gui.ai", provider, model,
            promptTokens: result.promptTokens, completionTokens: result.completionTokens,
            estimatedUSD: result.estimatedUSD,
          });
          return sendJson(res, 200, { ok: true, result });
        } catch (e) {
          return sendJson(res, 500, { error: (e as Error).message });
        }
      }
      if (path === "/api/doctor" && req.method === "GET") {
        const cfg = loadConfig();
        const checks: Array<{ name: string; ok: boolean; detail: string }> = [];
        const ueRoot = cfg.uePath ?? process.env.UE_4_27_PATH;
        checks.push({
          name: "UE 4.27 + UnrealPak",
          ok: !!(ueRoot && existsSync(join(ueRoot, "Engine", "Binaries", "Win64", "UnrealPak.exe"))),
          detail: ueRoot ? ueRoot : "set cfg.uePath or env UE_4_27_PATH",
        });
        checks.push({
          name: "Node >=18",
          ok: parseInt(process.versions.node.split(".")[0]!, 10) >= 18,
          detail: `running ${process.version}`,
        });
        const keyVar = cfg.keyEnvVar ?? "TURDMOD_AI_KEY";
        checks.push({
          name: "AI key (BYO)",
          ok: !!process.env[keyVar] || (cfg.aiProvider === "ollama"),
          detail: `provider=${cfg.aiProvider ?? "(unset)"} keyEnv=${keyVar} keyPresent=${!!process.env[keyVar]}`,
        });
        const pipeFile = process.env.LOCALAPPDATA
          ? join(process.env.LOCALAPPDATA, "TurdMOD", "engine", "pipe.txt")
          : null;
        checks.push({
          name: "TurdMOD bridge pipe.txt",
          ok: !!pipeFile && existsSync(pipeFile),
          detail: pipeFile ?? "LOCALAPPDATA unset",
        });
        return sendJson(res, 200, { checks });
      }
      if (path === "/api/config" && req.method === "GET") {
        return sendJson(res, 200, { config: loadConfig() });
      }
      if (path === "/api/config" && req.method === "POST") {
        const body = await readJsonBody(req);
        const cfg = loadConfig();
        const merged = { ...cfg, ...body };
        saveConfig(merged);
        return sendJson(res, 200, { ok: true, config: merged });
      }
      // Unknown route
      sendJson(res, 404, { error: "not found", path });
    } catch (e) {
      sendJson(res, 500, { error: (e as Error).message });
    }
  });

  server.listen(port, () => {
    const url = `http://localhost:${port}`;
    ok(`turdmod-creator GUI ready: ${url}`);
    info(`  project dir: ${projectDir}`);
    info(`  press Ctrl-C to stop`);
    if (!noOpen) {
      try {
        const cmd = process.platform === "win32" ? "cmd" : process.platform === "darwin" ? "open" : "xdg-open";
        const args = process.platform === "win32" ? ["/c", "start", "", url] : [url];
        spawn(cmd, args, { detached: true, stdio: "ignore" }).unref();
      } catch {
        warn(`could not auto-open browser. Open manually: ${url}`);
      }
    }
  });

  // Graceful shutdown.
  for (const sig of ["SIGINT", "SIGTERM"] as const) {
    process.on(sig, () => {
      info(`shutting down...`);
      server.close(() => process.exit(0));
    });
  }
}
