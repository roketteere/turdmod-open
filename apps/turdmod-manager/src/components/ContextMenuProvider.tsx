import { useEffect, useState, type ReactNode } from 'react';

// ---------------------------------------------------------------------------
// Custom context menu — intercepts the native right-click and renders a
// styled menu themed to match the active palette. Sensible defaults:
//   - if there's a text selection → Copy / Copy as plain text
//   - if right-click landed on an <input> / <textarea> → Cut / Copy / Paste
//   - always → Reload, Open DevTools (dev builds), Close menu
//
// Future: per-element overrides via a data-ctx-menu attribute carrying a
// JSON list of items. For now, the sensible defaults cover 90% of needs.
// ---------------------------------------------------------------------------

type MenuItem =
  | { kind: 'item'; label: string; action: () => void; disabled?: boolean; danger?: boolean }
  | { kind: 'sep' };

interface MenuState {
  x: number;
  y: number;
  items: MenuItem[];
}

function buildItems(e: MouseEvent): MenuItem[] {
  const items: MenuItem[] = [];
  const selection = window.getSelection()?.toString() ?? '';
  const target = e.target as HTMLElement | null;
  const isEditable =
    target instanceof HTMLInputElement ||
    target instanceof HTMLTextAreaElement ||
    target?.isContentEditable === true;

  if (selection) {
    items.push({
      kind: 'item',
      label: `Copy "${selection.length > 30 ? selection.slice(0, 30) + '…' : selection}"`,
      action: () => navigator.clipboard.writeText(selection),
    });
  }

  if (isEditable) {
    const el = target as HTMLInputElement | HTMLTextAreaElement;
    items.push({
      kind: 'item',
      label: 'Cut',
      disabled: !selection,
      action: () => {
        if (selection) {
          navigator.clipboard.writeText(selection);
          document.execCommand('cut');
        }
      },
    });
    items.push({
      kind: 'item',
      label: 'Copy',
      disabled: !selection,
      action: () => {
        if (selection) navigator.clipboard.writeText(selection);
      },
    });
    items.push({
      kind: 'item',
      label: 'Paste',
      action: async () => {
        try {
          const text = await navigator.clipboard.readText();
          const start = el.selectionStart ?? 0;
          const end = el.selectionEnd ?? 0;
          const before = el.value.slice(0, start);
          const after = el.value.slice(end);
          el.value = before + text + after;
          // Trigger React onChange listeners.
          el.dispatchEvent(new Event('input', { bubbles: true }));
          const pos = start + text.length;
          el.setSelectionRange(pos, pos);
        } catch (err) {
          console.warn('paste failed:', err);
        }
      },
    });
    items.push({
      kind: 'item',
      label: 'Select all',
      action: () => el.select(),
    });
  }

  if (items.length > 0) items.push({ kind: 'sep' });

  items.push({
    kind: 'item',
    label: 'Reload page',
    action: () => window.location.reload(),
  });

  return items;
}

export function ContextMenuProvider({ children }: { children: ReactNode }) {
  const [menu, setMenu] = useState<MenuState | null>(null);

  useEffect(() => {
    const onContext = (e: MouseEvent) => {
      e.preventDefault();
      const items = buildItems(e);
      setMenu({ x: e.clientX, y: e.clientY, items });
    };
    const onClick = () => setMenu(null);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setMenu(null);
    };
    window.addEventListener('contextmenu', onContext);
    window.addEventListener('click', onClick);
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('contextmenu', onContext);
      window.removeEventListener('click', onClick);
      window.removeEventListener('keydown', onKey);
    };
  }, []);

  // Position the menu so it doesn't overflow the viewport bottom/right.
  const menuStyle: React.CSSProperties = menu
    ? (() => {
        const W = 200;
        const H = menu.items.length * 28 + 8;
        const x = Math.min(menu.x, window.innerWidth - W - 4);
        const y = Math.min(menu.y, window.innerHeight - H - 4);
        return { left: x, top: y, width: W };
      })()
    : {};

  return (
    <>
      {children}
      {menu && (
        <div
          onClick={(e) => e.stopPropagation()}
          onContextMenu={(e) => e.preventDefault()}
          className="fixed z-[100] rounded border border-turd-bronze/50 bg-turd-bg-deep/95 py-1 text-xs shadow-2xl backdrop-blur"
          style={menuStyle}
        >
          {menu.items.map((item, i) => {
            if (item.kind === 'sep') {
              return (
                <div
                  key={`sep-${i}`}
                  className="my-1 border-t border-turd-bronze/30"
                />
              );
            }
            return (
              <button
                key={i}
                disabled={item.disabled}
                onClick={() => {
                  item.action();
                  setMenu(null);
                }}
                className={`flex w-full items-center px-3 py-1.5 text-left transition-colors disabled:cursor-not-allowed disabled:opacity-30 ${
                  item.danger
                    ? 'text-turd-red hover:bg-red-900/40'
                    : 'text-turd-cream hover:bg-turd-bg-soft/60'
                }`}
              >
                {item.label}
              </button>
            );
          })}
        </div>
      )}
    </>
  );
}
