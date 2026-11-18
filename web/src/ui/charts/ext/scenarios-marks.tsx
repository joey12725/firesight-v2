// Marks the Scenarios view needed that the kit did not have. They follow the kit's
// rules (ink for text, theme tokens for colour, explicit fill and fontSize so they
// survive export) and take the frame from context, so any view can use them inside a
// `ChartFrame` + `SvgLayer`.
//
//   ValueAxisX     a horizontal value axis (ticks and labels under the plot), for dot
//                  plots and interval charts whose measure runs left to right
//   IntervalMark   an estimate with its interval: a line with end caps and a dot,
//                  hollow when the interval includes the reference value
//   ZeroRule       the vertical reference line such charts are read against
//   PeakLabels     direct labels placed at a chosen point of each line (not its end),
//                  kept apart from one another, for lines that all end at the same value
import type { ReactNode } from 'react';
import { textWidth } from '../annotations';
import { useFrame } from '../ChartFrame';
import type { Scale } from '../scales';

export interface ValueAxisXProps {
  x: (value: number) => number;
  ticks: number[];
  format: (value: number) => string;
  /** Vertical gridlines through the plot at each tick. */
  grid?: boolean;
  /** Stated once, under the tick labels, centred. */
  title?: ReactNode;
  /** Words at the two ends of the axis saying what each direction means. */
  ends?: [string, string];
  /** Pixel y of the axis line. Default: the bottom of the plot. */
  y?: number;
  /** Draw the axis line. */
  rule?: boolean;
}

/** A value axis along the bottom. Allow 22 px of bottom margin, 38 with a title or end words. */
export function ValueAxisX({ x, ticks, format, grid = true, title, ends, y, rule = true }: ValueAxisXProps) {
  const { plot, theme } = useFrame();
  const base = y ?? plot.bottom;
  // Drop labels that would run into the previous one.
  const labelled = ticks.reduce<{ shown: boolean[]; right: number }>(
    (acc, t) => {
      const w = textWidth(format(t), 11);
      const cx = x(t);
      const fits = cx - w / 2 >= acc.right + 8;
      return { shown: [...acc.shown, fits], right: fits ? cx + w / 2 : acc.right };
    },
    { shown: [], right: -Infinity },
  ).shown;
  return (
    <g fontSize={11}>
      {grid && ticks.map((t) => <line key={`g${t}`} x1={Math.round(x(t)) + 0.5} x2={Math.round(x(t)) + 0.5} y1={plot.y} y2={base} stroke={theme.grid} strokeWidth={1} />)}
      {rule && <line x1={plot.x} x2={plot.right} y1={base + 0.5} y2={base + 0.5} stroke={theme.axis} strokeWidth={1} />}
      {ticks.map((t, i) => (
        <g key={t}>
          <line x1={Math.round(x(t)) + 0.5} x2={Math.round(x(t)) + 0.5} y1={base} y2={base + 4} stroke={theme.axis} strokeWidth={1} />
          {labelled[i] && (
            <text x={x(t)} y={base + 16} textAnchor="middle" fill={theme.label} style={{ fontVariantNumeric: 'tabular-nums' }}>
              {format(t)}
            </text>
          )}
        </g>
      ))}
      {ends && (
        <>
          <text x={plot.x} y={base + 32} fill={theme.ink.secondary}>
            ← {ends[0]}
          </text>
          <text x={plot.right} y={base + 32} textAnchor="end" fill={theme.ink.secondary}>
            {ends[1]} →
          </text>
        </>
      )}
      {title && !ends && (
        <text x={(plot.x + plot.right) / 2} y={base + 32} textAnchor="middle" fill={theme.label}>
          {title}
        </text>
      )}
    </g>
  );
}

/** The vertical line a difference chart is read against, with an optional label above it. */
export function ZeroRule({ x, label, y0, y1 }: { x: number; label?: string; y0?: number; y1?: number }) {
  const { plot, theme } = useFrame();
  const px = Math.round(x) + 0.5;
  return (
    <g fontSize={11}>
      <line x1={px} x2={px} y1={y0 ?? plot.y} y2={y1 ?? plot.bottom} stroke={theme.annotation} strokeWidth={1} />
      {label && (
        <text x={px} y={(y0 ?? plot.y) - 6} textAnchor={px < plot.x + 40 ? 'start' : px > plot.right - 40 ? 'end' : 'middle'} fill={theme.ink.secondary}>
          {label}
        </text>
      )}
    </g>
  );
}

export interface IntervalMarkProps {
  /** Pixel positions. */
  x: number;
  lo: number;
  hi: number;
  y: number;
  color: string;
  /** The interval includes the reference: the dot is drawn hollow and the line lighter. */
  hollow?: boolean;
  r?: number;
}

/** A point estimate with its interval, horizontal. */
export function IntervalMark({ x, lo, hi, y, color, hollow, r = 5 }: IntervalMarkProps) {
  const { theme } = useFrame();
  const stroke = hollow ? theme.ink.tertiary : color;
  return (
    <g>
      <line x1={lo} x2={hi} y1={y} y2={y} stroke={stroke} strokeWidth={2} strokeLinecap="round" />
      <line x1={lo} x2={lo} y1={y - 4} y2={y + 4} stroke={stroke} strokeWidth={2} strokeLinecap="round" />
      <line x1={hi} x2={hi} y1={y - 4} y2={y + 4} stroke={stroke} strokeWidth={2} strokeLinecap="round" />
      {hollow ? <circle cx={x} cy={y} r={r - 1} fill={theme.surface} stroke={stroke} strokeWidth={2} /> : <circle cx={x} cy={y} r={r} fill={color} stroke={theme.surface} strokeWidth={2} />}
    </g>
  );
}

export interface PeakLabel {
  key: string;
  /** The point on the line the label belongs to, in pixels. */
  x: number;
  y: number;
  text: string;
  color: string;
  /** Put the label above the point (a peak) or below it (a trough). */
  side: 'above' | 'below';
}

/**
 * Labels at a chosen point of each line, with a line key in the series colour and the
 * text in ink on a surface halo. A label that would overlap one already placed is
 * moved further from its point, and joined to it by a leader.
 */
export function PeakLabels({ labels }: { labels: PeakLabel[] }) {
  const { plot, theme } = useFrame();
  const placed: { x0: number; x1: number; y0: number; y1: number }[] = [];
  return (
    <g fontSize={11}>
      {labels.map((l) => {
        const w = textWidth(l.text, 11) + 18;
        const x0 = Math.min(plot.right - w, Math.max(plot.x + 2, l.x - w / 2));
        const direction = l.side === 'above' ? -1 : 1;
        let y = l.y + direction * 14;
        for (let tries = 0; tries < 6; tries++) {
          const box = { x0, x1: x0 + w, y0: y - 9, y1: y + 5 };
          const clash = placed.some((p) => box.x0 < p.x1 + 4 && box.x1 > p.x0 - 4 && box.y0 < p.y1 + 1 && box.y1 > p.y0 - 1);
          if (!clash && box.y0 >= plot.y - 2 && box.y1 <= plot.bottom + 2) break;
          y += direction * 14;
          if (y - 9 < plot.y - 2 || y + 5 > plot.bottom + 2) {
            y = l.y - direction * 14;
            break;
          }
        }
        placed.push({ x0, x1: x0 + w, y0: y - 9, y1: y + 5 });
        const moved = Math.abs(y - l.y) > 20;
        return (
          <g key={l.key}>
            {moved && <line x1={l.x} x2={l.x} y1={l.y + direction * 4} y2={y - direction * 6} stroke={theme.axis} strokeWidth={1} />}
            <line x1={x0} x2={x0 + 12} y1={y - 3} y2={y - 3} stroke={l.color} strokeWidth={2} strokeLinecap="round" />
            <text x={x0 + 17} y={y} fill={theme.ink.primary} stroke={theme.surface} strokeWidth={3} paintOrder="stroke" strokeLinejoin="round">
              {l.text}
            </text>
          </g>
        );
      })}
    </g>
  );
}

/** Round tick values for a difference axis that always include zero. */
export function zeroTicks(scale: Scale, count: number): number[] {
  const ticks = scale.ticks(count);
  return ticks.includes(0) ? ticks : [...ticks, 0].sort((a, b) => a - b);
}
