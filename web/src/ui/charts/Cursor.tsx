// Pointer, touch and keyboard interaction for a chart: a crosshair that snaps to the
// nearest data position, one tooltip listing every series there, optional drag-to-zoom
// on the time axis, and the same reading for keyboard and screen-reader users.
import { useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent, type PointerEvent, type ReactNode } from 'react';
import { useFrame } from './ChartFrame';
import { useChartSync, useFocus, useFocusSource } from './sync';

export interface CursorProps {
  /** Number of data positions (for a chart over time, the years). */
  count: number;
  /** Pixel x of a position, in frame coordinates. Positions outside the plot are skipped. */
  xOf: (index: number) => number;
  /** Accessible name of the interactive region, e.g. "Net worth by age". */
  label: string;
  /** One sentence for position `index`, announced to screen readers as the keyboard moves. */
  describe: (index: number) => string;
  /** Tooltip content for position `index`. Values lead, labels follow. */
  tooltip?: (index: number) => ReactNode;
  /** SVG marks at the focused position (dots on the lines), in frame coordinates. */
  marks?: (index: number) => ReactNode;
  /** 'self' shows the tooltip only on the chart being pointed at; 'always' also when a synced chart moves the focus. */
  showTooltip?: 'self' | 'always';
  /** Drag horizontally to zoom; receives the first and last index of the drag. */
  onBrush?: (from: number, to: number) => void;
  /** Click or Enter on a position; `y` is the pointer's pixel y (null from the keyboard). */
  onSelect?: (index: number, y: number | null) => void;
  /** Pointer position while hovering, for charts that hit-test individual marks. */
  onPointer?: (index: number | null, x: number, y: number) => void;
  /** Hide the vertical hairline (bar and dot charts highlight the mark instead). */
  crosshair?: boolean;
  /** Show the pointing-hand cursor: set it while the pointer is over something `onSelect` would open. */
  clickable?: boolean;
}

/**
 * The interaction layer. Place it last inside a `ChartFrame`. The focused position
 * lives in the enclosing `ChartSyncProvider`, so charts that share a time axis move
 * their crosshairs together.
 */
export function Cursor({ count, xOf, label, describe, tooltip, marks, showTooltip = 'self', onBrush, onSelect, onPointer, crosshair = true, clickable }: CursorProps) {
  const frame = useFrame();
  const { plot, theme } = frame;
  const sync = useChartSync();
  const id = useId();
  const focus = useFocus(sync);
  const source = useFocusSource(sync);
  const mine = source === id;
  const [spoken, setSpoken] = useState('');
  const [brush, setBrush] = useState<{ from: number; to: number } | null>(null);
  const drag = useRef<{ x: number; index: number; moved: boolean } | null>(null);
  const tip = useRef<HTMLDivElement>(null);
  const [tipSize, setTipSize] = useState({ width: 0, height: 0 });

  // Positions inside the plot, for snapping.
  const visible: number[] = [];
  for (let i = 0; i < count; i++) {
    const x = xOf(i);
    if (x >= plot.x - 0.5 && x <= plot.right + 0.5) visible.push(i);
  }
  const first = visible[0] ?? 0;
  const last = visible[visible.length - 1] ?? Math.max(0, count - 1);
  const nearest = (px: number) => {
    let best = first;
    let distance = Infinity;
    for (const i of visible) {
      const d = Math.abs(xOf(i) - px);
      if (d < distance) {
        distance = d;
        best = i;
      }
    }
    return best;
  };

  const index = focus !== null && focus >= first && focus <= last ? focus : null;
  const x = index === null ? 0 : xOf(index);
  const showTip = index !== null && tooltip !== undefined && (mine || showTooltip === 'always') && brush === null;

  useLayoutEffect(() => {
    if (!showTip || !tip.current) return;
    const { offsetWidth: width, offsetHeight: height } = tip.current;
    setTipSize((s) => (s.width === width && s.height === height ? s : { width, height }));
  }, [showTip, index]);

  // Leaving the chart by any route clears a focus this chart set.
  useEffect(() => () => {
    if (sync.getSource() === id) sync.setFocus(null);
  }, [sync, id]);

  const local = (e: PointerEvent) => {
    const rect = e.currentTarget.getBoundingClientRect();
    return { px: e.clientX - rect.left + plot.x, py: e.clientY - rect.top + plot.y };
  };

  const onMove = (e: PointerEvent) => {
    const { px, py } = local(e);
    const i = nearest(px);
    const d = drag.current;
    if (d && onBrush && e.pointerType !== 'touch' && Math.abs(px - d.x) > 6) {
      d.moved = true;
      setBrush({ from: Math.min(d.index, i), to: Math.max(d.index, i) });
    }
    sync.setFocus(i, id);
    onPointer?.(i, px, py);
  };

  const onKey = (e: KeyboardEvent) => {
    const at = index ?? (e.key === 'ArrowLeft' || e.key === 'End' ? last + 1 : first - 1);
    const step = e.shiftKey ? 5 : 1;
    let next: number | null = null;
    if (e.key === 'ArrowRight') next = Math.min(last, at + step);
    else if (e.key === 'ArrowLeft') next = Math.max(first, at - step);
    else if (e.key === 'Home') next = first;
    else if (e.key === 'End') next = last;
    else if (e.key === 'Escape') {
      sync.setFocus(null);
      setSpoken('');
      return;
    } else if ((e.key === 'Enter' || e.key === ' ') && index !== null && onSelect) {
      e.preventDefault();
      onSelect(index, null);
      return;
    }
    if (next === null) return;
    e.preventDefault();
    sync.setFocus(next, id);
    setSpoken(describe(next));
  };

  const tipLeft = x + 14 + tipSize.width > frame.width ? Math.max(0, x - 14 - tipSize.width) : x + 14;

  return (
    <>
      <svg
        data-chart-layer
        data-chart-export="skip"
        aria-hidden
        width={frame.width}
        height={frame.height}
        style={{ position: 'absolute', inset: 0, overflow: 'visible', pointerEvents: 'none' }}
      >
        {brush && (
          <rect
            x={xOf(brush.from)}
            y={plot.y}
            width={Math.max(1, xOf(brush.to) - xOf(brush.from))}
            height={plot.height}
            fill={theme.ink.primary}
            fillOpacity={0.08}
            stroke={theme.ink.tertiary}
            strokeWidth={1}
          />
        )}
        {index !== null && !brush && (
          <>
            {crosshair && <line x1={x} x2={x} y1={plot.y} y2={plot.bottom} stroke={theme.ink.secondary} strokeWidth={1} shapeRendering="crispEdges" />}
            {marks?.(index)}
          </>
        )}
      </svg>
      <div
        role="application"
        tabIndex={0}
        aria-label={`${label}. Use the left and right arrow keys to read values${onSelect ? ', Enter to open' : ''}.`}
        aria-describedby={`${id}-live`}
        data-chart-cursor
        className="rounded-xs outline-offset-2"
        style={{ position: 'absolute', left: plot.x, top: plot.y, width: plot.width, height: plot.height, touchAction: 'pan-y', cursor: clickable ? 'pointer' : 'crosshair' }}
        onPointerMove={onMove}
        onPointerDown={(e) => {
          const { px } = local(e);
          drag.current = { x: px, index: nearest(px), moved: false };
          if (onBrush && e.pointerType !== 'touch') e.currentTarget.setPointerCapture(e.pointerId);
          onMove(e);
        }}
        onPointerUp={(e) => {
          const d = drag.current;
          drag.current = null;
          if (brush) {
            if (brush.to - brush.from >= 2) onBrush?.(brush.from, brush.to);
            setBrush(null);
          } else if (d && !d.moved && onSelect) {
            const { px, py } = local(e);
            onSelect(nearest(px), py);
          }
        }}
        onPointerCancel={() => {
          drag.current = null;
          setBrush(null);
        }}
        onPointerLeave={(e) => {
          if (drag.current && e.pointerType !== 'touch') return;
          if (sync.getSource() === id) sync.setFocus(null);
          onPointer?.(null, 0, 0);
        }}
        onKeyDown={onKey}
        onBlur={() => {
          if (sync.getSource() === id) sync.setFocus(null);
        }}
      />
      <div id={`${id}-live`} aria-live="polite" className="sr-only">
        {spoken}
      </div>
      {showTip && index !== null && (
        <div
          ref={tip}
          role="presentation"
          className="pointer-events-none absolute z-10 rounded-md border border-line bg-surface-2 px-2.5 py-2 text-xs shadow-overlay"
          style={{ left: tipLeft, top: Math.max(0, plot.y + 4), minWidth: 132, maxWidth: Math.min(260, frame.width - 8) }}
        >
          {tooltip(index)}
        </div>
      )}
    </>
  );
}

// ---- tooltip building blocks -------------------------------------------------------

/** The heading row of a tooltip: where the crosshair is ("Age 65 · 2054"). */
export function TipTitle({ children, detail }: { children: ReactNode; detail?: ReactNode }) {
  return (
    <div className="mb-1.5 flex items-baseline justify-between gap-3 whitespace-nowrap">
      <span className="font-medium text-ink-1">{children}</span>
      {detail && <span className="num text-2xs text-ink-3">{detail}</span>}
    </div>
  );
}

export interface TipRowProps {
  /** Series colour, drawn as a short stroke (lines) or a small square (areas and bars). */
  color?: string;
  swatch?: 'line' | 'box' | 'dash';
  label: ReactNode;
  value: ReactNode;
  /** Emphasise the row the story is about (the median). */
  strong?: boolean;
}

/** One series in a tooltip: key, name, then the value in strong ink at the right. */
export function TipRow({ color, swatch = 'line', label, value, strong }: TipRowProps) {
  return (
    <div className="flex items-center gap-2 whitespace-nowrap leading-5">
      {color !== undefined && (
        <span
          aria-hidden
          className="inline-block shrink-0"
          style={
            swatch === 'box'
              ? { width: 8, height: 8, borderRadius: 2, background: color }
              : swatch === 'dash'
                ? { width: 12, height: 0, borderTop: `2px dashed ${color}` }
                : { width: 12, height: 2, borderRadius: 1, background: color }
          }
        />
      )}
      <span className={strong ? 'text-ink-1' : 'text-ink-2'}>{label}</span>
      <span className={`num ml-auto pl-3 text-ink-1 ${strong ? 'font-semibold' : 'font-medium'}`}>{value}</span>
    </div>
  );
}

/** A quiet closing line in a tooltip (sample size, a hint). */
export function TipNote({ children }: { children: ReactNode }) {
  return <div className="mt-1.5 border-t border-line pt-1.5 text-2xs leading-4 whitespace-normal text-ink-3">{children}</div>;
}
