import type { Config } from 'tailwindcss';

// Mirrors the turdmod.com palette so the desktop manager and the
// website look like one product.
export default {
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        // Each turd-* token reads from a CSS variable so themes can
        // override at the [data-theme="..."] level without rebuilding
        // Tailwind. Defaults (the original bronze/cream/mustard palette)
        // are set in styles.css under :root.
        'turd-bg-deep':         'var(--turd-bg-deep)',
        'turd-bg-mid':          'var(--turd-bg-mid)',
        'turd-bg-soft':         'var(--turd-bg-soft)',
        'turd-bronze':          'var(--turd-bronze)',
        'turd-bronze-bright':   'var(--turd-bronze-bright)',
        'turd-bronze-light':    'var(--turd-bronze-light)',
        'turd-cream':           'var(--turd-cream)',
        'turd-cream-dim':       'var(--turd-cream-dim)',
        'turd-mustard':         'var(--turd-mustard)',
        'turd-mustard-bright':  'var(--turd-mustard-bright)',
        'turd-mustard-soft':    'var(--turd-mustard-soft)',
        'turd-green':           'var(--turd-green)',
        'turd-red':             'var(--turd-red)',
      },
      fontFamily: {
        sans: ['ui-sans-serif', 'system-ui', '-apple-system', 'sans-serif'],
        mono: ['Consolas', 'Cascadia Mono', 'ui-monospace', 'monospace'],
        // Display now reads the --font-display token (Bricolage Grotesque).
        display: ['var(--font-display)'],
      },
      boxShadow: {
        'elev-1': 'var(--elev-1)',
        'elev-2': 'var(--elev-2)',
        'elev-3': 'var(--elev-3)',
      },
      transitionTimingFunction: {
        'out-soft': 'var(--ease-out-soft)',
        spring: 'var(--ease-spring)',
      },
      keyframes: {
        indeterminate: {
          '0%':   { transform: 'translateX(-100%)' },
          '50%':  { transform: 'translateX(150%)' },
          '100%': { transform: 'translateX(-100%)' },
        },
      },
    },
  },
  plugins: [],
} satisfies Config;
