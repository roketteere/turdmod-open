import {
  useMutation,
  useQuery,
  useQueryClient,
  type UseMutationResult,
} from '@tanstack/react-query';
import {
  engineGetStatus,
  engineInstall,
  engineStart,
  engineStop,
  type EnginePaths,
  type EngineStatus,
  type InstallReport,
} from '../lib/tauri-engine';

const STATUS_KEY = ['engine', 'status'] as const;

export function useEngineStatus() {
  return useQuery<EngineStatus, Error>({
    queryKey: STATUS_KEY,
    queryFn: engineGetStatus,
    refetchInterval: 1000,
    retry: false,
  });
}

export interface EngineStartArgs {
  serverInstall:   string;
  paths?:          EnginePaths;
  skipSafetyCheck?: boolean;
}

export function useEngineStart(): UseMutationResult<EngineStatus, Error, EngineStartArgs> {
  const qc = useQueryClient();
  return useMutation<EngineStatus, Error, EngineStartArgs>({
    mutationFn: ({ serverInstall, paths, skipSafetyCheck }) =>
      engineStart(serverInstall, { paths, skipSafetyCheck }),
    onSuccess: (status) => { qc.setQueryData(STATUS_KEY, status); },
    onSettled: () => { qc.invalidateQueries({ queryKey: STATUS_KEY }); },
  });
}

export function useEngineStop(): UseMutationResult<EngineStatus, Error, void> {
  const qc = useQueryClient();
  return useMutation<EngineStatus, Error, void>({
    mutationFn: () => engineStop(),
    onSuccess: (status) => { qc.setQueryData(STATUS_KEY, status); },
    onSettled: () => { qc.invalidateQueries({ queryKey: STATUS_KEY }); },
  });
}

export interface EngineInstallArgs {
  serverInstall: string;
  paths?:        EnginePaths;
}

export function useEngineInstall(): UseMutationResult<InstallReport, Error, EngineInstallArgs> {
  return useMutation<InstallReport, Error, EngineInstallArgs>({
    mutationFn: ({ serverInstall, paths }) => engineInstall(serverInstall, paths),
  });
}
