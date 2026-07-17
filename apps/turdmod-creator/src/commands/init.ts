import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { ask, validators, confirm } from "../lib/prompt.js";
import { advancedMode, info, ok, warn, logEvent, flag } from "../lib/logger.js";

const PROJECT_README = (name: string) => `# ${name}

Created with turdmod-creator. To work on this project:

\`\`\`
cd ${name}
tmc widget add <template>      # add a widget (noob mode)
tmc cook                       # build paks
tmc publish                    # ship to marketplace
\`\`\`

For advanced options: \`tmc <cmd> --advanced\`.
`;

const PROJECT_MANIFEST = (name: string, author: string) => ({
  name,
  version: "0.1.0",
  author,
  type: "turdmod-creator-project",
  createdAt: new Date().toISOString(),
  ueVersion: "4.27.2",
  widgets: [],
  // Advanced — UE project link (set when user chooses to integrate with full Editor)
  ueProjectPath: null as string | null,
});

const GITIGNORE = `
# turdmod-creator generated
node_modules/
dist/
.tmc/
*.pak
Saved/
Cooked/
`;

export async function cmdInit(args: string[]): Promise<void> {
  const projectName = args[0] || await ask({
    message: "Project name",
    validator: validators().identifier,
  });
  const dir = resolve(process.cwd(), projectName);
  if (existsSync(dir)) {
    warn(`directory exists: ${dir}`);
    const overwrite = await confirm("overwrite?", false);
    if (!overwrite) {
      info("init cancelled.");
      return;
    }
  }
  mkdirSync(dir, { recursive: true });
  mkdirSync(join(dir, ".tmc"),    { recursive: true });
  mkdirSync(join(dir, "widgets"), { recursive: true });

  const author = flag(args, "author") ?? await ask({
    message: "Author display name (shown on marketplace)",
    default: "Anonymous",
  });

  // Advanced toggle — link to a UE project directory?
  let ueProjectPath: string | null = null;
  if (advancedMode()) {
    const linkUe = await confirm("Link to a UE 4.27 project for full Editor integration?", false);
    if (linkUe) {
      ueProjectPath = await ask({
        message: "UE project root (containing .uproject)",
        validator: (s) => existsSync(s) ? null : "path does not exist",
      });
    }
  }

  const manifest = PROJECT_MANIFEST(projectName, author);
  if (ueProjectPath) manifest.ueProjectPath = ueProjectPath;
  writeFileSync(join(dir, "tmc.json"),   JSON.stringify(manifest, null, 2), "utf8");
  writeFileSync(join(dir, "README.md"),  PROJECT_README(projectName), "utf8");
  writeFileSync(join(dir, ".gitignore"), GITIGNORE, "utf8");

  ok(`project created: ${dir}`);
  info(`  next: cd ${projectName} && tmc widget add notification`);
  logEvent({ kind: "init", project: projectName, dir, advanced: advancedMode() });
}
