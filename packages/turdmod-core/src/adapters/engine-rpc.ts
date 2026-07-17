import type {
  LogTailEntry,
  ReadFileResult,
  RconResponse,
  ServerAdapter,
} from '../adapter.js';
import { ENGINE_CAPABILITIES, type TierCapabilities } from '../tier.js';

// Talks to TurdMODEngineBridge.dll over a named pipe (local) or
// SSH-tunneled named pipe (remote). The Engine tier's primary adapter.
//
// File access on the Engine tier still goes through the local-or-remote
// filesystem (the bridge runs in-process with GameServer.exe); the
// distinguishing feature is `engineRpc` which calls UFunctions directly.

export interface EngineRpcAdapterConfig {
  // Host the SCUM server runs on. "localhost" for same-machine; a
  // hostname for SSH-tunneled remote engine.
  host: string;

  // SSH config for remote tunneling. Undefined for local engine.
  ssh?: {
    port: number;
    user: string;
    privateKeyPath?: string;
  };

  // Pipe name on the engine host. Default
  // "TurdMODEngineBridge" matches the bridge cppmod's pipe name.
  pipeName?: string;

  // Caller's Tauri invoke.
  invoke: <T = unknown>(cmd: string, args?: object) => Promise<T>;

  // Caller's Tauri event subscribe — used by subscribeEvents.
  // Each app passes its own listen() so we stay Tauri-version-agnostic.
  listenEvent: (
    event: string,
    handler: (payload: unknown) => void,
  ) => Promise<() => void>;
}

export class EngineRpcAdapter implements ServerAdapter {
  readonly capabilities: TierCapabilities = ENGINE_CAPABILITIES;

  constructor(private readonly cfg: EngineRpcAdapterConfig) {}

  async readFile(path: string): Promise<ReadFileResult> {
    // For Engine-tier-local, file access is direct FS via the same
    // command the LocalFsAdapter uses. For remote-engine via SSH,
    // implementations should route to an SSH-aware command.
    return this.cfg.invoke<ReadFileResult>('manager_read_text_file', { path });
  }

  async writeFile(path: string, content: string): Promise<void> {
    await this.cfg.invoke('manager_write_text_file', { path, content });
  }

  async listFiles(_dir: string): Promise<string[]> {
    throw new Error('listFiles not implemented yet — wire to a Tauri command');
  }

  async tailLog(_path: string, _lines: number): Promise<LogTailEntry[]> {
    throw new Error('tailLog not implemented yet — wire to a Tauri command');
  }

  async runRcon(command: string): Promise<RconResponse> {
    // Engine-tier servers usually expose RCON too — same shape as Lite.
    return this.cfg.invoke<RconResponse>('manager_server_rcon', { command });
  }

  async engineRpc<T = unknown>(method: string, params?: object): Promise<T> {
    return this.cfg.invoke<T>('engine_rpc', {
      method,
      params: params ?? null,
    });
  }

  // Engine-tier event stream. Implementations should already have a
  // bridge → companion event pipe wired (the existing "bridgeReady"
  // event uses it). Subscribe by routing all bridge events through one
  // Tauri event channel and filtering on the TS side.
  subscribeEvents(handler: (event: { type: string; payload: unknown }) => void): () => void {
    let unlistenFn: (() => void) | null = null;
    let cancelled = false;

    this.cfg
      .listenEvent('bridge-event', (payload) => {
        if (
          payload &&
          typeof payload === 'object' &&
          'type' in payload
        ) {
          handler(payload as { type: string; payload: unknown });
        }
      })
      .then((unlisten) => {
        if (cancelled) {
          unlisten();
        } else {
          unlistenFn = unlisten;
        }
      })
      .catch((e) => {
        console.error('[EngineRpcAdapter] subscribeEvents failed:', e);
      });

    return () => {
      cancelled = true;
      if (unlistenFn) unlistenFn();
    };
  }

  async close(): Promise<void> {
    // Named pipe lifecycle is owned by the companion process — closing
    // it here would kill the engine connection for everyone. No-op.
  }
}
