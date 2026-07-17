import { info, warn, logEvent } from "../lib/logger.js";

export async function cmdPublish(_args: string[]): Promise<void> {
  warn(`publish is staged for v2 — turdmod-marketplace registration first.`);
  info(`When the marketplace ships:`);
  info(`  tmc publish              # publish all widgets`);
  info(`  tmc publish <widget>     # one widget`);
  info(`  tmc publish --visibility free|premium-included|premium-exclusive`);
  info(`Author signing key generation: tmc key generate (also v2).`);
  logEvent({ kind: "publish.stub" });
}
