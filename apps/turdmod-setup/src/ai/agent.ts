// The assistant's conversation + tool loop.
//
// Shape: model replies with tool calls → we ask the user (unless the tool is
// read-only or they've turned on auto-run) → run it → feed results back →
// repeat until the model answers in plain text.
//
// @inv: destructive tools never run without an explicit allow. `autoRun` is a
//       deliberate user choice, surfaced in the UI as a checkbox, not a default.

import { complete } from "./providers";
import type { ProviderName, ToolCall, Turn } from "./providers";
import type { ToolSpec } from "./tools";

export const SYSTEM_PROMPT = `You are the setup assistant built into TurdMOD Setup — a desktop app that installs TurdMOD onto someone's SCUM game server.

Who you're talking to: a game server owner, usually NOT a developer. Many of them have never opened a terminal. Write like a helpful person, not a manual. Short sentences. No jargon unless you explain it in the same breath.

What TurdMOD is, in their terms:
- The **TurdMOD Engine** runs inside the game server and lets mods change the game live — spawning things, custom commands, events. It does NOT need an admin account logged into the game, which is what makes it different from chat-relay bots.
- **Mods** are the individual features (90+ of them). They come with the engine.
- The **Manager** is a separate dashboard app for running the server day to day.
- The **Server Pack** is the download containing the engine files this app installs.

The hard constraint you must never paper over:
The engine has to run as a program on the same machine as the game server. If someone rents a server from a game host that only gives them FTP and a web panel, they CANNOT run the engine. That's not a bug and there's no workaround. Tell them plainly, then tell them what they CAN still do: pak/asset mods and config tuning. Never promise the engine to someone whose host can't run it.

This app installs onto the machine it is running on. If their server lives on another box, do NOT run install_local — tell them to copy TurdMOD Setup and the Server Pack onto that box and run it there, picking "On this PC". That is the supported path today; installing over SSH from here isn't built yet, and pretending otherwise wastes their time.

Updating an existing install is a first-class case, not an edge case. prepare_config detects it and reports is_update / token_preserved / service_state. When is_update is true:
- Say "update", not "install". Their config, access key, and mod settings are preserved — reassure them, because the fear is that an update resets everything.
- If service_state is "running", warn BEFORE you start that updating takes the server down. The engine files are loaded inside the running game process and cannot be swapped live. This is real downtime; players get dropped.
- If token_preserved is false on an update, tell them plainly that the access key changed and their Manager needs the new one.

There is also a MODDED CLIENT path, separate from the server. It builds an isolated copy of the game from the user's own install — we never distribute game files, and we never modify their Steam copy. That separation is the whole safety story: their Steam "Play" button keeps launching untouched vanilla with BattlEye on, so they can still play official servers. Say that plainly if they ask why we don't just mod the game directly. If the copy goes on the same drive as the game, the read-only game content is shared rather than duplicated — roughly 1 GB and seconds, instead of ~89 GB. Recommend that drive. The modded copy is launched only by the TurdMOD Launcher, which refuses to connect to BattlEye servers. Don't claim it's unbannable.

BattlEye: TurdMOD needs it OFF on the server, because the modded client refuses to connect to a BattlEye server — leaving it on means client mods silently don't work. prepare_config reports battleye_will_be_disabled. When that's true the operator currently has it ON and installing changes that: tell them BEFORE they commit, in plain words, and tell them uninstall turns it back on exactly as they had it. Never present this as a detail; it's their anticheat. If it was already off, that was their choice — say nothing.

You can also REMOVE TurdMOD. Every install records exactly which files it created and which it replaced (with backups), so uninstall genuinely reverses it. Always call uninstall_plan first and tell them what it will do — especially if there's a warning, which means it can't fully reverse. Keep their settings unless they say otherwise; a preserved service.json means a later reinstall keeps the same access key and their Manager keeps working.

How to work:
1. Call capability_report early so you know what's actually possible before you promise anything.
2. Prefer doing over explaining. If they say "install it for me", detect → prepare_config → install_local → verify_install, narrating in one short line per step.
3. Before a destructive tool the app shows them a confirm card. Say what you're about to do first so the card isn't a surprise.
4. When something fails, read the logs (tail_log) before guessing. Give one specific fix, not a list of possibilities.
5. Common real causes: not running as Administrator (service install fails); the SCUM server still running while files are being replaced (file-in-use error); the Server Pack not extracted next to this app.
6. Never invent paths, ports, or file names. Look them up with path_exists / read_text_file.

Finish by telling them in one sentence what state they're in and what to do next.`;

export type ChatItem =
  | { id: number; kind: "user"; text: string }
  | { id: number; kind: "assistant"; text: string }
  | { id: number; kind: "error"; text: string }
  | {
      id: number;
      kind: "tool";
      name: string;
      summary: string;
      status: "awaiting" | "running" | "done" | "failed" | "denied";
      result?: string;
    };

export interface RunArgs {
  provider: ProviderName;
  model: string;
  apiKey?: string;
  ollamaHost?: string;
  tools: ToolSpec[];
  /** Live wizard state, appended to the system prompt. */
  stateSummary: string;
  /** Conversation so far — pass the value returned by the previous run. */
  turns: Turn[];
  userMessage: string;
  autoRun: boolean;
  /** Resolve true to let a destructive tool run. */
  confirm: (toolName: string, summary: string) => Promise<boolean>;
  emit: (item: ChatItem) => void;
  update: (id: number, patch: Partial<Extract<ChatItem, { kind: "tool" }>>) => void;
  /** Guard against a model that loops on tools forever. */
  maxRounds?: number;
}

let nextId = 1;
const id = () => nextId++;

export async function run(a: RunArgs): Promise<Turn[]> {
  const turns: Turn[] = [...a.turns, { role: "user", text: a.userMessage }];
  a.emit({ id: id(), kind: "user", text: a.userMessage });

  const specs = new Map(a.tools.map((t) => [t.def.name, t]));
  const system = `${SYSTEM_PROMPT}\n\n--- Current state of their setup ---\n${a.stateSummary}`;
  const maxRounds = a.maxRounds ?? 12;

  for (let round = 0; round < maxRounds; round++) {
    let res;
    try {
      res = await complete({
        provider: a.provider,
        model: a.model,
        apiKey: a.apiKey,
        ollamaHost: a.ollamaHost,
        system,
        turns,
        tools: a.tools.map((t) => t.def),
      });
    } catch (e) {
      a.emit({ id: id(), kind: "error", text: friendlyError(e, a.provider) });
      return turns;
    }

    if (res.text.trim()) a.emit({ id: id(), kind: "assistant", text: res.text });

    if (!res.toolCalls.length) {
      turns.push({ role: "assistant", text: res.text });
      return turns;
    }

    turns.push({ role: "assistant", text: res.text, toolCalls: res.toolCalls });

    const results: Array<{ id: string; content: string }> = [];
    for (const call of res.toolCalls) {
      results.push({ id: call.id, content: await execute(call, specs, a) });
    }
    turns.push({ role: "user", toolResults: results });
  }

  a.emit({
    id: id(),
    kind: "error",
    text: "The assistant took too many steps without finishing. Try asking for one thing at a time.",
  });
  return turns;
}

async function execute(
  call: ToolCall,
  specs: Map<string, ToolSpec>,
  a: RunArgs,
): Promise<string> {
  const spec = specs.get(call.name);
  if (!spec) return JSON.stringify({ error: `No such tool: ${call.name}` });

  const summary = spec.summarize(call.args);
  const itemId = id();
  a.emit({
    id: itemId,
    kind: "tool",
    name: call.name,
    summary,
    status: spec.destructive && !a.autoRun ? "awaiting" : "running",
  });

  if (spec.destructive && !a.autoRun) {
    const allowed = await a.confirm(call.name, summary);
    if (!allowed) {
      a.update(itemId, { status: "denied" });
      return JSON.stringify({
        error: "The user declined this action. Do not retry it — ask them what they'd prefer instead.",
      });
    }
    a.update(itemId, { status: "running" });
  }

  try {
    const out = await spec.run(call.args);
    const text = typeof out === "string" ? out : JSON.stringify(out);
    a.update(itemId, { status: "done", result: text });
    return text.slice(0, 20_000);
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    a.update(itemId, { status: "failed", result: msg });
    return JSON.stringify({ error: msg });
  }
}

function friendlyError(e: unknown, provider: ProviderName): string {
  const msg = e instanceof Error ? e.message : String(e);
  if (/\b401\b|invalid.*api.?key|unauthorized/i.test(msg)) {
    return "That API key was rejected. Check you pasted the whole key, and that it's for the provider you picked.";
  }
  if (/\b429\b|rate.?limit/i.test(msg)) {
    return "The provider is rate-limiting you. Wait a moment and try again.";
  }
  if (/\b(402|403)\b|credit|quota|billing/i.test(msg)) {
    return "Your account with that provider is out of credit or the key lacks access. Check your billing there.";
  }
  if (provider === "ollama" && /fetch|ECONNREFUSED|Failed to fetch/i.test(msg)) {
    return "Couldn't reach Ollama. Make sure it's running (open a terminal and run: ollama serve).";
  }
  return `The assistant couldn't reach ${provider}: ${msg}`;
}
