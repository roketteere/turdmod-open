import { existsSync } from "node:fs";
import { join } from "node:path";
import { loadConfig } from "../lib/config.js";
import { info, ok, warn, err, jsonMode } from "../lib/logger.js";

export async function cmdDoctor(_args: string[]): Promise<void> {
  const cfg = loadConfig();
  const checks: Array<{ name: string; ok: boolean; detail: string }> = [];

  // 1. UE 4.27 install
  const ueRoot = cfg.uePath ?? process.env.UE_4_27_PATH;
  if (ueRoot && existsSync(join(ueRoot, "Engine", "Binaries", "Win64", "UnrealPak.exe"))) {
    checks.push({ name: "UE 4.27 + UnrealPak", ok: true, detail: ueRoot });
  } else {
    checks.push({
      name: "UE 4.27 + UnrealPak",
      ok: false,
      detail: ueRoot
        ? `UnrealPak.exe missing under ${ueRoot}/Engine/Binaries/Win64/`
        : `set cfg.uePath or env UE_4_27_PATH`,
    });
  }

  // 2. Node version
  const nodeOk = parseInt(process.versions.node.split(".")[0]!, 10) >= 18;
  checks.push({ name: "Node >=18", ok: nodeOk, detail: `running ${process.version}` });

  // 3. AI provider config
  const provider = cfg.aiProvider ?? "(unset)";
  const model    = cfg.aiModel ?? "(unset)";
  const keyVar   = cfg.keyEnvVar ?? "TURDMOD_AI_KEY";
  const hasKey   = !!process.env[keyVar];
  checks.push({
    name: "AI provider (BYO key)",
    ok: provider !== "(unset)" && (provider === "ollama" || hasKey),
    detail: `provider=${provider} model=${model} keyEnv=${keyVar} keyPresent=${hasKey}`,
  });

  // 4. Network reachability — we won't probe to avoid surprising the user.

  // 5. Bridge running?
  const pipeFile = process.env.LOCALAPPDATA
    ? join(process.env.LOCALAPPDATA, "TurdMOD", "engine", "pipe.txt")
    : null;
  checks.push({
    name: "TurdMOD bridge discovery file",
    ok: !!pipeFile && existsSync(pipeFile),
    detail: pipeFile ?? "LOCALAPPDATA unset",
  });

  if (jsonMode()) {
    console.log(JSON.stringify({ checks }, null, 2));
    return;
  }
  info("turdmod-creator doctor — setup health check\n");
  for (const c of checks) {
    if (c.ok) ok(`${c.name}: ${c.detail}`);
    else      err(`${c.name}: ${c.detail}`);
  }
  const failed = checks.filter(c => !c.ok).length;
  if (failed === 0) {
    info("\nAll checks passed.");
  } else {
    info(`\n${failed} check(s) failed. Fix them or proceed with limited functionality.`);
  }
}
