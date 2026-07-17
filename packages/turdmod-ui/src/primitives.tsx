import type { ReactNode } from 'react';

// Low-level visual primitives. The styling assumes Tailwind classes
// that match the consumer app's `turd-*` color palette (the
// tailwind.config.ts each app ships with). If you fork this package
// for a different design system, edit the className strings here.

export function PageHeader({
  title,
  subtitle,
  actions,
}: {
  title: string;
  subtitle?: string;
  actions?: ReactNode;
}) {
  return (
    <div className="mb-4 flex items-baseline justify-between gap-4">
      <div>
        <h1 className="font-display text-lg tracking-widest text-turd-mustard-bright">
          {title}
        </h1>
        {subtitle && (
          <p className="mt-1 text-xs text-turd-cream-dim">{subtitle}</p>
        )}
      </div>
      {actions && <div className="flex shrink-0 gap-2">{actions}</div>}
    </div>
  );
}

export function Section({
  children,
  title,
  className = '',
}: {
  children: ReactNode;
  title?: string;
  className?: string;
}) {
  return (
    <section
      className={`rounded border border-turd-bronze/30 bg-turd-bg-mid/40 p-4 ${className}`}
    >
      {title && (
        <h3 className="mb-2 text-xs uppercase tracking-wider text-turd-cream-dim/60">
          {title}
        </h3>
      )}
      {children}
    </section>
  );
}

export function Field({
  label,
  value,
  mono = false,
}: {
  label: string;
  value?: ReactNode;
  mono?: boolean;
}) {
  return (
    <div className="text-xs">
      <span className="uppercase tracking-wider text-turd-cream-dim/60">
        {label}:
      </span>{' '}
      <span className={mono ? 'font-mono text-turd-cream' : 'text-turd-cream'}>
        {value ?? '—'}
      </span>
    </div>
  );
}

export function EmptyState({
  message,
  action,
}: {
  message: string;
  action?: ReactNode;
}) {
  return (
    <div className="flex flex-col items-center gap-3 rounded border border-turd-bronze/30 bg-turd-bg-mid/40 p-8 text-center">
      <p className="text-sm text-turd-cream-dim">{message}</p>
      {action}
    </div>
  );
}

export function Button({
  children,
  onClick,
  disabled = false,
  variant = 'primary',
  type = 'button',
}: {
  children: ReactNode;
  onClick?: () => void;
  disabled?: boolean;
  variant?: 'primary' | 'secondary' | 'danger';
  type?: 'button' | 'submit';
}) {
  const variantClass =
    variant === 'primary'
      ? 'bg-turd-mustard-bright text-turd-bg-deep hover:bg-turd-mustard'
      : variant === 'danger'
        ? 'bg-red-600/80 text-turd-cream hover:bg-red-600'
        : 'bg-turd-bg-soft text-turd-cream hover:bg-turd-bg-soft/80';
  return (
    <button
      type={type}
      disabled={disabled}
      onClick={onClick}
      className={`rounded px-3 py-1.5 text-sm font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-40 ${variantClass}`}
    >
      {children}
    </button>
  );
}
