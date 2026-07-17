/**
 * `turdmod.network` — server-authoritative event dispatch.
 *
 * Private-server only — manifest must declare `mode: server-side` (or
 * `offline-only` running on a private server with the network capability).
 * Strategy A pak content can't use this; Strategy D external tools also
 * can't (they have their own IPC channel out-of-band).
 *
 * Events are typed by `channel` (string id, mod-namespaced); payloads are
 * arbitrary JSON. The host enforces (modId, channel) topic isolation —
 * mods can't send on another mod's channel.
 *
 * @since 1.0
 */

import { type Disposable, requireNamespace, TurdMODError } from "./runtime.js";

export interface NetMessage<T = unknown> {
  channel: string;
  /** Steam id of the sender, or "server" for server-emitted events. */
  from: string;
  payload: T;
  /** Server timestamp (ms since epoch). */
  ts: number;
}

interface NetworkRuntime {
  /** Listen on a channel. Returns disposable to unsubscribe. */
  on<T = unknown>(channel: string, handler: (msg: NetMessage<T>) => void): Disposable;

  /** Emit to all clients (server-side mods only). */
  broadcast<T = unknown>(channel: string, payload: T): Promise<void>;

  /** Emit to a specific player. */
  send<T = unknown>(channel: string, playerId: string, payload: T): Promise<void>;

  /** Server-side mods only: ask the server for an authoritative answer to a request. */
  rpc<TReq = unknown, TRes = unknown>(channel: string, payload: TReq): Promise<TRes>;
}

function rt(): NetworkRuntime {
  return requireNamespace<NetworkRuntime>("network");
}

function checkChannel(ch: string): void {
  if (!ch) throw new TurdMODError("network: channel is required", "BAD_CHANNEL");
  if (!/^[a-z][a-z0-9._-]{1,127}$/.test(ch)) {
    throw new TurdMODError("network: channel must match [a-z][a-z0-9._-]{1,127}", "BAD_CHANNEL");
  }
}

/** @since 1.0 */
export function on<T = unknown>(channel: string, handler: (msg: NetMessage<T>) => void): Disposable {
  checkChannel(channel);
  return rt().on<T>(channel, handler);
}

/** Server-side mods only. @since 1.0 */
export async function broadcast<T = unknown>(channel: string, payload: T): Promise<void> {
  checkChannel(channel);
  return rt().broadcast<T>(channel, payload);
}

/** @since 1.0 */
export async function send<T = unknown>(channel: string, playerId: string, payload: T): Promise<void> {
  checkChannel(channel);
  if (!playerId) throw new TurdMODError("send: playerId is required", "BAD_ID");
  return rt().send<T>(channel, playerId, payload);
}

/** @since 1.0 */
export async function rpc<TReq = unknown, TRes = unknown>(channel: string, payload: TReq): Promise<TRes> {
  checkChannel(channel);
  return rt().rpc<TReq, TRes>(channel, payload);
}
