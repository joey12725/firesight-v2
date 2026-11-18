// A horizontal value axis, for charts whose x is not time (a return, a dollar amount, a
// count of years). The kit's `AgeAxis` covers time; this is its counterpart for
// everything else: a hairline rule, round ticks, and the unit stated once beneath.
import { useFrame } from '../ChartFrame';
import { moneyTicks, percentTick } from '../format';
import { symlogScale, type Scale } from '../scales';

export type XFormat = 'money' | 'percent' | 'number' | ((value: number) => string);

export interface XAxisProps {
  x: Scale;
  format: XFormat;
  ticks?: number[];
  /** Approximate number of ticks when `ticks` is not given. */
  count?: number;
  /** The quantity and its unit, stated once: "Net worth at the end, today's dollars". */
  title?: string;
  /** Where the rule starts and ends, when the axis occupies only part of the plot. */
  from?: number;
  to?: number;
  /** Vertical hairlines at the ticks, up through the plot. */
  grid?: boolean;
  /** Pixel y of the axis; the plot's bottom edge by default. */
  y?: number;
}

export function xTickFormat(format: XFormat, ticks: number[]): (v: number) => string {
  if (typeof format === 'function') return format;
  if (format === 'money') return moneyTicks(ticks);
  if (format === 'percent') {
    const fine = ticks.some((t) => Math.abs(t * 100 - Math.round(t * 100)) > 1e-6);
    return (v) => percentTick(v, fine ? 1 : 0).replace('-', '−');
  }
  return (v) => (Number.isInteger(v) ? String(v) : v.toFixed(1));
}

/** Tick values that keep at least `gap` px between neighbours. */
export function thinTicks(ticks: number[], x: (v: number) => number, gap: number): number[] {
  const out: number[] = [];
  let last = -Infinity;
  for (const t of ticks) {
    const px = x(t);
    if (px - last >= gap) {
      out.push(t);
      last = px;
    }
  }
  return out;
}

export function XAxis({ x, format, ticks, count, title, from, to, grid, y }: XAxisProps) {
  const { plot, theme } = useFrame();
  const x0 = from ?? plot.x;
  const x1 = to ?? plot.right;
  const base = y ?? plot.bottom;
  const wanted = (ticks ?? x.ticks(count ?? Math.max(2, Math.floor((x1 - x0) / 72)))).filter((v) => x(v) >= x0 - 0.5 && x(v) <= x1 + 0.5);
  const fmt = xTickFormat(format, wanted);
  const values = thinTicks(wanted, x, 44);
  return (
    <g fontSize={11} style={{ fontVariantNumeric: 'tabular-nums' }}>
      {grid && values.map((v) => <line key={`g${v}`} x1={Math.round(x(v)) + 0.5} x2={Math.round(x(v)) + 0.5} y1={plot.y} y2={base} stroke={theme.grid} strokeWidth={1} shapeRendering="crispEdges" />)}
      <line x1={x0} x2={x1} y1={base + 0.5} y2={base + 0.5} stroke={theme.axis} strokeWidth={1} shapeRendering="crispEdges" />
      {values.map((v) => {
        const px = x(v);
        return (
          <g key={v}>
            <line x1={px} x2={px} y1={base} y2={base + 4} stroke={theme.axis} strokeWidth={1} />
            <text x={px} y={base + 16} textAnchor="middle" fill={theme.ink.secondary}>
              {fmt(v)}
            </text>
          </g>
        );
      })}
      {title && (
        <text x={x0} y={base + 31} fill={theme.label} fontSize={10.5}>
          {title}
        </text>
      )}
    </g>
  );
}

/**
 * A log-like scale for strictly positive money that spans decades (lifetime tax, wealth
 * at the end): the kit's symlog with its linear zone shrunk to the bottom of the domain,
 * so ticks fall on 1 and 3 times powers of ten.
 */
export function logMoneyScale(domain: [number, number], range: [number, number]): Scale {
  const lo = Math.max(1, domain[0]);
  const constant = 10 ** Math.floor(Math.log10(lo));
  return symlogScale([lo, Math.max(lo * 1.01, domain[1])], range, constant);
}
