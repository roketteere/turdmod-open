// Typed wrappers for the account command surface.
// Mirrors the `serde(rename_all = "camelCase")` shapes in
// src-tauri/src/account.rs.

import { invoke } from '@tauri-apps/api/core';

export interface AccountUser {
  id: number | null;
  username: string | null;
  displayName: string | null;
  avatarUrl: string | null;
  discordId: string | null;
  email: string | null;
}

export type AccountStatus =
  | { status: 'signed-out' }
  | { status: 'signed-in'; user: AccountUser };

export interface SignInStartInfo {
  url: string;
}

export function fetchAccountStatus(): Promise<AccountStatus> {
  return invoke<AccountStatus>('manager_account_status');
}

export function startSignIn(): Promise<SignInStartInfo> {
  return invoke<SignInStartInfo>('manager_account_sign_in_start');
}

export function pasteToken(token: string): Promise<AccountStatus> {
  return invoke<AccountStatus>('manager_account_paste_token', { token });
}

export function signOut(): Promise<void> {
  return invoke<void>('manager_account_sign_out');
}

/** Hand the URL returned by `startSignIn` to the OS default browser. */
export async function openInBrowser(url: string): Promise<void> {
  const { open } = await import('@tauri-apps/plugin-shell');
  await open(url);
}
