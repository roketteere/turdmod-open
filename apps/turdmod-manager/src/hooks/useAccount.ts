import { useCallback } from 'react';
import {
  useMutation,
  useQuery,
  useQueryClient,
  type UseMutationResult,
} from '@tanstack/react-query';
import {
  fetchAccountStatus,
  openInBrowser,
  pasteToken,
  signOut,
  startSignIn,
  type AccountStatus,
  type AccountUser,
} from '../lib/tauri-account';

const ACCOUNT_KEY = ['account', 'status'] as const;

export type AccountStatusKind = 'unknown' | 'signed-out' | 'signed-in';

export interface UseAccountResult {
  status: AccountStatusKind;
  user: AccountUser | null;
  error: Error | null;
  isPending: boolean;
  isRefreshing: boolean;
  signIn: () => Promise<void>;
  pasteToken: UseMutationResult<AccountStatus, Error, string>;
  signOut: () => Promise<void>;
  refresh: () => Promise<void>;
}

export function useAccount(): UseAccountResult {
  const qc = useQueryClient();

  const query = useQuery<AccountStatus, Error>({
    queryKey: ACCOUNT_KEY,
    queryFn: fetchAccountStatus,
    staleTime: 60_000,
    retry: 1,
  });

  const pasteMutation = useMutation<AccountStatus, Error, string>({
    mutationFn: (token) => pasteToken(token),
    onSuccess: (next) => {
      qc.setQueryData(ACCOUNT_KEY, next);
    },
  });

  const signOutMutation = useMutation<void, Error, void>({
    mutationFn: () => signOut(),
    onSuccess: () => {
      qc.setQueryData<AccountStatus>(ACCOUNT_KEY, { status: 'signed-out' });
    },
  });

  const signIn = useCallback(async () => {
    const info = await startSignIn();
    await openInBrowser(info.url);
  }, []);

  const refresh = useCallback(async () => {
    await qc.invalidateQueries({ queryKey: ACCOUNT_KEY });
  }, [qc]);

  let status: AccountStatusKind;
  let user: AccountUser | null = null;
  if (query.isPending && !query.data) {
    status = 'unknown';
  } else if (query.data?.status === 'signed-in') {
    status = 'signed-in';
    user = query.data.user;
  } else {
    status = 'signed-out';
  }

  return {
    status,
    user,
    error: query.error ?? null,
    isPending: query.isPending,
    isRefreshing: query.isFetching && !query.isPending,
    signIn,
    pasteToken: pasteMutation,
    signOut: () => signOutMutation.mutateAsync(),
    refresh,
  };
}
