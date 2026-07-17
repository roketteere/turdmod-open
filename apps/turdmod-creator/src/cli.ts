#!/usr/bin/env node
/**
 * turdmod-creator — zero-to-hero CLI for authoring UE 4.27 content paks.
 *
 * Joel 2026-05-23: "make it super easy for noobies to use and have advance
 * options for power users just tucked away ... AI to pipe in via cli ...
 * their own api keys their responsibility not ours."
 *
 * UX tiers:
 *   - Default (noob): 3-5 prompts → working widget. No UE Editor needed.
 *   - --advanced flag: exposes raw cook flags, .uasset paths, BP scripting.
 *   - --ai prompt "..." pipes user's choice of LLM (BYO key, BYO billing).
 *
 * All AI features are bring-your-own. We never proxy keys. We never see
 * keys. We never charge for AI usage. Your billing, your responsibility.
 */
import { argv } from "node:process";
import { cmdInit }     from "./commands/init.js";
import { cmdTemplate } from "./commands/template.js";
import { cmdWidget }   from "./commands/widget.js";
import { cmdCook }     from "./commands/cook.js";
import { cmdPublish }  from "./commands/publish.js";
import { cmdAi }       from "./commands/ai.js";
import { cmdDoctor }   from "./commands/doctor.js";
import { cmdServe }    from "./commands/serve.js";

const HELP = `
turdmod-creator (tmc) — content authoring CLI for SCUM/UE 4.27 modding.

  USAGE
    tmc <command> [options]

  COMMANDS
    init <project>            Create a new creator project
    template list             Show available widget templates
    template show <name>      Detail a template's parameters
    widget add <template>     Add a widget instance to project
    widget list               List widgets in current project
    cook                      Build paks from the project
    publish [widget]          Upload to turdmod-marketplace
    ai prompt "<text>"        Ask AI to generate/modify (BYO key)
    ai chat                   Interactive AI session (BYO key)
    serve [--port N]          Launch the GUI in your browser
    doctor                    Diagnose setup (UE path, keys, etc.)
    help                      Show this help

  GLOBAL FLAGS
    --advanced                Unlock advanced options + raw UE access
    --project <dir>           Operate on project at <dir> (default: cwd)
    --quiet                   Suppress non-essential output
    --json                    Machine-readable output

  AI FLAGS (all BYO — bring-your-own key)
    --provider <name>         openai | anthropic | deepseek | ollama | gemini
                              (default: ollama, local-only)
    --model <id>              e.g. claude-haiku-4-5 | gpt-4.1-mini | deepseek-chat
    --key-env <var>           env var holding the API key (default: TURDMOD_AI_KEY)

  CONFIG
    ~/.turdmod-creator/config.json  — paths + AI prefs (gitignored)
    ~/.turdmod-creator/logs/        — NDJSON command logs

  EXAMPLES
    tmc init my-first-widget
    tmc template list
    tmc widget add notification --name welcome --text "Hello island!"
    tmc cook
    tmc ai prompt "make me a healing wheel with 6 segments" --provider deepseek
    tmc doctor

  Read the docs: https://github.com/roketteere/turdmod
`;

async function main(): Promise<void> {
  const args = argv.slice(2);
  const cmd  = args[0];
  const rest = args.slice(1);
  if (!cmd || cmd === "help" || cmd === "-h" || cmd === "--help") {
    console.log(HELP);
    return;
  }
  try {
    switch (cmd) {
      case "init":     await cmdInit(rest);     return;
      case "template": await cmdTemplate(rest); return;
      case "widget":   await cmdWidget(rest);   return;
      case "cook":     await cmdCook(rest);     return;
      case "publish":  await cmdPublish(rest);  return;
      case "ai":       await cmdAi(rest);       return;
      case "doctor":   await cmdDoctor(rest);   return;
      case "serve":    await cmdServe(rest);    return;
      default:
        console.error(`Unknown command: ${cmd}`);
        console.error(`Run \`tmc help\` for usage.`);
        process.exit(2);
    }
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    console.error(`tmc: ${msg}`);
    process.exit(1);
  }
}

void main();
