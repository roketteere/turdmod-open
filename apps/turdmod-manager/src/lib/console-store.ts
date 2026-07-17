// Module-level console buffer. Subscriptions to companion + engine
// event streams happen ONCE at App boot, so the buffer fills even when
// the Console page isn't mounted. The ConsolePage subscribes to this
// store instead of registering its own Tauri event listeners.

import { subscribeToCompanionStdout, type CompanionStdoutLine } from './tauri-companion';
import { subscribeToEngineLog, type EngineLogLine } from './tauri-engine';

export type LogSource = 'companion' | 'launcher' | 'ue4ss' | 'loader' | 'scum';
export type LogLevel = 'info' | 'warn' | 'error';

export interface UnifiedLine {
  ts: string;
  source: LogSource;
  level: LogLevel;
  raw: string;
}

const MAX_LINES = 5000;

let buffer: UnifiedLine[] = [];
const subscribers = new Set<(lines: UnifiedLine[]) => void>();
let started = false;

function append(line: UnifiedLine) {
  buffer = buffer.length >= MAX_LINES ? [...buffer.slice(1), line] : [...buffer, line];
  for (const fn of subscribers) fn(buffer);
}

// Kick off the global subscriptions exactly once. Safe to call from any
// component mount (App or a page) — second + later calls are no-ops.
export function startConsoleStore() {
  if (started) return;
  started = true;
  void subscribeToCompanionStdout((line: CompanionStdoutLine) => {
    append({ ts: line.ts, level: line.level, raw: line.raw, source: 'companion' });
  });
  void subscribeToEngineLog((line: EngineLogLine) => {
    append({ ts: line.ts, level: line.level, raw: line.raw, source: line.source });
  });
}

export function getConsoleLines(): UnifiedLine[] {
  return buffer;
}

export function subscribeConsole(handler: (lines: UnifiedLine[]) => void): () => void {
  subscribers.add(handler);
  return () => subscribers.delete(handler);
}

export function clearConsole() {
  buffer = [];
  for (const fn of subscribers) fn(buffer);
}
