// How the decision moves lives between life stages, by age: scenario minus plan, in
// lives per 100, one line per stage. Two stacked charts side by side cannot show a
// shift of three lives in a hundred; the difference can. The lines always sum to zero
// (a life that leaves one stage enters another).
import { useMemo } from 'react';
import type { Summary } from '@/engine/output-types';
import {
  AgeAxis,
  CanvasLayer,
  ChartFrame,
  Cursor,
  Legend,
  STATUS_LABEL,
  STATUS_SHORT,
  STATUS_STACK,
  SvgLayer,
  TipNote,
  TipRow,
  TipTitle,
  ValueAxis,
  clipPlot,
  drawGrid,
  drawLine,
  linearScale,
  statusShares,
  summaryTime,
  useAnimated,
  useChartSync,
  useChartTheme,
  useTimeRange,
  type TableData,
} from '@/ui/charts';
import { PeakLabels, type PeakLabel } from '@/ui/charts/ext/scenarios-marks';
import { stageDiffs, type StageDiff } from './model';

const signed = (v: number) => {
  const n = Math.abs(v) * 100;
  const text = n < 0.05 ? '0' : n < 9.95 ? n.toFixed(1) : String(Math.round(n));
  return `${text === '0' ? '' : v > 0 ? '+' : '−'}${text}`;
};

export function stageTable(base: Summary, scenario: Summary): TableData {
  const pct = (v: number) => `${v > 0 ? '+' : ''}${(v * 100).toFixed(1)}`;
  return {
    caption: 'Change in the share of simulated lives in each life stage, by age: scenario minus plan, in lives per 100',
    columns: [{ key: 'age', label: 'Age' }, { key: 'year', label: 'Year', format: String }, ...[...STATUS_STACK].reverse().map((s) => ({ key: s, label: STATUS_LABEL[s], format: pct }))],
    rows: base.axis.ages.map((age, y) => ({
      age: Math.floor(age),
      year: base.axis.years[y] ?? null,
      ...Object.fromEntries(STATUS_STACK.map((s) => [s, (statusShares(scenario, s)[y] ?? 0) - (statusShares(base, s)[y] ?? 0)])),
    })),
  };
}

export function StageLegend({ diffs, className }: { diffs: StageDiff[]; className?: string }) {
  const theme = useChartTheme();
  if (diffs.length === 0) return null;
  return (
    <div data-chart-legend>
      <Legend className={className} items={[...diffs].reverse().map((d) => ({ key: d.status, label: STATUS_LABEL[d.status], color: theme.status[d.status], swatch: 'line' as const }))} />
    </div>
  );
}

export function StageShift({ base, scenario }: { base: Summary; scenario: Summary }) {
  const sync = useChartSync();
  const time = useMemo(() => summaryTime(base), [base]);
  const range = useTimeRange(time, sync);
  const diffs = useMemo(() => stageDiffs(base, scenario), [base, scenario]);
  const target = useMemo(() => diffs.map((d) => d.values), [diffs]);
  const values = useAnimated(target, { resetKey: `${base.axis.start}:${base.axis.years.length}:${diffs.map((d) => d.status).join()}`, duration: 360 });
  const { first, last } = range;

  return (
    <ChartFrame height={(w) => Math.round(Math.min(300, Math.max(220, w * 0.4)))} margin={{ top: 22, right: 12, bottom: 36, left: 52 }} aria-label="Change in the share of lives in each life stage by age, scenario minus plan">
      {({ plot, theme }) => {
        const x = linearScale(range.domain, [plot.x, plot.right]);
        const index = Array.from({ length: Math.max(0, last - first + 1) }, (_, k) => first + k);
        const xs = index.map((i) => x(time.ages[i] as number));
        const visible = values.flatMap((row) => index.map((i) => row[i] ?? 0));
        const extent = Math.max(0.01, ...visible.map(Math.abs)) * 1.4;
        const y = linearScale([-extent, extent], [plot.bottom, plot.y]);
        const ticks = y.ticks(5);

        // Each line is labelled at its largest shift within view.
        const labels: PeakLabel[] = diffs
          .map((d, s) => {
            let best = index[0] ?? 0;
            for (const i of index) if (Math.abs(values[s]?.[i] ?? 0) > Math.abs(values[s]?.[best] ?? 0)) best = i;
            const v = values[s]?.[best] ?? 0;
            return { key: d.status, x: x(time.ages[best] as number), y: y(v), text: STATUS_SHORT[d.status], color: theme.status[d.status], side: (v >= 0 ? 'above' : 'below') as PeakLabel['side'], size: Math.abs(v) };
          })
          .filter((l) => l.size >= (extent / 1.4) * 0.25)
          .sort((a, b) => b.size - a.size);

        return (
          <>
            <CanvasLayer
              deps={[values, first, last]}
              draw={(ctx) => {
                drawGrid(ctx, plot, y, ticks, theme.grid, theme.axis);
                const done = clipPlot(ctx, plot, 1);
                diffs.forEach((d, s) => drawLine(ctx, xs, index.map((i) => values[s]?.[i] ?? null), y, { color: theme.status[d.status], width: 2 }));
                done();
              }}
            />
            <SvgLayer>
              <ValueAxis y={y} format={signed} ticks={ticks} title="Lives per 100, with the change minus without" grid={false} />
              <AgeAxis x={x} time={time} />
              {diffs.length === 0 ? (
                <text x={(plot.x + plot.right) / 2} y={y(0) - 10} textAnchor="middle" fontSize={12} fill={theme.ink.secondary}>
                  No stage gains or loses as many as 1 life in 200 at any age.
                </text>
              ) : (
                <PeakLabels labels={labels} />
              )}
            </SvgLayer>
            <Cursor
              count={index.length}
              xOf={(k) => xs[k] ?? 0}
              label="Change in life stages by age"
              describe={(k) => {
                const i = index[k] as number;
                const parts = diffs.map((d) => `${STATUS_LABEL[d.status].toLowerCase()} ${signed(d.values[i] ?? 0)}`);
                return `Age ${Math.floor(time.ages[i] as number)}, ${time.years[i]}. Change in lives per 100: ${parts.join(', ') || 'none'}.`;
              }}
              marks={(k) => {
                const i = index[k] as number;
                return (
                  <g>
                    {diffs.map((d, s) => (
                      <circle key={d.status} cx={xs[k]} cy={y(values[s]?.[i] ?? 0)} r={3.5} fill={theme.status[d.status]} stroke={theme.surface} strokeWidth={2} />
                    ))}
                  </g>
                );
              }}
              tooltip={(k) => {
                const i = index[k] as number;
                const rows = [...diffs].map((d) => ({ d, v: d.values[i] ?? 0 })).sort((a, b) => b.v - a.v);
                return (
                  <>
                    <TipTitle detail={String(time.years[i])}>Age {Math.floor(time.ages[i] as number)}</TipTitle>
                    {rows.map(({ d, v }) => (
                      <TipRow key={d.status} color={theme.status[d.status]} label={STATUS_LABEL[d.status]} value={`${signed(v)} of 100`} />
                    ))}
                    <TipNote>Lives in each stage with the change, minus without it.</TipNote>
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
