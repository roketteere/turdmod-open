import type { TierCapabilities } from './tier.js';

// The unified operation surface that all three apps consume. Each method
// has the SAME shape regardless of which adapter implements it — that's
// the whole point of the abstraction. Lite-tier adapters throw on
// engineRpc / subscribeEvents calls; UI code checks `capabilities` before
// invoking them.
//
// Implementations are expected to live behind each app's Tauri Rust
// backend; this TS interface is the contract the UI codes against and
// the Rust side fulfills via `invoke()`.

export interface ReadFileResult {
  content: string;
  existed: boolean;
}

export interface LogTailEntry {
  timestamp: string;
  line: string;
}

export interface RconResponse {
  ok: boolean;
  text: string;
}

// All paths are POSIX-style and rooted at SCUM's `Saved/` directory unless
// the adapter doc says otherwise. The adapter is responsible for mapping
// them to local paths or FTP paths.
//
// Examples:
//   readFile("Config/WindowsServer/Notifications.json")
//   tailLog("Logs/SCUM.log", 200)
export interface ServerAdapter {
  readonly capabilities: TierCapabilities;

  readFile(path: string): Promise<ReadFileResult>;
  writeFile(path: string, content: string): Promise<void>;
  listFiles(dir: string): Promise<string[]>;

  // Streams the latest N lines of a log file. Adapters that can't tail
  // efficiently (RemoteFtp) should fetch the file and slice.
  tailLog(path: string, lines: number): Promise<LogTailEntry[]>;

  // Sends a single RCON command. Implementations open + close per call
  // (or pool; that's an implementation detail). Returns the server's
  // response text.
  runRcon(command: string): Promise<RconResponse>;

  // Engine-tier only. Lite adapters throw if called.
  // Wire format: JSON params in, JSON response out. The bridge's named
  // pipe protocol is documented in packages/turdmod-api.
  engineRpc<T = unknown>(method: string, params?: object): Promise<T>;

  // Engine-tier only. Subscribes to a server-emitted event stream
  // (chat events, kill events, vehicle spawns). Returns an unsubscribe
  // function.
  subscribeEvents(handler: (event: { type: string; payload: unknown }) => void): () => void;

  // Lifecycle. Implementations may eagerly open connections; close()
  // releases them.
  close(): Promise<void>;
}

// Helper that throws a uniform error for unsupported operations on
// Lite-tier adapters. UI code that's been honest about checking
// capabilities won't hit this.
export class UnsupportedOperationError extends Error {
  constructor(operation: string, tier: string) {
    super(`Operation "${operation}" is not supported on tier "${tier}". Check adapter.capabilities first.`);
    this.name = 'UnsupportedOperationError';
  }
}
