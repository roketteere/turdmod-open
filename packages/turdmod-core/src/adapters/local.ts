import type {
  LogTailEntry,
  ReadFileResult,
  RconResponse,
  ServerAdapter,
} from '../adapter.js';
import { UnsupportedOperationError } from '../adapter.js';
import { LITE_CAPABILITIES, type TierCapabilities } from '../tier.js';

// Talks to a SCUM install on the same machine the Manager / Lite app runs
// on. Used by Admin for "local install" mode and by Lite if the user has
// SCUM Server installed locally instead of on a remote managed host.
//
// All file ops go through the host app's Tauri Rust backend; this class
// is the thin TS-side contract. Each app injects its own `invoke` so this
// module doesn't import @tauri-apps/api directly — keeps the package
// Tauri-version-agnostic.

export interface LocalAdapterConfig {
  // SCUM install root containing `SCUM/Saved/`. Paths passed to readFile
  // etc. are resolved relative to this.
  installPath: string;

  // Tauri invoke function. Caller passes their app's invoke so we don't
  // pin a specific Tauri version.
  invoke: <T = unknown>(cmd: string, args?: object) => Promise<T>;

  // Optional RCON config if the local install is reachable via RCON.
  rcon?: { host: string; port: number; password: string };
}

export class LocalFsAdapter implements ServerAdapter {
  readonly capabilities: TierCapabilities = LITE_CAPABILITIES;

  constructor(private readonly cfg: LocalAdapterConfig) {}

  async readFile(path: string): Promise<ReadFileResult> {
    return this.cfg.invoke<ReadFileResult>('manager_read_text_file', {
      path: `${this.cfg.installPath}/${path}`,
    });
  }

  async writeFile(path: string, content: string): Promise<void> {
    await this.cfg.invoke('manager_write_text_file', {
      path: `${this.cfg.installPath}/${path}`,
      content,
    });
  }

  async listFiles(_dir: string): Promise<string[]> {
    throw new Error('listFiles not implemented yet — wire to a Tauri command');
  }

  async tailLog(_path: string, _lines: number): Promise<LogTailEntry[]> {
    throw new Error('tailLog not implemented yet — wire to a Tauri command');
  }

  async runRcon(command: string): Promise<RconResponse> {
    if (!this.cfg.rcon) {
      return { ok: false, text: 'No RCON configured for this local adapter' };
    }
    return this.cfg.invoke<RconResponse>('manager_server_rcon', {
      host: this.cfg.rcon.host,
      port: this.cfg.rcon.port,
      password: this.cfg.rcon.password,
      command,
    });
  }

  async engineRpc<T = unknown>(_method: string, _params?: object): Promise<T> {
    throw new UnsupportedOperationError('engineRpc', this.capabilities.tier);
  }

  subscribeEvents(): () => void {
    throw new UnsupportedOperationError('subscribeEvents', this.capabilities.tier);
  }

  async close(): Promise<void> {
    // No persistent connections held.
  }
}
