// Active install target hooks. Switching the target invalidates every
// install-path-dependent query so Library, mod state, config, and detect
// results all re-fetch automatically.

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  getActiveTarget,
  listDetectedInstalls,
  setActiveTarget,
  type DetectedInstalls,
  type InstallTarget,
} from '../lib/tauri-target';

export const DETECTED_INSTALLS_KEY = ['detected-installs'] as const;
export const ACTIVE_TARGET_KEY = ['active-target'] as const;

const INSTALL_DEPENDENT_KEY_PREFIXES = [
  'installed',
  'mod-state',
  'mod-manifest',
  'mod-config',
  'detect-scum',
];

export function useDetectedInstalls() {
  return useQuery<DetectedInstalls, Error>({
    queryKey: DETECTED_INSTALLS_KEY,
    queryFn: listDetectedInstalls,
    staleTime: 60_000,
  });
}

export function useActiveTarget() {
  return useQuery<InstallTarget, Error>({
    queryKey: ACTIVE_TARGET_KEY,
    queryFn: getActiveTarget,
  });
}

export function useSetActiveTarget() {
  const qc = useQueryClient();
  return useMutation<void, Error, InstallTarget>({
    mutationFn: (target) => setActiveTarget(target),
    onSuccess: (_data, target) => {
      qc.setQueryData<InstallTarget>(ACTIVE_TARGET_KEY, target);
      qc.invalidateQueries({
        predicate: (query) => {
          const [head] = query.queryKey;
          return typeof head === 'string' && INSTALL_DEPENDENT_KEY_PREFIXES.includes(head);
        },
      });
    },
  });
}
