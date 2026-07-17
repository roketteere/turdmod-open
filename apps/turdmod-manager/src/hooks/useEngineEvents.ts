// useEngineEvents — subscribes to the engine pipe's event firehose
// (rebroadcast as Tauri `engine://event` by src-tauri/src/engine_events.rs)
// and maintains a bounded ring buffer of received events.
//
// The Rust listener keeps a long-lived pipe connection open as soon as the
// engine is reachable; we just attach to the Tauri channel here. Multiple
// components calling this hook safely share the buffer because each one
// gets its own listener, but the firehose is cheap (one Tauri emit per
// event from one place).

import { useEffect, useMemo, useRef, useState } from 'react';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export interface EngineEvent {
  /** Event kind, e.g. "chat", "login", "vehicleSpawned". */
  event: string;
  /** Per-event payload — shape is event-specific. */
  data: unknown;
  /** ms since epoch when the Rust listener received the frame. */
  receivedAtMs: number;
}

const CHANNEL = 'engine://event';

// Default ring size — enough to scrub recent activity without unbounded
// memory growth on a chat-heavy server.
const DEFAULT_BUFFER_SIZE = 500;

export interface UseEngineEventsOptions {
  /** Maximum events to retain in the ring buffer. Default 500. */
  bufferSize?: number;
  /** If true, the listener stops appending new events (still subscribed
   *  underneath so the Rust side keeps flowing — we just drop on the
   *  React side). Useful for a Pause toggle. */
  paused?: boolean;
  /** Optional list of event kinds to retain. Anything not in the list is
   *  dropped on the React side. `null` / `undefined` = retain all. */
  filter?: string[] | null;
}

export interface UseEngineEventsReturn {
  events: EngineEvent[];
  clear: () => void;
  /** Per-kind counts across the lifetime of this hook instance. Useful for
   *  filter-chip counters. NOT bounded by the ring buffer — total events
   *  seen since mount. */
  countsByKind: Record<string, number>;
  totalSeen: number;
}

export function useEngineEvents(
  options?: UseEngineEventsOptions,
): UseEngineEventsReturn {
  const bufferSize = options?.bufferSize ?? DEFAULT_BUFFER_SIZE;
  const paused = options?.paused ?? false;
  const filter = options?.filter ?? null;

  const [events, setEvents] = useState<EngineEvent[]>([]);
  const [totalSeen, setTotalSeen] = useState(0);
  const [countsByKind, setCountsByKind] = useState<Record<string, number>>({});

  // Refs so the listener closure (created once) sees the latest values.
  const pausedRef = useRef(paused);
  const filterRef = useRef(filter);
  const bufferSizeRef = useRef(bufferSize);
  useEffect(() => { pausedRef.current = paused; }, [paused]);
  useEffect(() => { filterRef.current = filter; }, [filter]);
  useEffect(() => { bufferSizeRef.current = bufferSize; }, [bufferSize]);

  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    let cancelled = false;
    (async () => {
      unlisten = await listen<EngineEvent>(CHANNEL, (e) => {
        const payload = e.payload;
        // Always count — filter only affects the ring buffer.
        setTotalSeen((n) => n + 1);
        setCountsByKind((prev) => ({
          ...prev,
          [payload.event]: (prev[payload.event] ?? 0) + 1,
        }));

        if (pausedRef.current) return;
        const f = filterRef.current;
        if (f && !f.includes(payload.event)) return;

        setEvents((prev) => {
          const next = prev.length >= bufferSizeRef.current
            ? prev.slice(prev.length - bufferSizeRef.current + 1)
            : prev.slice();
          next.push(payload);
          return next;
        });
      });
      if (cancelled && unlisten) {
        unlisten();
        unlisten = null;
      }
    })();
    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, []);

  return useMemo(
    () => ({
      events,
      clear: () => setEvents([]),
      countsByKind,
      totalSeen,
    }),
    [events, countsByKind, totalSeen],
  );
}
