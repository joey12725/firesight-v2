// Spending delivered against spending planned. A plan can "succeed" while cutting
// spending for decades, so the bands of what lives actually spend are drawn against the
// plan line, with the share of retired lives below 90% of plan underneath.
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
  drawBand,
  drawLine,
  linearScale,
  ofHundred,
  quantileSeries,
  summaryTime,
  useAnimated,
  useChartSync,
  useChartTheme,
  useTimeRange,
  withAlpha,
  type EndLabel,
  type TableData,
} from '@/ui/charts';

function model(summary: Summary) {
  return {
    p5: quantileSeries(summary, 'spending', 0.05),
    p25: quantileSeries(summary, 'spending', 0.25),
    p50: quantileSeries(summary, 'spending', 0.5),
    p75: quantileSeries(summary, 'spending', 0.75),
    p95: quantileSeries(summary, 'spending', 0.95),
    // The plan line is the median life's planned spending (the engine's own quantile), comparable with the median delivered.
    planned: quantileSeries(summary, 'spend_planned', 0.5),
    plannedLo: quantileSeries(summary, 'spend_planned', 0.25),
    plannedHi: quantileSeries(summary, 'spend_planned', 0.75),
    below90: summary.spending.by_year.below_90,
    below75: summary.spending.by_year.below_75,
    depleted: summary.spending.by_year.depleted,
    shortfall: summary.spending.by_year.depleted_shortfall,
  };
}

export function spendingTable(summary: Summary): TableData {
  const m = model(summary);
  const pct = (v: number) => `${(v * 100).toFixed(1)}%`;
  return {
    caption: "Annual spending by age among households with someone alive, today's dollars",
    columns: [
      { key: 'age', label: 'Age' },
      { key: 'year', label: 'Year', format: String },
      { key: 'planned', label: 'Planned (median life)', format: moneyCompact },
      { key: 'p5', label: '5th percentile', format: moneyCompact },
      { key: 'p25', label: '25th', format: moneyCompact },
      { key: 'p50', label: 'Median', format: moneyCompact },
      { key: 'p75', label: '75th', format: moneyCompact },
      { key: 'p95', label: '95th', format: moneyCompact },
      { key: 'below90', label: 'Retired lives under 90% of plan', format: pct },
      { key: 'below75', label: 'Under 75% of plan', format: pct },
      { key: 'depleted', label: 'Living households short of essentials', format: pct },
      { key: 'shortfall', label: 'Their average shortfall against plan, a year', format: moneyCompact },
    ],
    rows: summary.axis.ages.map((age, y) => ({
      age: Math.floor(age),
      year: summary.axis.years[y] ?? null,
      planned: m.planned[y] ?? null,
      p5: m.p5[y] ?? null,
      p25: m.p25[y] ?? null,
      p50: m.p50[y] ?? null,
      p75: m.p75[y] ?? null,
      p95: m.p95[y] ?? null,
      below90: m.below90[y] ?? null,
      below75: m.below75[y] ?? null,
      depleted: m.depleted[y] ?? null,
      shortfall: m.shortfall[y] ?? null,
    })),
  };
}

export function SpendingLegend() {
  const theme = useChartTheme();
  return (
    <div data-chart-legend>
      <Legend
        items={[
          { key: 'planned', label: 'Planned (median life)', color: theme.ink.primary, swatch: 'dash' },
          { key: 'median', label: 'Median delivered', color: theme.baseline.median, swatch: 'line' },
          { key: 'mid', label: 'Middle 50 of 100 lives', color: theme.baseline.bands[2] },
          { key: 'outer', label: 'Middle 90', color: theme.baseline.bands[0] },
        ]}
      />
    </div>
  );
}

export function SpendingChart({ summary, height }: { summary: Summary; height?: number }) {
  const sync = useChartSync();
  const time = useMemo(() => summaryTime(summary), [summary]);
  const range = useTimeRange(time, sync);
  const target = useMemo(() => model(summary), [summary]);
  const m = useAnimated(target, { resetKey: `${summary.axis.start}:${summary.axis.years.length}`, duration: 360 });
  const { first, last } = range;
  const index = Array.from({ length: last - first + 1 }, (_, k) => first + k);
  const strip = 40;
  const stripGap = 26;

  return (
    <ChartFrame
      height={(w) => height ?? Math.round(Math.min(320, Math.max(220, w * 0.4))) + strip + stripGap}
      margin={{ top: 22, right: 64, bottom: 36 + strip + stripGap, left: 52 }}
      aria-label="Annual spending by age: what simulated lives spend against what the plan intends"
    >
      {(frame) => {
        const { plot, theme } = frame;
        const x = linearScale(range.domain, [plot.x, plot.right]);
        const slice = (row: (number | null)[]) => index.map((i) => row[i] ?? null);
        const top = Math.max(1, ...slice(m.p95).filter((v): v is number => v !== null), ...slice(m.planned).filter((v): v is number => v !== null));
        const y = linearScale([0, top * 1.06], [plot.bottom, plot.y]);
        const xs = index.map((i) => x(time.ages[i] as number));
        const stripTop = plot.bottom + 36 + stripGap;
        const peak = Math.max(0.1, ...slice(m.below90).map((v) => v ?? 0));
        const yCut = linearScale([0, Math.min(1, Math.ceil(peak * 10) / 10)], [stripTop + strip, stripTop]);
        const lastValue = (row: (number | null)[]) => {
          const s = slice(row);
          for (let k = s.length - 1; k >= 0; k--) if (s[k] !== null) return s[k] as number;
          return null;
        };
        const labels: EndLabel[] = [];
        const p50 = lastValue(m.p50);
        const planned = lastValue(m.planned);
        if (p50 !== null) labels.push({ key: 'p50', y: y(p50), text: 'Median', color: theme.baseline.median, strong: true });
        if (planned !== null) labels.push({ key: 'planned', y: y(planned), text: 'Planned', color: theme.ink.primary });

        const cutArea = (row: (number | null)[]) => {
          let d = '';
          let open = false;
          let lastX = 0;
          index.forEach((i, k) => {
            const v = row[i];
            if (v === null || v === undefined) {
              if (open) d += `L${lastX.toFixed(1)},${yCut(0).toFixed(1)}Z`;
              open = false;
              return;
            }
            const px = xs[k] as number;
            d += open ? `L${px.toFixed(1)},${yCut(v).toFixed(1)}` : `M${px.toFixed(1)},${yCut(0).toFixed(1)}L${px.toFixed(1)},${yCut(v).toFixed(1)}`;
            open = true;
            lastX = px;
          });
          if (open) d += `L${lastX.toFixed(1)},${yCut(0).toFixed(1)}Z`;
          return d;
        };

        return (
          <>
            <CanvasLayer
              deps={[m, first, last]}
              draw={(ctx) => {
                const done = clipPlot(ctx, plot, 1);
                drawBand(ctx, xs, slice(m.p5), slice(m.p95), y, theme.baseline.bands[0]);
                drawBand(ctx, xs, slice(m.p25), slice(m.p75), y, theme.baseline.bands[2]);
                drawLine(ctx, xs, slice(m.p50), y, { color: theme.baseline.median, width: 2 });
                drawLine(ctx, xs, slice(m.planned), y, { color: theme.ink.primary, width: 1.5, dash: [5, 4] });
                done();
              }}
            />
            <SvgLayer>
              <ValueAxis y={y} format="money" title="Spending a year, today's dollars" />
              <AgeAxis x={x} time={time} />
              <EndLabels labels={labels} />
              <g fontSize={10}>
                <path d={cutArea(m.below90)} fill={withAlpha(theme.status.retired_cutting, 0.55)} />
                <path d={cutArea(m.below75)} fill={theme.status.depleted} fillOpacity={0.75} />
                <line x1={plot.x} x2={plot.right} y1={stripTop + strip + 0.5} y2={stripTop + strip + 0.5} stroke={theme.axis} strokeWidth={1} />
                <text x={plot.x} y={stripTop - 7} fill={theme.label} fontSize={11}>
                  Retired lives spending under 90% of plan (darker: under 75%)
                </text>
                <text x={plot.x - 8} y={stripTop + 8} textAnchor="end" fill={theme.label}>
                  {Math.round(yCut.domain[1] * 100)}%
                </text>
              </g>
            </SvgLayer>
            <Cursor
              count={index.length}
              xOf={(k) => xs[k] ?? 0}
              label="Spending by age"
              describe={(k) => {
                const i = index[k] as number;
                const v = (row: (number | null)[]) => (row[i] === null || row[i] === undefined ? 'no data' : moneyCompact(row[i] as number));
                const cut = target.below90[i];
                return `Age ${Math.floor(time.ages[i] as number)}, ${time.years[i]}: planned ${v(target.planned)}, median spent ${v(target.p50)}, middle half ${v(target.p25)} to ${v(target.p75)}.${cut === null || cut === undefined ? '' : ` ${ofHundred(cut).text} retired lives are under 90% of plan.`}`;
              }}
              marks={(k) => {
                const i = index[k] as number;
                const p = target.p50[i];
                const pl = target.planned[i];
                const cut = target.below90[i];
                return (
                  <g>
                    {pl !== null && pl !== undefined && <circle cx={xs[k]} cy={y(pl)} r={3.5} fill={theme.ink.primary} stroke={theme.surface} strokeWidth={2} />}
                    {p !== null && p !== undefined && <circle cx={xs[k]} cy={y(p)} r={4} fill={theme.baseline.median} stroke={theme.surface} strokeWidth={2} />}
                    {cut !== null && cut !== undefined && <circle cx={xs[k]} cy={yCut(cut)} r={3} fill={theme.ink.secondary} stroke={theme.surface} strokeWidth={2} />}
                  </g>
                );
              }}
              tooltip={(k) => {
                const i = index[k] as number;
                const v = (row: (number | null)[]) => (row[i] === null || row[i] === undefined ? 'n/a' : moneyCompact(row[i] as number));
                const cut = target.below90[i];
                const short = target.depleted[i];
                const depth = target.shortfall[i];
                return (
                  <>
                    <TipTitle detail={String(time.years[i])}>Age {Math.floor(time.ages[i] as number)}</TipTitle>
                    <TipRow color={theme.ink.primary} swatch="dash" label="Planned" value={v(target.planned)} />
                    <TipRow color={withAlpha(theme.baseline.median, 0.55)} label="95th percentile" value={v(target.p95)} />
                    <TipRow color={withAlpha(theme.baseline.median, 0.55)} label="75th percentile" value={v(target.p75)} />
                    <TipRow color={theme.baseline.median} label="Median spent" value={v(target.p50)} strong />
                    <TipRow color={withAlpha(theme.baseline.median, 0.55)} label="25th percentile" value={v(target.p25)} />
                    <TipRow color={withAlpha(theme.baseline.median, 0.55)} label="5th percentile" value={v(target.p5)} />
                    {cut !== null && cut !== undefined && <TipNote>{ofHundred(cut).text} retired lives are spending under 90% of plan.</TipNote>}
                    {short !== null && short !== undefined && short >= 0.0005 && (
                      <TipNote>
                        {ofHundred(short).text} living households are short of essentials{depth !== null && depth !== undefined ? `, by ${moneyCompact(depth)} a year against plan on average` : ''}.
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
