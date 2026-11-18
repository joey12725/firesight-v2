// Axes and gridlines, drawn in SVG inside a `ChartFrame`. Recessive by design: solid
// hairlines one step off the surface, muted tabular labels, the unit stated once.
import type { ReactNode } from 'react';
import { useFrame } from './ChartFrame';
import { formatValue, moneyTicks, percentTick, type ValueFormat } from './format';
import { ageTicks, type Scale, type TimeAxis } from './scales';

export interface AgeAxisProps {
  /** Scale from the first person's age to pixel x. */
  x: Scale;
  time: TimeAxis;
  /** Row label for the primary ticks: "Age" or "Maya's age". */
  label?: string;
  /** Hide the calendar-year row (compact charts). */
  years?: boolean;
  /** Draw the tick marks and labels but no baseline rule (when the plot draws its own). */
  rule?: boolean;
}

/**
 * The time axis: the first person's age as the primary ticks, calendar years beneath
 * as the secondary reading. Needs about 36 px of bottom margin (22 without years).
 */
export function AgeAxis({ x, time, label = 'Age', years = true, rule = true }: AgeAxisProps) {
  const { plot, theme, margin } = useFrame();
  const ticks = ageTicks(x.domain, plot.width);
  const y = plot.bottom;
  const showRowLabels = margin.left >= 40;
  return (
    <g fontSize={11} style={{ fontVariantNumeric: 'tabular-nums' }}>
      {rule && <line x1={plot.x} x2={plot.right} y1={y + 0.5} y2={y + 0.5} stroke={theme.axis} strokeWidth={1} shapeRendering="crispEdges" />}
      {showRowLabels && (
        <>
          <text x={plot.x - 8} y={y + 16} textAnchor="end" fill={theme.label}>
            {label}
          </text>
          {years && (
            <text x={plot.x - 8} y={y + 30} textAnchor="end" fill={theme.ink.tertiary} fontSize={10}>
              Year
            </text>
          )}
        </>
      )}
      {ticks.map((age) => {
        const px = x(age);
        return (
          <g key={age}>
            <line x1={px} x2={px} y1={y} y2={y + 4} stroke={theme.axis} strokeWidth={1} />
            <text x={px} y={y + 16} textAnchor="middle" fill={theme.ink.secondary}>
              {age}
            </text>
            {years && (
              <text x={px} y={y + 30} textAnchor="middle" fill={theme.ink.tertiary} fontSize={10}>
                {time.yearAtAge(age)}
              </text>
            )}
          </g>
        );
      })}
    </g>
  );
}

export interface ValueAxisProps {
  y: Scale;
  format: ValueFormat | ((value: number) => string);
  /** Approximate number of ticks; the scale picks round values. */
  count?: number;
  ticks?: number[];
  /** Stated once above the axis: "Net worth, today's dollars". */
  title?: ReactNode;
  /** Baseline y of the title, when rule annotations occupy the space just above the plot. Default: 8 px above the plot. */
  titleY?: number;
  grid?: boolean;
  /** Emphasise the zero line when the domain crosses it. */
  zero?: boolean;
}

/** Horizontal gridlines with tick labels in the left margin, and the axis title above the plot. */
export function ValueAxis({ y, format, count, ticks, title, titleY, grid = true, zero = true }: ValueAxisProps) {
  const { plot, theme } = useFrame();
  const values = (ticks ?? y.ticks(count ?? Math.max(2, Math.round(plot.height / 56)))).filter((v) => {
    const py = y(v);
    return py >= plot.y - 0.5 && py <= plot.bottom + 0.5;
  });
  const fmt =
    typeof format === 'function'
      ? format
      : format === 'money'
        ? moneyTicks(values)
        : format === 'percent'
          ? (v: number) => percentTick(v)
          : format === 'share100'
            ? (v: number) => String(Math.round(v * 100))
            : (v: number) => formatValue(v, format);
  return (
    <g fontSize={11} style={{ fontVariantNumeric: 'tabular-nums' }}>
      {title && (
        <text x={0} y={titleY ?? plot.y - 8} fill={theme.label} fontSize={11}>
          {title}
        </text>
      )}
      {values.map((v) => {
        const py = Math.round(y(v)) + 0.5;
        const isZero = v === 0 && zero && y.domain[0] < 0;
        return (
          <g key={v}>
            {grid && <line x1={plot.x} x2={plot.right} y1={py} y2={py} stroke={isZero ? theme.axis : theme.grid} strokeWidth={1} shapeRendering="crispEdges" />}
            <text x={plot.x - 8} y={py + 3.5} textAnchor="end" fill={theme.label}>
              {fmt(v)}
            </text>
          </g>
        );
      })}
    </g>
  );
}

/** Vertical gridlines at the age ticks, for charts read along x. */
export function AgeGrid({ x }: { x: Scale }) {
  const { plot, theme } = useFrame();
  return (
    <g>
      {ageTicks(x.domain, plot.width).map((age) => {
        const px = Math.round(x(age)) + 0.5;
        return <line key={age} x1={px} x2={px} y1={plot.y} y2={plot.bottom} stroke={theme.grid} strokeWidth={1} shapeRendering="crispEdges" />;
      })}
    </g>
  );
}
