import { readFileSync, existsSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { loadConfig } from "../lib/config.js";
import { advancedMode, flag, info, ok, warn, err, logEvent } from "../lib/logger.js";
import { callAi, type ProviderName } from "../ai/providers.js";
import { ask, confirm } from "../lib/prompt.js";

const WIDGET_AUTHOR_SYSTEM_PROMPT = `You are a TurdMOD widget-authoring assistant for UE 4.27 UMG widgets.

The user is creating SCUM-server mod widgets via turdmod-creator. You help them:
- Pick the right template (notification, healing-wheel, kit-picker, blank)
- Tune parameters
- Compose multi-widget kits
- Debug widget behavior

When the user describes what they want, respond with a STRUCTURED JSON proposal
(wrapped in a \`\`\`json code block) that the CLI will parse and ask the user
to confirm. Schema:

{
  "kind": "widget.proposal",
  "rationale": "<one paragraph>",
  "actions": [
    { "type": "init",     "project": "<name>",      "author": "<name>" },
    { "type": "widget.add", "template": "<name>",   "name": "<widget>", "parameters": { "k": "v" } },
    { "type": "cook" },
    { "type": "advice",   "text": "<explanation>" }
  ]
}

Only emit JSON when you have a concrete action proposal. For pure Q&A, respond
in natural language.

Constraints:
- The user is responsible for their API costs. Be concise.
- Don't invent template names; ask the user to run 'tmc template list' to see
  what's available before proposing.
- For 'blank' template, you can suggest custom Blueprint logic, but flag that
  it requires --advanced.
- Cost-sensitive: avoid unnecessary back-and-forth. One round-trip per task.`;

export async function cmdAi(args: string[]): Promise<void> {
  const sub = args[0];
  if (!sub) {
    info("Usage:");
    info("  tmc ai prompt \"<text>\"          # one-shot");
    info("  tmc ai chat                       # interactive (BYO key)");
    info("  tmc ai apply <proposal.json>      # apply a saved proposal");
    info("");
    info("Provider flags (BYO key — your billing, your responsibility):");
    info("  --provider openai|anthropic|deepseek|ollama|gemini  (default: ollama)");
    info("  --model <id>                                        (provider-specific)");
    info("  --key-env <name>                                    (default: TURDMOD_AI_KEY)");
    return;
  }
  if (sub === "prompt") return aiPrompt(args.slice(1));
  if (sub === "chat")   return aiChat(args.slice(1));
  if (sub === "apply")  return aiApply(args.slice(1));
  throw new Error(`unknown ai subcommand: ${sub}`);
}

function resolveAiArgs(args: string[]): { provider: ProviderName; model: string; keyEnv?: string } {
  const cfg = loadConfig();
  const provider = (flag(args, "provider") ?? cfg.aiProvider ?? "ollama") as ProviderName;
  const model = flag(args, "model") ?? cfg.aiModel ?? defaultModelFor(provider);
  const keyEnv = flag(args, "key-env") ?? cfg.keyEnvVar;
  const result: { provider: ProviderName; model: string; keyEnv?: string } = { provider, model };
  if (keyEnv !== undefined) result.keyEnv = keyEnv;
  return result;
}

function defaultModelFor(p: ProviderName): string {
  switch (p) {
    case "openai":    return "gpt-4.1-mini";
    case "anthropic": return "claude-haiku-4-5-20251001";
    case "deepseek":  return "deepseek-chat";
    case "gemini":    return "gemini-1.5-flash";
    case "ollama":    return "qwen2.5-coder:7b";
  }
}

async function aiPrompt(args: string[]): Promise<void> {
  const text = args.filter(a => !a.startsWith("--")).join(" ");
  if (!text) throw new Error("usage: tmc ai prompt \"<text>\"");
  const ai = resolveAiArgs(args);
  info(`AI: ${ai.provider}/${ai.model}  (BYO key — your billing)`);
  const result = await callAi({
    ...ai,
    systemPrompt: WIDGET_AUTHOR_SYSTEM_PROMPT,
    userPrompt: text,
    temperature: 0.3,
  });

  console.log("\n--- AI RESPONSE ---");
  console.log(result.text);
  console.log("---");

  if (result.estimatedUSD !== undefined) {
    info(`Cost estimate: ~$${result.estimatedUSD.toFixed(4)} (${result.promptTokens ?? "?"} in + ${result.completionTokens ?? "?"} out tokens)`);
  }

  // If the response contains a JSON proposal, ask before applying.
  const proposal = extractJsonProposal(result.text);
  if (proposal) {
    info(`\nDetected proposal with ${proposal.actions?.length ?? 0} action(s).`);
    const saveName = `proposal-${new Date().toISOString().replace(/[:.]/g, "-")}.json`;
    writeFileSync(saveName, JSON.stringify(proposal, null, 2), "utf8");
    ok(`saved: ${saveName}`);
    const apply = await confirm("Apply now?", false);
    if (apply) await aiApply([saveName]);
    else info(`  apply later: tmc ai apply ${saveName}`);
  }

  logEvent({
    kind: "ai.prompt",
    provider: ai.provider,
    model: ai.model,
    promptTokens: result.promptTokens,
    completionTokens: result.completionTokens,
    estimatedUSD: result.estimatedUSD,
  });
}

async function aiChat(args: string[]): Promise<void> {
  const ai = resolveAiArgs(args);
  info(`AI chat: ${ai.provider}/${ai.model}  (BYO key — your billing)`);
  info(`Type your message. Empty line to quit.`);
  while (true) {
    const text = await ask({ message: ">", default: "" });
    if (!text) break;
    const result = await callAi({
      ...ai,
      systemPrompt: WIDGET_AUTHOR_SYSTEM_PROMPT,
      userPrompt: text,
      temperature: 0.3,
    });
    console.log("\n" + result.text + "\n");
    if (result.estimatedUSD !== undefined) {
      info(`  (~$${result.estimatedUSD.toFixed(4)} this round)`);
    }
  }
  ok("chat ended.");
}

interface Proposal {
  kind: string;
  rationale?: string;
  actions: Array<{ type: string; [k: string]: unknown }>;
}

function extractJsonProposal(text: string): Proposal | null {
  const m = text.match(/```json\s*([\s\S]*?)```/);
  if (!m) return null;
  try {
    const obj = JSON.parse(m[1]!) as Proposal;
    if (obj.kind === "widget.proposal" && Array.isArray(obj.actions)) return obj;
    return null;
  } catch {
    return null;
  }
}

async function aiApply(args: string[]): Promise<void> {
  const path = args[0];
  if (!path) throw new Error("usage: tmc ai apply <proposal.json>");
  if (!existsSync(path)) throw new Error(`proposal not found: ${path}`);
  const proposal = JSON.parse(readFileSync(path, "utf8")) as Proposal;
  if (proposal.kind !== "widget.proposal") throw new Error(`not a widget.proposal: ${proposal.kind}`);

  info(`Applying ${proposal.actions.length} action(s)...`);
  for (const a of proposal.actions) {
    if (a.type === "advice") {
      info(`  [advice] ${a.text}`);
    } else if (a.type === "init") {
      info(`  [init] would create project '${a.project}' (run \`tmc init ${a.project}\` to apply)`);
    } else if (a.type === "widget.add") {
      info(`  [widget.add] would add ${a.template} as '${a.name}' with params ${JSON.stringify(a.parameters ?? {})}`);
      info(`     run: tmc widget add ${a.template} --name ${a.name}` + Object.entries((a.parameters as Record<string, unknown>) ?? {}).map(([k, v]) => ` --${k} ${v}`).join(""));
    } else if (a.type === "cook") {
      info(`  [cook] run \`tmc cook\` when ready`);
    } else {
      warn(`  unknown action type: ${a.type}`);
    }
  }
  warn(`v1 prints commands; v2 will execute them after a confirmation per action.`);
  logEvent({ kind: "ai.apply.preview", file: resolve(path), actions: proposal.actions.length });
}
