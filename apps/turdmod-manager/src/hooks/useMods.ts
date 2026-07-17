import { useEffect, useRef } from 'react';
import {
  useMutation,
  useQuery,
  useQueryClient,
  type UseMutationResult,
} from '@tanstack/react-query';
import {
  getMod,
  installMod,
  listInstalled,
  listMods,
  uninstallMod,
  type InstallProgress,
  type InstalledMod,
  type ListModsArgs,
  type ModDetail,
  type ModSummary,
} from '../lib/tauri';

export function useMods(args: ListModsArgs) {
  return useQuery<ModSummary[], Error>({
    queryKey: ['mods', args.category ?? null, args.search ?? null],
    queryFn: () => listMods(args),
  });
}

export function useMod(slug: string | undefined) {
  return useQuery<ModDetail, Error>({
    queryKey: ['mod', slug],
    queryFn: () => getMod(slug as string),
    enabled: Boolean(slug),
  });
}

export function useInstalledMods() {
  return useQuery<InstalledMod[], Error>({
    queryKey: ['installed'],
    queryFn: () => listInstalled(),
    refetchInterval: 5000,
  });
}

export interface InstallVars {
  slug: string;
  authToken?: string | null;
  onProgress?: (p: InstallProgress) => void;
}

export function useInstallMod(): UseMutationResult<
  InstalledMod,
  Error,
  InstallVars
> {
  const qc = useQueryClient();
  const unlistenRef = useRef<(() => void) | null>(null);

  useEffect(
    () => () => {
      if (unlistenRef.current) {
        unlistenRef.current();
        unlistenRef.current = null;
      }
    },
    [],
  );

  return useMutation<InstalledMod, Error, InstallVars>({
    mutationFn: async ({ slug, authToken, onProgress }) => {
      if (onProgress) {
        const { listen } = await import('@tauri-apps/api/event');
        const unlisten = await listen<InstallProgress>(
          'mod-install-progress',
          (e) => onProgress(e.payload),
        );
        unlistenRef.current = unlisten;
      }
      return installMod(slug, authToken);
    },
    onSettled: () => {
      if (unlistenRef.current) {
        unlistenRef.current();
        unlistenRef.current = null;
      }
      qc.invalidateQueries({ queryKey: ['installed'] });
    },
  });
}

export function useUninstallMod(): UseMutationResult<void, Error, string> {
  const qc = useQueryClient();
  return useMutation<void, Error, string>({
    mutationFn: (slug) => uninstallMod(slug),
    onSettled: () => {
      qc.invalidateQueries({ queryKey: ['installed'] });
    },
  });
}
