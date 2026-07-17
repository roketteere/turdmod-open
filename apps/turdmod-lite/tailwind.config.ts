import type { Config } from 'tailwindcss';

// Mirrors the turdmod.com palette so the desktop manager and the
// website look like one product.
export default {
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        'turd-bg-deep': '#1a0f08',
        'turd-bg-mid': '#2e1f0f',
        'turd-bg-soft': '#3b2a1f',
        'turd-bronze': '#8c5a23',
        'turd-bronze-bright': '#a96f2c',
        'turd-cream': '#f5deb3',
        'turd-cream-dim': '#b89e7a',
        'turd-mustard': '#d9a03c',
        'turd-mustard-bright': '#ffc850',
        'turd-mustard-soft': '#a8761a',
        'turd-green': '#8cd960',
        'turd-red': '#d9523c',
      },
      fontFamily: {
        sans: ['ui-sans-serif', 'system-ui', '-apple-system', 'sans-serif'],
        mono: ['Consolas', 'Cascadia Mono', 'ui-monospace', 'monospace'],
        display: ['Cascadia Mono', 'Consolas', 'ui-monospace', 'monospace'],
      },
    },
  },
  plugins: [],
} satisfies Config;
