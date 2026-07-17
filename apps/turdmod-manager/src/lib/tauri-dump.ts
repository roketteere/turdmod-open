// Typed wrappers for the Dump Management Tauri commands.
// Naming reminder: invoke() takes the literal Rust function name in
// snake_case. `rename_all = "camelCase"` on the command attribute only
// renames PARAMETERS, not the function. See apps/turdmod-manager/CLAUDE.md.

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type PhaseId =
  | 'detect'
  | 'phase-a'
  | 'phase-b'
  | 'phase-b-client'
  | 'phase-c';

export interface PhaseACount {
  count: number;
  ok: boolean;
}

export interface PhaseAResults {
  classes: PhaseACount;
  enums: PhaseACount;
  structs: PhaseACount;
}

export interface PhaseBMeta {
  dumpedAt: string | null;
  fileCount: number;
  byteCount: number;
}

export interface PhaseCCategory {
  count: number;
  files: number;
  bytes: number;
}

export interface PhaseCMeta {
  dumpedAt: string | null;
  widgets: PhaseCCategory;
  datatables: PhaseCCategory;
  strings: PhaseCCategory;
  errors: number;
}

export interface DumpStatus {
  scumdumpPresent: boolean;
  scumdumpRoot: string | null;
  steamBuild: string | null;
  steamClientBuild: string | null;
  extractedBuild: string | null;
  updateAvailable: boolean;
  aesFingerprint: string | null;
  phaseA: PhaseAResults | null;
  phaseADumpedAt: string | null;
  phaseB: PhaseBMeta | null;
  phaseBClient: PhaseBMeta | null;
  phaseC: PhaseCMeta | null;
  latestBuildDir: string | null;
}

export interface DumpUpdateCheck {
  steamBuild: string | null;
  extractedBuild: string | null;
  updateAvailable: boolean;
}

export interface DumpLogLine {
  phase: PhaseId;
  stream: 'stdout' | 'stderr';
  line: string;
}

export function dumpStatus(): Promise<DumpStatus> {
  return invoke<DumpStatus>('dump_status');
}

export function dumpCheckUpdates(): Promise<DumpUpdateCheck> {
  return invoke<DumpUpdateCheck>('dump_check_updates');
}

export function dumpRunPhase(phase: PhaseId): Promise<number> {
  return invoke<number>('dump_run_phase', { phase });
}

export function dumpRunAll(): Promise<number> {
  return invoke<number>('dump_run_all');
}

export function dumpOpenFolder(): Promise<string> {
  return invoke<string>('dump_open_folder');
}

export function dumpExtractAes(): Promise<number> {
  return invoke<number>('dump_extract_aes');
}

/**
 * Subscribe to `dump://log` events. Returns the unlisten function —
 * call it on component unmount.
 */
export async function onDumpLog(
  handler: (line: DumpLogLine) => void,
): Promise<UnlistenFn> {
  return listen<DumpLogLine>('dump://log', (e) => handler(e.payload));
}

// ---------------------------------------------------------------------------
// Forensic archive
// ---------------------------------------------------------------------------

export interface ArchiveEntry {
  ts: string;
  scumBuild: string | null;
  scumServerSha256: string | null;
  scumExeSha256: string | null;
  keyType: string;
  value: string;
  source: string | null;
  notes: string | null;
  target: 'server' | 'client' | null;
}

export interface ArchiveListing {
  entries: ArchiveEntry[];
  total: number;
  keyTypes: string[];
  builds: string[];
  archivePath: string | null;
}

export function dumpArchiveEntries(
  filter?: { keyType?: string; build?: string; limit?: number },
): Promise<ArchiveListing> {
  return invoke<ArchiveListing>('dump_archive_entries', {
    keyType: filter?.keyType,
    build: filter?.build,
    limit: filter?.limit,
  });
}

// ---------------------------------------------------------------------------
// Diff report — matches scumdump's _diff.json shape
// ---------------------------------------------------------------------------

export interface DiffNameSet {
  added: string[];
  removed: string[];
  changedCount: number;
  prevCount: number;
  currCount: number;
}

export interface DiffPhaseA {
  classes: DiffNameSet;
  enums: DiffNameSet;
  structs: DiffNameSet;
}

export interface DiffPhaseC {
  widgets: DiffNameSet;
  datatables: DiffNameSet;
  strings: DiffNameSet;
}

export interface DiffReport {
  previousBuild: string;
  currentBuild: string;
  computedAt: string;
  phaseA?: DiffPhaseA;
  phaseB?: DiffNameSet;
  phaseBClient?: DiffNameSet;
  phaseC?: DiffPhaseC;
  notes: string[];
}

export function dumpDiffSummary(build?: string): Promise<DiffReport | null> {
  return invoke<DiffReport | null>('dump_diff_summary', { build });
}

export function dumpListBuilds(): Promise<string[]> {
  return invoke<string[]>('dump_list_builds');
}

// ---------------------------------------------------------------------------
// DLL injection — always elevated. Manager invokes turdmod-injector.exe
// via ShellExecuteEx runas so it works against both non-elevated client
// processes and elevated GameServer.exe uniformly. UAC will prompt.
// ---------------------------------------------------------------------------

export interface InjectResult {
  target: string;
  dllPath: string;
  /** Exit code from turdmod-injector.exe. 0 = success; non-zero = mapped
   *  to a human message. -1 specifically = UAC cancelled. */
  exitCode: number;
  message: string;
}

/**
 * Inject `Dumper-7.dll` into the running SCUM server or client process.
 * Always elevated — UAC prompts on each call.
 */
export function dumpInjectDumper7(
  target: 'server' | 'client',
): Promise<InjectResult> {
  return invoke<InjectResult>('dump_inject_dumper7', { target });
}

/**
 * Generic DLL inject — same elevated path. Use for UE4SS standalone,
 * custom mod DLLs, etc.
 */
export function dumpInjectDll(
  target: string,
  dllPath: string,
): Promise<InjectResult> {
  return invoke<InjectResult>('dump_inject_dll', { target, dllPath });
}
