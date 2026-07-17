// Typed wrappers around the Rust-side `invoke` surface.
// Field shapes mirror `serde(rename_all = "camelCase")` on the Rust structs.

import { invoke } from '@tauri-apps/api/core';

export type ModCategory =
  | 'UI'
  | 'HUD'
  | 'Squad'
  | 'Admin'
  | 'Gameplay'
  | 'Map'
  | 'Vehicle'
  | 'NPC';

export interface ModAuthor {
  displayName: string;
  userId: number | null;
}

export interface ModSummary {
  slug: string;
  name: string;
  tag: string;
  category: ModCategory;
  summary: string;
  // Catalog list endpoint doesn't return these; they arrive via the
  // detail endpoint or download manifest. Treat as optional everywhere.
  version?: string;
  author?: ModAuthor;
  proposedBy?: string | null;
  isPremium: boolean;
  isPremiumBundled: boolean;
  priceCents: number | null;
  battleyePolicy?: string;
}

export interface ModDetail extends ModSummary {
  description: string;
  sizeBytes: number | null;
  releasedAt: string | null;
  changelog: string | null;
}

export interface InstalledMod {
  slug: string;
  name: string;
  version: string;
  installedAt: string;
  installPath: string;
  sizeBytes: number;
}

export interface DetectResult {
  /** Resolved active install path. null = nothing detected and no override. */
  active: string | null;
  activeTarget: 'game' | 'server';
  game: string | null;
  server: string | null;
  modsDir: string | null;
  battleyeDisabled: boolean | null;
}

export type InstallProgress =
  | { kind: 'Downloading'; received: number; total: number | null }
  | { kind: 'Verifying' }
  | { kind: 'Extracting' }
  | { kind: 'Done' };

export interface ListModsArgs {
  category?: ModCategory | null;
  search?: string | null;
}

export function detectScum(): Promise<DetectResult> {
  return invoke<DetectResult>('manager_detect_scum');
}

export function setInstallPath(path: string): Promise<void> {
  return invoke<void>('manager_set_install_path', { path });
}

export function getInstallPath(): Promise<string | null> {
  return invoke<string | null>('manager_get_install_path');
}

export function listMods(args: ListModsArgs = {}): Promise<ModSummary[]> {
  return invoke<ModSummary[]>('manager_list_mods', {
    category: args.category ?? null,
    search: args.search ?? null,
  });
}

export function getMod(slug: string): Promise<ModDetail> {
  return invoke<ModDetail>('manager_get_mod', { slug });
}

export function installMod(
  slug: string,
  authToken?: string | null,
): Promise<InstalledMod> {
  return invoke<InstalledMod>('manager_install_mod', {
    slug,
    authToken: authToken ?? null,
  });
}

export function uninstallMod(slug: string): Promise<void> {
  return invoke<void>('manager_uninstall_mod', { slug });
}

export function listInstalled(): Promise<InstalledMod[]> {
  return invoke<InstalledMod[]>('manager_list_installed');
}

export const MOD_CATEGORIES: ModCategory[] = [
  'UI',
  'HUD',
  'Squad',
  'Admin',
  'Gameplay',
  'Map',
  'Vehicle',
  'NPC',
];
