// Typed wrappers around the engine lifecycle commands.
// Mirrors the EngineStatus / EngineLogLine serde shapes from engine.rs.

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getEngineHost } from './engineHost';

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type EngineStatus =
  | { kind: 'stopped' }
  | { kind: 'starting' }
  | { kind: 'running'; pid: number; startedAtIso: string }
  | { kind: 'crashed'; exitCode: number | null };

export interface EnginePaths {
  ue4ssDll?:    string | null;
  bridgeDll?:   string | null;
  loaderDll?:   string | null;
  launcherExe?: string | null;
}

export interface InstallReport {
  ue4ssDllPath:  string;
  bridgeDllPath: string;
  settingsIni:   string;
  modsTxt:       string;
  wroteIni:      boolean;
  wroteModsLine: boolean;
}

export interface EngineLogLine {
  ts:     string;
  source: 'launcher' | 'ue4ss' | 'loader' | 'scum';
  level:  'info' | 'warn' | 'error';
  raw:    string;
}

// ---------------------------------------------------------------------------
// Command wrappers
// ---------------------------------------------------------------------------

export function engineInstall(
  serverInstall: string,
  paths?: EnginePaths,
): Promise<InstallReport> {
  return invoke<InstallReport>('engine_install', { serverInstall, paths: paths ?? null });
}

export function engineStart(
  serverInstall: string,
  opts?: { paths?: EnginePaths; skipSafetyCheck?: boolean },
): Promise<EngineStatus> {
  return invoke<EngineStatus>('engine_start', {
    serverInstall,
    paths: opts?.paths ?? null,
    skipSafetyCheck: opts?.skipSafetyCheck ?? true,
  });
}

export function engineStop(): Promise<EngineStatus> {
  return invoke<EngineStatus>('engine_stop');
}

export function engineGetStatus(): Promise<EngineStatus> {
  return invoke<EngineStatus>('engine_get_status');
}

/**
 * Generic JSON-RPC pass-through to the bridge cppmod via the engine
 * named pipe. Used by the Schema Browser and any future UI page that
 * needs to call a bridge handler directly.
 *
 * Throws if the engine isn't running (no pipe.txt) or the RPC times
 * out after 30 s.
 */
export function engineRpc<T = unknown>(
  method: string,
  params?: Record<string, unknown>,
): Promise<T> {
  // Route to the active host. remote_engine_rpc already unwraps `result`
  // server-side (remote.rs), so both commands return the same shape.
  const cmd = getEngineHost() === 'remote' ? 'remote_engine_rpc' : 'engine_rpc';
  return invoke<T>(cmd, { method, params: params ?? null });
}

// ---------------------------------------------------------------------------
// Bridge handler response shapes
// ---------------------------------------------------------------------------

export interface DumpWidgetsResponse {
  totalWidgets: number;
  emitted: number;
  limit: number;
  grep: string;
  widgets: WidgetEntry[];
}

export interface WidgetEntry {
  name: string;
  kind: 'cpp' | 'bp';
  parent?: string;
}

export interface DescribeWidgetResponse {
  found: boolean;
  name?: string;
  kind?: 'cpp' | 'bp';
  inheritance?: string[];
  totalProperties?: number;
  emitted?: number;
  properties?: WidgetProperty[];
  error?: string;
}

export interface WidgetProperty {
  name: string;
  type: string;
  offset: number;
  ownerClass?: string;
}

export interface FindFunctionsResponse {
  totalFunctions: number;
  emitted: number;
  limit: number;
  grep: string;
  functions: FunctionEntry[];
}

export interface FunctionEntry {
  name: string;
  owner: string;
}

export interface DescribeFunctionResponse {
  found: boolean;
  owner?: string;
  params?: FunctionParam[];
  error?: string;
}

export interface FunctionParam {
  name: string;
  type: string;
}

// readClassValues — populates current values from the CDO so the Schema
// page can render real numbers/strings/bools next to each property
// alongside its type and offset (rather than just the layout).
export type PropertyValueKind =
  | 'bool'
  | 'int'
  | 'float'
  | 'byte'
  | 'name'
  | 'string'
  | 'object'
  | 'unsupported';

export interface PropertyValue {
  name: string;
  type: string;
  offset: number;
  value: boolean | number | string | null;
  valueKind: PropertyValueKind;
}

export interface ReadClassValuesResponse {
  found: boolean;
  name?: string;
  values?: PropertyValue[];
  error?: string;
}

// writeClassDefault — mutates a single property value on a UClass's CDO.
// V1 supports only scalar kinds (bool/int/float/byte); name/string/object
// writes are deferred until we add FString allocation + FName interning.
export interface WriteClassDefaultResponse {
  ok: boolean;
  className?: string;
  propertyName?: string;
  valueKind?: string;
  offset?: number;
  error?: string;
}

// applyRecipe — batch-apply property overrides to a live CDO.
export interface ApplyRecipeResponse {
  ok: boolean;
  className?: string;
  applied: number;
  failed: number;
  results?: Array<{
    property: string;
    ok: boolean;
    error?: string;
    offset?: number;
    valueKind?: string;
  }>;
  error?: string;
}

// showPanel — push a widget command to connected players.
export interface ShowPanelResponse {
  ok: boolean;
  command?: string;
  widgetClass?: string;
  routerPtr?: string;
  usingCDO?: boolean;
  error?: string;
}

// ---------------------------------------------------------------------------
// Event subscriptions
// ---------------------------------------------------------------------------

/** Subscribe to engine log lines (launcher stdout + ue4ss.log + loader.log). */
export function subscribeToEngineLog(
  handler: (line: EngineLogLine) => void,
): Promise<UnlistenFn> {
  return listen<EngineLogLine>('engine://log', (event) => {
    handler(event.payload);
  });
}
