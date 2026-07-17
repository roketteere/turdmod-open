/**
 * Minimal interactive prompt — readline-based, no external deps.
 * Keeps the noob UX simple: ask, validate, default.
 */
import { createInterface } from "node:readline/promises";
import { stdin, stdout } from "node:process";

export type ValidatorResult = string | null; // null = ok, string = error msg

export interface PromptOptions {
  message: string;
  default?: string;
  /** Return null if valid, else error message. */
  validator?: (s: string) => ValidatorResult;
  /** If true, suppress echo (passwords). Currently best-effort only. */
  secret?: boolean;
}

const NON_INTERACTIVE = !stdin.isTTY || process.argv.includes("--json");

export async function ask(opts: PromptOptions): Promise<string> {
  if (NON_INTERACTIVE) {
    if (opts.default !== undefined) return opts.default;
    throw new Error(`prompt "${opts.message}" requires interactive TTY (no default set)`);
  }
  const rl = createInterface({ input: stdin, output: stdout });
  try {
    while (true) {
      const def = opts.default !== undefined ? ` [${opts.default}]` : "";
      const ans = (await rl.question(`${opts.message}${def}: `)).trim();
      const val = ans.length > 0 ? ans : (opts.default ?? "");
      if (opts.validator) {
        const err = opts.validator(val);
        if (err !== null) {
          console.error(`  ${err}`);
          continue;
        }
      }
      return val;
    }
  } finally {
    rl.close();
  }
}

export async function confirm(message: string, defaultYes = true): Promise<boolean> {
  if (NON_INTERACTIVE) return defaultYes;
  const def = defaultYes ? "Y/n" : "y/N";
  const ans = (await ask({ message: `${message} [${def}]`, default: defaultYes ? "y" : "n" })).toLowerCase();
  return ans === "y" || ans === "yes";
}

export function validators() {
  return {
    notEmpty: (s: string) => s.length > 0 ? null : "value required",
    identifier: (s: string) => /^[a-zA-Z][a-zA-Z0-9_-]*$/.test(s) ? null : "letters, digits, _ or - only; must start with letter",
    hexColor: (s: string) => /^#?[0-9a-fA-F]{6}$/.test(s) ? null : "expected #RRGGBB hex color",
    intRange: (min: number, max: number) => (s: string) => {
      const n = parseInt(s, 10);
      return Number.isFinite(n) && n >= min && n <= max ? null : `expected integer ${min}..${max}`;
    },
  };
}
