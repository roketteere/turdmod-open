// Multi-provider AI client with tool-use support.
//
// BYO key, BYO billing — the key goes straight from the OS keychain to the
// provider's endpoint. We never proxy it, never store it anywhere else.
// Ollama needs no key at all (free, local) — that's the zero-cost path.
//
// Extends turdmod-creator's single-shot providers.ts with conversation +
// tool calling, which the assistant needs to actually perform an install.

export type ProviderName = "anthropic" | "openai" | "deepseek" | "gemini" | "ollama";

export interface ProviderInfo {
  id: ProviderName;
  label: string;
  defaultModel: string;
  needsKey: boolean;
  keyUrl?: string;
  note: string;
}

export const PROVIDERS: ProviderInfo[] = [
  {
    id: "anthropic",
    label: "Claude (Anthropic)",
    defaultModel: "claude-sonnet-4-20250514",
    needsKey: true,
    keyUrl: "https://console.anthropic.com/settings/keys",
    note: "Best at multi-step installs. A full setup costs a few cents.",
  },
  {
    id: "openai",
    label: "ChatGPT (OpenAI)",
    defaultModel: "gpt-4o",
    needsKey: true,
    keyUrl: "https://platform.openai.com/api-keys",
    note: "Works well. A full setup costs a few cents.",
  },
  {
    id: "deepseek",
    label: "DeepSeek",
    defaultModel: "deepseek-chat",
    needsKey: true,
    keyUrl: "https://platform.deepseek.com/api_keys",
    note: "Cheapest paid option.",
  },
  {
    id: "gemini",
    label: "Gemini (Google)",
    defaultModel: "gemini-2.0-flash",
    needsKey: true,
    keyUrl: "https://aistudio.google.com/apikey",
    note: "Generous free tier.",
  },
  {
    id: "ollama",
    label: "Ollama (local, free)",
    defaultModel: "llama3.1",
    needsKey: false,
    note: "Runs on your PC. Completely free, no account. Needs Ollama installed.",
  },
];

export interface ToolDef {
  name: string;
  description: string;
  parameters: Record<string, unknown>;
}

export interface ToolCall {
  id: string;
  name: string;
  args: Record<string, unknown>;
}

export interface Turn {
  role: "user" | "assistant";
  /** Plain text content. */
  text?: string;
  /** Tool calls the model wants to make (assistant turns). */
  toolCalls?: ToolCall[];
  /** Results being returned to the model (user turns). */
  toolResults?: Array<{ id: string; content: string }>;
}

export interface CompletionResult {
  text: string;
  toolCalls: ToolCall[];
}

export interface CallOptions {
  provider: ProviderName;
  model: string;
  apiKey?: string;
  ollamaHost?: string;
  system: string;
  turns: Turn[];
  tools: ToolDef[];
}

// ─── Anthropic ─────────────────────────────────────────────────────────────

async function callAnthropic(o: CallOptions): Promise<CompletionResult> {
  const messages = o.turns.map((t) => {
    if (t.role === "assistant") {
      const content: unknown[] = [];
      if (t.text) content.push({ type: "text", text: t.text });
      for (const c of t.toolCalls ?? []) {
        content.push({ type: "tool_use", id: c.id, name: c.name, input: c.args });
      }
      return { role: "assistant", content };
    }
    if (t.toolResults?.length) {
      return {
        role: "user",
        content: t.toolResults.map((r) => ({
          type: "tool_result",
          tool_use_id: r.id,
          content: r.content,
        })),
      };
    }
    return { role: "user", content: t.text ?? "" };
  });

  const res = await fetch("https://api.anthropic.com/v1/messages", {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "x-api-key": o.apiKey ?? "",
      "anthropic-version": "2023-06-01",
      "anthropic-dangerous-direct-browser-access": "true",
    },
    body: JSON.stringify({
      model: o.model,
      max_tokens: 4096,
      system: o.system,
      messages,
      tools: o.tools.map((t) => ({
        name: t.name,
        description: t.description,
        input_schema: t.parameters,
      })),
    }),
  });

  if (!res.ok) throw new Error(`Anthropic ${res.status}: ${(await res.text()).slice(0, 300)}`);
  const j = await res.json();

  const text = (j.content ?? [])
    .filter((b: { type: string }) => b.type === "text")
    .map((b: { text: string }) => b.text)
    .join("");
  const toolCalls: ToolCall[] = (j.content ?? [])
    .filter((b: { type: string }) => b.type === "tool_use")
    .map((b: { id: string; name: string; input: Record<string, unknown> }) => ({
      id: b.id,
      name: b.name,
      args: b.input ?? {},
    }));

  return { text, toolCalls };
}

// ─── OpenAI-compatible (OpenAI, DeepSeek, Ollama) ──────────────────────────

async function callOpenAiCompat(o: CallOptions, baseUrl: string): Promise<CompletionResult> {
  const messages: unknown[] = [{ role: "system", content: o.system }];

  for (const t of o.turns) {
    if (t.role === "assistant") {
      messages.push({
        role: "assistant",
        content: t.text ?? null,
        ...(t.toolCalls?.length
          ? {
              tool_calls: t.toolCalls.map((c) => ({
                id: c.id,
                type: "function",
                function: { name: c.name, arguments: JSON.stringify(c.args) },
              })),
            }
          : {}),
      });
    } else if (t.toolResults?.length) {
      for (const r of t.toolResults) {
        messages.push({ role: "tool", tool_call_id: r.id, content: r.content });
      }
    } else {
      messages.push({ role: "user", content: t.text ?? "" });
    }
  }

  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (o.provider !== "ollama") headers["Authorization"] = `Bearer ${o.apiKey ?? ""}`;

  const res = await fetch(`${baseUrl.replace(/\/+$/, "")}/chat/completions`, {
    method: "POST",
    headers,
    body: JSON.stringify({
      model: o.model,
      messages,
      max_tokens: 4096,
      tools: o.tools.map((t) => ({
        type: "function",
        function: { name: t.name, description: t.description, parameters: t.parameters },
      })),
    }),
  });

  if (!res.ok) throw new Error(`${o.provider} ${res.status}: ${(await res.text()).slice(0, 300)}`);
  const j = await res.json();
  const msg = j.choices?.[0]?.message ?? {};

  const toolCalls: ToolCall[] = (msg.tool_calls ?? []).map(
    (c: { id: string; function: { name: string; arguments: string } }) => {
      let args: Record<string, unknown> = {};
      try {
        args = JSON.parse(c.function.arguments || "{}");
      } catch {
        /* model emitted bad JSON — treat as no args */
      }
      return { id: c.id, name: c.function.name, args };
    },
  );

  return { text: msg.content ?? "", toolCalls };
}

// ─── Gemini ────────────────────────────────────────────────────────────────

async function callGemini(o: CallOptions): Promise<CompletionResult> {
  const contents = o.turns.map((t) => {
    if (t.toolResults?.length) {
      return {
        role: "user",
        parts: t.toolResults.map((r) => ({
          functionResponse: { name: r.id, response: { result: r.content } },
        })),
      };
    }
    if (t.role === "assistant" && t.toolCalls?.length) {
      return {
        role: "model",
        parts: t.toolCalls.map((c) => ({ functionCall: { name: c.name, args: c.args } })),
      };
    }
    return { role: t.role === "assistant" ? "model" : "user", parts: [{ text: t.text ?? "" }] };
  });

  const url = `https://generativelanguage.googleapis.com/v1beta/models/${o.model}:generateContent?key=${o.apiKey ?? ""}`;
  const res = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      systemInstruction: { parts: [{ text: o.system }] },
      contents,
      tools: [
        {
          functionDeclarations: o.tools.map((t) => ({
            name: t.name,
            description: t.description,
            parameters: t.parameters,
          })),
        },
      ],
    }),
  });

  if (!res.ok) throw new Error(`Gemini ${res.status}: ${(await res.text()).slice(0, 300)}`);
  const j = await res.json();
  const parts = j.candidates?.[0]?.content?.parts ?? [];

  const text = parts
    .filter((p: { text?: string }) => p.text)
    .map((p: { text: string }) => p.text)
    .join("");
  const toolCalls: ToolCall[] = parts
    .filter((p: { functionCall?: unknown }) => p.functionCall)
    .map((p: { functionCall: { name: string; args: Record<string, unknown> } }, i: number) => ({
      id: `${p.functionCall.name}-${i}`,
      name: p.functionCall.name,
      args: p.functionCall.args ?? {},
    }));

  return { text, toolCalls };
}

// ─── Dispatch ──────────────────────────────────────────────────────────────

export async function complete(o: CallOptions): Promise<CompletionResult> {
  switch (o.provider) {
    case "anthropic":
      return callAnthropic(o);
    case "openai":
      return callOpenAiCompat(o, "https://api.openai.com/v1");
    case "deepseek":
      return callOpenAiCompat(o, "https://api.deepseek.com/v1");
    case "ollama":
      return callOpenAiCompat(o, `${o.ollamaHost ?? "http://127.0.0.1:11434"}/v1`);
    case "gemini":
      return callGemini(o);
  }
}
