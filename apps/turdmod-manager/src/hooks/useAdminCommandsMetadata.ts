import { useQuery } from '@tanstack/react-query';
import { useMemo } from 'react';
import { engineRpc } from '../lib/tauri-engine';

// Live admin-command metadata pulled from the bridge's dumpAdminCommands
// RPC. The bridge walks GUObjectArray for every Blueprint class whose
// SuperStruct chain includes AdminCommand_*, then emits each one's CDO
// with the populated _verb / _argumentDescriptions / _numberOfRequiredArguments
// defaults. See `reference_scum_admin_command_classes` in memory for the
// underlying engine pattern.
//
// We cache results in localStorage for 24 hours — the bridge dump takes
// ~200 ms (scans ~1.5 M UObjects) but the metadata only changes when SCUM
// itself updates its admin-command BP set, which is rare.

const CACHE_KEY = 'turdmod.adminCommandsMetadata.v1';
const STALE_MS = 24 * 60 * 60 * 1000;

export interface BridgeArg {
  name: string;                      // human display name from SCUM autocomplete, e.g. "Currency Type"
  description: string;               // per-arg help text, e.g. "Player whose balance to change"
  dataType: string;                  // AdminCommandArgumentDataType_String / Numeric / Bool
  completionClass: string | null;    // e.g. "Player", "Item", "PrisonerBodyMuscleGroup_C"
  completionValues: string[];        // populated only for _Constant-derived completion classes
}

export interface BridgeCommandSpec {
  verb: string;
  class: string;                     // BP class name like "SetCurrencyBalance_C"
  description: string;               // verb-level summary, e.g. "Adds specified currency amount..."
  numRequired: number;
  args: BridgeArg[];
}

interface BridgeResponse {
  ok: boolean;
  scanned: number;
  adminCommands: number;
  emitted: number;
  commands: BridgeCommandSpec[];
}

function loadCache(): BridgeResponse | null {
  try {
    const raw = localStorage.getItem(CACHE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as { ts: number; data: BridgeResponse };
    if (Date.now() - parsed.ts > STALE_MS) return null;
    return parsed.data;
  } catch {
    return null;
  }
}

function saveCache(data: BridgeResponse) {
  try {
    localStorage.setItem(CACHE_KEY, JSON.stringify({ ts: Date.now(), data }));
  } catch {
    // localStorage full / disabled — silently skip; we'll just refetch next session.
  }
}

export function useAdminCommandsMetadata() {
  const seeded = useMemo(() => loadCache(), []);

  const query = useQuery<BridgeResponse, Error>({
    queryKey: ['engine', 'dumpAdminCommands'],
    queryFn: async () => {
      const data = await engineRpc<BridgeResponse>('dumpAdminCommands', {});
      saveCache(data);
      return data;
    },
    initialData: seeded ?? undefined,
    staleTime: STALE_MS,
    retry: false,
  });

  const byVerb = useMemo(() => {
    const map = new Map<string, BridgeCommandSpec>();
    for (const cmd of query.data?.commands ?? []) {
      map.set(cmd.verb, cmd);
    }
    return map;
  }, [query.data]);

  return {
    byVerb,
    count: query.data?.commands?.length ?? 0,
    isLoading: query.isLoading,
    isError: query.isError,
    error: query.error,
    refetch: query.refetch,
    hasData: byVerb.size > 0,
  };
}
