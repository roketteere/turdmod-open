// Typed wrappers for the AI Assistant Tauri commands.
//
// Backend lives in apps/turdmod-manager/src-tauri/src/assist.rs.
// All commands are snake_case literal at the invoke boundary per
// the Tauri naming gotcha (see apps/turdmod-manager/CLAUDE.md).

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export interface GpuInfo {
  name: string | null;
  vramMib: number | null;
  source: 'nvidia-smi' | 'unavailable';
  note: string | null;
}

export interface LocalModel {
  name: string;
  size: number;
  modifiedAt: string | null;
  estimatedVramMib: number;
}

export interface PullProgress {
  status: string;
  completed: number;
  total: number;
  digest: string | null;
}

export function assistGpuInfo(): Promise<GpuInfo> {
  return invoke<GpuInfo>('assist_gpu_info');
}

export function assistListModels(): Promise<LocalModel[]> {
  return invoke<LocalModel[]>('assist_list_models');
}

export function assistPullModel(model: string): Promise<string> {
  return invoke<string>('assist_pull_model', { model });
}

export function assistChat(model: string, prompt: string): Promise<string> {
  return invoke<string>('assist_chat', { model, prompt });
}

export function assistSummarizeDiff(
  model: string,
  diffJson: string,
): Promise<string> {
  return invoke<string>('assist_summarize_diff', { model, diffJson });
}

export function assistExplainPhaseLog(
  model: string,
  phase: string,
  logLines: string,
): Promise<string> {
  return invoke<string>('assist_explain_phase_log', { model, phase, logLines });
}

export async function onAssistProgress(
  handler: (p: PullProgress) => void,
): Promise<UnlistenFn> {
  return listen<PullProgress>('assist://progress', (e) => handler(e.payload));
}

// ---------------------------------------------------------------------------
// Local persistence — simple localStorage settings until we migrate to
// tauri-plugin-store. The persisted shape is intentionally tiny so it
// can be hand-edited from devtools if needed.
// ---------------------------------------------------------------------------

const STORAGE_KEY = 'turdmod.assist.settings.v1';

export interface AssistSettings {
  /** Master toggle — when false, all assist features are hidden. */
  enabled: boolean;
  /** The Ollama model name to use for assist requests. */
  model: string;
}

const DEFAULT_SETTINGS: AssistSettings = {
  enabled: false,
  model: 'qwen2.5-coder:7b',
};

export function loadAssistSettings(): AssistSettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return { ...DEFAULT_SETTINGS };
    const parsed = JSON.parse(raw) as Partial<AssistSettings>;
    return { ...DEFAULT_SETTINGS, ...parsed };
  } catch {
    return { ...DEFAULT_SETTINGS };
  }
}

export function saveAssistSettings(s: AssistSettings): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(s));
  } catch {
    /* localStorage full or disabled — silent no-op */
  }
}

// ---------------------------------------------------------------------------
// Model-fit recommender — given GPU VRAM, classify a model as
// "fits / tight / doesn't fit / unknown."
// ---------------------------------------------------------------------------

export type ModelFit = 'fits' | 'tight' | 'over' | 'unknown';

export function classifyModelFit(
  modelVramMib: number | null | undefined,
  gpuVramMib: number | null | undefined,
): ModelFit {
  if (!modelVramMib || !gpuVramMib) return 'unknown';
  // Headroom: reserve 1 GiB for OS / overlay / browser.
  const usable = Math.max(0, gpuVramMib - 1024);
  if (modelVramMib <= usable * 0.85) return 'fits';
  if (modelVramMib <= usable) return 'tight';
  return 'over';
}
