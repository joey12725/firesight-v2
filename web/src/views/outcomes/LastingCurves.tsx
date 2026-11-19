// Does the money outlast you? Two curves on one scale, the share of lives with someone
// still alive at each age and the share whose money has lasted that long, and beneath
// them the number that matters: lives that are alive AND short of essentials that year.
// That last series is the engine's own joint count, not the product of the two curves.
import { useMemo } from 'react';
import type { Summary } from '@/engine/output-types';
import { moneyCompact } from '@/ui';
import {
  AgeAxis,
  CanvasLayer,
  ChartFrame,
  Cursor,
  EndLabels,
  Legend,
  SvgLayer,
  TipNote,
  TipRow,
  TipTitle,
  ValueAxis,
  clipPlot,
  drawLine,
  linearScale,
  ofHundred,
  summaryTime,
  useAnimated,
  useChartSync,
  useChartTheme,
  useTimeRange,
  withAlpha,
  type TableData,
} from '@/ui/charts';
import { lastingModel } from './data';

const share = (v: number) => `${(v * 100).toFixed(1)}%`;

export function lastingTable(summary: Summary): TableData {
  const m = lastingModel(summary);
  return {
    caption: 'Share of all simulated lives, by age: someone still alive, money not yet run short, and alive but short of essentials',
    columns: [
      { key: 'age', label: 'Age' },
      { key: 'year', label: 'Year', format: String },
      { key: 'alive', label: 'Someone still alive', format: share },
      { key: 'lasted', label: 'Money has lasted this long', format: share },
      { key: 'short', label: 'Alive and short of essentials', format: share },
      { key: 'among', label: 'Short, of those still alive', format: share },
      { key: 'shortfall', label: 'Their average shortfall against plan, a year', format: moneyCompact },
    ],
    rows: summary.axis.ages.map((age, y) => ({ age: Math.floor(age), year: summary.axis.years[y] ?? null, alive: m.alive[y] ?? null, lasted: m.lasted[y] ?? null, short: m.short[y] ?? null, among: m.shortAmongLiving[y] ?? null, shortfall: m.shortfall[y] ?? null })),
  };
}

export function LastingLegend() {
  const theme = useChartTheme();
  return (
    <div data-chart-legend>
      <Legend
        items={[
          { key: 'lasted', label: 'The money has lasted to this age', color: theme.baseline.line, swatch: 'line' },
          { key: 'alive', label: 'Someone is still alive', color: theme.ink.secondary, swatch: 'line' },
          { key: 'short', label: 'Alive and short of essentials', color: theme.status.depleted, swatch: 'box' },
        ]}
      />
    </div>
  );
}

const STRIP = 64;
const GAP = 30;

export function LastingCurves({ summary }: { summary: Summary }) {
  const sync = useChartSync();
  const time = useMemo(() => summaryTime(summary), [summary]);
  const range = useTimeRange(time, sync);
  const target = useMemo(() => lastingModel(summary), [summary]);
  const model = useAnimated(target, { resetKey: `${summary.axis.start}:${summary.axis.years.length}`, duration: 360 });
  const peak = Math.max(0, ...target.short);
  const stripMax = Math.max(0.02, Math.ceil(peak * 100 * 1.15) / 100);

  return (
    <ChartFrame
      height={(w) => Math.round(Math.min(320, Math.max(220, w * 0.34))) + STRIP + GAP}
      margin={{ top: 22, right: 96, bottom: 36 + STRIP + GAP, left: 52 }}
      aria-label="Share of lives with someone alive, with money that has lasted, and alive but short of essentials, by age"
    >
      {({ plot, theme }) => {
        const x = linearScale(range.domain, [plot.x, plot.right]);
        const index = Array.from({ length: range.last - range.first + 1 }, (_, k) => range.first + k);
        const xs = index.map((i) => x(time.ages[i] as number));
        const pick = (values: number[]) => index.map((i) => values[i] ?? null);
        const y = linearScale([0, 1], [plot.bottom, plot.y]);
        const stripTop = plot.bottom + 36 + GAP;
        const yShort = linearScale([0, stripMax], [stripTop + STRIP, stripTop]);
        const lastOf = (values: number[]) => values[index[index.length - 1] as number] ?? 0;
        const peakIndex = target.short.indexOf(peak);
        const peakVisible = peak >= 0.0005 && peakIndex >= range.first && peakIndex <= range.last;

        return (
          <>
            <CanvasLayer
              deps={[model, range.first, range.last, stripMax]}
              draw={(ctx) => {
                const done = clipPlot(ctx, plot, 1);
                drawLine(ctx, xs, pick(model.alive), y, { color: theme.ink.secondary, width: 2 });
                drawLine(ctx, xs, pick(model.lasted), y, { color: theme.baseline.line, width: 2 });
                done();
                // Alive and short: its own strip and its own scale, because it is a small share of everyone.
                ctx.fillStyle = withAlpha(theme.status.depleted, 0.85);
                ctx.beginPath();
                ctx.moveTo(xs[0] as number, yShort(0));
                index.forEach((i, k) => ctx.lineTo(xs[k] as number, yShort(Math.min(stripMax, model.short[i] ?? 0))));
                ctx.lineTo(xs[xs.length - 1] as number, yShort(0));
                ctx.closePath();
                ctx.fill();
              }}
            />
            <SvgLayer>
              <ValueAxis y={y} format="share100" ticks={[0, 0.25, 0.5, 0.75, 1]} title="Lives, of 100" />
              <AgeAxis x={x} time={time} />
              <EndLabels
                labels={[
                  { key: 'lasted', y: y(lastOf(model.lasted)), text: 'Money lasted', color: theme.baseline.line, strong: true },
                  { key: 'alive', y: y(lastOf(model.alive)), text: 'Still alive', color: theme.ink.secondary },
                ]}
              />
              <g fontSize={11}>
                <text x={0} y={stripTop - 8} fill={theme.label}>
                  Alive and short of essentials, of 100 lives
                </text>
                <line x1={plot.x} x2={plot.right} y1={stripTop + STRIP + 0.5} y2={stripTop + STRIP + 0.5} stroke={theme.axis} strokeWidth={1} />
                <line x1={plot.x} x2={plot.right} y1={Math.round(yShort(stripMax)) + 0.5} y2={Math.round(yShort(stripMax)) + 0.5} stroke={theme.grid} strokeWidth={1} />
                <text x={plot.x - 8} y={yShort(stripMax) + 4} textAnchor="end" fill={theme.label}>
                  {Math.round(stripMax * 100)}
                </text>
                <text x={plot.x - 8} y={stripTop + STRIP + 3} textAnchor="end" fill={theme.label}>
                  0
                </text>
                {peakVisible && (
                  <text x={Math.min(plot.right - 4, x(time.ages[peakIndex] as number) + 6)} y={Math.max(stripTop + 10, yShort(peak) - 5)} fill={theme.ink.secondary} stroke={theme.surface} strokeWidth={3} paintOrder="stroke">
                    {ofHundred(peak).text} at {Math.floor(time.ages[peakIndex] as number)}
                  </text>
                )}
              </g>
            </SvgLayer>
            <Cursor
              count={index.length}
              xOf={(k) => xs[k] ?? 0}
              label="Survival and how long the money lasts, by age"
              describe={(k) => {
                const i = index[k] as number;
                return `Age ${Math.floor(time.ages[i] as number)}: someone is alive in ${ofHundred(target.alive[i] ?? 0).text} lives, the money has lasted in ${ofHundred(target.lasted[i] ?? 0).text}, and ${ofHundred(target.short[i] ?? 0).text} are alive and short of essentials.`;
              }}
              onBrush={(a, b) => range.set([time.ages[index[a] as number] as number, time.ages[index[b] as number] as number])}
              marks={(k) => {
                const i = index[k] as number;
                return (
                  <g>
                    <circle cx={xs[k]} cy={y(target.alive[i] ?? 0)} r={4} fill={theme.ink.secondary} stroke={theme.surface} strokeWidth={2} />
                    <circle cx={xs[k]} cy={y(target.lasted[i] ?? 0)} r={4} fill={theme.baseline.line} stroke={theme.surface} strokeWidth={2} />
                    <circle cx={xs[k]} cy={yShort(Math.min(stripMax, target.short[i] ?? 0))} r={3.5} fill={theme.status.depleted} stroke={theme.surface} strokeWidth={2} />
                  </g>
                );
              }}
              tooltip={(k) => {
                const i = index[k] as number;
                const among = target.shortAmongLiving[i];
                const depth = target.shortfall[i];
                return (
                  <>
                    <TipTitle detail={String(time.years[i])}>Age {Math.floor(time.ages[i] as number)}</TipTitle>
                    <TipRow color={theme.baseline.line} label="Money has lasted" value={ofHundred(target.lasted[i] ?? 0).text} />
                    <TipRow color={theme.ink.secondary} label="Someone alive" value={ofHundred(target.alive[i] ?? 0).text} />
                    <TipRow color={theme.status.depleted} swatch="box" label="Alive and short" value={ofHundred(target.short[i] ?? 0).text} strong />
                    {among !== null && among !== undefined && (
                      <TipNote>
                        Of the lives still going at this age, {ofHundred(among).text} are short of essentials{depth !== null && depth !== undefined && among > 0 ? `, spending ${moneyCompact(depth)} a year less than planned on average` : ''}.
                      </TipNote>
                    )}
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
