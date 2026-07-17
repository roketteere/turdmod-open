/**
 * Network sinks — `network.broadcast(channel, payload)` calls inside a mod
 * route through the host's sink registry. The companion ships three:
 *
 *  - `console`: pretty-prints events for local development (default on)
 *  - `discord-webhook`: posts an embed to a Discord webhook URL
 *  - `file`: appends NDJSON to a logfile for later processing
 *
 * More sinks (scummap web push, websocket fan-out) plug in here without
 * touching the mods.
 */

import { appendFileSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";

export interface SinkContext {
  modId: string;
  channel: string;
  ts: number;
}

export type SinkPayload = unknown;

export interface Sink {
  name: string;
  publish(ctx: SinkContext, payload: SinkPayload): Promise<void> | void;
}

export class ConsoleSink implements Sink {
  name = "console";
  publish(ctx: SinkContext, payload: SinkPayload): void {
    const ts = new Date(ctx.ts).toISOString();
    const tag = `[${ctx.modId}] ${ctx.channel}`;
    if (typeof payload === "string") {
      console.log(`${ts} ${tag} ${payload}`);
    } else {
      console.log(`${ts} ${tag} ${JSON.stringify(payload)}`);
    }
  }
}

export class FileSink implements Sink {
  name = "file";
  constructor(private path: string) {
    mkdirSync(dirname(path), { recursive: true });
  }
  publish(ctx: SinkContext, payload: SinkPayload): void {
    const line = JSON.stringify({ ts: ctx.ts, modId: ctx.modId, channel: ctx.channel, payload });
    appendFileSync(this.path, line + "\n", "utf-8");
  }
}

export class DiscordWebhookSink implements Sink {
  name = "discord-webhook";
  constructor(private url: string) {}
  async publish(ctx: SinkContext, payload: SinkPayload): Promise<void> {
    // Convention: payload may be either a string (treated as content) or an
    // object with `embed` / `content`. Anything else is JSON-stringified
    // into a code block as a safe default.
    let body: Record<string, unknown>;
    if (typeof payload === "string") {
      body = { content: payload };
    } else if (payload && typeof payload === "object" && ("embed" in payload || "content" in payload)) {
      const p = payload as { embed?: unknown; content?: string };
      body = {
        ...(p.content ? { content: p.content } : {}),
        ...(p.embed ? { embeds: [p.embed] } : {}),
      };
    } else {
      body = { content: "```json\n" + JSON.stringify(payload, null, 2).slice(0, 1800) + "\n```" };
    }
    body.username = body.username || `TurdMOD/${ctx.modId}`;
    try {
      await fetch(this.url, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      });
    } catch (e) {
      console.warn(`discord-webhook publish failed for ${ctx.modId}: ${(e as Error).message}`);
    }
  }
}

/** Fan-out: every sink receives every event. */
export class SinkRouter {
  private sinks: Sink[] = [];
  add(sink: Sink): void { this.sinks.push(sink); }
  async publish(ctx: SinkContext, payload: SinkPayload): Promise<void> {
    await Promise.all(this.sinks.map(s => Promise.resolve(s.publish(ctx, payload))));
  }
}
