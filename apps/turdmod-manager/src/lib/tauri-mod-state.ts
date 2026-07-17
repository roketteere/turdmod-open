// Typed wrappers for mod freeze state + per-mod config commands.

import { invoke } from '@tauri-apps/api/core';

export interface ModState {
  frozen: string[];
}

export interface ConfigFieldBase {
  key: string;
  label?: string;
  description?: string;
}

export interface StringConfigField extends ConfigFieldBase {
  type: 'string';
  default: string;
}

export interface NumberConfigField extends ConfigFieldBase {
  type: 'number';
  default: number;
  min?: number;
  max?: number;
}

export interface BooleanConfigField extends ConfigFieldBase {
  type: 'boolean';
  default: boolean;
}

export interface EnumConfigField extends ConfigFieldBase {
  type: 'enum';
  default: string;
  options: string[];
}

export type ConfigField =
  | StringConfigField
  | NumberConfigField
  | BooleanConfigField
  | EnumConfigField;

export interface ModManifest {
  id: string;
  name: string;
  version: string;
  configSchema?: ConfigField[];
}

export type ModConfig = Record<string, string | number | boolean>;

export function getModState(): Promise<ModState> {
  return invoke<ModState>('manager_get_mod_state');
}

export function setModFrozen(slug: string, frozen: boolean): Promise<ModState> {
  return invoke<ModState>('manager_set_mod_frozen', { slug, frozen });
}

export function readModConfig(slug: string): Promise<ModConfig> {
  return invoke<ModConfig>('manager_read_mod_config', { slug });
}

export function writeModConfig(slug: string, config: ModConfig): Promise<void> {
  return invoke<void>('manager_write_mod_config', { slug, config });
}

export function getModManifest(slug: string): Promise<ModManifest> {
  return invoke<ModManifest>('manager_get_mod_manifest', { slug });
}
