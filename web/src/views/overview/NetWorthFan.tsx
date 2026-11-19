// The density fan of wealth among the living: shading from the engine's yearly
// histogram (darker is more likely at that age), the median and the 5/25/75/95
// quantile lines labelled at their ends, a handful of real simulated lives drawn
// through it, and a strip beneath showing how many lives are still going.
import { useMemo, useState } from 'react';
import type { HistSeries, Summary } from '@/engine/output-types';
import type { Profile } from '@/engine/types';
import { navigateToLife, type LifePick } from '@/state/route';
import { moneyCompact } from '@/ui';
import {
  AgeAxis,
  CanvasLayer,
  ChartFrame,
  Cursor,
  EndLabels,
  RuleAnnotations,
  SvgLayer,
  TipNote,
  TipRow,
  TipTitle,
  ValueAxis,
  clipPlot,
  drawDensityFan,
  drawGrid,
  PlotClip,
  drawLine,
  fanColumns,
  fanModel,
  interpolateFan,
  linearScale,
  ofHundred,
  planMoments,
  rulesHeight,
  quantileSeries,
  summaryTime,
  symlogScale,
  useAnimated,
  useChartSync,
  useTimeRange,
  withAlpha,
  type EndLabel,
  type Scale,
  type TableData,
} from '@/ui/charts';

export type FanSeries = Extract<HistSeries, 'net_worth' | 'investable'>;
export type FanScale = 'linear' | 'log';

export const FAN_SERIES_LABEL: Record<FanSeries, string> = { net_worth: 'Net worth', investable: 'Investable assets' };

/** Index into QUANTILE_LEVELS of the lines drawn: 5, 25, 50, 75, 95. */
const LINES = [
  { level: 1, key: 'p5', label: '5th' },
  { level: 3, key: 'p25', label: '25th' },
  { level: 4, key: 'p50', label: 'Median' },
  { level: 5, key: 'p75', label: '75th' },
  { level: 7, key: 'p95', label: '95th' },
] as const;

export interface NetWorthFanProps {
  summary: Summary;
  profile: Profile | null;
  series: FanSeries;
  scale: FanScale;
  compact?: boolean;
  height?: number;
  /** A scenario on the same lives: its median is drawn over the baseline fan. */
  compare?: { name: string; summary: Summary };
  /** One of the named lives to pick out (set by the caption's links). */
  highlight?: NamedLife['why'] | null;
}

export interface NamedLife {
  why: Extract<LifePick, 'p10' | 'median' | 'p90'>;
  /** "An unlucky life". */
  title: string;
  path_index: number;
  net_worth: number[];
  investable: number[];
}

const NAMED: { why: NamedLife['why']; title: string }[] = [
  { why: 'p10', title: 'An unlucky life' },
  { why: 'median', title: 'A middling life' },
  { why: 'p90', title: 'A lucky life' },
];

/**
 * The unlucky, middling and lucky lives (ranked 10th, 50th and 90th of 100 by wealth at
 * the end), with their yearly series from the Summary's `pick_paths`: no trace needed.
 */
export function namedLives(summary: Summary): NamedLife[] {
  return NAMED.flatMap((n) => {
    const pick = summary.picks.find((p) => p.label === n.why);
    const path = pick && summary.pick_paths.find((p) => p.path_index === pick.path_index);
    return path && path.net_worth.length > 1 ? [{ ...n, path_index: path.path_index, net_worth: path.net_worth, investable: path.investable }] : [];
  });
}

/**
 * The scale that shows this plan best until the reader chooses: linear, unless the top
 * of the fan dwarfs the typical level (compounding over a very long retirement), where
 * a linear axis would press nearly every life against the floor.
 */
export function defaultFanScale(summary: Summary, series: FanSeries): FanScale {
  const p95 = quantileSeries(summary, series, 0.95).filter((v): v is number => v !== null);
  const p50 = quantileSeries(summary, series, 0.5).filter((v): v is number => v !== null && v > 0).sort((a, b) => a - b);
  const typical = p50[Math.floor(p50.length / 2)];
  if (!typical || p95.length === 0) return 'linear';
  return Math.max(...p95) / typical > 12 ? 'log' : 'linear';
}

interface FanLife {
  path_index: number;
  net_worth: number[];
  investable: number[];
  named: NamedLife | null;
}

/** A handful of the sample lives, spread from unlucky to lucky by where they end up. */
function chooseSamples(summary: Summary, series: FanSeries, count: number) {
  const ranked = summary.samples
    .filter((s) => s[series].length > 1)
    .map((s) => ({ sample: s, end: s[series][s[series].length - 1] ?? 0 }))
    .sort((a, b) => a.end - b.end);
  if (ranked.length <= count) return ranked.map((r) => r.sample);
  return Array.from({ length: count }, (_, i) => (ranked[Math.round(((i + 0.5) / count) * (ranked.length - 1))] as (typeof ranked)[number]).sample);
}

export function fanTable(summary: Summary, series: FanSeries): TableData {
  const q = summary.quantiles.find((x) => x.series === series);
  const fmt = (v: number) => moneyCompact(v);
  return {
    caption: `${FAN_SERIES_LABEL[series]} by age among households with someone alive, today's dollars`,
    columns: [
      { key: 'age', label: 'Age' },
      { key: 'year', label: 'Year', format: String },
      { key: 'p5', label: '5th percentile', format: fmt },
      { key: 'p25', label: '25th', format: fmt },
      { key: 'p50', label: 'Median', format: fmt },
      { key: 'p75', label: '75th', format: fmt },
      { key: 'p95', label: '95th', format: fmt },
      { key: 'alive', label: 'Share still alive', format: (v) => `${(v * 100).toFixed(1)}%` },
    ],
    rows: summary.axis.ages.map((age, y) => ({
      age: Math.floor(age),
      year: summary.axis.years[y] ?? null,
      p5: q?.values[1]?.[y] ?? null,
      p25: q?.values[3]?.[y] ?? null,
      p50: q?.values[4]?.[y] ?? null,
      p75: q?.values[5]?.[y] ?? null,
      p95: q?.values[7]?.[y] ?? null,
      alive: summary.survival.household[y] ?? null,
    })),
  };
}

export function NetWorthFan({ summary, profile, series, scale, compact, height, compare, highlight }: NetWorthFanProps) {
  const sync = useChartSync();
  const time = useMemo(() => summaryTime(summary), [summary]);
  const range = useTimeRange(time, sync);
  const target = useMemo(() => fanModel(summary, series), [summary, series]);
  const model = useAnimated(target, { blend: interpolateFan, resetKey: `${series}:${summary.axis.start}:${summary.axis.years.length}`, duration: 360 });
  const named = useMemo(() => (compact ? [] : namedLives(summary)), [summary, compact]);
  // Anonymous lives for texture, then the three named ones: one list, so hover and click treat them alike.
  const samples = useMemo<FanLife[]>(() => {
    const taken = new Set(named.map((n) => n.path_index));
    const plain = chooseSamples(summary, series, compact ? 3 : 4).filter((s) => !taken.has(s.path_index));
    return [...plain.map((s) => ({ path_index: s.path_index, net_worth: s.net_worth, investable: s.investable, named: null })), ...named.map((n) => ({ ...n, named: n }))];
  }, [summary, series, compact, named]);
  const moments = useMemo(() => (profile ? planMoments(summary, profile) : []), [summary, profile]);
  const [hovered, setHovered] = useState<number | null>(null);
  const compareMedian = useMemo(() => (compare ? quantileSeries(compare.summary, series, 0.5) : null), [compare, series]);

  if (!model) return null;
  const { first, last } = range;
  const q = model.quantiles;
  const visible = (row: (number | null)[] | undefined) => (row ?? []).slice(first, last + 1).filter((v): v is number => v !== null);
  const top = Math.max(1, ...visible(q[scale === 'log' ? 8 : 7]));
  const bottom = Math.min(0, ...visible(q[1]));

  const strip = compact ? 0 : 34;
  const stripGap = compact ? 0 : 10;
  const margin = { top: compact ? 10 : moments.length ? rulesHeight(moments.length) + 14 : 22, right: compact ? 56 : 66, bottom: (compact ? 22 : 36) + strip + stripGap, left: compact ? 40 : 52 };

  return (
    <ChartFrame
      height={(w) => height ?? (compact ? 190 : Math.round(Math.min(460, Math.max(300, w * 0.52))) + strip + stripGap)}
      margin={margin}
      aria-label={`${FAN_SERIES_LABEL[series]} by age across ${summary.paths.toLocaleString('en-US')} simulated lives`}
    >
      {(frame) => {
        const { plot, theme } = frame;
        const x = linearScale(range.domain, [plot.x, plot.right]);
        const pad = (top - bottom) * 0.06;
        const y: Scale =
          scale === 'log'
            ? symlogScale([bottom * 1.1, top * 1.15], [plot.bottom, plot.y], Math.max(10_000, 10 ** Math.floor(Math.log10(Math.max(top, 1e5)) - 2)))
            : linearScale([bottom - (bottom < 0 ? pad : 0), top + pad], [plot.bottom, plot.y]);
        const index = Array.from({ length: last - first + 1 }, (_, k) => first + k);
        const xs = index.map((i) => x(time.ages[i] as number));
        const slice = (row: (number | null)[] | undefined) => index.map((i) => row?.[i] ?? null);
        const ticks = y.ticks(compact ? 3 : Math.max(2, Math.round(plot.height / 56)));
        const stripTop = plot.bottom + (compact ? 22 : 36) + stripGap;
        const yAlive = linearScale([0, 1], [stripTop + strip, stripTop]);

        const endLabels: EndLabel[] = LINES.flatMap((l) => {
          const row = slice(q[l.level]);
          let k = row.length - 1;
          while (k >= 0 && row[k] === null) k--;
          const v = k >= 0 ? (row[k] as number) : null;
          if (v === null) return [];
          return [{ key: l.key, y: Math.min(plot.bottom, Math.max(plot.y, y(v))), text: l.label, color: l.key === 'p50' ? theme.baseline.median : withAlpha(theme.baseline.median, l.key === 'p5' || l.key === 'p95' ? 0.45 : 0.75), strong: l.key === 'p50' }];
        });

        if (compareMedian && compare) {
          const row = slice(compareMedian).filter((v): v is number => v !== null);
          const v = row[row.length - 1];
          if (v !== undefined) endLabels.push({ key: 'compare', y: Math.min(plot.bottom, Math.max(plot.y, y(v))), text: compare.name.length > 9 ? 'Scenario' : compare.name, color: theme.scenario.line, strong: true });
        }

        const samplePath = (values: number[]) => {
          let d = '';
          for (const i of index) {
            const v = values[i];
            if (v === undefined) break;
            d += `${d ? 'L' : 'M'}${(x(time.ages[i] as number)).toFixed(1)},${y(v).toFixed(1)}`;
          }
          return d;
        };
        const hoveredSample = hovered !== null ? (samples[hovered] ?? null) : highlight ? (samples.find((s) => s.named?.why === highlight) ?? null) : null;
        const hoveredEnd = hoveredSample ? index.filter((i) => hoveredSample[series][i] !== undefined).at(-1) : undefined;

        return (
          <>
            <CanvasLayer
              deps={[model, scale, first, last, samples, compareMedian]}
              draw={(ctx) => {
                drawGrid(ctx, plot, y, ticks, theme.grid, bottom < 0 ? theme.axis : undefined);
                drawDensityFan(ctx, plot, xs, fanColumns(model, first, last), y, { ramp: theme.baseline.ramp });
                const done = clipPlot(ctx, plot, 1);
                for (const s of samples) drawLine(ctx, xs, index.map((i) => s[series][i] ?? null), y, s.named ? { color: theme.ink.secondary, width: 1.1, alpha: 0.6 } : { color: theme.baseline.samplePath, width: 1 });
                for (const l of LINES) {
                  const outer = l.key === 'p5' || l.key === 'p95';
                  drawLine(ctx, xs, slice(q[l.level]), y, { color: theme.baseline.median, width: l.key === 'p50' ? 2 : 1, alpha: l.key === 'p50' ? 1 : outer ? 0.45 : 0.75 });
                }
                if (compareMedian) drawLine(ctx, xs, slice(compareMedian), y, { color: theme.scenario.line, width: 2 });
                done();
                if (strip) {
                  // The share of lives still going, beneath the fan: quantiles above are over these lives only.
                  ctx.fillStyle = withAlpha(theme.ink.tertiary, 0.28);
                  ctx.beginPath();
                  ctx.moveTo(xs[0] as number, yAlive(0));
                  index.forEach((i, k) => ctx.lineTo(xs[k] as number, yAlive(model.alive[i] ?? 0)));
                  ctx.lineTo(xs[xs.length - 1] as number, yAlive(0));
                  ctx.closePath();
                  ctx.fill();
                }
              }}
            />
            <SvgLayer>
              <ValueAxis y={y} format="money" title={compact ? undefined : `${FAN_SERIES_LABEL[series]}, today's dollars`} titleY={moments.length ? 10 : undefined} ticks={ticks} grid={false} />
              <AgeAxis x={x} time={time} years={!compact} label={compact ? '' : 'Age'} />
              {!compact && <RuleAnnotations items={moments.map((m) => ({ x: x(m.age), label: m.label, detail: m.detail, tone: m.key === 'fi' ? 'event' : 'reference' }))} />}
              <EndLabels labels={endLabels} gap={compact ? 12 : 14} />
              {strip > 0 && (
                <g fontSize={10}>
                  <line x1={plot.x} x2={plot.right} y1={stripTop + strip + 0.5} y2={stripTop + strip + 0.5} stroke={theme.axis} strokeWidth={1} />
                  <text x={plot.x + 4} y={stripTop + strip - 5} fill={theme.ink.secondary} stroke={theme.surface} strokeWidth={2.5} paintOrder="stroke">
                    Lives with someone still alive
                  </text>
                  <text x={plot.x - 8} y={stripTop + 8} textAnchor="end" fill={theme.label}>
                    100%
                  </text>
                </g>
              )}
              {hoveredSample && (
                <PlotClip>
                  <path d={samplePath(hoveredSample[series])} fill="none" stroke={theme.surface} strokeWidth={4.5} strokeLinejoin="round" opacity={0.7} />
                  <path d={samplePath(hoveredSample[series])} fill="none" stroke={theme.ink.primary} strokeWidth={2} strokeLinejoin="round" />
                  {hoveredSample.named && hoveredEnd !== undefined && (
                    <text
                      x={Math.min(plot.right - 6, x(time.ages[hoveredEnd] as number) - 6)}
                      y={Math.max(plot.y + 12, Math.min(plot.bottom - 6, y(hoveredSample[series][hoveredEnd] as number) - 8))}
                      textAnchor="end"
                      fontSize={11}
                      fontWeight={600}
                      fill={theme.ink.primary}
                      stroke={theme.surface}
                      strokeWidth={3}
                      paintOrder="stroke"
                    >
                      {hoveredSample.named.title}
                    </text>
                  )}
                </PlotClip>
              )}
            </SvgLayer>
            <Cursor
              count={index.length}
              xOf={(k) => xs[k] ?? 0}
              label={`${FAN_SERIES_LABEL[series]} by age`}
              showTooltip="self"
              describe={(k) => {
                const i = index[k] as number;
                const v = (level: number) => {
                  const value = q[level]?.[i];
                  return value === null || value === undefined ? 'no data' : moneyCompact(value);
                };
                return `Age ${Math.floor(time.ages[i] as number)}, ${time.years[i]}: median ${v(4)}; middle half ${v(3)} to ${v(5)}; 5th to 95th percentile ${v(1)} to ${v(7)}. ${ofHundred(model.alive[i] ?? 0).text} lives still have someone alive.`;
              }}
              onBrush={compact ? undefined : (a, b) => range.set([time.ages[index[a] as number] as number, time.ages[index[b] as number] as number])}
              onPointer={(k, _px, py) => {
                if (k === null || compact) return setHovered(null);
                const i = index[k] as number;
                let best: number | null = null;
                let distance = 12;
                samples.forEach((s, n) => {
                  const v = s[series][i];
                  if (v === undefined) return;
                  const d = Math.abs(y(v) - py);
                  if (d < distance) {
                    distance = d;
                    best = n;
                  }
                });
                setHovered(best);
              }}
              onSelect={compact ? undefined : () => hoveredSample && navigateToLife(hoveredSample.path_index, hoveredSample.named?.why ?? 'sample')}
              clickable={hoveredSample !== null}
              marks={(k) => {
                const i = index[k] as number;
                return (
                  <g>
                    {LINES.map((l) => {
                      const v = q[l.level]?.[i];
                      return v === null || v === undefined ? null : <circle key={l.key} cx={xs[k]} cy={y(v)} r={l.key === 'p50' ? 4 : 3} fill={theme.baseline.median} stroke={theme.surface} strokeWidth={2} />;
                    })}
                    {strip > 0 && <circle cx={xs[k]} cy={yAlive(model.alive[i] ?? 0)} r={3} fill={theme.ink.secondary} stroke={theme.surface} strokeWidth={2} />}
                  </g>
                );
              }}
              tooltip={(k) => {
                const i = index[k] as number;
                const value = (level: number) => {
                  const v = q[level]?.[i];
                  return v === null || v === undefined ? 'n/a' : moneyCompact(v);
                };
                const sampleValue = hoveredSample?.[series][i];
                return (
                  <>
                    <TipTitle detail={String(time.years[i])}>Age {Math.floor(time.ages[i] as number)}</TipTitle>
                    {[...LINES].reverse().map((l) => (
                      <TipRow key={l.key} color={l.key === 'p50' ? theme.baseline.median : withAlpha(theme.baseline.median, 0.55)} label={l.key === 'p50' ? 'Median' : `${l.label} percentile`} value={value(l.level)} strong={l.key === 'p50'} />
                    ))}
                    {hoveredSample && sampleValue !== undefined && <TipRow color={theme.ink.primary} label={hoveredSample.named ? `${hoveredSample.named.title} (#${hoveredSample.path_index})` : `Life #${hoveredSample.path_index}`} value={moneyCompact(sampleValue)} />}
                    <TipNote>
                      {ofHundred(model.alive[i] ?? 0).text} lives still have someone alive.{hoveredSample ? ' Click to open this life.' : ''}
                    </TipNote>
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
