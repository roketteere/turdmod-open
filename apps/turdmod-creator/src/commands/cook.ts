import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { loadConfig } from "../lib/config.js";
import { advancedMode, flag, info, ok, warn, err, logEvent } from "../lib/logger.js";

export async function cmdCook(args: string[]): Promise<void> {
  const projDir = flag(args, "project") ?? process.cwd();
  const projManifestPath = join(resolve(projDir), "tmc.json");
  if (!existsSync(projManifestPath)) {
    throw new Error(`no tmc.json found. Run \`tmc init\` first.`);
  }
  const m = JSON.parse(readFileSync(projManifestPath, "utf8")) as {
    name: string;
    widgets: Array<{ name: string; template: string }>;
    ueProjectPath?: string | null;
  };

  if (m.widgets.length === 0) {
    warn("no widgets to cook. Add one first: tmc widget add <template>");
    return;
  }

  // Cooking real .uasset → .pak requires UE 4.27 + UnrealPak.exe. We resolve
  // it from config or env. In noob mode, this just prints the equivalent
  // command — real cook delegates to UE Editor's File → Cook Content menu
  // OR to UnrealPak.exe directly.
  const cfg = loadConfig();
  const uePath = flag(args, "ue") ?? cfg.uePath ?? process.env.UE_4_27_PATH;
  if (!uePath) {
    err(`UE 4.27 path not configured. Run \`tmc doctor\` then \`tmc config set uePath <path>\`.`);
    err(`Or pass --ue <path> to cook directly.`);
    info(`v1 stub: this is where UnrealPak.exe would be invoked.`);
    logEvent({ kind: "cook.stub", widgets: m.widgets.length, reason: "ue path missing" });
    return;
  }

  const unrealPak = join(uePath, "Engine", "Binaries", "Win64", "UnrealPak.exe");
  if (!existsSync(unrealPak)) {
    err(`UnrealPak.exe not found at ${unrealPak}`);
    return;
  }

  info(`Cooking ${m.widgets.length} widget(s) into pak(s)...`);
  for (const w of m.widgets) {
    info(`  ${w.name} (${w.template})`);
  }

  if (advancedMode()) {
    info(`\nAdvanced: full UnrealPak invocation`);
    info(`  ${unrealPak} <output>.pak -Create=<filelist>.txt -encrypt=<key> ...`);
    info(`  See: https://docs.unrealengine.com/4.27/en-US/SharingAndReleasing/Packaging/Pak/`);
  }

  // For v1, we ship the SCAFFOLD. The actual .uasset content lives in
  // a sibling UE project the user opens in UE Editor. When that's set,
  // `tmc cook` orchestrates the UnrealPak invocation.
  warn(`v1 cooks via UE Editor. Open ${m.ueProjectPath ?? "<no UE project linked>"} in UE 4.27, File → Package Project → Windows Server.`);
  warn(`The output .pak files will land in ${m.ueProjectPath ?? "<UE project>"}/Saved/Cooked/.`);
  warn(`Future versions will fully automate this — for now the recipe is here:`);
  info(`  docs/guides/ue4-content-pak-recipe.md`);

  ok(`cook plan written. See recipe doc to complete.`);
  logEvent({ kind: "cook.planned", widgets: m.widgets.length });
}
