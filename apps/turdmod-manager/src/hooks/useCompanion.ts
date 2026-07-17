import {
  useMutation,
  useQuery,
  useQueryClient,
  type UseMutationResult,
} from '@tanstack/react-query';
import {
  companionGetStatus,
  companionRestart,
  companionStart,
  companionStop,
  type CompanionStatus,
} from '../lib/tauri-companion';

const STATUS_KEY = ['companion', 'status'] as const;

export function useCompanionStatus() {
  return useQuery<CompanionStatus, Error>({
    queryKey: STATUS_KEY,
    queryFn: companionGetStatus,
    refetchInterval: 1000,
    // Don't throw on error — a dead companion is expected state.
    retry: false,
  });
}

export function useCompanionStart(): UseMutationResult<CompanionStatus, Error, void> {
  const qc = useQueryClient();
  return useMutation<CompanionStatus, Error, void>({
    mutationFn: () => companionStart(),
    onSuccess: (status) => {
      qc.setQueryData(STATUS_KEY, status);
    },
    onSettled: () => {
      qc.invalidateQueries({ queryKey: STATUS_KEY });
    },
  });
}

export function useCompanionStop(): UseMutationResult<CompanionStatus, Error, void> {
  const qc = useQueryClient();
  return useMutation<CompanionStatus, Error, void>({
    mutationFn: () => companionStop(),
    onSuccess: (status) => {
      qc.setQueryData(STATUS_KEY, status);
    },
    onSettled: () => {
      qc.invalidateQueries({ queryKey: STATUS_KEY });
    },
  });
}

export function useCompanionRestart(): UseMutationResult<CompanionStatus, Error, void> {
  const qc = useQueryClient();
  return useMutation<CompanionStatus, Error, void>({
    mutationFn: () => companionRestart(),
    onSuccess: (status) => {
      qc.setQueryData(STATUS_KEY, status);
    },
    onSettled: () => {
      qc.invalidateQueries({ queryKey: STATUS_KEY });
    },
  });
}