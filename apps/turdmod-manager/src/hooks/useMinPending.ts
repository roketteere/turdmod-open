import { useEffect, useRef, useState } from 'react';

// Returns a "pending" flag that stays true for at least `minMs` after
// the underlying mutation flips. Without this the spinner for fast
// operations (file copy, ~50ms) flashes too quickly to see.
//
// Usage:
//   const installVisuallyPending = useMinPending(install.isPending, 500);
export function useMinPending(realPending: boolean, minMs = 500): boolean {
  const [visuallyPending, setVisuallyPending] = useState(realPending);
  const startRef = useRef<number | null>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (realPending) {
      startRef.current = Date.now();
      if (timerRef.current) {
        clearTimeout(timerRef.current);
        timerRef.current = null;
      }
      setVisuallyPending(true);
      return;
    }
    // Flipped to not-pending. Hold the visual state until minMs has passed.
    const elapsed = startRef.current ? Date.now() - startRef.current : minMs;
    const remaining = Math.max(0, minMs - elapsed);
    if (remaining === 0) {
      setVisuallyPending(false);
    } else {
      timerRef.current = setTimeout(() => {
        setVisuallyPending(false);
        timerRef.current = null;
      }, remaining);
    }
    return () => {
      if (timerRef.current) {
        clearTimeout(timerRef.current);
        timerRef.current = null;
      }
    };
  }, [realPending, minMs]);

  return visuallyPending;
}
