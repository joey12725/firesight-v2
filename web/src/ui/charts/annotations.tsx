// Annotation primitives, in SVG: labelled vertical rules for moments in the plan
// ("Work optional", "Social Security starts"), event markers, and direct labels that
// ride the ends of lines with collisions resolved by leader lines rather than stacking.
import type { ReactNode } from 'react';
import { useFrame } from './ChartFrame';
import { clamp } from './scales';

/** Rough text width without touching the DOM (Geist at the given size); good to a few pixels. */
export function textWidth(text: string, fontSize = 11): number {
  let w = 0;
  for (const ch of text) w += /[ilI.,:;'|!]/.test(ch) ? 0.3 : /[mwMW%]/.test(ch) ? 0.88 : /[A-Z0-9$]/.test(ch) ? 0.64 : 0.54;
  return w * fontSize;
}

export interface RuleItem {
  /** Pixel x in frame coordinates. */
  x: number;
  label: string;
  /** Secondary text after the label, in lighter ink: "median 51.8". */
  detail?: string;
  /** 'event' draws a solid hairline; 'reference' a lighter one. */
  tone?: 'event' | 'reference';
}

/**
 * Labelled vertical rules. Labels sit in the top margin and are laid out in rows so
 * they never overlap: allow `rulesHeight(items.length)` of top margin (16 px per row).
 */
export function RuleAnnotations({ items }: { items: RuleItem[] }) {
  const { plot, theme } = useFrame();
  const inside = items.filter((it) => it.x >= plot.x - 1 && it.x <= plot.right + 1).sort((a, b) => a.x - b.x);
  // Greedy rows: each label goes in the lowest row where it clears the previous label.
  const rows: number[] = [];
  const placed = inside.map((it) => {
    const width = textWidth(it.label, 11) + (it.detail ? textWidth(` ${it.detail}`, 11) : 0) + 6;
    const flip = it.x + width > plot.right;
    const left = flip ? it.x - width : it.x;
    let row = rows.findIndex((end) => left > end + 8);
    if (row < 0) row = rows.length;
    rows[row] = left + width;
    return { ...it, flip, row };
  });
  const rowCount = Math.max(1, rows.length);
  return (
    <g fontSize={11}>
      {placed.map((it) => {
        const top = plot.y - 6 - (rowCount - 1 - it.row) * 15;
        const x = Math.round(it.x) + 0.5;
        return (
          <g key={`${it.label}-${it.x}`}>
            <line x1={x} x2={x} y1={top - 9} y2={plot.bottom} stroke={theme.annotation} strokeOpacity={it.tone === 'reference' ? 0.35 : 0.6} strokeWidth={1} shapeRendering="crispEdges" />
            <text x={it.flip ? x - 5 : x + 5} y={top} textAnchor={it.flip ? 'end' : 'start'} fill={theme.annotation} stroke={theme.surface} strokeWidth={3} paintOrder="stroke" strokeLinejoin="round">
              <tspan fontWeight={500}>{it.label}</tspan>
              {it.detail && <tspan fill={theme.label}> {it.detail}</tspan>}
            </text>
          </g>
        );
      })}
    </g>
  );
}

/** Top margin needed for `count` rule labels in the worst case (all in separate rows, capped at 3). */
export const rulesHeight = (count: number) => 14 + Math.min(3, Math.max(1, count)) * 15;

export interface EventMarkerProps {
  x: number;
  y: number;
  label?: string;
  color?: string;
  /** Place the label above or below the dot. */
  side?: 'above' | 'below';
  children?: ReactNode;
}

/** A dot with a surface ring and an optional short label: one event on one line. */
export function EventMarker({ x, y, label, color, side = 'above' }: EventMarkerProps) {
  const { theme, plot } = useFrame();
  const anchor = x > plot.right - 60 ? 'end' : x < plot.x + 60 ? 'start' : 'middle';
  return (
    <g>
      <circle cx={x} cy={y} r={4} fill={color ?? theme.ink.primary} stroke={theme.surface} strokeWidth={2} />
      {label && (
        <text x={x} y={side === 'above' ? y - 9 : y + 17} textAnchor={anchor} fontSize={11} fill={theme.annotation} stroke={theme.surface} strokeWidth={3} paintOrder="stroke" strokeLinejoin="round">
          {label}
        </text>
      )}
    </g>
  );
}

export interface EndLabel {
  key: string;
  /** Pixel y of the line's end. */
  y: number;
  text: string;
  /** Lighter text after the label (a value). */
  detail?: string;
  /** Mark colour for the key. Text stays in ink. */
  color: string;
  strong?: boolean;
  /** Key shape: a stroke for lines, a square for areas. */
  swatch?: 'line' | 'box' | 'none';
}

/** Spreads label positions apart to at least `gap` px while keeping their order and staying inside [min, max]. */
export function spreadLabels(ys: number[], gap: number, min: number, max: number): number[] {
  const order = ys.map((y, i) => ({ y, i })).sort((a, b) => a.y - b.y);
  const out = order.map((o) => clamp(o.y, min, max));
  for (let pass = 0; pass < 4; pass++) {
    for (let k = 1; k < out.length; k++) if ((out[k] as number) - (out[k - 1] as number) < gap) out[k] = (out[k - 1] as number) + gap;
    const over = (out[out.length - 1] ?? 0) - max;
    if (over > 0) for (let k = 0; k < out.length; k++) out[k] = (out[k] as number) - over;
    for (let k = out.length - 2; k >= 0; k--) if ((out[k + 1] as number) - (out[k] as number) < gap) out[k] = (out[k + 1] as number) - gap;
    if ((out[0] ?? 0) >= min) break;
    const under = min - (out[0] ?? 0);
    for (let k = 0; k < out.length; k++) out[k] = (out[k] as number) + under;
  }
  const result = new Array<number>(ys.length);
  order.forEach((o, k) => (result[o.i] = out[k] as number));
  return result;
}

/**
 * Direct labels at the right end of lines, in the right margin. Labels that would
 * collide are moved apart and joined to their line end by a thin leader. Allow about
 * 64 px of right margin (more for long names).
 */
export function EndLabels({ labels, x, gap = 14 }: { labels: EndLabel[]; x?: number; gap?: number }) {
  const { plot, theme } = useFrame();
  const at = x ?? plot.right;
  const ys = spreadLabels(labels.map((l) => l.y), gap, plot.y + 4, plot.bottom - 2);
  return (
    <g fontSize={11}>
      {labels.map((l, i) => {
        const y = ys[i] as number;
        const moved = Math.abs(y - l.y) > 3;
        const tx = at + (l.swatch === 'none' ? 8 : 22);
        return (
          <g key={l.key}>
            {moved && <path d={`M${at + 1},${l.y} L${at + 6},${y}`} stroke={theme.axis} strokeWidth={1} fill="none" />}
            {l.swatch === 'box' ? (
              <rect x={at + 8} y={y - 4} width={8} height={8} rx={2} fill={l.color} />
            ) : l.swatch === 'none' ? null : (
              <line x1={at + 7} x2={at + 18} y1={y} y2={y} stroke={l.color} strokeWidth={l.strong ? 2.5 : 1.5} strokeLinecap="round" />
            )}
            <text x={tx} y={y + 3.5} fill={l.strong ? theme.ink.primary : theme.ink.secondary} fontWeight={l.strong ? 600 : 400}>
              {l.text}
              {l.detail && (
                <tspan fill={theme.label} fontWeight={400} style={{ fontVariantNumeric: 'tabular-nums' }}>
                  {' '}
                  {l.detail}
                </tspan>
              )}
            </text>
          </g>
        );
      })}
    </g>
  );
}
