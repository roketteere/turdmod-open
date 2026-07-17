// TanStack Query hooks for the Dump Management page.
//
// - useDumpStatus: polls dump_status every 10s for the status pane.
// - useDumpUpdateCheck: one-shot check at session start; the App-level
//   banner uses it without polling.
// - useDumpRunPhase / useDumpRunAll: mutation hooks; each subscribes to
//   `dump://log` events for the duration of the call and returns the
//   accumulated log buffer plus the exit code.

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  type DiffReport,
  type DumpLogLine,
  type DumpStatus,
  type DumpUpdateCheck,
  type PhaseId,
  dumpCheckUpdates,
  dumpDiffSummary,
  dumpExtractAes,
  dumpListBuilds,
  dumpOpenFolder,
  dumpRunAll,
  dumpRunPhase,
  dumpStatus,
  onDumpLog,
} from '../lib/tauri-dump';
import { formatTs } from '../lib/format-ts';

/** Streamed log line + the wall-clock time it arrived in the UI.
 *
 *  `statusGroup` is set when the underlying scumdump line matched the
 *  `[STATUS|<groupId>] …` heartbeat convention. The buffer treats
 *  these specially: a new entry with the same groupId REPLACES the
 *  previous one in-place (no jump in row order, no append). Polling
 *  loops can thus emit progress without flooding the log.
 *
 *  When the heartbeat payload after the prefix is JSON, additional
 *  structured fields are attached (phaseLabel, percent, details) so
 *  a dedicated "Current phase" card can render a progress bar +
 *  expandable details. Free-form text heartbeats still work — only
 *  `line` is populated in that case. */
export type BufferedLogLine = DumpLogLine & {
  ts: string;
  tsMs: number;
  statusGroup?: string;
  phaseLabel?: string;
  percent?: number;
  details?: Record<string, string | number>;
};

const STATUS_LINE_RE = /^\[STATUS\|([^\]]+)\]\s*(.*)$/;

interface ParsedStatus {
  statusGroup: string;
  line: string;
  phaseLabel?: string;
  percent?: number;
  details?: Record<string, string | number>;
}

function parseStatusLine(raw: DumpLogLine): ParsedStatus | null {
  const m = STATUS_LINE_RE.exec(raw.line);
  if (!m) return null;
  const groupId = m[1]!;
  const payload = (m[2] ?? '').trim();

  // Try JSON first — newer scumdump status lines carry structured
  // progress. Fall back to plain text for backward compatibility
  // and for any external tool that emits the prefix without a
  // structured body.
  if (payload.startsWith('{')) {
    try {
      const j = JSON.parse(payload) as {
        text?: string;
        phaseLabel?: string;
        percent?: number;
        details?: Record<string, string | number>;
      };
      return {
        statusGroup: groupId,
        line: j.text ?? '',
        phaseLabel: j.phaseLabel,
        percent: typeof j.percent === 'number' ? j.percent : undefined,
        details: j.details,
      };
    } catch {
      /* fall through to text */
    }
  }
  return { statusGroup: groupId, line: payload };
}

const STATUS_KEY = ['dump', 'status'] as const;
const UPDATE_CHECK_KEY = ['dump', 'update-check'] as const;

export function useDumpStatus() {
  return useQuery<DumpStatus>({
    queryKey: STATUS_KEY,
    queryFn: dumpStatus,
    refetchInterval: 10_000,
    staleTime: 5_000,
  });
}

export function useDumpUpdateCheck() {
  return useQuery<DumpUpdateCheck>({
    queryKey: UPDATE_CHECK_KEY,
    queryFn: dumpCheckUpdates,
    // Single check at session start; user can manually refetch.
    refetchInterval: false,
    staleTime: Infinity,
  });
}

/**
 * Streaming log buffer for an in-flight phase run. Subscribes to
 * `dump://log` events for the lifetime of the hook. The buffer caps
 * at 2000 lines so a runaway phase doesn't blow memory.
 */
/** TTL after which a heartbeat is considered stale — used by the
 *  Current Phase card to auto-dim/hide once a phase finishes.
 *  Bumped from 10 s to 60 s 2026-05-21 so the card doesn't blink
 *  in and out during brief gaps between heartbeats (e.g. the
 *  wait→stable transition or buffered-stdout hitches through
 *  the pnpm → tsx → tokio pipe). */
const STATUS_TTL_MS = 60_000;

export function useDumpLogBuffer(maxLines = 2000) {
  const [lines, setLines] = useState<BufferedLogLine[]>([]);
  const unlistenRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    let active = true;
    onDumpLog((line) => {
      if (!active) return;
      // Capture arrival timestamp here — the gap between server emit
      // and JS receipt is sub-millisecond for in-process Tauri events,
      // so this is effectively the event's actual time.
      const tsMs = Date.now();
      const tsLabel = formatTs(new Date(tsMs));
      const status = parseStatusLine(line);
      const stamped: BufferedLogLine = status
        ? {
            ...line,
            line: status.line,
            statusGroup: status.statusGroup,
            phaseLabel: status.phaseLabel,
            percent: status.percent,
            details: status.details,
            ts: tsLabel,
            tsMs,
          }
        : { ...line, ts: tsLabel, tsMs };

      setLines((prev) => {
        // Heartbeat replacement: if this is a status line whose
        // groupId already exists in the buffer, replace that entry
        // in place (keep its position, update content + ts). Else
        // append normally.
        if (stamped.statusGroup) {
          const idx = prev.findIndex(
            (e) => e.statusGroup === stamped.statusGroup,
          );
          if (idx !== -1) {
            const next = prev.slice();
            next[idx] = stamped;
            return next;
          }
        }
        const next = prev.concat(stamped);
        return next.length > maxLines ? next.slice(next.length - maxLines) : next;
      });
    }).then((unlisten) => {
      if (!active) {
        unlisten();
        return;
      }
      unlistenRef.current = unlisten;
    });
    return () => {
      active = false;
      unlistenRef.current?.();
      unlistenRef.current = null;
    };
  }, [maxLines]);

  const clear = useCallback(() => setLines([]), []);
  return { lines, clear };
}

/** Resolves the most-recently-updated status line that's still
 *  within the TTL — used to drive the Current Phase card.
 *
 *  The card auto-hides when no fresh heartbeat exists. We re-tick
 *  every 1 s to ensure the card disappears even when no new lines
 *  arrive (the buffer alone wouldn't trigger a re-render). */
export function useDumpActiveStatus(
  lines: BufferedLogLine[],
): BufferedLogLine | null {
  const [tick, setTick] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setTick((t) => (t + 1) % 1_000_000), 1000);
    return () => clearInterval(id);
  }, []);
  return useMemo(() => {
    // tick is read so memo re-runs each second
    void tick;
    const now = Date.now();
    let best: BufferedLogLine | null = null;
    for (let i = lines.length - 1; i >= 0; i -= 1) {
      const l = lines[i]!;
      if (!l.statusGroup) continue;
      if (now - l.tsMs > STATUS_TTL_MS) continue;
      if (!best || l.tsMs > best.tsMs) best = l;
    }
    return best;
  }, [lines, tick]);
}

export function useDumpRunPhase() {
  const qc = useQueryClient();
  return useMutation<number, string, PhaseId>({
    mutationFn: (phase) => dumpRunPhase(phase),
    onSettled: () => {
      qc.invalidateQueries({ queryKey: STATUS_KEY });
      qc.invalidateQueries({ queryKey: UPDATE_CHECK_KEY });
    },
  });
}

export function useDumpRunAll() {
  const qc = useQueryClient();
  return useMutation<number, string, void>({
    mutationFn: () => dumpRunAll(),
    onSettled: () => {
      qc.invalidateQueries({ queryKey: STATUS_KEY });
      qc.invalidateQueries({ queryKey: UPDATE_CHECK_KEY });
    },
  });
}

export function useDumpExtractAes() {
  const qc = useQueryClient();
  return useMutation<number, string, void>({
    mutationFn: () => dumpExtractAes(),
    onSettled: () => {
      qc.invalidateQueries({ queryKey: STATUS_KEY });
    },
  });
}

export function useDumpOpenFolder() {
  return useMutation<string, string, void>({
    mutationFn: () => dumpOpenFolder(),
  });
}

/**
 * Read the diff report for the latest build (or a specific build).
 * Returns `null` if no diff has been computed yet — page should
 * render an empty state rather than treat null as an error.
 */
export function useDumpDiffSummary(build?: string) {
  return useQuery<DiffReport | null>({
    queryKey: ['dump', 'diff', build ?? 'latest'],
    queryFn: () => dumpDiffSummary(build),
    refetchInterval: 30_000,
    staleTime: 15_000,
  });
}

// List all extracted build IDs (newest first) for the diff card's
// build-picker dropdown. Reads data/extracted/v*/ directory names
// via dump_list_builds Tauri command.
export function useDumpListBuilds() {
  return useQuery<string[]>({
    queryKey: ['dump', 'builds'],
    queryFn: () => dumpListBuilds(),
    refetchInterval: 60_000,
    staleTime: 30_000,
  });
}
