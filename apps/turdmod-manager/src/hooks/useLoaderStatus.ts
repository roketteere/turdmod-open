import {
  useMutation,
  useQuery,
  useQueryClient,
  type UseMutationResult,
  type UseQueryResult,
} from '@tanstack/react-query';
import { enqueuePanel, loaderPing, type EnqueuePanelResult, type LoaderPing } from '../lib/tauri-loader';

const STATUS_KEY = ['loader', 'status'] as const;

/** Polls the in-game loader's ping endpoint every 5s. Resolves with the
 *  ping body when the loader is online; the query goes to error state
 *  when the loader isn't injected / the pipe isn't reachable. */
export function useLoaderStatus(): UseQueryResult<LoaderPing, Error> {
  return useQuery<LoaderPing, Error>({
    queryKey: STATUS_KEY,
    queryFn: loaderPing,
    refetchInterval: 5000,
    retry: false,
  });
}

/** Mutation: send one panel into the in-game render queue. */
export function useEnqueuePanel(): UseMutationResult<
  EnqueuePanelResult,
  Error,
  Record<string, unknown>
> {
  const qc = useQueryClient();
  return useMutation<EnqueuePanelResult, Error, Record<string, unknown>>({
    mutationFn: (payload) => enqueuePanel(payload),
    // A successful enqueue is a strong liveness signal — fold it back into
    // the status cache so the badge flips green even if the next poll is
    // still 4s away.
    onSuccess: () => { qc.invalidateQueries({ queryKey: STATUS_KEY }); },
  });
}
