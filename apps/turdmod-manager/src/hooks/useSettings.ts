import {
  useMutation,
  useQuery,
  useQueryClient,
  type UseMutationResult,
} from '@tanstack/react-query';
import {
  settingsGetAll,
  settingsSetPartial,
  type ManagerSettings,
  type SettingsPatch,
} from '../lib/tauri-settings';

const SETTINGS_KEY = ['settings'] as const;

export function useSettings() {
  return useQuery<ManagerSettings, Error>({
    queryKey: SETTINGS_KEY,
    queryFn: settingsGetAll,
    staleTime: 5_000,
  });
}

export function useUpdateSettings(): UseMutationResult<
  ManagerSettings,
  Error,
  SettingsPatch
> {
  const qc = useQueryClient();
  return useMutation<ManagerSettings, Error, SettingsPatch>({
    mutationFn: (patch) => settingsSetPartial(patch),
    onSuccess: (updated) => {
      qc.setQueryData(SETTINGS_KEY, updated);
    },
    onSettled: () => {
      qc.invalidateQueries({ queryKey: SETTINGS_KEY });
    },
  });
}