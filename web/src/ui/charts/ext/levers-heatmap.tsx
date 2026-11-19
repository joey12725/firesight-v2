// Marks for a metric over two variables (a heat map): flat cells on canvas, labelled
// iso-lines from marching squares, and a two-dimensional interaction layer (the kit's
// `Cursor` reads along one axis only). Built for the What matters view; nothing here
// knows about levers, so any grid of numbers can use it.
import { useId, useLayoutEffect, useRef, useState, type KeyboardEvent, type PointerEvent, type ReactNode } from 'react';
import { useFrame } from '../ChartFrame';
import { inkOn, rampCss } from '../theme';

export type Point = [number, number];

/**
 * Iso-lines of a grid by marching squares. `values[row][col]` sits at the point
 * (col, row); the result is a list of polylines in those grid coordinates, linearly
 * interpolated along cell edges. Squares with a missing corner are skipped.
 */
export function contourLines(values: readonly (readonly (number | null)[])[], level: number): Point[][] {
  const segments: [Point, Point][] = [];
  const at = (r: number, c: number) => values[r]?.[c] ?? null;
  for (let r = 0; r + 1 < values.length; r++) {
    for (let c = 0; c + 1 < (values[r]?.length ?? 0); c++) {
      const a = at(r, c);
      const b = at(r, c + 1);
      const cc = at(r + 1, c + 1);
      const d = at(r + 1, c);
      if (a === null || b === null || cc === null || d === null) continue;
      const cross = (p: Point, pv: number, q: Point, qv: number): Point | null => {
        if (pv >= level === qv >= level) return null;
        const t = (level - pv) / (qv - pv);
        return [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t];
      };
      const A: Point = [c, r];
      const B: Point = [c + 1, r];
      const C: Point = [c + 1, r + 1];
      const D: Point = [c, r + 1];
      const ab = cross(A, a, B, b);
      const bc = cross(B, b, C, cc);
      const cd = cross(C, cc, D, d);
      const da = cross(D, d, A, a);
      const hits = [ab, bc, cd, da].filter((p): p is Point => p !== null);
      if (hits.length === 2) segments.push([hits[0] as Point, hits[1] as Point]);
      else if (ab && bc && cd && da) {
        // A saddle: the centre decides which pair of opposite corners is joined.
        const centre = (a + b + cc + d) / 4;
        if (centre >= level === a >= level) segments.push([ab, bc], [cd, da]);
        else segments.push([da, ab], [bc, cd]);
      }
    }
  }
  return joinSegments(segments);
}

const key = (p: Point) => `${Math.round(p[0] * 1e5)},${Math.round(p[1] * 1e5)}`;

/** Chains loose segments that share end points into polylines. */
export function joinSegments(segments: readonly [Point, Point][]): Point[][] {
  const ends = new Map<string, number[]>();
  segments.forEach(([p, q], i) => {
    for (const k of [key(p), key(q)]) ends.set(k, [...(ends.get(k) ?? []), i]);
  });
  const used = new Array<boolean>(segments.length).fill(false);
  const grow = (line: Point[], forward: boolean) => {
    for (;;) {
      const tip = forward ? (line[line.length - 1] as Point) : (line[0] as Point);
      const next = (ends.get(key(tip)) ?? []).find((i) => !used[i]);
      if (next === undefined) return;
      used[next] = true;
      const [p, q] = segments[next] as [Point, Point];
      const other = key(p) === key(tip) ? q : p;
      if (forward) line.push(other);
      else line.unshift(other);
    }
  };
  const lines: Point[][] = [];
  segments.forEach(([p, q], i) => {
    if (used[i]) return;
    used[i] = true;
    const line: Point[] = [p, q];
    grow(line, true);
    grow(line, false);
    lines.push(line);
  });
  return lines;
}

/** Length of a polyline and the point a fraction `t` of the way along it (0.5: where a lone label goes). */
export function polylineAt(line: readonly Point[], t = 0.5): { length: number; point: Point } {
  let length = 0;
  for (let i = 1; i < line.length; i++) length += Math.hypot((line[i] as Point)[0] - (line[i - 1] as Point)[0], (line[i] as Point)[1] - (line[i - 1] as Point)[1]);
  const want = length * Math.min(1, Math.max(0, t));
  let walked = 0;
  for (let i = 1; i < line.length; i++) {
    const p = line[i - 1] as Point;
    const q = line[i] as Point;
    const step = Math.hypot(q[0] - p[0], q[1] - p[1]);
    if (walked + step >= want && step > 0) {
      const f = (want - walked) / step;
      return { length, point: [p[0] + (q[0] - p[0]) * f, p[1] + (q[1] - p[1]) * f] };
    }
    walked += step;
  }
  return { length, point: (line[line.length - 1] as Point | undefined) ?? [0, 0] };
}

export interface HeatCell {
  x: number;
  y: number;
  width: number;
  height: number;
  /** Position on the ramp, 0..1. Null draws an empty (unmeasured) cell. */
  t: number | null;
  /** Printed inside the cell when it fits. */
  label?: string;
  /** A provisional value (from a coarser pass): drawn without a label, slightly washed. */
  provisional?: boolean;
}

/** Flat cells with a 2 px gap in the surface colour, each optionally labelled with its value. */
/**
 * The printed value of each cell that has room for it, for drawing above anything laid
 * over the cells (iso-lines): position, text, its colour and the cell's own fill.
 */
export function heatCellLabels(cells: readonly HeatCell[], ramp: string[], measure: (text: string, size: number) => number, gap = 2): { x: number; y: number; text: string; size: number; ink: string; fill: string }[] {
  const out: { x: number; y: number; text: string; size: number; ink: string; fill: string }[] = [];
  for (const cell of cells) {
    if (cell.t === null || !cell.label || cell.provisional) continue;
    const w = Math.max(1, Math.round(cell.x + cell.width) - Math.round(cell.x) - gap);
    const h = Math.max(1, Math.round(cell.y + cell.height) - Math.round(cell.y) - gap);
    const size = w >= 44 && h >= 26 ? 11 : 10;
    if (measure(cell.label, size) > w - 4 || h < size + 4) continue;
    const fill = rampCss(ramp, cell.t);
    out.push({ x: Math.round(cell.x) + gap / 2 + w / 2, y: Math.round(cell.y) + gap / 2 + h / 2, text: cell.label, size, ink: inkOn(fill), fill });
  }
  return out;
}

export function drawHeatCells(ctx: CanvasRenderingContext2D, cells: readonly HeatCell[], ramp: string[], style: { surface: string; empty: string; font: string; gap?: number; labels?: boolean }) {
  const gap = style.gap ?? 2;
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  for (const cell of cells) {
    const x = Math.round(cell.x) + gap / 2;
    const y = Math.round(cell.y) + gap / 2;
    const w = Math.max(1, Math.round(cell.x + cell.width) - Math.round(cell.x) - gap);
    const h = Math.max(1, Math.round(cell.y + cell.height) - Math.round(cell.y) - gap);
    if (cell.t === null) {
      ctx.fillStyle = style.empty;
      ctx.fillRect(x, y, w, h);
      continue;
    }
    const fill = rampCss(ramp, cell.t);
    ctx.globalAlpha = cell.provisional ? 0.55 : 1;
    ctx.fillStyle = fill;
    ctx.fillRect(x, y, w, h);
    ctx.globalAlpha = 1;
    if (style.labels !== false && cell.label && !cell.provisional) {
      const size = w >= 44 && h >= 26 ? 11 : 10;
      ctx.font = `500 ${size}px ${style.font}`;
      if (ctx.measureText(cell.label).width <= w - 4 && h >= size + 4) {
        ctx.fillStyle = inkOn(fill);
        ctx.fillText(cell.label, x + w / 2, y + h / 2 + 0.5);
      }
    }
  }
}

export interface GridCursorProps {
  cols: number;
  rows: number;
  /** Frame rectangle of a cell. Row 0 is the first row of the data (the view decides which way is up). */
  rect: (col: number, row: number) => { x: number; y: number; width: number; height: number };
  /** Which row an Up arrow moves to: +1 when row indices grow upwards on screen. */
  up?: 1 | -1;
  label: string;
  describe: (col: number, row: number) => string;
  tooltip?: (col: number, row: number) => ReactNode;
  onSelect?: (col: number, row: number) => void;
  selected?: { col: number; row: number } | null;
}

/**
 * Pointer, touch and keyboard reading of a grid of cells: an outline on the cell under
 * the pointer, one tooltip, arrow keys in both directions, Enter to select. Place it
 * last inside a `ChartFrame`.
 */
export function GridCursor({ cols, rows, rect, up = 1, label, describe, tooltip, onSelect, selected }: GridCursorProps) {
  const frame = useFrame();
  const { plot, theme } = frame;
  const id = useId();
  const [focus, setFocus] = useState<{ col: number; row: number } | null>(null);
  const [spoken, setSpoken] = useState('');
  const tip = useRef<HTMLDivElement>(null);
  const [tipSize, setTipSize] = useState({ width: 0, height: 0 });
  const at = focus && focus.col < cols && focus.row < rows ? focus : null;

  useLayoutEffect(() => {
    if (!at || !tip.current) return;
    const { offsetWidth: width, offsetHeight: height } = tip.current;
    setTipSize((s) => (s.width === width && s.height === height ? s : { width, height }));
  }, [at?.col, at?.row, at]);

  const hit = (e: PointerEvent): { col: number; row: number } | null => {
    const box = e.currentTarget.getBoundingClientRect();
    const px = e.clientX - box.left + plot.x;
    const py = e.clientY - box.top + plot.y;
    for (let r = 0; r < rows; r++) {
      for (let c = 0; c < cols; c++) {
        const k = rect(c, r);
        if (px >= k.x && px <= k.x + k.width && py >= k.y && py <= k.y + k.height) return { col: c, row: r };
      }
    }
    return null;
  };

  const onKey = (e: KeyboardEvent) => {
    const from = at ?? selected ?? { col: 0, row: 0 };
    let next: { col: number; row: number } | null = null;
    if (e.key === 'ArrowRight') next = { col: Math.min(cols - 1, from.col + (at ? 1 : 0)), row: from.row };
    else if (e.key === 'ArrowLeft') next = { col: Math.max(0, from.col - (at ? 1 : 0)), row: from.row };
    else if (e.key === 'ArrowUp') next = { col: from.col, row: Math.min(rows - 1, Math.max(0, from.row + (at ? up : 0))) };
    else if (e.key === 'ArrowDown') next = { col: from.col, row: Math.min(rows - 1, Math.max(0, from.row - (at ? up : 0))) };
    else if (e.key === 'Escape') {
      setFocus(null);
      setSpoken('');
      return;
    } else if ((e.key === 'Enter' || e.key === ' ') && at && onSelect) {
      e.preventDefault();
      onSelect(at.col, at.row);
      return;
    }
    if (!next) return;
    e.preventDefault();
    setFocus(next);
    setSpoken(describe(next.col, next.row));
  };

  const box = at ? rect(at.col, at.row) : null;
  const chosen = selected && selected.col < cols && selected.row < rows ? rect(selected.col, selected.row) : null;
  const tipLeft = box ? (box.x + box.width + 10 + tipSize.width > frame.width ? Math.max(0, box.x - 10 - tipSize.width) : box.x + box.width + 10) : 0;
  const tipTop = box ? Math.max(0, Math.min(frame.height - tipSize.height, box.y + box.height / 2 - tipSize.height / 2)) : 0;

  return (
    <>
      <svg data-chart-layer data-chart-export="skip" aria-hidden width={frame.width} height={frame.height} style={{ position: 'absolute', inset: 0, overflow: 'visible', pointerEvents: 'none' }}>
        {chosen && <rect x={chosen.x + 1} y={chosen.y + 1} width={chosen.width - 2} height={chosen.height - 2} fill="none" stroke={theme.ink.primary} strokeWidth={2} rx={2} />}
        {box && (
          <>
            <rect x={box.x} y={box.y} width={box.width} height={box.height} fill="none" stroke={theme.surface} strokeWidth={4} rx={3} />
            <rect x={box.x} y={box.y} width={box.width} height={box.height} fill="none" stroke={theme.ink.primary} strokeWidth={1.5} rx={3} />
          </>
        )}
      </svg>
      <div
        role="application"
        tabIndex={0}
        aria-label={`${label}. Use the arrow keys to read cells${onSelect ? ', Enter to choose one' : ''}.`}
        aria-describedby={`${id}-live`}
        data-chart-cursor
        className="rounded-xs outline-offset-2"
        style={{ position: 'absolute', left: plot.x, top: plot.y, width: plot.width, height: plot.height, touchAction: 'pan-y', cursor: onSelect ? 'pointer' : 'crosshair' }}
        onPointerMove={(e) => setFocus(hit(e))}
        onPointerDown={(e) => setFocus(hit(e))}
        onPointerLeave={(e) => {
          if (e.pointerType !== 'touch') setFocus(null);
        }}
        onClick={() => at && onSelect?.(at.col, at.row)}
        onKeyDown={onKey}
        onBlur={() => setFocus(null)}
      />
      <div id={`${id}-live`} aria-live="polite" className="sr-only">
        {spoken}
      </div>
      {at && tooltip && (
        <div ref={tip} role="presentation" className="pointer-events-none absolute z-10 rounded-md border border-line bg-surface-2 px-2.5 py-2 text-xs shadow-overlay" style={{ left: tipLeft, top: tipTop, minWidth: 150, maxWidth: Math.min(250, frame.width - 8) }}>
          {tooltip(at.col, at.row)}
        </div>
      )}
    </>
  );
}
