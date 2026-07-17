/**
 * Engine JSON-RPC client over a Windows named pipe.
 *
 * Discovers the pipe name from %LOCALAPPDATA%\TurdMOD\engine\pipe.txt
 * (written by the loader DLL on startup), connects, and provides:
 *   - call(method, params) -> Promise<result>  // typed RPC
 *   - ping()                                    // convenience
 *   - onEvent(handler)                          // engine -> companion events
 *   - disconnect()
 *
 * Wire protocol — matches apps/turdmod-server-loader/src/admin_api.rs:
 *   frame = [uint32 LE body-length] + [UTF-8 JSON body]
 *   request:  { id: string, method: string, params?: unknown }
 *   response: { id: string, result?: unknown, error?: { code: number, message: string } }
 *   event:    { event: string, data: unknown }     // no id, fan-out from server
 *
 * The loader's pipe name format is `\\.\pipe\turdmod-engine-{PID}`; PID
 * changes every server restart, so we always re-read the discovery file.
 */

import { createConnection, type Socket } from "node:net";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { randomUUID } from "node:crypto";

export interface EngineRpcRequest {
  id: string;
  method: string;
  params?: unknown;
}

export interface EngineRpcResponse {
  id: string;
  result?: unknown;
  error?: { code: number; message: string };
}

export interface EngineEvent {
  event: string;
  data: unknown;
}

interface Pending {
  resolve: (resp: EngineRpcResponse) => void;
  reject: (err: Error) => void;
  timeoutHandle: NodeJS.Timeout;
}

const DISCOVERY_FILE_REL = ["TurdMOD", "engine", "pipe.txt"];
const REQUEST_TIMEOUT_MS = 30_000;
const MAX_FRAME_BYTES = 4 * 1024 * 1024;

export class EngineClient {
  private socket: Socket | null = null;
  private pipeName: string | null = null;
  private pending = new Map<string, Pending>();
  private eventListeners: Array<(evt: EngineEvent) => void> = [];
  private readBuffer = Buffer.alloc(0);

  /** Discover the pipe name written by the loader. Returns null if absent. */
  private discoverPipeName(): string | null {
    const base = process.env.LOCALAPPDATA || process.env.APPDATA;
    if (!base) {
      console.warn("[engine-client] LOCALAPPDATA/APPDATA not set");
      return null;
    }
    const path = join(base, ...DISCOVERY_FILE_REL);
    if (!existsSync(path)) {
      console.warn(`[engine-client] discovery file not found: ${path}`);
      return null;
    }
    try {
      const name = readFileSync(path, "utf-8").trim();
      return name || null;
    } catch (e) {
      console.warn(`[engine-client] read discovery file: ${(e as Error).message}`);
      return null;
    }
  }

  /**
   * Connect with exponential backoff. Throws if exhausted without success.
   * Re-reads the discovery file on every attempt — the server PID (and
   * therefore the pipe name) can change between attempts.
   */
  async connect(maxRetries = 10, initialBackoffMs = 500): Promise<void> {
    for (let attempt = 0; attempt < maxRetries; attempt++) {
      const name = this.discoverPipeName();
      if (name) {
        try {
          await this.tryConnect(name);
          this.pipeName = name;
          console.log(`[engine-client] connected to ${name}`);
          return;
        } catch (e) {
          console.warn(`[engine-client] attempt ${attempt + 1}: ${(e as Error).message}`);
        }
      }
      const wait = Math.min(initialBackoffMs * 2 ** attempt, 8_000);
      await new Promise((r) => setTimeout(r, wait));
    }
    throw new Error(`[engine-client] could not connect after ${maxRetries} attempts`);
  }

  private tryConnect(pipeName: string): Promise<void> {
    return new Promise((resolve, reject) => {
      const socket = createConnection(pipeName);
      const cleanup = () => {
        socket.removeAllListeners("error");
        socket.removeAllListeners("connect");
      };
      socket.once("error", (e) => {
        cleanup();
        socket.destroy();
        reject(e);
      });
      socket.once("connect", () => {
        cleanup();
        socket.on("error", (e) => {
          console.warn(`[engine-client] socket error: ${e.message}`);
          this.socket = null;
          this.failAllPending(e);
        });
        socket.on("close", () => {
          console.warn("[engine-client] socket closed");
          this.socket = null;
          this.failAllPending(new Error("socket closed"));
        });
        socket.on("data", (chunk) => this.handleData(chunk));
        this.socket = socket;
        resolve();
      });
    });
  }

  /** Send an RPC request and await the typed result. */
  async call<T = unknown>(method: string, params?: unknown): Promise<T> {
    if (!this.socket) throw new Error("[engine-client] not connected");
    const id = randomUUID();
    const req: EngineRpcRequest = { id, method, params };
    return new Promise<T>((resolve, reject) => {
      const timeoutHandle = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`[engine-client] request ${method}#${id} timed out`));
      }, REQUEST_TIMEOUT_MS);

      this.pending.set(id, {
        resolve: (resp) => {
          clearTimeout(timeoutHandle);
          this.pending.delete(id);
          if (resp.error) {
            reject(new Error(`engine RPC ${resp.error.code}: ${resp.error.message}`));
          } else {
            resolve(resp.result as T);
          }
        },
        reject: (e) => {
          clearTimeout(timeoutHandle);
          this.pending.delete(id);
          reject(e);
        },
        timeoutHandle,
      });

      try {
        this.socket!.write(this.encodeFrame(req));
      } catch (e) {
        this.pending.delete(id);
        clearTimeout(timeoutHandle);
        reject(e as Error);
      }
    });
  }

  /** Convenience — pings the engine. */
  async ping(): Promise<Record<string, unknown>> {
    return this.call<Record<string, unknown>>("ping");
  }

  /** Subscribe to engine-pushed events. Returns an unsubscribe fn. */
  onEvent(listener: (evt: EngineEvent) => void): () => void {
    this.eventListeners.push(listener);
    return () => {
      const i = this.eventListeners.indexOf(listener);
      if (i >= 0) this.eventListeners.splice(i, 1);
    };
  }

  disconnect(): void {
    if (this.socket) {
      this.socket.destroy();
      this.socket = null;
    }
    this.failAllPending(new Error("disconnected"));
  }

  // ─── Internal ────────────────────────────────────────────────────────

  private handleData(chunk: Buffer): void {
    this.readBuffer = Buffer.concat([this.readBuffer, chunk]);
    while (this.readBuffer.length >= 4) {
      const len = this.readBuffer.readUInt32LE(0);
      if (len > MAX_FRAME_BYTES) {
        console.error(`[engine-client] frame too large (${len}); dropping connection`);
        this.socket?.destroy();
        this.readBuffer = Buffer.alloc(0);
        return;
      }
      if (this.readBuffer.length < 4 + len) return; // partial frame
      const body = this.readBuffer.subarray(4, 4 + len);
      this.readBuffer = this.readBuffer.subarray(4 + len);
      try {
        this.dispatchMessage(JSON.parse(body.toString("utf-8")));
      } catch (e) {
        console.warn(`[engine-client] bad frame: ${(e as Error).message}`);
      }
    }
  }

  private dispatchMessage(msg: unknown): void {
    if (typeof msg !== "object" || msg === null) return;
    const m = msg as Record<string, unknown>;
    if (typeof m.id === "string") {
      const pending = this.pending.get(m.id);
      if (pending) pending.resolve(m as unknown as EngineRpcResponse);
      return;
    }
    if (typeof m.event === "string") {
      const evt: EngineEvent = { event: m.event, data: m.data ?? null };
      for (const l of this.eventListeners) {
        try { l(evt); } catch (e) {
          console.warn(`[engine-client] listener error: ${(e as Error).message}`);
        }
      }
    }
  }

  private encodeFrame(obj: unknown): Buffer {
    const body = Buffer.from(JSON.stringify(obj), "utf-8");
    const header = Buffer.alloc(4);
    header.writeUInt32LE(body.length, 0);
    return Buffer.concat([header, body]);
  }

  private failAllPending(err: Error): void {
    for (const p of this.pending.values()) {
      clearTimeout(p.timeoutHandle);
      p.reject(err);
    }
    this.pending.clear();
  }
}

/**
 * Module-global singleton — mods reach the engine via this. Set by index.ts
 * when the companion successfully connects.
 */
declare global {
  // eslint-disable-next-line no-var
  var __turdmod_engine_client: EngineClient | undefined;
}
