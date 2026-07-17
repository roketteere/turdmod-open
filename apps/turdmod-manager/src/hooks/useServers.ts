import {
  useMutation,
  useQuery,
  useQueryClient,
  type UseMutationResult,
} from '@tanstack/react-query';
import {
  addServer,
  knownLogs,
  listRemoteMods,
  listServers,
  rconSend,
  removeRemoteMod,
  removeServer,
  tailLog,
  testServer,
  uploadModToServer,
  type RemoteMod,
  type RemotePath,
  type ServerProbeResult,
  type ServerProfile,
  type ServerProfileInput,
} from '../lib/tauri-servers';

export function useServers() {
  return useQuery<ServerProfile[], Error>({
    queryKey: ['servers'],
    queryFn: listServers,
  });
}

export interface AddServerVars {
  profile: ServerProfileInput;
  secretSsh: string | null;
  secretRcon: string | null;
}

export function useAddServer(): UseMutationResult<
  ServerProfile,
  Error,
  AddServerVars
> {
  const qc = useQueryClient();
  return useMutation<ServerProfile, Error, AddServerVars>({
    mutationFn: ({ profile, secretSsh, secretRcon }) =>
      addServer(profile, secretSsh, secretRcon),
    onSettled: () => {
      qc.invalidateQueries({ queryKey: ['servers'] });
    },
  });
}

export function useRemoveServer(): UseMutationResult<void, Error, string> {
  const qc = useQueryClient();
  return useMutation<void, Error, string>({
    mutationFn: (id) => removeServer(id),
    onSettled: () => {
      qc.invalidateQueries({ queryKey: ['servers'] });
    },
  });
}

export function useTestServer(): UseMutationResult<
  ServerProbeResult,
  Error,
  string
> {
  const qc = useQueryClient();
  return useMutation<ServerProbeResult, Error, string>({
    mutationFn: (id) => testServer(id),
    onSuccess: (result, id) => {
      qc.setQueryData(['server-probe', id], result);
    },
  });
}

export function useServerProbe(id: string | null) {
  return useQuery<ServerProbeResult | null, Error>({
    queryKey: ['server-probe', id],
    queryFn: () => (id ? testServer(id) : Promise.resolve(null)),
    enabled: false,
    staleTime: Infinity,
  });
}

export function useRemoteMods(id: string | null) {
  return useQuery<RemoteMod[], Error>({
    queryKey: ['remote-mods', id],
    queryFn: () => (id ? listRemoteMods(id) : Promise.resolve([])),
    enabled: Boolean(id),
  });
}

export interface PushModVars {
  id: string;
  slug: string;
}

export function usePushMod(): UseMutationResult<RemotePath, Error, PushModVars> {
  const qc = useQueryClient();
  return useMutation<RemotePath, Error, PushModVars>({
    mutationFn: ({ id, slug }) => uploadModToServer(id, slug),
    onSuccess: (_d, { id }) => {
      qc.invalidateQueries({ queryKey: ['remote-mods', id] });
    },
  });
}

export function useRemoveRemoteMod(): UseMutationResult<void, Error, PushModVars> {
  const qc = useQueryClient();
  return useMutation<void, Error, PushModVars>({
    mutationFn: ({ id, slug }) => removeRemoteMod(id, slug),
    onSuccess: (_d, { id }) => {
      qc.invalidateQueries({ queryKey: ['remote-mods', id] });
    },
  });
}

export interface TailLogVars {
  id: string;
  log: string;
  lines: number;
}

export function useTailLog(vars: TailLogVars | null) {
  return useQuery<string[], Error>({
    queryKey: ['log-tail', vars?.id, vars?.log, vars?.lines],
    queryFn: () =>
      vars ? tailLog(vars.id, vars.log, vars.lines) : Promise.resolve([]),
    enabled: Boolean(vars?.id),
  });
}

export interface RconVars {
  id: string;
  command: string;
}

export function useRcon(): UseMutationResult<string, Error, RconVars> {
  return useMutation<string, Error, RconVars>({
    mutationFn: ({ id, command }) => rconSend(id, command),
  });
}

export function useKnownLogs() {
  return useQuery<string[], Error>({
    queryKey: ['known-logs'],
    queryFn: knownLogs,
    staleTime: Infinity,
  });
}
