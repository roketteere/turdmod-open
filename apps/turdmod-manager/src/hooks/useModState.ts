import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  getModState,
  setModFrozen,
  readModConfig,
  writeModConfig,
  getModManifest,
  type ModConfig,
  type ModManifest,
  type ModState,
} from '../lib/tauri-mod-state';

const MOD_STATE_KEY = ['mod-state'] as const;

export function useModState() {
  return useQuery<ModState, Error>({
    queryKey: MOD_STATE_KEY,
    queryFn: getModState,
    refetchInterval: 10_000,
  });
}

export function useSetModFrozen() {
  const qc = useQueryClient();
  return useMutation<ModState, Error, { slug: string; frozen: boolean }>({
    mutationFn: ({ slug, frozen }) => setModFrozen(slug, frozen),
    onSettled: () => {
      qc.invalidateQueries({ queryKey: MOD_STATE_KEY });
    },
  });
}

export function useModManifest(slug: string | undefined) {
  return useQuery<ModManifest, Error>({
    queryKey: ['mod-manifest', slug],
    queryFn: () => getModManifest(slug as string),
    enabled: Boolean(slug),
    staleTime: 60_000,
  });
}

export function useModConfig(slug: string | undefined) {
  return useQuery<ModConfig, Error>({
    queryKey: ['mod-config', slug],
    queryFn: () => readModConfig(slug as string),
    enabled: Boolean(slug),
  });
}

export function useWriteModConfig(slug: string) {
  const qc = useQueryClient();
  return useMutation<void, Error, ModConfig>({
    mutationFn: (config) => writeModConfig(slug, config),
    onSettled: () => {
      qc.invalidateQueries({ queryKey: ['mod-config', slug] });
    },
  });
}
