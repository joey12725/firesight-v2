// A stacked area chart over the age axis, in SVG, that can stack in both directions
// from zero: "up" series above the line and "down" series below it on one scale (money
// in against money out, assets against debts, paid in against drawn out). Bands are
// labelled inside where the name fits, lines can ride over the stack (a median, a
// total), a second measure can sit in its own strip beneath the plot, and one year can
// be marked as selected. It follows the view's shared crosshair and time range.
import { useMemo, type ReactNode } from 'react';
import { moneyCompact } from '@/ui';
import {
  AgeAxis,
  ChartFrame,
  Cursor,
  EndLabels,
  Legend,
  PlotClip,
  RuleAnnotations,
  SvgLayer,
  TipNote,
  TipRow,
  TipTitle,
  ValueAxis,
  inkOn,
  linearScale,
  moneyTicks,
  percentTick,
  rulesHeight,
  textWidth,
  useAnimated,
  useChartSync,
  useTimeRange,
  withAlpha,
  type EndLabel,
  type RuleItem,
  type TimeAxis,
} from '@/ui/charts';

export interface StackSeries {
  key: string;
  label: string;
  /** Shorter name for the label inside the band. */
  short?: string;
  color: string;
  /** By year index, as magnitudes (never negative). Null is treated as zero. */
  values: readonly (number | null)[];
  /** 'down' stacks below the zero line. Default 'up'. */
  side?: 'up' | 'down';
}

export interface StackLine {
  key: string;
  label: string;
  /** Short name for the label at the line's end, when the full label would not fit in the margin. */
  end?: string;
  color: string;
  /** By year index, signed, on the same scale as the stack. Null breaks the line. */
  values: readonly (number | null)[];
  strong?: boolean;
  /** Dashed: a notional amount that is not part of the stack (paper value outside net worth). */
  dash?: boolean;
}

export interface StackStrip {
  /** Label inside the strip: "Tax as a share of money in". */
  label: string;
  values: readonly (number | null)[];
  format: (value: number) => string;
  height?: number;
}

export interface StackMoment {
  age: number;
  label: string;
  detail?: string;
  tone?: RuleItem['tone'];
}

export interface StackedAreasProps {
  time: TimeAxis;
  series: StackSeries[];
  lines?: StackLine[];
  /** First and last year index that carry data (default: the whole axis). */
  from?: number;
  to?: number;
  format?: 'money' | 'percent';
  /** Axis title: "Average per household, today's dollars". */
  title?: string;
  /** Names of the two halves, written inside the plot at the left: { up: 'Money in', down: 'Money out' }. */
  sides?: { up: string; down: string };
  /** Tooltip rows for the totals of each half. */
  totals?: { up?: string; down?: string };
  moments?: StackMoment[];
  strip?: StackStrip;
  /** A marked year (index), with the action that moves it. */
  selected?: number | null;
  onSelect?: (index: number) => void;
  tooltipNote?: (index: number) => ReactNode;
  height?: number | ((width: number) => number);
  compact?: boolean;
  /** Changes when the plan's shape changes, so a different plan snaps instead of morphing. */
  resetKey: string;
  /** Accessible name: "Money in and money out by age". */
  label: string;
}

const num = (v: number | null | undefined) => (v === null || v === undefined || !Number.isFinite(v) ? 0 : v);

/** The legend for a stacked chart, in reading order: the up stack from the top, then the down stack from the line. */
export function StackLegend({ series, lines, className }: { series: StackSeries[]; lines?: StackLine[]; className?: string }) {
  const up = series.filter((s) => s.side !== 'down').reverse();
  const down = series.filter((s) => s.side === 'down');
  return (
    <div data-chart-legend>
      <Legend
        className={className}
        items={[
          ...(lines ?? []).map((l) => ({ key: l.key, label: l.label, color: l.color, swatch: l.dash ? ('dash' as const) : ('line' as const) })),
          ...[...up, ...down].map((s) => ({ key: s.key, label: s.label, color: s.color })),
        ]}
      />
    </div>
  );
}

export function StackedAreas({ time, series, lines = [], from = 0, to, format = 'money', title, sides, totals, moments = [], strip, selected, onSelect, tooltipNote, height, compact, resetKey, label }: StackedAreasProps) {
  const sync = useChartSync();
  const range = useTimeRange(time, sync);
  const target = useMemo(() => series.map((s) => s.values.map(num)), [series]);
  const values = useAnimated(target, { resetKey: `${resetKey}:${series.map((s) => s.key).join(',')}`, duration: 360 });
  const end = Math.min(to ?? time.length - 1, time.length - 1);
  const first = Math.max(range.first, from);
  const last = Math.min(range.last, end);
  const index = useMemo(() => Array.from({ length: Math.max(0, last - first + 1) }, (_, k) => first + k), [first, last]);

  const ups = series.map((s, n) => ({ s, n })).filter((e) => e.s.side !== 'down');
  const downs = series.map((s, n) => ({ s, n })).filter((e) => e.s.side === 'down');
  // Cumulative tops of each half, from the animated values (drawn) and the target (scale, tooltip).
  const cumulate = (members: typeof ups, source: number[][]) => {
    const running = new Array<number>(time.length).fill(0);
    return members.map(({ n }) => {
      const row = source[n] ?? [];
      for (let i = 0; i < time.length; i++) running[i] = (running[i] as number) + num(row[i]);
      return running.slice();
    });
  };
  const upTops = cumulate(ups, values);
  const downTops = cumulate(downs, values);
  const sum = (members: typeof ups, i: number) => members.reduce((t, { n }) => t + num(target[n]?.[i]), 0);

  let top = 0;
  let bottom = 0;
  for (const i of index) {
    top = Math.max(top, sum(ups, i));
    bottom = Math.max(bottom, sum(downs, i));
    for (const l of lines) {
      const v = l.values[i];
      if (v !== null && v !== undefined) {
        top = Math.max(top, v);
        bottom = Math.max(bottom, -v);
      }
    }
  }
  if (top === 0 && bottom === 0) top = 1;

  const stripHeight = strip && !compact ? (strip.height ?? 44) : 0;
  const stripGap = stripHeight ? 22 : 0;
  const axisBand = compact ? 22 : 36;
  const hasEndLabels = lines.length > 0;
  const margin = {
    top: compact ? 8 : moments.length ? rulesHeight(moments.length) + 14 : 22,
    right: hasEndLabels ? (compact ? 62 : 76) : compact ? 8 : 12,
    bottom: axisBand + stripHeight + stripGap,
    left: compact ? 40 : 52,
  };

  return (
    <ChartFrame
      height={height ?? ((w) => (compact ? 190 : Math.round(Math.min(downs.length ? 420 : 340, Math.max(downs.length ? 300 : 240, w * (downs.length ? 0.5 : 0.42))) + stripHeight + stripGap)))}
      margin={margin}
      aria-label={label}
    >
      {(frame) => {
        const { plot, theme } = frame;
        const x = linearScale(range.domain, [plot.x, plot.right]);
        const y = linearScale([-bottom * 1.06, top * 1.06], [plot.bottom, plot.y]);
        const xs = index.map((i) => x(time.ages[i] as number));
        const ticks = y.ticks(compact ? 3 : Math.max(3, Math.round(plot.height / 52)));
        const tickFormat = format === 'money' ? moneyTicks(ticks.map(Math.abs)) : (v: number) => percentTick(v);
        const fmt = (v: number) => (format === 'money' ? moneyCompact(v) : percentTick(v, 1));
        const step = Math.max(1, (xs[1] ?? 1) - (xs[0] ?? 0));

        const band = (tops: number[][], k: number, sign: 1 | -1, s: StackSeries) => {
          const outer = tops[k] ?? [];
          const inner = k === 0 ? null : (tops[k - 1] ?? []);
          const at = (row: number[] | null, i: number) => y(sign * (row ? num(row[i]) : 0));
          let d = '';
          index.forEach((i, j) => (d += `${j ? 'L' : 'M'}${(xs[j] as number).toFixed(1)},${at(outer, i).toFixed(1)}`));
          for (let j = index.length - 1; j >= 0; j--) d += `L${(xs[j] as number).toFixed(1)},${at(inner, index[j] as number).toFixed(1)}`;
          // A label where the band is thickest, if the name fits inside it there.
          let placed: { x: number; y: number; text: string } | null = null;
          for (const text of s.short && s.short !== s.label ? [s.label, s.short] : [s.label]) {
            const half = Math.ceil((textWidth(text, 11) / 2 + 6) / step);
            let room = 15;
            for (let j = half; j < index.length - half; j++) {
              let hi = -Infinity;
              let lo = Infinity;
              for (let q = j - half; q <= j + half; q++) {
                const a = at(outer, index[q] as number);
                const b = at(inner, index[q] as number);
                hi = Math.max(hi, Math.min(a, b));
                lo = Math.min(lo, Math.max(a, b));
              }
              const mid = (hi + lo) / 2;
              // Not where a line would run through the words.
              let crossed = false;
              for (const l of lines) {
                for (let q = j - half; q <= j + half && !crossed; q++) {
                  const v = l.values[index[q] as number];
                  if (v !== null && v !== undefined && Math.abs(y(v) - mid) < 10) crossed = true;
                }
              }
              if (lo - hi > room && !crossed) {
                room = lo - hi;
                placed = { x: xs[j] as number, y: (hi + lo) / 2 + 4, text };
              }
            }
            if (placed) break;
          }
          return { key: s.key, d, color: s.color, label: placed as { x: number; y: number; text: string } | null };
        };
        const areas = [...ups.map(({ s }, k) => band(upTops, k, 1, s)), ...downs.map(({ s }, k) => band(downTops, k, -1, s))];

        const linePath = (row: readonly (number | null)[]) => {
          let d = '';
          let pen = false;
          index.forEach((i, j) => {
            const v = row[i];
            if (v === null || v === undefined) {
              pen = false;
              return;
            }
            d += `${pen ? 'L' : 'M'}${(xs[j] as number).toFixed(1)},${y(v).toFixed(1)}`;
            pen = true;
          });
          return d;
        };
        const endLabels: EndLabel[] = lines.flatMap((l) => {
          for (let j = index.length - 1; j >= 0; j--) {
            const v = l.values[index[j] as number];
            if (v !== null && v !== undefined) return [{ key: l.key, y: Math.min(plot.bottom, Math.max(plot.y, y(v))), text: l.end ?? l.label, color: l.color, strong: l.strong }];
          }
          return [];
        });

        const stripTop = plot.bottom + axisBand + stripGap;
        const stripMax = strip ? Math.max(1e-9, ...index.map((i) => num(strip.values[i]))) : 1;
        const yStrip = linearScale([0, stripMax], [stripTop + stripHeight, stripTop + 4]);
        let stripPath = '';
        if (strip && stripHeight && index.length) {
          stripPath = `M${(xs[0] as number).toFixed(1)},${yStrip(0).toFixed(1)}`;
          index.forEach((i, j) => (stripPath += `L${(xs[j] as number).toFixed(1)},${yStrip(num(strip.values[i])).toFixed(1)}`));
          stripPath += `L${(xs[xs.length - 1] as number).toFixed(1)},${yStrip(0).toFixed(1)}Z`;
        }
        const selectedX = selected !== null && selected !== undefined && selected >= first && selected <= last ? x(time.ages[selected] as number) : null;

        return (
          <>
            <SvgLayer>
              <ValueAxis y={y} format={(v) => tickFormat(Math.abs(v))} ticks={ticks} title={compact ? undefined : title} titleY={moments.length ? 10 : undefined} />
              <PlotClip>
                {areas.map((a) => (
                  <path key={a.key} d={a.d} fill={a.color} stroke={theme.surface} strokeWidth={1.5} strokeLinejoin="round" />
                ))}
                {lines.map((l) => (
                  <g key={l.key}>
                    <path d={linePath(l.values)} fill="none" stroke={theme.surface} strokeWidth={l.strong ? 5 : 4} strokeLinejoin="round" strokeLinecap="round" />
                    <path d={linePath(l.values)} fill="none" stroke={l.color} strokeWidth={l.strong ? 2 : 1.5} strokeDasharray={l.dash ? '5 4' : undefined} strokeLinejoin="round" strokeLinecap="round" />
                  </g>
                ))}
              </PlotClip>
              {downs.length > 0 && <line x1={plot.x} x2={plot.right} y1={Math.round(y(0)) + 0.5} y2={Math.round(y(0)) + 0.5} stroke={theme.ink.secondary} strokeWidth={1} shapeRendering="crispEdges" />}
              <AgeAxis x={x} time={time} years={!compact} label={compact ? '' : 'Age'} />
              {!compact && moments.length > 0 && <RuleAnnotations items={moments.map((m) => ({ x: x(m.age), label: m.label, detail: m.detail, tone: m.tone ?? 'reference' }))} />}
              <g fontSize={11} fontWeight={500}>
                {areas.map(
                  (a) =>
                    a.label && (
                      <text key={a.key} x={a.label.x} y={a.label.y} textAnchor="middle" fill={inkOn(a.color)}>
                        {a.label.text}
                      </text>
                    ),
                )}
              </g>
              {sides &&
                !compact &&
                downs.length > 0 &&
                y(0) - plot.y > 40 &&
                plot.bottom - y(0) > 40 &&
                (() => {
                  // Each caption goes to the end of the plot where its half of the stack is shorter.
                  const a = index[0] as number;
                  const b = index[index.length - 1] as number;
                  const caption = (text: string, members: typeof ups, py: number) => {
                    const atLeft = sum(members, a) < sum(members, b);
                    return (
                      <text x={atLeft ? plot.x + 4 : plot.right - 2} y={py} textAnchor={atLeft ? 'start' : 'end'} stroke={theme.surface} strokeWidth={3} paintOrder="stroke">
                        {text}
                      </text>
                    );
                  };
                  return (
                    <g fontSize={10} fill={theme.label}>
                      {caption(`${sides.up} ↑`, ups, plot.y + 10)}
                      {caption(`${sides.down} ↓`, downs, plot.bottom - 5)}
                    </g>
                  );
                })()}
              {endLabels.length > 0 && <EndLabels labels={endLabels} gap={compact ? 12 : 14} />}
              {selectedX !== null && (
                <g>
                  <line x1={selectedX} x2={selectedX} y1={plot.y} y2={plot.bottom} stroke={theme.ink.primary} strokeWidth={1} strokeOpacity={0.7} />
                  <path d={`M${selectedX - 5},${plot.y - 6}h10l-5,6z`} fill={theme.ink.primary} />
                  <path d={`M${selectedX - 5},${plot.bottom + 6}h10l-5,-6z`} fill={theme.ink.primary} />
                </g>
              )}
              {strip && stripHeight > 0 && (
                <g fontSize={10}>
                  <path d={stripPath} fill={withAlpha(theme.ink.tertiary, 0.3)} />
                  <line x1={plot.x} x2={plot.right} y1={stripTop + stripHeight + 0.5} y2={stripTop + stripHeight + 0.5} stroke={theme.axis} strokeWidth={1} />
                  <text x={plot.x} y={stripTop - 4} fill={theme.label}>
                    {strip.label}
                  </text>
                  <text x={plot.x - 8} y={stripTop + 11} textAnchor="end" fill={theme.label}>
                    {strip.format(stripMax)}
                  </text>
                  <text x={plot.x - 8} y={stripTop + stripHeight + 1} textAnchor="end" fill={theme.label}>
                    {strip.format(0)}
                  </text>
                </g>
              )}
            </SvgLayer>
            <Cursor
              count={index.length}
              xOf={(k) => xs[k] ?? 0}
              label={label}
              onBrush={compact ? undefined : (a, b) => range.set([time.ages[index[a] as number] as number, time.ages[index[b] as number] as number])}
              onSelect={onSelect ? (k) => onSelect(index[k] as number) : undefined}
              clickable={onSelect !== undefined}
              describe={(k) => {
                const i = index[k] as number;
                const part = (members: typeof ups) =>
                  members
                    .map(({ s, n }) => ({ s, v: num(target[n]?.[i]) }))
                    .filter((p) => p.v > 0)
                    .map((p) => `${p.s.label} ${fmt(p.v)}`)
                    .join(', ');
                const down = part(downs);
                return `Age ${Math.floor(time.ages[i] as number)}, ${time.years[i]}. ${sides && down ? `${sides.up}: ` : ''}${part(ups) || 'nothing'}.${down ? ` ${sides ? `${sides.down}: ` : 'Below the line: '}${down}.` : ''}`;
              }}
              marks={(k) => {
                const i = index[k] as number;
                return (
                  <g>
                    {lines.map((l) => {
                      const v = l.values[i];
                      return v === null || v === undefined ? null : <circle key={l.key} cx={xs[k]} cy={y(v)} r={l.strong ? 4 : 3} fill={l.color} stroke={theme.surface} strokeWidth={2} />;
                    })}
                    {strip && stripHeight > 0 && <circle cx={xs[k]} cy={yStrip(num(strip.values[i]))} r={3} fill={theme.ink.secondary} stroke={theme.surface} strokeWidth={2} />}
                  </g>
                );
              }}
              tooltip={(k) => {
                const i = index[k] as number;
                const rows = (members: typeof ups) =>
                  members.map(({ s, n }) => {
                    const v = num(target[n]?.[i]);
                    return v <= 0 ? null : <TipRow key={s.key} color={s.color} swatch="box" label={s.label} value={fmt(v)} />;
                  });
                const upTotal = sum(ups, i);
                const downTotal = sum(downs, i);
                return (
                  <>
                    <TipTitle detail={String(time.years[i])}>Age {Math.floor(time.ages[i] as number)}</TipTitle>
                    {lines.map((l) => {
                      const v = l.values[i];
                      return v === null || v === undefined ? null : <TipRow key={l.key} color={l.color} label={l.label} value={fmt(v)} strong={l.strong} />;
                    })}
                    {totals?.up && <TipRow label={totals.up} value={fmt(upTotal)} strong />}
                    {rows([...ups].reverse())}
                    {totals?.down && downs.length > 0 && (
                      <div className="mt-1.5">
                        <TipRow label={totals.down} value={fmt(downTotal)} strong />
                      </div>
                    )}
                    {rows(downs)}
                    {strip && <TipRow label={strip.label} value={strip.format(num(strip.values[i]))} />}
                    {tooltipNote && <TipNote>{tooltipNote(i)}</TipNote>}
                  </>
                );
              }}
            />
          </>
        );
      }}
    </ChartFrame>
  );
}
