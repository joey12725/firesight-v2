// One life's net worth and investable assets month by month on the age axis, drawn over
// the range of all simulated lives, with the life's events marked along it. Event labels
// sit in lanes above the plot so they never collide; on a crowded life the least
// important labels are dropped (their dots stay, and the tooltip names them).
import { useMemo } from 'react';
import type { Status, Summary } from '@/engine/output-types';
import { moneyCompact } from '@/ui';
import {
  AgeAxis,
  CanvasLayer,
  ChartFrame,
  Cursor,
  EndLabels,
  Legend,
  STATUS_LABEL,
  STATUS_STACK,
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
  textWidth,
  useChartSync,
  useChartTheme,
  useTimeRange,
  withAlpha,
  type EndLabel,
  type TableData,
} from '@/ui/charts';
import { layoutLanes } from '@/ui/charts/ext/cashflow-lanes';
import type { Life, LifeEvent } from './life';
import type { Crowd } from './words';

export type { Crowd };

export function crowdFor(life: Life, summary: Summary | null): Crowd | null {
  const q = summary?.quantiles.find((x) => x.series === 'net_worth');
  if (!summary || !q) return null;
  const row = (level: number) => {
    const values = q.values[q.levels.findIndex((l) => Math.abs(l - level) < 1e-9)] ?? [];
    return life.time.years.map((year) => {
      const i = summary.axis.years.indexOf(year);
      return i < 0 ? null : (values[i] ?? null);
    });
  };
  return { p5: row(0.05), p25: row(0.25), p50: row(0.5), p75: row(0.75), p95: row(0.95) };
}

/** The last month of each year index. */
export function yearEnds(life: Life): number[] {
  const out = new Array<number>(life.time.length).fill(0);
  life.monthYear.forEach((y, m) => (out[y] = m));
  return out;
}

export function timelineTable(life: Life, crowd: Crowd | null): TableData {
  const ends = yearEnds(life);
  const fmt = (v: number) => moneyCompact(v);
  return {
    caption: `Life #${life.trace.path_index}: year-end net worth and investable assets, the middle of all simulated lives, and the year's events. Today's dollars`,
    columns: [
      { key: 'age', label: life.hasAges ? 'Age' : 'Year' },
      { key: 'year', label: 'Year', format: String },
      { key: 'net_worth', label: 'Net worth', format: fmt },
      { key: 'investable', label: 'Investable assets', format: fmt },
      ...(crowd ? [{ key: 'p25', label: 'All lives: 25th percentile', format: fmt }, { key: 'p50', label: 'All lives: median', format: fmt }, { key: 'p75', label: 'All lives: 75th percentile', format: fmt }] : []),
      { key: 'events', label: 'Events', align: 'left' as const },
    ],
    rows: life.time.years.map((year, y) => ({
      age: Math.floor(life.time.ages[y] ?? 0),
      year,
      net_worth: life.netWorth[ends[y] ?? 0] ?? null,
      investable: life.investable[ends[y] ?? 0] ?? null,
      p25: crowd?.p25[y] ?? null,
      p50: crowd?.p50[y] ?? null,
      p75: crowd?.p75[y] ?? null,
      events: life.events
        .filter((e) => e.year === y)
        .map((e) => e.detail)
        .join(' '),
    })),
  };
}

function presentStatuses(life: Life): Status[] {
  const seen = new Set(life.status);
  return STATUS_STACK.filter((s) => seen.has(s));
}

export function TimelineLegend({ life, crowd, className }: { life: Life; crowd: Crowd | null; className?: string }) {
  const theme = useChartTheme();
  return (
    <div data-chart-legend>
      <Legend
        className={className}
        items={[
          { key: 'nw', label: 'This life: net worth', color: theme.ink.primary, swatch: 'line' },
          { key: 'inv', label: 'Investable assets', color: withAlpha(theme.ink.primary, 0.45), swatch: 'line' },
          ...(crowd
            ? [
                { key: 'mid', label: 'Middle half of all lives', color: theme.baseline.bands[2] },
                { key: 'outer', label: '9 in 10 of all lives', color: theme.baseline.bands[1] },
              ]
            : []),
          ...[...presentStatuses(life)].reverse().map((s) => ({ key: s, label: STATUS_LABEL[s], color: theme.status[s] })),
        ]}
      />
    </div>
  );
}

export interface TimelineProps {
  life: Life;
  crowd: Crowd | null;
  /** The year whose ledger is open (index), marked on the axis. */
  selected?: number | null;
  onSelect?: (year: number) => void;
  compact?: boolean;
}

export function Timeline({ life, crowd, selected, onSelect, compact }: TimelineProps) {
  const sync = useChartSync();
  const { time } = life;
  const range = useTimeRange(time, sync);
  const ends = useMemo(() => yearEnds(life), [life]);
  const { first, last } = range;
  const index = Array.from({ length: last - first + 1 }, (_, k) => first + k);
  const [lo, hi] = range.domain;
  const months: number[] = [];
  life.monthAges.forEach((a, m) => {
    if (a >= lo - 0.1 && a <= hi + 0.1) months.push(m);
  });
  const mine = months.map((m) => life.netWorth[m] ?? 0);
  const top0 = Math.max(1, ...mine, ...months.map((m) => life.investable[m] ?? 0));
  const crowdTop = crowd ? Math.max(0, ...index.map((i) => crowd.p75[i] ?? 0)) : 0;
  // Room for the crowd's upper quartile, but never so much that this life is pressed against the floor.
  const top = Math.max(top0, Math.min(crowdTop, top0 * 1.6));
  const bottom = Math.min(0, ...mine);

  const stripH = compact ? 0 : 7;
  const axisBand = life.hasAges && !compact ? 36 : 22;

  const rows = compact ? 2 : 5;
  const left = compact ? 40 : 52;
  const right = compact ? 62 : 76;

  return (
    <ChartFrame
      height={(w) => (compact ? 150 : Math.round(Math.min(400, Math.max(260, w * 0.36)))) + rows * 15 + axisBand + (stripH ? stripH + 12 : 0)}
      margin={{ top: (compact ? 8 : 22) + rows * 15 + 4, right, bottom: axisBand + (stripH ? stripH + 12 : 0), left }}
      aria-label={`Life number ${life.trace.path_index}: net worth by ${life.hasAges ? 'age' : 'year'} with its events, against the range of all simulated lives`}
    >
      {({ plot, theme, width }) => {
        const x = linearScale([lo, hi], [plot.x, plot.right]);
        const pad = (top - bottom) * 0.06;
        const y = linearScale([bottom - (bottom < 0 ? pad : 0), top + pad], [plot.bottom, plot.y]);
        const xs = index.map((i) => x(time.ages[i] as number));
        const xm = months.map((m) => x(life.monthAges[m] as number));
        const ticks = y.ticks(compact ? 3 : Math.max(2, Math.round(plot.height / 56)));
        const slice = (row: (number | null)[]) => index.map((i) => row[i] ?? null);

        // Event markers: a dot on the net-worth line, a label in a lane above.
        const visible = life.events.filter((e) => e.age >= lo - 1e-6 && e.age <= hi + 1e-6 && (!compact || e.priority >= 4));
        const size = compact ? 10 : 11;
        const placed = layoutLanes(
          visible.map((e) => ({ x: x(e.age), width: e.priority < 1 ? Infinity : textWidth(e.title, size) + 4, priority: e.priority + (e.trouble ? 0.5 : 0) })),
          { min: 2, max: width - 2, rows, gap: 8 },
        );
        const rowY = (row: number) => plot.y - 9 - row * 15;
        const dotY = (e: LifeEvent) => Math.min(plot.bottom, Math.max(plot.y, y(life.netWorth[e.month] ?? 0)));
        const color = (e: LifeEvent) => (e.trouble ? theme.status.depleted : theme.ink.primary);

        const endLabels: EndLabel[] = [];
        const lastMonth = months[months.length - 1];
        if (lastMonth !== undefined) {
          endLabels.push({ key: 'nw', y: Math.min(plot.bottom, Math.max(plot.y, y(life.netWorth[lastMonth] ?? 0))), text: 'Net worth', color: theme.ink.primary, strong: true });
          if (Math.abs((life.investable[lastMonth] ?? 0) - (life.netWorth[lastMonth] ?? 0)) > (top - bottom) * 0.02 || !compact) endLabels.push({ key: 'inv', y: Math.min(plot.bottom, Math.max(plot.y, y(life.investable[lastMonth] ?? 0))), text: 'Investable', color: withAlpha(theme.ink.primary, 0.45) });
        }

        // The life's stage, month by month, as a ribbon under the axis.
        const stripTop = plot.bottom + axisBand + 6;
        const runs: { from: number; to: number; status: Status }[] = [];
        if (stripH) {
          for (const m of months) {
            const s = life.status[m] as Status;
            const lastRun = runs[runs.length - 1];
            if (lastRun && lastRun.status === s) lastRun.to = m;
            else runs.push({ from: m, to: m, status: s });
          }
        }
        const selectedX = selected !== null && selected !== undefined && selected >= first && selected <= last ? x(time.ages[selected] as number) : null;

        return (
          <>
            <CanvasLayer
              deps={[life, crowd, first, last, lo, hi]}
              draw={(ctx) => {
                drawGrid(ctx, plot, y, ticks, theme.grid, bottom < 0 ? theme.axis : undefined);
                const done = clipPlot(ctx, plot, 1);
                if (crowd) {
                  drawBand(ctx, xs, slice(crowd.p5), slice(crowd.p95), y, theme.baseline.bands[0]);
                  drawBand(ctx, xs, slice(crowd.p25), slice(crowd.p75), y, theme.baseline.bands[1]);
                  drawLine(ctx, xs, slice(crowd.p50), y, { color: theme.baseline.median, width: 1, alpha: 0.55 });
                }
                drawLine(ctx, xm, months.map((m) => life.investable[m] ?? null), y, { color: theme.ink.primary, width: 1.5, alpha: 0.45 });
                drawLine(ctx, xm, mine, y, { color: theme.ink.primary, width: 2 });
                done();
              }}
            />
            <SvgLayer>
              <ValueAxis y={y} format="money" title={compact ? undefined : "Today's dollars"} titleY={10} ticks={ticks} grid={false} />
              <AgeAxis x={x} time={time} years={life.hasAges && !compact} label={compact ? '' : life.hasAges ? 'Age' : 'Year'} />
              {runs.map((r) => {
                const x0 = Math.max(plot.x, x(life.monthAges[r.from] as number) - (x(1 / 12) - x(0)));
                const x1 = Math.min(plot.right, x(life.monthAges[r.to] as number));
                return x1 > x0 ? <rect key={r.from} x={x0} y={stripTop} width={x1 - x0} height={stripH} fill={theme.status[r.status]} /> : null;
              })}
              {stripH > 0 && (
                <text x={plot.x - 8} y={stripTop + stripH} textAnchor="end" fontSize={10} fill={theme.label}>
                  Stage
                </text>
              )}
              <g fontSize={size}>
                {visible.map((e, i) => {
                  const p = placed[i];
                  if (!p || p.row < 0) return null;
                  const ex = x(e.age);
                  return <line key={`l${e.month}-${e.kind}`} x1={ex} x2={ex} y1={rowY(p.row) + 4} y2={dotY(e)} stroke={theme.annotation} strokeOpacity={0.4} strokeWidth={1} />;
                })}
                {visible.map((e, i) => {
                  const p = placed[i];
                  const ex = x(e.age);
                  const labelled = p && p.row >= 0;
                  return (
                    <g key={`${e.month}-${e.kind}`}>
                      <circle cx={ex} cy={dotY(e)} r={labelled ? 3.5 : 2.5} fill={labelled ? color(e) : theme.surface} stroke={labelled ? theme.surface : color(e)} strokeWidth={labelled ? 2 : 1.5} />
                      {labelled && (
                        <text x={p.left + 2} y={rowY(p.row)} fill={e.priority >= 5 ? theme.ink.primary : theme.annotation} fontWeight={e.priority >= 5 ? 600 : 400} stroke={theme.surface} strokeWidth={3} paintOrder="stroke" strokeLinejoin="round">
                          {e.title}
                        </text>
                      )}
                    </g>
                  );
                })}
              </g>
              <EndLabels labels={endLabels} gap={compact ? 12 : 14} />
              {selectedX !== null && (
                <g>
                  <line x1={selectedX} x2={selectedX} y1={plot.y} y2={plot.bottom} stroke={theme.ink.primary} strokeWidth={1} strokeOpacity={0.55} />
                  <path d={`M${selectedX - 5},${plot.bottom + 6}h10l-5,-6z`} fill={theme.ink.primary} />
                </g>
              )}
            </SvgLayer>
            <Cursor
              count={index.length}
              xOf={(k) => xs[k] ?? 0}
              label={`Life number ${life.trace.path_index}, year by year`}
              onBrush={compact ? undefined : (a, b) => range.set([time.ages[index[a] as number] as number, time.ages[index[b] as number] as number])}
              onSelect={onSelect ? (k) => onSelect(index[k] as number) : undefined}
              clickable={onSelect !== undefined}
              describe={(k) => {
                const i = index[k] as number;
                const events = life.events.filter((e) => e.year === i).map((e) => e.detail);
                return `${life.hasAges ? `Age ${Math.floor(time.ages[i] as number)}, ` : ''}${time.years[i]}: net worth ${moneyCompact(life.netWorth[ends[i] ?? 0] ?? 0)}, investable ${moneyCompact(life.investable[ends[i] ?? 0] ?? 0)}. ${events.join(' ')}`;
              }}
              marks={(k) => {
                const i = index[k] as number;
                const m = ends[i] ?? 0;
                return (
                  <g>
                    <circle cx={xs[k]} cy={y(life.investable[m] ?? 0)} r={3} fill={theme.ink.secondary} stroke={theme.surface} strokeWidth={2} />
                    <circle cx={xs[k]} cy={y(life.netWorth[m] ?? 0)} r={4} fill={theme.ink.primary} stroke={theme.surface} strokeWidth={2} />
                  </g>
                );
              }}
              tooltip={(k) => {
                const i = index[k] as number;
                const m = ends[i] ?? 0;
                const events = life.events.filter((e) => e.year === i);
                const median = crowd?.p50[i];
                const lowQ = crowd?.p25[i];
                const highQ = crowd?.p75[i];
                return (
                  <>
                    <TipTitle detail={life.hasAges ? String(time.years[i]) : undefined}>{life.hasAges ? `Age ${Math.floor(time.ages[i] as number)}` : time.years[i]}</TipTitle>
                    <TipRow color={theme.ink.primary} label="Net worth" value={moneyCompact(life.netWorth[m] ?? 0)} strong />
                    <TipRow color={withAlpha(theme.ink.primary, 0.45)} label="Investable" value={moneyCompact(life.investable[m] ?? 0)} />
                    {median !== null && median !== undefined && <TipRow color={theme.baseline.median} label="Middle of all lives" value={moneyCompact(median)} />}
                    {lowQ !== null && lowQ !== undefined && highQ !== null && highQ !== undefined && <TipRow color={theme.baseline.bands[2]} swatch="box" label="Middle half" value={`${moneyCompact(lowQ)} to ${moneyCompact(highQ)}`} />}
                    <TipRow color={theme.status[life.status[m] as Status]} swatch="box" label="Stage" value={STATUS_LABEL[life.status[m] as Status]} />
                    {events.length > 0 && (
                      <div className="mt-1.5 max-w-60 space-y-1 border-t border-line pt-1.5 text-2xs leading-4 whitespace-normal text-ink-2">
                        {events.slice(0, 4).map((e) => (
                          <p key={`${e.month}-${e.kind}`}>{e.detail}</p>
                        ))}
                        {events.length > 4 && <p className="text-ink-3">and {events.length - 4} more</p>}
                      </div>
                    )}
                    {onSelect && <TipNote>Click to open this year’s ledger.</TipNote>}
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
