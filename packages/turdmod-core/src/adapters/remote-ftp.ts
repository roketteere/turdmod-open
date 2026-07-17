import type {
  LogTailEntry,
  ReadFileResult,
  RconResponse,
  ServerAdapter,
} from '../adapter.js';
import { UnsupportedOperationError } from '../adapter.js';
import { LITE_CAPABILITIES, type TierCapabilities } from '../tier.js';

// Talks to a managed-host SCUM server over FTP/SFTP for config + log
// access and over TCP for RCON. The Lite tier's primary adapter.
//
// File-system access is restricted to whatever the host exposes — on
// G-Portal that's `/SCUM/Saved/` and nothing else (no `Binaries/`).
// Verified hosts: G-Portal. Likely-compatible: Nitrado, Host Havoc,
// Survival Servers, GTX, PingPerfect.

export interface RemoteFtpAdapterConfig {
  // FTP / SFTP target.
  host: string;
  port: number;
  username: string;
  password: string;
  protocol: 'ftp' | 'sftp';

  // Path inside the FTP tree where `SCUM/Saved/` is rooted. On G-Portal
  // this is empty (the FTP root IS the SCUM/Saved equivalent).
  remoteRoot: string;

  // RCON config — required for the live-admin half of Lite.
  rcon: { host: string; port: number; password: string };

  // Caller's Tauri invoke. The Rust side owns FTP/SFTP client choice
  // (russh_sftp vs native FTP via the http plugin etc.).
  invoke: <T = unknown>(cmd: string, args?: object) => Promise<T>;

  // Stable opaque identifier the Rust side uses to look up the
  // connection's credentials in its secret store. Saves the TS layer
  // from holding the password across calls.
  serverId: string;
}

export class RemoteFtpAdapter implements ServerAdapter {
  readonly capabilities: TierCapabilities = LITE_CAPABILITIES;

  constructor(private readonly cfg: RemoteFtpAdapterConfig) {}

  async readFile(path: string): Promise<ReadFileResult> {
    // Rust-side command TBD — Manager's current server_commands.rs has
    // a similar shape that can be lifted into a unified one.
    return this.cfg.invoke<ReadFileResult>('manager_server_read_remote_file', {
      serverId: this.cfg.serverId,
      path: this.joinRemote(path),
    });
  }

  async writeFile(path: string, content: string): Promise<void> {
    await this.cfg.invoke('manager_server_write_remote_file', {
      serverId: this.cfg.serverId,
      path: this.joinRemote(path),
      content,
    });
  }

  async listFiles(dir: string): Promise<string[]> {
    return this.cfg.invoke<string[]>('manager_server_list_remote_files', {
      serverId: this.cfg.serverId,
      dir: this.joinRemote(dir),
    });
  }

  async tailLog(path: string, lines: number): Promise<LogTailEntry[]> {
    return this.cfg.invoke<LogTailEntry[]>('manager_server_tail_log', {
      serverId: this.cfg.serverId,
      path: this.joinRemote(path),
      lines,
    });
  }

  async runRcon(command: string): Promise<RconResponse> {
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
    // Rust side handles connection pooling — TS-side close is a no-op
    // unless the user explicitly logs out of the server.
  }

  private joinRemote(path: string): string {
    if (!this.cfg.remoteRoot) return path;
    return `${this.cfg.remoteRoot.replace(/\/+$/, '')}/${path.replace(/^\/+/, '')}`;
  }
}
