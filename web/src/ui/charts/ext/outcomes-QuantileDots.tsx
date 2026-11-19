// A quantile dot plot for any per-life quantity (Kay et al. 2016): each dot is one of
// 100 equally likely lives, stacked where its value falls. Generalises the Overview's
// age-work-becomes-optional chart: any linear or log axis, and any number of named
// groups for lives that have no place on the axis ("never", "no cut", "nothing left"),
// which are drawn as their own stack beside it instead of being dropped.
import type { ReactNode } from 'react';
import { AgeAxis } from '../Axis';
import { ChartFrame, SvgLayer, useFrame } from '../ChartFrame';
import { Cursor, TipNote, TipRow, TipTitle } from '../Cursor';
import { linearScale, type Scale, type TimeAxis } from '../scales';
import { ChartSyncProvider } from '../sync';
import { XAxis, logMoneyScale, type XFormat } from './outcomes-XAxis';

export interface DotGroup {
  key: string;
  /** Under the stack: "Never", "No cut". */
  label: string;
  side: 'left' | 'right';
  /** Hollow dots, for "it never happened". */
  hollow?: boolean;
  /** One sentence for the tooltip. */
  note?: string;
}

export interface DotsLayoutInput {
  dots: (number | null)[];
  /** The named group a dot belongs to, or null when it sits on the axis. */
  groupOf?: (value: number | null) => string | null;
  groups?: DotGroup[];
  scale: 'linear' | 'log';
  /** Data-space bin widths to choose from (whole ages, whole years). Without it, bins are one dot wide in pixels. */
  steps?: number[];
  domain?: [number, number];
  width: number;
  height: number;
  maxDiameter?: number;
  /** A fixed width for each named group, so that dot plots stacked on one axis line up whatever their dot size. */
  groupWidth?: number;
}

export interface DotBin {
  /** Data range covered: [lo, hi). */
  lo: number;
  hi: number;
  cx: number;
  count: number;
  /** Dots at or below the top of this bin, groups on the left included. */
  cumulative: number;
}

export interface DotsLayout {
  diameter: number;
  /** Pixel extent of the axis part, from the left edge of the layout. */
  axis: [number, number];
  domain: [number, number];
  bins: DotBin[];
  groups: { group: DotGroup; count: number; x0: number; width: number; cumulative: number }[];
  /** One per input dot, in input order: where it is drawn. `row` counts up from the baseline. */
  circles: { cx: number; row: number; group: string | null; bin: number }[];
  total: number;
}

const GROUP_GAP = 26;

const transform = (kind: 'linear' | 'log') => (kind === 'log' ? (v: number) => Math.log10(Math.max(v, 1e-9)) : (v: number) => v);

function dataDomain(values: number[], kind: 'linear' | 'log', steps: number[] | undefined): [number, number] {
  if (values.length === 0) return kind === 'log' ? [1, 10] : [0, 1];
  const lo = Math.min(...values);
  const hi = Math.max(...values);
  if (kind === 'log') return [Math.max(1, lo) / 1.12, Math.max(lo * 1.5, hi) * 1.12];
  if (steps) return [lo, hi];
  const pad = (hi - lo || Math.abs(hi) || 1) * 0.04;
  return [lo - pad, hi + pad];
}

/**
 * Places the dots. Pure, so the picture can be tested: every input dot gets exactly one
 * place, either in a bin on the axis or in its named group, and the stacks fit the height.
 */
export function layoutDots(input: DotsLayoutInput): DotsLayout {
  const { dots, groupOf, groups = [], scale, steps, width, height } = input;
  const maxDiameter = input.maxDiameter ?? 14;
  const membership = dots.map((d) => (groupOf ? groupOf(d) : d === null ? (groups[0]?.key ?? null) : null));
  const onAxis = dots.filter((d, i): d is number => membership[i] === null && d !== null);
  const used = groups.map((group) => ({ group, count: membership.filter((m) => m === group.key).length })).filter((g) => g.count > 0 || input.groupWidth !== undefined);
  const t = transform(scale);
  const [lo, top] = input.domain ?? dataDomain(onAxis, scale, steps);
  const hi = top > lo ? top : lo + 1;

  const attempt = (diameter: number, step: number | undefined) => {
    const rowsMax = Math.max(1, Math.floor(height / diameter));
    const natural = (count: number) => Math.max(2, Math.ceil(count / Math.min(rowsMax, 10))) * (diameter + 1);
    if (input.groupWidth !== undefined && used.some((g) => natural(g.count) > input.groupWidth! + 0.5)) return null;
    // A group is at least as wide as its own label (about 6 px a character at 11 px), so the label never runs into the axis beside it.
    const groupWidth = (count: number, label = '') => input.groupWidth ?? Math.max(natural(count), Math.min(72, label.length * 6));
    const reserved = used.reduce((w, g) => w + groupWidth(g.count, g.group.label) + GROUP_GAP, 0);
    const axisWidth = width - reserved;
    if (axisWidth < 80) return null;
    let d0 = lo;
    let d1 = hi;
    let columns: number;
    // Dots side by side within one bin: a whole-year bin is often wide enough for several, which keeps a spike (everyone retiring at 50) inside the plot.
    let perBin = 1;
    if (step !== undefined) {
      d0 = Math.floor(lo / step + 1e-9) * step;
      d1 = Math.max(d0 + step, Math.ceil(hi / step - 1e-9) * step);
      columns = Math.max(1, Math.round((d1 - d0) / step));
      perBin = Math.floor(axisWidth / columns / diameter);
      if (perBin < 1) return null;
    } else {
      columns = Math.max(1, Math.floor(axisWidth / diameter));
    }
    const span = t(d1) - t(d0);
    const counts = new Array<number>(columns).fill(0);
    const columnOf = (v: number) => Math.min(columns - 1, Math.max(0, Math.floor(((t(v) - t(d0)) / span) * columns + 1e-9)));
    for (const v of onAxis) counts[columnOf(v)] = (counts[columnOf(v)] ?? 0) + 1;
    if (Math.ceil(Math.max(0, ...counts) / perBin) * diameter > height) return null;
    // No wider than the tallest stack needs.
    perBin = Math.max(1, Math.min(perBin, Math.ceil(Math.max(0, ...counts) / Math.max(1, Math.floor(height / diameter)))));
    return { diameter, d0, d1, columns, counts, columnOf, groupWidth, axisWidth, perBin };
  };

  // The finest bins that still give dots big enough to count; failing that, whichever bins give the biggest dots.
  type Fit = NonNullable<ReturnType<typeof attempt>>;
  let fit = null as Fit | null;
  for (const step of steps ?? [undefined]) {
    let best = null as Fit | null;
    for (let d = maxDiameter; d >= 4 && !best; d -= 0.5) best = attempt(d, step);
    if (best && (!fit || best.diameter > fit.diameter)) fit = best;
    if (fit && fit.diameter >= 8) break;
  }
  // Nothing fits (a very narrow frame): the smallest dots on the coarsest bins, overflow clipped by the stack height.
  fit ??= attempt(4, steps?.[steps.length - 1]) ?? { diameter: 4, d0: lo, d1: hi, columns: 1, counts: [onAxis.length], columnOf: () => 0, groupWidth: () => 10, axisWidth: Math.max(1, width), perBin: 1 };

  const { diameter, d0, d1, columns, counts, columnOf, groupWidth, perBin } = fit;
  let cursor = 0;
  const placed: DotsLayout['groups'] = [];
  for (const g of used.filter((u) => u.group.side === 'left')) {
    placed.push({ ...g, x0: cursor, width: groupWidth(g.count, g.group.label), cumulative: 0 });
    cursor += groupWidth(g.count, g.group.label) + GROUP_GAP;
  }
  const axis0 = cursor;
  const axisWidth = fit.axisWidth;
  const columnWidth = axisWidth / columns;
  cursor += axisWidth;
  for (const g of used.filter((u) => u.group.side === 'right')) {
    cursor += GROUP_GAP;
    placed.push({ ...g, x0: cursor, width: groupWidth(g.count, g.group.label), cumulative: 0 });
    cursor += groupWidth(g.count, g.group.label);
  }

  const inverse = scale === 'log' ? (x: number) => 10 ** x : (x: number) => x;
  const edge = (c: number) => inverse(t(d0) + ((t(d1) - t(d0)) * c) / columns);
  let running = placed.filter((g) => g.group.side === 'left').reduce((n, g) => n + g.count, 0);
  placed.filter((g) => g.group.side === 'left').reduce((n, g) => (g.cumulative = n + g.count), 0);
  const bins: DotBin[] = [];
  const binIndex = new Map<number, number>();
  counts.forEach((count, c) => {
    if (count === 0) return;
    running += count;
    binIndex.set(c, bins.length);
    bins.push({ lo: edge(c), hi: edge(c + 1), cx: axis0 + (c + 0.5) * columnWidth, count, cumulative: running });
  });
  for (const g of placed.filter((p) => p.group.side === 'right')) {
    running += g.count;
    g.cumulative = running;
  }

  const stack = new Map<string, number>();
  const rowsMax = Math.max(1, Math.min(10, Math.floor(height / diameter)));
  const circles = dots.map((d, i) => {
    const key = membership[i] ?? null;
    if (key !== null || d === null) {
      const g = placed.find((p) => p.group.key === key);
      const n = stack.get(`g:${key}`) ?? 0;
      stack.set(`g:${key}`, n + 1);
      const x0 = g?.x0 ?? 0;
      return { cx: x0 + Math.floor(n / rowsMax) * (diameter + 1) + diameter / 2, row: n % rowsMax, group: key, bin: -1 };
    }
    const c = columnOf(d);
    const n = stack.get(`c:${c}`) ?? 0;
    stack.set(`c:${c}`, n + 1);
    const cx = axis0 + (c + 0.5) * columnWidth + ((n % perBin) - (perBin - 1) / 2) * diameter;
    return { cx, row: Math.floor(n / perBin), group: null, bin: binIndex.get(c) ?? -1 };
  });

  return { diameter, axis: [axis0, axis0 + axisWidth], domain: [d0, d1], bins, groups: placed, circles, total: dots.length };
}

export interface DotInterval {
  p10: number | null;
  p50: number | null;
  p90: number | null;
}

export interface QuantileDotsProps {
  /** Equally likely lives (the engine's 100 quantile dots, or one per life when fewer were simulated). */
  dots: (number | null)[];
  groupOf?: (value: number | null) => string | null;
  groups?: DotGroup[];
  scale?: 'linear' | 'log';
  steps?: number[];
  domain?: [number, number];
  /** How a value reads in a tooltip and under the interval bar. */
  format: (value: number) => string;
  /** How a bin reads: "ages 38 to 39", "$1.2M to $1.5M". Defaults to the formatted range. */
  formatRange?: (lo: number, hi: number) => string;
  axis: { kind: 'age'; time: TimeAxis; years?: boolean } | { kind: 'value'; format: XFormat; title?: string; ticks?: number[] };
  /** What the quantity is, for assistive technology: "net worth at the end of life". */
  noun: string;
  /** A short label set at the top left of the plot, when several dot plots share an axis. */
  rowLabel?: string;
  color?: string;
  /** The middle 80 of 100 lives and the median, drawn under the axis. Null values mean "in a group". */
  interval?: DotInterval;
  intervalText?: string;
  height?: number;
  /** Fixed width reserved for each named group (see `DotsLayoutInput.groupWidth`). */
  groupWidth?: number;
  note?: (bin: DotBin) => ReactNode;
}

const GLIDE = { transition: 'cx var(--duration-slow) var(--easing), cy var(--duration-slow) var(--easing)' };

export function QuantileDots(props: QuantileDotsProps) {
  const axisBand = props.axis.kind === 'age' ? (props.axis.years === false ? 22 : 36) : props.axis.title ? 38 : 22;
  const intervalBand = props.interval ? (props.intervalText ? 30 : 14) : 0;
  return (
    // A dot plot's positions are bins, not years: it keeps its own focus so it never moves a time chart's crosshair.
    <ChartSyncProvider>
      <ChartFrame
        height={props.height ?? 170 + axisBand + intervalBand}
        margin={{ top: props.rowLabel ? 20 : 8, right: 8, bottom: axisBand + intervalBand + 4, left: 8 }}
        aria-label={`${props.noun}: one dot for each of ${props.dots.length} equally likely simulated lives`}
      >
        <Body {...props} axisBand={axisBand} />
      </ChartFrame>
    </ChartSyncProvider>
  );
}

function Body({ dots, groupOf, groups, scale = 'linear', steps, domain, format, formatRange, axis, noun, rowLabel, color, interval, intervalText, note, groupWidth, axisBand }: QuantileDotsProps & { axisBand: number }) {
  const { plot, theme } = useFrame();
  const layout = layoutDots({ dots, groupOf, groups, scale, steps, domain, width: plot.width, height: plot.height, groupWidth });
  const { diameter, bins, total } = layout;
  const x0 = plot.x + layout.axis[0];
  const x1 = plot.x + layout.axis[1];
  const x: Scale = scale === 'log' ? logMoneyScale(layout.domain, [x0, x1]) : linearScale(layout.domain, [x0, x1]);
  const r = diameter / 2 - 0.75;
  const base = plot.bottom - 1;
  const fill = color ?? theme.baseline.line;
  const range = formatRange ?? ((lo: number, hi: number) => `${format(lo)} to ${format(hi)}`);

  // Cursor positions: left groups, bins, right groups, in reading order.
  const left = layout.groups.filter((g) => g.group.side === 'left' && g.count > 0);
  const right = layout.groups.filter((g) => g.group.side === 'right' && g.count > 0);
  type Stop = { kind: 'bin'; bin: DotBin; index: number; x: number } | { kind: 'group'; group: (typeof layout.groups)[number]; x: number };
  const stops: Stop[] = [
    ...left.map((group): Stop => ({ kind: 'group', group, x: plot.x + group.x0 + group.width / 2 })),
    ...bins.map((bin, index): Stop => ({ kind: 'bin', bin, index, x: plot.x + bin.cx })),
    ...right.map((group): Stop => ({ kind: 'group', group, x: plot.x + group.x0 + group.width / 2 })),
  ];
  const describe = (i: number) => {
    const s = stops[i];
    if (!s) return '';
    if (s.kind === 'group') return `${s.group.group.label}: ${s.group.count} of ${total} lives.${s.group.group.note ? ` ${s.group.group.note}` : ''}`;
    return `${range(s.bin.lo, s.bin.hi)}: ${s.bin.count} of ${total} lives. This or less: ${s.bin.cumulative} of ${total}.`;
  };
  const isFocused = (c: (typeof layout.circles)[number], s: Stop) => (s.kind === 'group' ? c.group === s.group.group.key : c.bin === s.index);

  const p10 = interval?.p10 ?? null;
  const p50 = interval?.p50 ?? null;
  const p90 = interval?.p90 ?? null;
  const barY = plot.bottom + axisBand + 8;
  const clampX = (v: number) => Math.min(x1, Math.max(x0, x(v)));

  return (
    <>
      <SvgLayer>
        {axis.kind === 'age' ? <AgeAxis x={x} time={axis.time} years={axis.years !== false} label="" rule={false} /> : <XAxis x={x} format={axis.format} title={axis.title} ticks={axis.ticks} from={x0} to={x1} />}
        {axis.kind === 'age' && <line x1={x0} x2={x1} y1={plot.bottom + 0.5} y2={plot.bottom + 0.5} stroke={theme.axis} strokeWidth={1} shapeRendering="crispEdges" />}
        {rowLabel && (
          <text x={plot.x} y={plot.y - 8} fontSize={11} fontWeight={500} fill={theme.ink.secondary}>
            {rowLabel}
          </text>
        )}
        {[...left, ...right].map((g) => (
          <g key={g.group.key} fontSize={11}>
            <line x1={plot.x + g.x0 - 3} x2={plot.x + g.x0 + g.width + 3} y1={plot.bottom + 0.5} y2={plot.bottom + 0.5} stroke={theme.axis} strokeWidth={1} />
            <text x={plot.x + g.x0 + g.width / 2} y={plot.bottom + 16} textAnchor={g.group.side === 'left' ? (g.x0 + g.width / 2 < 30 ? 'start' : 'middle') : plot.width - g.x0 - g.width / 2 < 30 ? 'end' : 'middle'} dx={g.group.side === 'left' && g.x0 + g.width / 2 < 30 ? -g.width / 2 : g.group.side === 'right' && plot.width - g.x0 - g.width / 2 < 30 ? g.width / 2 : 0} fill={theme.ink.secondary}>
              {g.group.label}
            </text>
          </g>
        ))}
        {layout.circles.map((c, i) => {
          const hollow = c.group !== null && (groups?.find((g) => g.key === c.group)?.hollow ?? false);
          const cy = base - diameter / 2 - c.row * diameter;
          return hollow ? (
            <circle key={i} cx={plot.x + c.cx} cy={cy} r={Math.max(1, r - 0.5)} fill="none" stroke={theme.ink.tertiary} strokeWidth={1.5} style={GLIDE} />
          ) : (
            <circle key={i} cx={plot.x + c.cx} cy={cy} r={Math.max(1, r)} fill={c.group === null ? fill : theme.ink.tertiary} style={GLIDE} />
          );
        })}
        {interval && p10 !== null && (
          <g>
            <line x1={clampX(p10)} x2={p90 === null ? x1 : clampX(p90)} y1={barY} y2={barY} stroke={theme.ink.secondary} strokeWidth={2} strokeLinecap="round" strokeDasharray={p90 === null ? '2 4' : undefined} />
            <line x1={clampX(p10)} x2={clampX(p10)} y1={barY - 3} y2={barY + 3} stroke={theme.ink.secondary} strokeWidth={2} strokeLinecap="round" />
            {p90 !== null && <line x1={clampX(p90)} x2={clampX(p90)} y1={barY - 3} y2={barY + 3} stroke={theme.ink.secondary} strokeWidth={2} strokeLinecap="round" />}
            {p50 !== null && <circle cx={clampX(p50)} cy={barY} r={4} fill={theme.ink.primary} stroke={theme.surface} strokeWidth={2} />}
          </g>
        )}
        {interval && intervalText && (
          <text x={x0} y={barY + 17} fontSize={11} fill={theme.ink.secondary}>
            {intervalText}
          </text>
        )}
      </SvgLayer>
      <Cursor
        count={stops.length}
        xOf={(i) => stops[i]?.x ?? 0}
        label={`${noun}, ${total} simulated lives`}
        describe={describe}
        crosshair={false}
        marks={(i) => {
          const s = stops[i];
          if (!s) return null;
          return (
            <g>
              {layout.circles.map((c, k) => (isFocused(c, s) ? <circle key={k} cx={plot.x + c.cx} cy={base - diameter / 2 - c.row * diameter} r={r + 1.5} fill="none" stroke={theme.ink.primary} strokeWidth={1.5} /> : null))}
            </g>
          );
        }}
        tooltip={(i) => {
          const s = stops[i];
          if (!s) return null;
          if (s.kind === 'group')
            return (
              <>
                <TipTitle>{s.group.group.label}</TipTitle>
                <TipRow label="Lives" value={`${s.group.count} of ${total}`} strong />
                {s.group.group.note && <TipNote>{s.group.group.note}</TipNote>}
              </>
            );
          return (
            <>
              <TipTitle>{range(s.bin.lo, s.bin.hi)}</TipTitle>
              <TipRow color={fill} swatch="box" label="Lives here" value={`${s.bin.count} of ${total}`} />
              <TipRow label="This or less" value={`${s.bin.cumulative} of ${total}`} strong />
              {note && <TipNote>{note(s.bin)}</TipNote>}
            </>
          );
        }}
      />
    </>
  );
}
