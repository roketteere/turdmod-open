/**
 * IPC server — exposes parsed ServerEvents to subscribers over HTTP.
 *
 * Two endpoints:
 *
 *   GET /events   — Server-Sent Events stream. Subscribers (the loader,
 *                   the guard daemon) hold the connection open and
 *                   receive `data: <json>\n\n` for every event the
 *                   companion fans out.
 *   GET /health   — basic liveness check.
 *
 * Auto-picks a free port unless `TURDMOD_COMPANION_PORT` is set, then
 * writes a discovery file at `~/.scummy-map/turdmod-companion.json` so
 * peer processes find us without a hardcoded port. Mirrors the
 * convention used by `apps/turdmod-guard/backend/`.
 */

import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import { mkdirSync, writeFileSync, unlinkSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

import type { ServerEvent } from "./parsers.js";
import type { CompanionRuntime, ModBinding } from "./runtime.js";

const DISCOVERY_DIR = join(homedir(), ".scummy-map");
const DISCOVERY_FILE = join(DISCOVERY_DIR, "turdmod-companion.json");

interface Subscriber {
  id: number;
  res: ServerResponse;
}

export interface ModManager {
  runtime: CompanionRuntime;
  loadMod(modId: string): Promise<void>;
  unloadMod(modId: string): Promise<void>;
  reloadMod(modId: string): Promise<void>;
  listMods(): { loaded: string[]; available: string[] };
}

export class IpcServer {
  private subscribers = new Map<number, Subscriber>();
  private nextId = 1;
  private port: number | null = null;
  modManager: ModManager | null = null;

  constructor(private requestedPort = Number(process.env.TURDMOD_COMPANION_PORT || 0)) {}

  async start(): Promise<{ port: number; url: string }> {
    const server = createServer((req, res) => this.handle(req, res));
    await new Promise<void>((resolve, reject) => {
      server.once("error", reject);
      server.listen(this.requestedPort || 0, "127.0.0.1", () => resolve());
    });
    const addr = server.address();
    if (!addr || typeof addr !== "object") throw new Error("ipc server: no address after listen");
    this.port = addr.port;
    const url = `http://127.0.0.1:${this.port}`;
    this.writeDiscovery(url);

    const cleanup = () => {
      try { unlinkSync(DISCOVERY_FILE); } catch { /* best-effort */ }
      server.close();
    };
    process.on("SIGINT", cleanup);
    process.on("SIGTERM", cleanup);
    process.on("exit", cleanup);

    return { port: this.port, url };
  }

  private writeDiscovery(url: string): void {
    try {
      mkdirSync(DISCOVERY_DIR, { recursive: true });
      writeFileSync(DISCOVERY_FILE, JSON.stringify({
        app: "turdmod-companion",
        port: this.port,
        url,
        pid: process.pid,
        startedAt: new Date().toISOString(),
      }, null, 2));
    } catch (e) {
      console.warn(`[ipc] could not write discovery file ${DISCOVERY_FILE}: ${(e as Error).message}`);
    }
  }

  private handle(req: IncomingMessage, res: ServerResponse): void {
    if (req.url === "/health") {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ ok: true, subscribers: this.subscribers.size }));
      return;
    }
    if (req.url === "/mods" && req.method === "GET") {
      const mods = this.modManager?.listMods() ?? { loaded: [], available: [] };
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify(mods));
      return;
    }
    if (req.url?.startsWith("/mods/") && req.method === "POST") {
      this.handleModAction(req, res);
      return;
    }
    if (req.url === "/events") {
      res.writeHead(200, {
        "content-type": "text/event-stream",
        "cache-control": "no-cache",
        "connection": "keep-alive",
        "x-accel-buffering": "no",
      });
      res.write(": welcome\n\n"); // SSE comment line; opens the stream
      const id = this.nextId++;
      this.subscribers.set(id, { id, res });
      console.log(`[ipc] subscriber ${id} connected (${this.subscribers.size} total)`);
      req.on("close", () => {
        this.subscribers.delete(id);
        console.log(`[ipc] subscriber ${id} disconnected (${this.subscribers.size} total)`);
      });
      // Heartbeat every 25s so middleboxes (and Rust ureq's keepalive) don't time us out.
      const hb = setInterval(() => {
        try { res.write(": hb\n\n"); } catch { /* noop */ }
      }, 25_000);
      req.on("close", () => clearInterval(hb));
      return;
    }
    res.writeHead(404, { "content-type": "text/plain" });
    res.end("not found");
  }

  /**
   * Fan an event to every connected subscriber. Subscribers that error on
   * write are dropped silently — they'll reconnect on their own.
   */
  broadcast(event: ServerEvent): void {
    if (this.subscribers.size === 0) return;
    const line = `data: ${JSON.stringify(event)}\n\n`;
    for (const [id, sub] of this.subscribers) {
      try {
        sub.res.write(line);
      } catch {
        this.subscribers.delete(id);
      }
    }
  }

  subscriberCount(): number {
    return this.subscribers.size;
  }

  private handleModAction(req: IncomingMessage, res: ServerResponse): void {
    if (!this.modManager) {
      res.writeHead(503, { "content-type": "application/json" });
      res.end(JSON.stringify({ error: "mod manager not initialized" }));
      return;
    }
    let body = "";
    req.on("data", (chunk: Buffer) => { body += chunk.toString(); });
    req.on("end", async () => {
      const url = req.url!;
      const action = url.split("/mods/")[1]?.split("?")[0];
      let modId: string;
      try {
        const parsed = body ? JSON.parse(body) : {};
        modId = parsed.modId || url.split("modId=")[1] || "";
      } catch {
        modId = "";
      }
      if (!modId && action !== "list") {
        res.writeHead(400, { "content-type": "application/json" });
        res.end(JSON.stringify({ error: "modId required" }));
        return;
      }
      try {
        switch (action) {
          case "load":
            await this.modManager!.loadMod(modId);
            res.writeHead(200, { "content-type": "application/json" });
            res.end(JSON.stringify({ ok: true, action: "loaded", modId }));
            break;
          case "unload":
            await this.modManager!.unloadMod(modId);
            res.writeHead(200, { "content-type": "application/json" });
            res.end(JSON.stringify({ ok: true, action: "unloaded", modId }));
            break;
          case "reload":
            await this.modManager!.reloadMod(modId);
            res.writeHead(200, { "content-type": "application/json" });
            res.end(JSON.stringify({ ok: true, action: "reloaded", modId }));
            break;
          default:
            res.writeHead(404, { "content-type": "application/json" });
            res.end(JSON.stringify({ error: `unknown action: ${action}` }));
        }
      } catch (e) {
        res.writeHead(500, { "content-type": "application/json" });
        res.end(JSON.stringify({ error: (e as Error).message }));
      }
    });
  }
}
