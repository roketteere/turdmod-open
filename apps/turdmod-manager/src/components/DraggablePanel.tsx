// Reusable draggable + resizable floating panel for the Admin Map — same drag/
// resize behavior as MapPage's player panel, extracted so other overlays (e.g.
// ServerControls) get the move/resize affordance instead of a locked overlay.
// @ctx: positions relative to the nearest positioned ancestor (the map container).
import { useRef, useState } from 'react';

// Geometry + cursor for an edge/corner resize handle (mirrors MapPage).
function resizeHandleStyle(d: string): React.CSSProperties {
  const t = 7;
  const cur =
    d.length === 2
      ? d === 'ne' || d === 'sw'
        ? 'nesw-resize'
        : 'nwse-resize'
      : d === 'n' || d === 's'
        ? 'ns-resize'
        : 'ew-resize';
  const s: React.CSSProperties = { position: 'absolute', cursor: cur };
  if (d.includes('n')) s.top = -t / 2;
  if (d.includes('s')) s.bottom = -t / 2;
  if (d.includes('e')) s.right = -t / 2;
  if (d.includes('w')) s.left = -t / 2;
  if (d === 'n' || d === 's') {
    s.left = t;
    s.right = t;
    s.height = t;
  } else if (d === 'e' || d === 'w') {
    s.top = t;
    s.bottom = t;
    s.width = t;
  } else {
    s.width = t * 2;
    s.height = t * 2;
  }
  return s;
}

const CORNER: Record<string, string> = {
  tl: 'left-3 top-3',
  tr: 'right-3 top-3',
  bl: 'left-3 bottom-3',
  br: 'right-3 bottom-3',
};

export interface DraggablePanelProps {
  title: React.ReactNode;
  icon?: string;
  headerRight?: React.ReactNode;
  defaultCorner?: 'tl' | 'tr' | 'bl' | 'br';
  defaultWidth?: number;
  defaultHeight?: number;
  minW?: number;
  minH?: number;
  onClose?: () => void;
  children: React.ReactNode;
}

export default function DraggablePanel({
  title,
  icon,
  headerRight,
  defaultCorner = 'tl',
  defaultWidth = 280,
  defaultHeight = 240,
  minW = 224,
  minH = 120,
  onClose,
  children,
}: DraggablePanelProps) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);
  const [size, setSize] = useState<{ w: number; h: number } | null>(null);

  const startResize = (dir: string) => (e: React.MouseEvent) => {
    if (!ref.current) return;
    e.preventDefault();
    e.stopPropagation();
    const parent = ref.current.offsetParent as HTMLElement | null;
    const pr = parent?.getBoundingClientRect();
    const rect = ref.current.getBoundingClientRect();
    const base = { x: rect.left - (pr?.left ?? 0), y: rect.top - (pr?.top ?? 0), w: rect.width, h: rect.height };
    const sx = e.clientX;
    const sy = e.clientY;
    const move = (ev: MouseEvent) => {
      let { x, y, w, h } = base;
      const dx = ev.clientX - sx;
      const dy = ev.clientY - sy;
      if (dir.includes('e')) w = Math.max(minW, base.w + dx);
      if (dir.includes('s')) h = Math.max(minH, base.h + dy);
      if (dir.includes('w')) {
        w = Math.max(minW, base.w - dx);
        x = base.x + (base.w - w);
      }
      if (dir.includes('n')) {
        h = Math.max(minH, base.h - dy);
        y = base.y + (base.h - h);
      }
      setPos({ x, y });
      setSize({ w, h });
    };
    const up = () => {
      window.removeEventListener('mousemove', move);
      window.removeEventListener('mouseup', up);
    };
    window.addEventListener('mousemove', move);
    window.addEventListener('mouseup', up);
  };

  const startDrag = (e: React.MouseEvent) => {
    if (!ref.current) return;
    e.preventDefault();
    const parent = ref.current.offsetParent as HTMLElement | null;
    const pr = parent?.getBoundingClientRect();
    const rect = ref.current.getBoundingClientRect();
    const baseX = rect.left - (pr?.left ?? 0);
    const baseY = rect.top - (pr?.top ?? 0);
    const sx = e.clientX;
    const sy = e.clientY;
    const move = (ev: MouseEvent) => setPos({ x: baseX + (ev.clientX - sx), y: baseY + (ev.clientY - sy) });
    const up = () => {
      window.removeEventListener('mousemove', move);
      window.removeEventListener('mouseup', up);
    };
    window.addEventListener('mousemove', move);
    window.addEventListener('mouseup', up);
  };

  return (
    <div
      ref={ref}
      className={`absolute z-50 overflow-hidden rounded-lg border border-turd-bronze/50 bg-turd-bg-deep shadow-2xl ${pos ? '' : CORNER[defaultCorner]}`}
      style={{
        width: size?.w ?? defaultWidth,
        height: size?.h ?? defaultHeight,
        maxHeight: '88%',
        minWidth: minW,
        minHeight: minH,
        ...(pos ? { left: pos.x, top: pos.y } : {}),
      }}
    >
      {(['n', 's', 'e', 'w', 'ne', 'nw', 'se', 'sw'] as const).map((d) => (
        <div key={d} onMouseDown={startResize(d)} style={resizeHandleStyle(d)} className="z-20" />
      ))}
      <div className="absolute inset-0 flex flex-col overflow-hidden">
        <div
          onMouseDown={startDrag}
          className="flex shrink-0 cursor-move select-none items-center gap-2 border-b border-turd-bronze/30 bg-turd-bg-mid/40 px-3 py-2"
        >
          {icon && (
            <img
              src={icon}
              alt=""
              className="h-5 w-5 shrink-0"
              onError={(e) => {
                e.currentTarget.style.display = 'none';
              }}
            />
          )}
          <span className="flex-1 truncate font-display text-sm text-turd-mustard-bright">{title}</span>
          {headerRight}
          {onClose && (
            <button
              onMouseDown={(e) => e.stopPropagation()}
              onClick={onClose}
              className="text-xs text-turd-cream-dim hover:text-turd-cream"
            >
              ✕
            </button>
          )}
        </div>
        <div className="flex-1 overflow-auto">{children}</div>
      </div>
    </div>
  );
}
