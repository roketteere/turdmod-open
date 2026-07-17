// Typed wrappers around the SCUM-server connector commands.
// Sibling to ./tauri.ts which the UI agent owns; we keep the server
// surface in its own file so changes here can't break the mod-install
// invoke wrappers.

import { invoke } from '@tauri-apps/api/core';

export type Transport = 'Sftp' | 'Ftp' | 'RconOnly';

export interface ServerProfile {
  id: string;
  name: string;
  host: string;
  port: number;
  transport: Transport;
  username: string;
  scumRoot: string;
  rconPort: number | null;
  rconPasswordRef: string | null;
  createdAt: number;
}

export interface ServerProfileInput {
  id?: string;
  name: string;
  host: string;
  port: number;
  transport: Transport;
  username: string;
  scumRoot: string;
  rconPort?: number | null;
  rconPasswordRef?: string | null;
  createdAt?: number;
}

export interface RemoteMod {
  slug: string;
  sizeBytes: number;
  modified: number | null;
}

export interface RemotePath {
  path: string;
  sizeBytes: number;
}

export interface ServerProbeResult {
  sftpOk: boolean;
  rconOk: boolean;
  latencyMs: number | null;
  error: string | null;
}

function normalizeProfile(input: ServerProfileInput): ServerProfile {
  return {
    id: input.id ?? '',
    name: input.name,
    host: input.host,
    port: input.port,
    transport: input.transport,
    username: input.username,
    scumRoot: input.scumRoot,
    rconPort: input.rconPort ?? null,
    rconPasswordRef: input.rconPasswordRef ?? null,
    createdAt: input.createdAt ?? 0,
  };
}

export function addServer(
  profile: ServerProfileInput,
  secretSsh: string | null,
  secretRcon: string | null,
): Promise<ServerProfile> {
  return invoke<ServerProfile>('manager_server_add', {
    profile: normalizeProfile(profile),
    secretSsh,
    secretRcon,
  });
}

export function removeServer(id: string): Promise<void> {
  return invoke<void>('manager_server_remove', { id });
}

export function listServers(): Promise<ServerProfile[]> {
  return invoke<ServerProfile[]>('manager_server_list');
}

export function testServer(id: string): Promise<ServerProbeResult> {
  return invoke<ServerProbeResult>('manager_server_test', { id });
}

export function uploadModToServer(id: string, slug: string): Promise<RemotePath> {
  return invoke<RemotePath>('manager_server_upload_mod', { id, slug });
}

export function removeRemoteMod(id: string, slug: string): Promise<void> {
  return invoke<void>('manager_server_remove_remote_mod', { id, slug });
}

export function listRemoteMods(id: string): Promise<RemoteMod[]> {
  return invoke<RemoteMod[]>('manager_server_list_remote_mods', { id });
}

export function tailLog(
  id: string,
  log: string,
  lines: number,
): Promise<string[]> {
  return invoke<string[]>('manager_server_tail_log', { id, log, lines });
}

export function rconSend(id: string, command: string): Promise<string> {
  return invoke<string>('manager_server_rcon', { id, command });
}

export function knownLogs(): Promise<string[]> {
  return invoke<string[]>('manager_server_known_logs');
}

export const TRANSPORTS: Transport[] = ['Sftp', 'Ftp', 'RconOnly'];

export const TRANSPORT_LABELS: Record<Transport, string> = {
  Sftp: 'SFTP (SSH)',
  Ftp: 'FTP',
  RconOnly: 'RCON only',
};

export function defaultPortFor(t: Transport): number {
  if (t === 'Sftp') return 22;
  if (t === 'Ftp') return 21;
  return 27015;
}
