import { useEffect, useState } from 'react';
import { Store } from '@tauri-apps/plugin-store';

// ---------------------------------------------------------------------------
// ThemeSwitcher — applies a data-theme attribute on <html> and persists the
// choice via the Tauri store plugin. CSS variables in styles.css respond.
// ---------------------------------------------------------------------------

type ThemeId =
  // 8 hacker
  | 'hacker-matrix'
  | 'hacker-glass'
  | 'hacker-amber'
  | 'hacker-cyberpunk'
  | 'hacker-frost'
  | 'hacker-bloodmoon'
  | 'hacker-onyx'
  | 'hacker-vapor'
  // 8 anime
  | 'anime-sakura'
  | 'anime-ghibli'
  | 'anime-eva'
  | 'anime-shoujo'
  | 'anime-shonen'
  | 'anime-yuru'
  | 'anime-tsuki'
  | 'anime-kawaii'
  // 8 girly
  | 'spunk-bubblegum'
  | 'spunk-rosegold'
  | 'spunk-lavender'
  | 'spunk-cottagecore'
  | 'spunk-cottoncandy'
  | 'spunk-strawberry'
  | 'spunk-unicorn'
  | 'spunk-glam'
  | 'default';

interface ThemeSpec {
  id: ThemeId;
  label: string;
  swatch: [string, string, string]; // bg / accent / text for the preview chip
}

const THEMES: { group: string; items: ThemeSpec[] }[] = [
  {
    group: 'Default',
    items: [{ id: 'default', label: 'TurdMOD (bronze)', swatch: ['#1a0f08', '#ffc850', '#f5deb3'] }],
  },
  {
    group: 'Hacker',
    items: [
      { id: 'hacker-matrix',    label: 'Matrix',     swatch: ['#000000', '#5dff5d', '#c8ffc8'] },
      { id: 'hacker-glass',     label: 'Glass cyan', swatch: ['#04060a', '#6edcff', '#d9f3ff'] },
      { id: 'hacker-amber',     label: 'CRT amber',  swatch: ['#100802', '#ffaa2a', '#ffd28a'] },
      { id: 'hacker-cyberpunk', label: 'Cyberpunk',  swatch: ['#07041a', '#ff3aa1', '#00f0ff'] },
      { id: 'hacker-frost',     label: 'Frost glass',swatch: ['#0a1018', '#8ad8ff', '#e6f3ff'] },
      { id: 'hacker-bloodmoon', label: 'Blood moon', swatch: ['#0a0405', '#ff4060', '#ffdadf'] },
      { id: 'hacker-onyx',      label: 'Onyx',       swatch: ['#000000', '#ffffff', '#ffffff'] },
      { id: 'hacker-vapor',     label: 'Vaporwave',  swatch: ['#160d2a', '#ff80d6', '#66f8ff'] },
    ],
  },
  {
    group: 'Anime',
    items: [
      { id: 'anime-sakura',  label: 'Sakura',  swatch: ['#221016', '#ff80b5', '#ffe8f0'] },
      { id: 'anime-ghibli',  label: 'Ghibli',  swatch: ['#14201a', '#d8c468', '#f5ecd0'] },
      { id: 'anime-eva',     label: 'Eva',     swatch: ['#0a0814', '#80ff40', '#e8d6ff'] },
      { id: 'anime-shoujo',  label: 'Shoujo',  swatch: ['#2a1830', '#ffd060', '#ffeefa'] },
      { id: 'anime-shonen',  label: 'Shonen',  swatch: ['#14182a', '#ff8040', '#fff4e0'] },
      { id: 'anime-yuru',    label: 'Yuru',    swatch: ['#1d1812', '#d8a878', '#fff0d8'] },
      { id: 'anime-tsuki',   label: 'Tsuki',   swatch: ['#0d1424', '#c0c8ff', '#e8eeff'] },
      { id: 'anime-kawaii',  label: 'Kawaii',  swatch: ['#2a1c2e', '#ffe080', '#fff0fa'] },
    ],
  },
  {
    group: 'Spunk',
    items: [
      { id: 'spunk-bubblegum',   label: 'Bubblegum',   swatch: ['#2e1424', '#ff5cae', '#fff0f8'] },
      { id: 'spunk-rosegold',    label: 'Rose gold',   swatch: ['#261620', '#f8b878', '#fff0e8'] },
      { id: 'spunk-lavender',    label: 'Lavender',    swatch: ['#1f1830', '#c0a0ff', '#f0e8ff'] },
      { id: 'spunk-cottagecore', label: 'Cottagecore', swatch: ['#20180f', '#f8a890', '#fff0d8'] },
      { id: 'spunk-cottoncandy', label: 'Cotton candy',swatch: ['#1a1c30', '#ff98c8', '#fff0fa'] },
      { id: 'spunk-strawberry',  label: 'Strawberry',  swatch: ['#2a1018', '#ff7080', '#fff0e0'] },
      { id: 'spunk-unicorn',     label: 'Unicorn',     swatch: ['#1f1430', '#ffd060', '#fff0f8'] },
      { id: 'spunk-glam',        label: 'Glam noir',   swatch: ['#0e0a14', '#ff60b0', '#fff4d8'] },
    ],
  },
];

const STORE_FILE = 'manager.json';
const STORE_KEY = 'theme';

function applyTheme(id: ThemeId) {
  if (id === 'default') {
    document.documentElement.removeAttribute('data-theme');
  } else {
    document.documentElement.setAttribute('data-theme', id);
  }
}

export function ThemeSwitcher() {
  const [current, setCurrent] = useState<ThemeId>('default');
  const [open, setOpen] = useState(false);

  // Load persisted theme once at mount. Legacy migration: the group
  // was originally "Girly" with `girly-*` ids; renamed to "Spunk" in
  // 2026-05-18. Anyone with the old id saved gets remapped silently.
  useEffect(() => {
    let mounted = true;
    (async () => {
      try {
        const store = await Store.load(STORE_FILE);
        const savedRaw = (await store.get<string>(STORE_KEY)) ?? 'default';
        const saved = (
          savedRaw.startsWith('girly-')
            ? savedRaw.replace('girly-', 'spunk-')
            : savedRaw
        ) as ThemeId;
        if (mounted) {
          setCurrent(saved);
          applyTheme(saved);
          // Persist the migrated id so we don't redo the remap.
          if (saved !== savedRaw) {
            await store.set(STORE_KEY, saved);
            await store.save();
          }
        }
      } catch (e) {
        console.warn('[ThemeSwitcher] load failed:', e);
      }
    })();
    return () => {
      mounted = false;
    };
  }, []);

  const select = async (id: ThemeId) => {
    setCurrent(id);
    applyTheme(id);
    setOpen(false);
    try {
      const store = await Store.load(STORE_FILE);
      await store.set(STORE_KEY, id);
      await store.save();
    } catch (e) {
      console.warn('[ThemeSwitcher] save failed:', e);
    }
  };

  const currentSpec =
    THEMES.flatMap((g) => g.items).find((t) => t.id === current) ??
    THEMES[0].items[0];

  return (
    <div className="relative">
      <button
        onClick={() => setOpen((v) => !v)}
        className="flex w-full items-center gap-2 rounded border border-turd-bronze/40 bg-turd-bg-soft/40 px-2 py-1.5 text-left text-[11px] text-turd-cream hover:border-turd-bronze"
      >
        <span
          className="inline-block h-3 w-3 rounded-full ring-1 ring-turd-bronze/40"
          style={{
            background: `linear-gradient(135deg, ${currentSpec.swatch[0]} 0%, ${currentSpec.swatch[1]} 100%)`,
          }}
        />
        <span className="flex-1 truncate">{currentSpec.label}</span>
        <span className="text-turd-cream-dim">▾</span>
      </button>

      {open && (
        <>
          {/* Backdrop catches clicks-outside */}
          <div
            onClick={() => setOpen(false)}
            className="fixed inset-0 z-40"
          />
          <div className="absolute left-0 top-full z-50 mt-1 max-h-[70vh] w-[280px] overflow-y-auto rounded border border-turd-bronze/40 bg-turd-bg-deep shadow-xl">
            {THEMES.map((group) => (
              <div key={group.group}>
                <div className="bg-turd-bg-mid/60 px-2 py-1 text-[9px] uppercase tracking-wider text-turd-cream-dim/70">
                  {group.group}
                </div>
                {group.items.map((t) => (
                  <button
                    key={t.id}
                    onClick={() => select(t.id)}
                    className={`flex w-full items-center gap-2 px-2 py-1.5 text-left text-[11px] hover:bg-turd-bg-soft/50 ${
                      current === t.id
                        ? 'bg-turd-bg-soft/30 text-turd-mustard-bright'
                        : 'text-turd-cream'
                    }`}
                  >
                    <span
                      className="inline-block h-3 w-3 shrink-0 rounded-full ring-1 ring-white/10"
                      style={{
                        background: `linear-gradient(135deg, ${t.swatch[0]} 0%, ${t.swatch[1]} 100%)`,
                      }}
                    />
                    <span className="flex-1 truncate">{t.label}</span>
                    {current === t.id && (
                      <span className="text-[10px] text-turd-mustard-bright">✓</span>
                    )}
                  </button>
                ))}
              </div>
            ))}
          </div>
        </>
      )}
    </div>
  );
}
