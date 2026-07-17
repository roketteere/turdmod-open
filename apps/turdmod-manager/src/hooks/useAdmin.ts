import {
  useMutation,
  useQuery,
  useQueryClient,
  type UseMutationResult,
} from '@tanstack/react-query';
import {
  type AdminFilename,
  type ServerSettingsForm,
  type SettingsPatch,
  parseServerSettings,
  parseUserList,
  readAdminFile,
  saveServerSettingsPartial,
  serializeUserList,
  writeAdminFile,
} from '../lib/tauri-admin';

export function useAdminFile(serverId: string | null, filename: AdminFilename) {
  return useQuery<string, Error>({
    queryKey: ['admin-file', serverId, filename],
    queryFn: () =>
      serverId
        ? readAdminFile(serverId, filename)
        : Promise.reject(new Error('no server')),
    enabled: Boolean(serverId),
  });
}

export interface WriteAdminFileVars {
  serverId: string;
  filename: AdminFilename;
  contents: string;
}

export function useWriteAdminFile(): UseMutationResult<void, Error, WriteAdminFileVars> {
  const qc = useQueryClient();
  return useMutation<void, Error, WriteAdminFileVars>({
    mutationFn: ({ serverId, filename, contents }) =>
      writeAdminFile(serverId, filename, contents),
    onSuccess: (_d, { serverId, filename }) => {
      qc.invalidateQueries({ queryKey: ['admin-file', serverId, filename] });
    },
  });
}

export interface SaveUserListVars {
  serverId: string;
  filename: AdminFilename;
  ids: string[];
}

export function useSaveUserList(): UseMutationResult<void, Error, SaveUserListVars> {
  const qc = useQueryClient();
  return useMutation<void, Error, SaveUserListVars>({
    mutationFn: ({ serverId, filename, ids }) =>
      writeAdminFile(serverId, filename, serializeUserList(ids)),
    onSuccess: (_d, { serverId, filename }) => {
      qc.invalidateQueries({ queryKey: ['admin-file', serverId, filename] });
    },
  });
}

export function useUserList(serverId: string | null, filename: AdminFilename) {
  const raw = useAdminFile(serverId, filename);
  return {
    ...raw,
    ids: raw.data ? parseUserList(raw.data) : undefined,
  };
}

export function useServerSettingsForm(serverId: string | null) {
  return useQuery<ServerSettingsForm, Error>({
    queryKey: ['server-settings-form', serverId],
    queryFn: () =>
      serverId
        ? parseServerSettings(serverId)
        : Promise.reject(new Error('no server')),
    enabled: Boolean(serverId),
  });
}

export interface SaveSettingsVars {
  serverId: string;
  patch: SettingsPatch;
}

export function useSaveServerSettings(): UseMutationResult<void, Error, SaveSettingsVars> {
  const qc = useQueryClient();
  return useMutation<void, Error, SaveSettingsVars>({
    mutationFn: ({ serverId, patch }) => saveServerSettingsPartial(serverId, patch),
    onSuccess: (_d, { serverId }) => {
      qc.invalidateQueries({ queryKey: ['server-settings-form', serverId] });
      qc.invalidateQueries({
        queryKey: ['admin-file', serverId, 'ServerSettings.ini'],
      });
    },
  });
}
