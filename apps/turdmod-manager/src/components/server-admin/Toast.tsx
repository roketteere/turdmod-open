import type { ReactNode } from 'react';

interface ToastProps {
  kind: 'ok' | 'warn';
  children: ReactNode;
  onClose: () => void;
}

export function Toast({ kind, children, onClose }: ToastProps) {
  return (
    <div
      role="status"
      className={[
        'flex items-start justify-between gap-3 rounded border px-4 py-3 text-sm',
        kind === 'ok'
          ? 'border-turd-green/40 bg-turd-green/10 text-turd-green'
          : 'border-turd-mustard/40 bg-turd-mustard/10 text-turd-mustard-bright',
      ].join(' ')}
    >
      <span>{children}</span>
      <button
        type="button"
        onClick={onClose}
        aria-label="Dismiss"
        className="shrink-0 opacity-60 transition-opacity hover:opacity-100"
      >
        ✕
      </button>
    </div>
  );
}
