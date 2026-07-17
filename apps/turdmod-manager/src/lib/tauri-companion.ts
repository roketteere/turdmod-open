// Typed wrappers around the companion lifecycle commands.
// Mirrors the CompanionStatus serde shape from companion.rs.

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type CompanionStatus =
  | { kind: 'stopped' }
  | { kind: 'starting' }
  | { kind: 'running'; pid: number; startedAtIso: string }
  | { kind: 'crashed'; exitCode: number | null };

export interface CompanionStdoutLine {
  ts: string;
  level: 'info' | 'warn' | 'error';
  raw: string;
}

// ---------------------------------------------------------------------------
// Command wrappers
// ---------------------------------------------------------------------------

export function companionStart(): Promise<CompanionStatus> {
  return invoke<CompanionStatus>('companion_start');
}

export function companionStop(): Promise<CompanionStatus> {
  return invoke<CompanionStatus>('companion_stop');
}

export function companionRestart(): Promise<CompanionStatus> {
  return invoke<CompanionStatus>('companion_restart');
}

export function companionGetStatus(): Promise<CompanionStatus> {
  return invoke<CompanionStatus>('companion_get_status');
}

// ---------------------------------------------------------------------------
// Event subscription
// ---------------------------------------------------------------------------

/**
 * Subscribe to live stdout/stderr lines from the companion process.
 * Returns an unsubscribe function — call it in a useEffect cleanup.
 */
export function subscribeToCompanionStdout(
  handler: (line: CompanionStdoutLine) => void,
): Promise<UnlistenFn> {
  return listen<CompanionStdoutLine>('companion://stdout', (event) => {
    handler(event.payload);
  });
}