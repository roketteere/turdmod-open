/**
 * AI provider abstraction — BYO API key, BYO billing.
 *
 * We never store keys. We never proxy. The CLI reads from env var and
 * passes it straight to the provider's HTTPS endpoint. The user owns
 * the conversation.
 *
 * Supported providers:
 *   openai     — https://api.openai.com/v1
 *   anthropic  — https://api.anthropic.com/v1
 *   deepseek   — https://api.deepseek.com/v1
 *   ollama     — local, default http://127.0.0.1:11434 (no key needed)
 *   gemini     — https://generativelanguage.googleapis.com/v1beta
 *
 * All except Anthropic + Gemini share the OpenAI-compatible chat schema.
 */
import { loadConfig } from "../lib/config.js";

export type ProviderName = "openai" | "anthropic" | "deepseek" | "ollama" | "gemini";

export interface AiCallOptions {
  provider: ProviderName;
  model: string;
  systemPrompt: string;
  userPrompt: string;
  keyEnv?: string;        // env var holding the API key
  ollamaHost?: string;    // override for ollama (default http://127.0.0.1:11434)
  maxTokens?: number;
  temperature?: number;
}

export interface AiCallResult {
  text: string;
  promptTokens?: number;
  completionTokens?: number;
  estimatedUSD?: number;
}

export async function callAi(opts: AiCallOptions): Promise<AiCallResult> {
  switch (opts.provider) {
    case "openai":
      return callOpenAiCompat(opts, "https://api.openai.com/v1");
    case "deepseek":
      return callOpenAiCompat(opts, "https://api.deepseek.com/v1");
    case "ollama":
      return callOpenAiCompat({
        ...opts,
        keyEnv: undefined, // no key needed for local Ollama
      }, opts.ollamaHost ?? loadConfig().ollamaHost ?? "http://127.0.0.1:11434/v1");
    case "anthropic":
      return callAnthropic(opts);
    case "gemini":
      return callGemini(opts);
    default:
      throw new Error(`unsupported provider: ${opts.provider}`);
  }
}

function getKey(envVar?: string): string {
  const name = envVar || "TURDMOD_AI_KEY";
  const v = process.env[name];
  if (!v) {
    throw new Error(
      `API key env var "${name}" is not set. ` +
      `Set it before running: setx ${name} "<your-key>" (Windows) or export ${name}=<your-key>. ` +
      `Your key, your billing — we never see it.`
    );
  }
  return v;
}

async function callOpenAiCompat(opts: AiCallOptions, baseUrl: string): Promise<AiCallResult> {
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (opts.provider !== "ollama") {
    headers["Authorization"] = `Bearer ${getKey(opts.keyEnv)}`;
  }
  const body = {
    model: opts.model,
    messages: [
      { role: "system", content: opts.systemPrompt },
      { role: "user",   content: opts.userPrompt },
    ],
    max_tokens: opts.maxTokens ?? 4096,
    temperature: opts.temperature ?? 0.3,
  };
  const res = await fetch(`${baseUrl.replace(/\/+$/, "")}/chat/completions`, {
    method: "POST",
    headers,
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const t = await res.text().catch(() => "<unreadable>");
    throw new Error(`${opts.provider} ${res.status}: ${t.slice(0, 400)}`);
  }
  const j = await res.json() as {
    choices: Array<{ message: { content: string } }>;
    usage?: { prompt_tokens?: number; completion_tokens?: number };
  };
  const text = j.choices?.[0]?.message?.content ?? "";
  return {
    text,
    promptTokens: j.usage?.prompt_tokens,
    completionTokens: j.usage?.completion_tokens,
    estimatedUSD: estimateCost(opts.provider, opts.model, j.usage?.prompt_tokens, j.usage?.completion_tokens),
  };
}

async function callAnthropic(opts: AiCallOptions): Promise<AiCallResult> {
  const key = getKey(opts.keyEnv ?? "ANTHROPIC_API_KEY");
  const body = {
    model: opts.model,
    max_tokens: opts.maxTokens ?? 4096,
    temperature: opts.temperature ?? 0.3,
    system: opts.systemPrompt,
    messages: [{ role: "user", content: opts.userPrompt }],
  };
  const res = await fetch("https://api.anthropic.com/v1/messages", {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "x-api-key": key,
      "anthropic-version": "2023-06-01",
    },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const t = await res.text().catch(() => "<unreadable>");
    throw new Error(`anthropic ${res.status}: ${t.slice(0, 400)}`);
  }
  const j = await res.json() as {
    content: Array<{ type: string; text?: string }>;
    usage?: { input_tokens?: number; output_tokens?: number };
  };
  const text = (j.content ?? []).filter(b => b.type === "text").map(b => b.text ?? "").join("");
  return {
    text,
    promptTokens: j.usage?.input_tokens,
    completionTokens: j.usage?.output_tokens,
    estimatedUSD: estimateCost("anthropic", opts.model, j.usage?.input_tokens, j.usage?.output_tokens),
  };
}

async function callGemini(opts: AiCallOptions): Promise<AiCallResult> {
  const key = getKey(opts.keyEnv ?? "GEMINI_API_KEY");
  const url = `https://generativelanguage.googleapis.com/v1beta/models/${opts.model}:generateContent?key=${key}`;
  const body = {
    contents: [{ role: "user", parts: [{ text: opts.systemPrompt + "\n\n" + opts.userPrompt }] }],
    generationConfig: {
      maxOutputTokens: opts.maxTokens ?? 4096,
      temperature: opts.temperature ?? 0.3,
    },
  };
  const res = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const t = await res.text().catch(() => "<unreadable>");
    throw new Error(`gemini ${res.status}: ${t.slice(0, 400)}`);
  }
  const j = await res.json() as {
    candidates: Array<{ content: { parts: Array<{ text: string }> } }>;
    usageMetadata?: { promptTokenCount?: number; candidatesTokenCount?: number };
  };
  const text = j.candidates?.[0]?.content?.parts?.map(p => p.text).join("") ?? "";
  return {
    text,
    promptTokens: j.usageMetadata?.promptTokenCount,
    completionTokens: j.usageMetadata?.candidatesTokenCount,
    estimatedUSD: estimateCost("gemini", opts.model, j.usageMetadata?.promptTokenCount, j.usageMetadata?.candidatesTokenCount),
  };
}

// Rough cost estimates per 1M tokens (subject to change; check provider pricing).
// We err on the high side so the printed estimate is a soft cap.
const COST_TABLE: Record<string, { in: number; out: number }> = {
  // openai
  "gpt-4.1":          { in: 2.00,  out: 8.00 },
  "gpt-4.1-mini":     { in: 0.40,  out: 1.60 },
  "gpt-5":            { in: 1.25,  out: 10.00 },
  // anthropic
  "claude-opus-4-7":  { in: 15.00, out: 75.00 },
  "claude-sonnet-4-6":{ in: 3.00,  out: 15.00 },
  "claude-haiku-4-5-20251001": { in: 1.00, out: 5.00 },
  // deepseek
  "deepseek-chat":    { in: 0.27,  out: 1.10 },
  "deepseek-reasoner":{ in: 0.55,  out: 2.19 },
  // gemini (rough)
  "gemini-1.5-pro":   { in: 1.25,  out: 5.00 },
  "gemini-1.5-flash": { in: 0.075, out: 0.30 },
};

function estimateCost(_provider: ProviderName, model: string, pTok?: number, cTok?: number): number | undefined {
  const c = COST_TABLE[model];
  if (!c || pTok === undefined || cTok === undefined) return undefined;
  return (pTok / 1_000_000) * c.in + (cTok / 1_000_000) * c.out;
}
