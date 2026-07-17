interface PriceBadgeProps {
  isPremium: boolean;
  isPremiumBundled: boolean;
  priceCents: number | null;
}

export function PriceBadge({
  isPremium,
  isPremiumBundled,
  priceCents,
}: PriceBadgeProps) {
  if (isPremiumBundled) {
    return (
      <span className="rounded border border-turd-mustard/60 px-2 py-0.5 font-mono text-[10px] uppercase tracking-wider text-turd-mustard-bright">
        Premium
      </span>
    );
  }
  if (isPremium && priceCents != null) {
    return (
      <span className="rounded bg-turd-bg-soft px-2 py-0.5 font-mono text-[10px] uppercase tracking-wider text-turd-mustard-bright">
        ${(priceCents / 100).toFixed(2)}
      </span>
    );
  }
  return (
    <span className="rounded bg-turd-bg-soft px-2 py-0.5 font-mono text-[10px] uppercase tracking-wider text-turd-green">
      Free
    </span>
  );
}
