import { join } from "node:path";
import { homedir } from "node:os";
import { mkdirSync, existsSync, readFileSync, writeFileSync } from "node:fs";

export interface CreatorConfig {
  /** Path to UE 4.27 install root (contains Engine/Binaries/Win64/UnrealPak.exe). */
  uePath?: string;
  /** Default AI provider for `ai` subcommands. */
  aiProvider?: "openai" | "anthropic" | "deepseek" | "ollama" | "gemini";
  /** Default model id per provider. */
  aiModel?: string;
  /** Env var name holding the API key. We never read or store the key itself. */
  keyEnvVar?: string;
  /** Default ollama host if provider=ollama. */
  ollamaHost?: string;
  /** Default project directory if not specified per-command. */
  defaultProjectDir?: string;
}

export function configRoot(): string {
  const dir = join(homedir(), ".turdmod-creator");
  if (!existsSync(dir)) mkdirSync(dir, { recursive: true });
  return dir;
}

export function configPath(): string {
  return join(configRoot(), "config.json");
}

export function logsDir(): string {
  const d = join(configRoot(), "logs");
  if (!existsSync(d)) mkdirSync(d, { recursive: true });
  return d;
}

export function loadConfig(): CreatorConfig {
  const p = configPath();
  if (!existsSync(p)) return {};
  try {
    return JSON.parse(readFileSync(p, "utf8")) as CreatorConfig;
  } catch {
    return {};
  }
}

export function saveConfig(cfg: CreatorConfig): void {
  writeFileSync(configPath(), JSON.stringify(cfg, null, 2), "utf8");
}
