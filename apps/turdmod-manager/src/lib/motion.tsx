import { motion, AnimatePresence, type Variants, type Transition } from 'framer-motion';
import type { ReactNode } from 'react';

// Shared motion vocabulary for the "refined glass cockpit" look. Springs
// echo the CSS --ease-spring/--ease-out-soft tokens so CSS and JS motion
// feel like one system. Keep durations short — elegance is in the settle,
// not the length.

export const spring: Transition = { type: 'spring', stiffness: 420, damping: 34, mass: 0.8 };
export const springSoft: Transition = { type: 'spring', stiffness: 240, damping: 28 };
export const easeOut: Transition = { duration: 0.22, ease: [0.22, 1, 0.36, 1] };

// Cross-fade + slight rise between keyed children. Use for tab/page content
// swaps so panels dissolve into each other instead of hard-cutting.
export function FadeSwap({ swapKey, children, className }: { swapKey: string; children: ReactNode; className?: string }) {
  return (
    <AnimatePresence mode="wait" initial={false}>
      <motion.div
        key={swapKey}
        initial={{ opacity: 0, y: 6 }}
        animate={{ opacity: 1, y: 0 }}
        exit={{ opacity: 0, y: -6 }}
        transition={easeOut}
        className={className}
      >
        {children}
      </motion.div>
    </AnimatePresence>
  );
}

// Animate conditional content in/out by height+opacity so surrounding
// elements GLIDE to their new position instead of jumping. This is the
// primary fix for size-change layout shift.
export function Reveal({ show, children, className }: { show: boolean; children: ReactNode; className?: string }) {
  return (
    <AnimatePresence initial={false}>
      {show && (
        <motion.div
          initial={{ opacity: 0, height: 0 }}
          animate={{ opacity: 1, height: 'auto' }}
          exit={{ opacity: 0, height: 0 }}
          transition={{ duration: 0.24, ease: [0.22, 1, 0.36, 1] }}
          style={{ overflow: 'hidden' }}
          className={className}
        >
          {children}
        </motion.div>
      )}
    </AnimatePresence>
  );
}

export interface TabSpec<T extends string> { key: T; label: string }

// Tab strip with a single sliding indicator (shared layoutId) that glides
// under the active tab. The hallmark iOS/segmented-control motion.
export function SlideTabs<T extends string>({
  tabs,
  value,
  onChange,
  layoutId = 'slide-tabs-underline',
}: {
  tabs: TabSpec<T>[];
  value: T;
  onChange: (key: T) => void;
  layoutId?: string;
}) {
  return (
    <div className="flex gap-1 border-b border-turd-bronze/25">
      {tabs.map((t) => {
        const active = t.key === value;
        return (
          <button
            key={t.key}
            type="button"
            role="tab"
            aria-selected={active}
            onClick={() => onChange(t.key)}
            className={`relative px-3.5 py-2 font-display text-xs font-semibold uppercase tracking-wider ${
              active ? 'text-turd-mustard-bright' : 'text-turd-cream-dim hover:text-turd-cream'
            }`}
          >
            {t.label}
            {active && (
              <motion.span
                layoutId={layoutId}
                transition={spring}
                className="absolute inset-x-1.5 -bottom-px h-0.5 rounded-full bg-turd-mustard-bright shadow-[0_0_8px_var(--turd-mustard)]"
              />
            )}
          </button>
        );
      })}
    </div>
  );
}

// Staggered reveal — parent orchestrates, children rise in sequence. Use on
// first mount of a list/grid for a composed entrance.
export const staggerContainer: Variants = {
  hidden: {},
  show: { transition: { staggerChildren: 0.035, delayChildren: 0.02 } },
};
export const staggerItem: Variants = {
  hidden: { opacity: 0, y: 8 },
  show: { opacity: 1, y: 0, transition: spring },
};

export function Stagger({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <motion.div variants={staggerContainer} initial="hidden" animate="show" className={className}>
      {children}
    </motion.div>
  );
}
export function StaggerItem({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <motion.div variants={staggerItem} className={className}>
      {children}
    </motion.div>
  );
}

// Press-responsive wrapper — subtle scale on tap, for primary buttons/cards
// that should feel physical. Reads as the iOS "press" without being toylike.
export function Pressable({ children, className, onClick, disabled }: { children: ReactNode; className?: string; onClick?: () => void; disabled?: boolean }) {
  return (
    <motion.button
      type="button"
      onClick={onClick}
      disabled={disabled}
      whileTap={disabled ? undefined : { scale: 0.97 }}
      whileHover={disabled ? undefined : { y: -1 }}
      transition={spring}
      className={className}
    >
      {children}
    </motion.button>
  );
}
