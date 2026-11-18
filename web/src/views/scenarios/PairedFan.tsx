// The plan and a scenario over a lifetime, on one axis: each one's median line and the
// band holding its middle half of lives, in the kit's two-plan colours. Two full fans
// on top of each other turn to mud, so the bands stop at the middle half and the gap
// between the two medians gets a strip of its own beneath, on its own scale, where a
// difference too small to see against the spread can still be read.
import { useMemo } from 'react';
import type { HistSeries, Summary } from '@/engine/output-types';
import { moneyCompact } from '@/ui';
import {
  AgeAxis,
  CanvasLayer,
  ChartFrame,
  Cursor,
  EndLabels,
  Legend,
  RuleAnnotations,
  SvgLayer,
  TipNote,
  TipRow,
  TipTitle,
  ValueAxis,
  clipPlot,
  drawBand,
  drawGrid,
  drawLine,
  linearScale,
  moneyTicks,
  niceDomain,
  quantileSeries,
  rulesHeight,
  summaryTime,
  symlogScale,
  useAnimated,
  useChartSync,
  useChartTheme,
  useTimeRange,
  withAlpha,
  type EndLabel,
  type RuleItem,
  type Scale,
  type TableData,
} from '@/ui/charts';

export type PairedSeries = Extract<HistSeries, 'net_worth' | 'spending'>;

export const PAIRED_LABEL: Record<PairedSeries, string> = { net_worth: 'Net worth', spending: 'Spending a year' };

type Row = (number | null)[];

interface Bands {
  p25: Row;
  p50: Row;
  p75: Row;
}

const bandsOf = (summary: Summary, series: PairedSeries): Bands => ({ p25: quantileSeries(summary, series, 0.25), p50: quantileSeries(summary, series, 0.5), p75: quantileSeries(summary, series, 0.75) });

/** Scenario median minus plan median, by year (null where either is missing). */
export function medianGap(base: Summary, scenario: Summary, series: PairedSeries): Row {
  const a = quantileSeries(base, series, 0.5);
  const b = quantileSeries(scenario, series, 0.5);
  return a.map((v, i) => {
    const w = b[i];
    return v === null || w === null || w === undefined ? null : w - v;
  });
}

export function pairedFanTable(base: Summary, scenario: Summary, series: PairedSeries, name: string): TableData {
  const a = bandsOf(base, series);
  const b = bandsOf(scenario, series);
  const gap = medianGap(base, scenario, series);
  const fmt = (v: number) => moneyCompact(v);
  return {
    caption: `${PAIRED_LABEL[series]} by age among households with someone alive, the plan against “${name}”, today's dollars`,
    columns: [
      { key: 'age', label: 'Age' },
      { key: 'year', label: 'Year', format: String },
      { key: 'a25', label: 'Plan, 25th percentile', format: fmt },
      { key: 'a50', label: 'Plan, median', format: fmt },
      { key: 'a75', label: 'Plan, 75th', format: fmt },
      { key: 'b25', label: 'Scenario, 25th percentile', format: fmt },
      { key: 'b50', label: 'Scenario, median', format: fmt },
      { key: 'b75', label: 'Scenario, 75th', format: fmt },
      { key: 'gap', label: 'Scenario median minus plan median', format: fmt },
    ],
    rows: base.axis.ages.map((age, y) => ({
      age: Math.floor(age),
      year: base.axis.years[y] ?? null,
      a25: a.p25[y] ?? null,
      a50: a.p50[y] ?? null,
      a75: a.p75[y] ?? null,
      b25: b.p25[y] ?? null,
      b50: b.p50[y] ?? null,
      b75: b.p75[y] ?? null,
      gap: gap[y] ?? null,
    })),
  };
}

export function PairedLegend({ name, className }: { name: string; className?: string }) {
  const theme = useChartTheme();
  return (
    <div data-chart-legend>
      <Legend
        className={className}
        items={[
          { key: 'a', label: 'The plan, median', color: theme.baseline.median, swatch: 'line' },
          { key: 'b', label: `${name}, median`, color: theme.scenario.line, swatch: 'line' },
          { key: 'band', label: 'Middle half of lives, each', color: withAlpha(theme.ink.tertiary, 0.35), swatch: 'box' },
        ]}
      />
    </div>
  );
}

export interface PairedFanProps {
  base: Summary;
  scenario: Summary;
  series: PairedSeries;
  /** The scenario's name, for labels. */
  name: string;
  compact?: boolean;
  /** Draw only the medians (small multiples). */
  bare?: boolean;
  height?: number;
  /** A shared top of the y axis, so small multiples can be compared. */
  top?: number;
}

export function PairedFan({ base, scenario, series, name, compact, bare, height, top }: PairedFanProps) {
  const sync = useChartSync();
  const time = useMemo(() => summaryTime(base), [base]);
  const range = useTimeRange(time, sync);
  const target = useMemo(() => ({ a: bandsOf(base, series), b: bandsOf(scenario, series) }), [base, scenario, series]);
  const model = useAnimated(target, { resetKey: `${series}:${base.axis.start}:${base.axis.years.length}:${scenario.axis.years.length}`, duration: 360 });
  const gapTarget = useMemo(() => medianGap(base, scenario, series), [base, scenario, series]);
  const gap = useAnimated(gapTarget, { resetKey: `${series}:${base.axis.start}:${base.axis.years.length}`, duration: 360 });

  const retiredA = base.headline.retirement_age.p50;
  const retiredB = scenario.headline.retirement_age.p50;
  const rules: { age: number; label: string; detail?: string }[] = [];
  if (!compact && !bare) {
    if (retiredA !== null && retiredB !== null && Math.abs(retiredA - retiredB) >= 0.75) {
      rules.push({ age: retiredA, label: 'Plan retires', detail: `median ${retiredA.toFixed(1)}` }, { age: retiredB, label: 'Scenario retires', detail: `median ${retiredB.toFixed(1)}` });
    } else if (retiredA !== null) {
      rules.push({ age: retiredA, label: 'Retired', detail: `median ${retiredA.toFixed(1)}` });
    }
  }

  const { first, last } = range;
  const strip = compact || bare ? 0 : 52;
  const stripGap = strip ? 22 : 0;
  const axisBand = bare ? 4 : compact ? 22 : 36;
  const short = name.length > 14 ? 'Scenario' : name;
  const margin = { top: bare ? 6 : compact ? 10 : rules.length ? rulesHeight(rules.length) + 14 : 22, right: bare ? 6 : Math.max(58, Math.min(110, short.length * 6.2 + 30)), bottom: axisBand + strip + stripGap, left: bare ? 6 : compact ? 40 : 52 };

  return (
    <ChartFrame height={(w) => height ?? (compact ? 190 : Math.round(Math.min(340, Math.max(230, w * 0.44))) + strip + stripGap)} margin={margin} aria-label={`${PAIRED_LABEL[series]} by age: the plan against ${name}`}>
      {({ plot, theme }) => {
        const x = linearScale(range.domain, [plot.x, plot.right]);
        const index = Array.from({ length: Math.max(0, last - first + 1) }, (_, k) => first + k);
        const xs = index.map((i) => x(time.ages[i] as number));
        const slice = (row: Row) => index.map((i) => row[i] ?? null);
        const numbers = (row: Row) => slice(row).filter((v): v is number => v !== null);
        const hi = top ?? Math.max(1, ...numbers(bare ? model.a.p50 : model.a.p75), ...numbers(bare ? model.b.p50 : model.b.p75));
        const lo = Math.min(0, ...numbers(bare ? model.a.p50 : model.a.p25), ...numbers(bare ? model.b.p50 : model.b.p25));
        // Linear unless the upper band dwarfs the typical level (decades of compounding), where
        // a linear axis would press both medians against the floor: the Overview's rule.
        const medians = [...numbers(model.a.p50), ...numbers(model.b.p50)].filter((v) => v > 0).sort((a, b) => a - b);
        const typical = medians[Math.floor(medians.length / 2)];
        const log = !bare && top === undefined && typical !== undefined && hi / typical > 8;
        const y: Scale = log ? symlogScale([lo * 1.1, hi * 1.15], [plot.bottom, plot.y], Math.max(10_000, 10 ** Math.floor(Math.log10(Math.max(hi, 1e5)) - 2))) : linearScale([lo, hi * 1.06], [plot.bottom, plot.y]);
        const ticks = y.ticks(compact || bare ? 3 : Math.max(2, Math.round(plot.height / 56)));

        const stripTop = plot.bottom + axisBand + stripGap;
        const gaps = slice(gap).filter((v): v is number => v !== null);
        const gapDomain = niceDomain(Math.min(0, ...gaps), Math.max(0, ...gaps), 2);
        const flat = gapDomain[1] - gapDomain[0] < 1;
        const yGap = linearScale(flat ? [-1, 1] : gapDomain, [stripTop + strip, stripTop]);
        const gapTicks = flat ? [0] : [gapDomain[0], 0, gapDomain[1]].filter((t, i, all) => all.indexOf(t) === i);
        const gapFormat = moneyTicks(gapTicks);

        const end = (row: Row) => {
          const s = slice(row);
          let k = s.length - 1;
          while (k >= 0 && s[k] === null) k--;
          return k >= 0 ? (s[k] as number) : null;
        };
        const endA = end(model.a.p50);
        const endB = end(model.b.p50);
        const labels: EndLabel[] = [];
        if (endA !== null) labels.push({ key: 'a', y: y(endA), text: 'Plan', color: theme.baseline.median, strong: true });
        if (endB !== null) labels.push({ key: 'b', y: y(endB), text: short, color: theme.scenario.line, strong: true });
        const ruleItems: RuleItem[] = rules.filter((r) => r.age >= range.domain[0] && r.age <= range.domain[1]).map((r, i) => ({ x: x(r.age), label: r.label, detail: r.detail, tone: i === 0 ? 'event' : 'reference' }));

        const value = (row: Row, i: number) => {
          const v = row[i];
          return v === null || v === undefined ? 'n/a' : moneyCompact(v);
        };

        return (
          <>
            <CanvasLayer
              deps={[model, gap, first, last, top, bare]}
              draw={(ctx) => {
                if (!bare) drawGrid(ctx, plot, y, ticks, theme.grid, lo < 0 ? theme.axis : undefined);
                const done = clipPlot(ctx, plot, 1);
                if (!bare) {
                  drawBand(ctx, xs, slice(model.a.p25), slice(model.a.p75), y, theme.baseline.bands[1]);
                  drawBand(ctx, xs, slice(model.b.p25), slice(model.b.p75), y, theme.scenario.bands[1]);
                }
                drawLine(ctx, xs, slice(model.a.p50), y, { color: theme.baseline.median, width: 2 });
                drawLine(ctx, xs, slice(model.b.p50), y, { color: theme.scenario.line, width: 2 });
                done();
                if (strip) {
                  // The gap between the medians, filled from zero in the scenario's colour.
                  const zero = yGap(0);
                  ctx.fillStyle = theme.scenario.bands[2];
                  ctx.beginPath();
                  let open = false;
                  let lastX = 0;
                  slice(gap).forEach((v, k) => {
                    const px = xs[k] as number;
                    if (v === null) {
                      if (open) {
                        ctx.lineTo(lastX, zero);
                        ctx.closePath();
                        open = false;
                      }
                      return;
                    }
                    if (!open) {
                      ctx.moveTo(px, zero);
                      open = true;
                    }
                    ctx.lineTo(px, yGap(v));
                    lastX = px;
                  });
                  if (open) {
                    ctx.lineTo(lastX, zero);
                    ctx.closePath();
                  }
                  ctx.fill();
                  drawLine(ctx, xs, slice(gap), yGap, { color: theme.scenario.line, width: 1.5 });
                }
              }}
            />
            <SvgLayer>
              {!bare && <ValueAxis y={y} format="money" title={compact ? undefined : `${PAIRED_LABEL[series]}, today's dollars${log ? ' (log scale)' : ''}`} titleY={rules.length ? 10 : undefined} ticks={ticks} grid={false} />}
              {bare ? <line x1={plot.x} x2={plot.right} y1={plot.bottom + 0.5} y2={plot.bottom + 0.5} stroke={theme.axis} strokeWidth={1} /> : <AgeAxis x={x} time={time} years={!compact} label={compact ? '' : 'Age'} />}
              {ruleItems.length > 0 && <RuleAnnotations items={ruleItems} />}
              {!bare && <EndLabels labels={labels} gap={14} />}
              {strip > 0 && (
                <g fontSize={10}>
                  <line x1={plot.x} x2={plot.right} y1={Math.round(yGap(0)) + 0.5} y2={Math.round(yGap(0)) + 0.5} stroke={theme.axis} strokeWidth={1} />
                  <text x={plot.x} y={stripTop - 7} fontSize={11} fill={theme.ink.secondary}>
                    Scenario median minus plan median{flat ? ': no difference' : ''}
                  </text>
                  {gapTicks.map((t) => (
                    <text key={t} x={plot.x - 8} y={yGap(t) + 3.5} textAnchor="end" fill={theme.label} style={{ fontVariantNumeric: 'tabular-nums' }}>
                      {t === 0 ? '$0' : `${t > 0 ? '+' : ''}${gapFormat(t)}`}
                    </text>
                  ))}
                </g>
              )}
            </SvgLayer>
            {!bare && (
              <Cursor
                count={index.length}
                xOf={(k) => xs[k] ?? 0}
                label={`${PAIRED_LABEL[series]} by age, the plan against ${name}`}
                describe={(k) => {
                  const i = index[k] as number;
                  const g = gapTarget[i];
                  return `Age ${Math.floor(time.ages[i] as number)}, ${time.years[i]}: plan median ${value(target.a.p50, i)}, ${name} median ${value(target.b.p50, i)}${g === null || g === undefined ? '' : `, a difference of ${moneyCompact(g)}`}.`;
                }}
                onBrush={compact ? undefined : (a, b) => range.set([time.ages[index[a] as number] as number, time.ages[index[b] as number] as number])}
                marks={(k) => {
                  const i = index[k] as number;
                  const a = model.a.p50[i];
                  const b = model.b.p50[i];
                  const g = gap[i];
                  return (
                    <g>
                      {a !== null && a !== undefined && <circle cx={xs[k]} cy={y(a)} r={4} fill={theme.baseline.median} stroke={theme.surface} strokeWidth={2} />}
                      {b !== null && b !== undefined && <circle cx={xs[k]} cy={y(b)} r={4} fill={theme.scenario.line} stroke={theme.surface} strokeWidth={2} />}
                      {strip > 0 && g !== null && g !== undefined && <circle cx={xs[k]} cy={yGap(g)} r={3} fill={theme.scenario.line} stroke={theme.surface} strokeWidth={2} />}
                    </g>
                  );
                }}
                tooltip={(k) => {
                  const i = index[k] as number;
                  const g = gapTarget[i];
                  return (
                    <>
                      <TipTitle detail={String(time.years[i])}>Age {Math.floor(time.ages[i] as number)}</TipTitle>
                      <TipRow color={theme.scenario.line} label={`${short}, median`} value={value(target.b.p50, i)} strong />
                      <TipRow color={theme.scenario.bands[2]} swatch="box" label="its middle half" value={`${value(target.b.p25, i)} to ${value(target.b.p75, i)}`} />
                      <TipRow color={theme.baseline.median} label="Plan, median" value={value(target.a.p50, i)} strong />
                      <TipRow color={theme.baseline.bands[2]} swatch="box" label="its middle half" value={`${value(target.a.p25, i)} to ${value(target.a.p75, i)}`} />
                      {g !== null && g !== undefined && <TipRow label="Difference of medians" value={`${g > 0 ? '+' : ''}${moneyCompact(g)}`} />}
                      <TipNote>Among households with someone alive at this age.</TipNote>
                    </>
                  );
                }}
              />
            )}
          </>
        );
      }}
    </ChartFrame>
  );
}
