// The historical model's own picture: under it, each simulated life starts in a real
// month of the record and lives through what followed. One traced life per starting
// year is drawn as a wealth path coloured by its fate, above a strip of starting years
// showing how many of the lives that began there ran short. Lives that outrun the
// record continue from its first month; that part is drawn fainter and said out loud.
import { useEffect, useMemo, useState } from 'react';
import type { EnginePool } from '@/engine';
import type { Summary } from '@/engine/output-types';
import type { Profile } from '@/engine/types';
import { navigateToLife } from '@/state/route';
import { moneyCompact } from '@/ui';
import {
  AgeAxis,
  CanvasLayer,
  ChartFrame,
  ChartSyncProvider,
  Cursor,
  Legend,
  PlotClip,
  SvgLayer,
  TipNote,
  TipRow,
  TipTitle,
  ValueAxis,
  clipPlot,
  drawGrid,
  linearScale,
  ofHundred,
  summaryTime,
  symlogScale,
  useChartTheme,
  withAlpha,
  type Scale,
  type TableData,
} from '@/ui/charts';
import { XAxis } from '@/ui/charts/ext/outcomes-XAxis';
import { HISTORY, cohortYears, historyLastYear, pathForYear, type CohortYear } from './data';

export interface CohortLife {
  year: number;
  path: number;
  /** Year-end net worth, by year index of the plan. */
  netWorth: number[];
  held: boolean;
  /** Year index from which the life is replaying the start of the record. */
  wrapsAt: number;
}

export interface CohortTraces {
  lives: CohortLife[];
  total: number;
  error: string | null;
}

const MONTHS = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'];
const startMonth = (year: number) => MONTHS[year === HISTORY.firstYear ? HISTORY.firstMonth - 1 : 0];

/**
 * Traces one life per starting year of the record (about a hundred, a few milliseconds
 * each), a handful at a time so the picture fills in and an edit can abandon the rest.
 */
export function useCohortTraces(profile: Profile, seed: number, pool: EnginePool, enabled: boolean): CohortTraces {
  const [state, setState] = useState<CohortTraces>({ lives: [], total: 0, error: null });
  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    const last = historyLastYear();
    const starts: { year: number; path: number }[] = [];
    for (let year = HISTORY.firstYear; year <= last; year++) {
      const path = pathForYear(year);
      if (path !== null) starts.push({ year, path });
    }
    const timer = window.setTimeout(async () => {
      const lives: CohortLife[] = [];
      setState({ lives: [], total: starts.length, error: null });
      try {
        for (let i = 0; i < starts.length && !cancelled; i += 6) {
          const batch = await Promise.all(starts.slice(i, i + 6).map(async ({ year, path }) => ({ year, path, trace: await pool.trace(profile, path, seed) })));
          for (const { year, path, trace } of batch) {
            const netWorth = trace.annual.columns.find((c) => c.key === 'net_worth')?.values.map((v) => v ?? 0) ?? [];
            const depleted = trace.scalars.find((s) => s.scalar === 'depletion_age')?.value ?? null;
            lives.push({ year, path, netWorth, held: depleted === null, wrapsAt: Math.max(0, last - year + 1) });
          }
          if (!cancelled) setState({ lives: [...lives], total: starts.length, error: null });
        }
      } catch (error) {
        if (!cancelled) setState({ lives: [...lives], total: starts.length, error: error instanceof Error ? error.message : 'The lives could not be traced.' });
      }
    }, 350);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [profile, seed, pool, enabled]);
  return state;
}

export function cohortTable(summary: Summary): TableData {
  return {
    caption: 'Outcomes by the year of the historical record in which a simulated life starts',
    columns: [
      { key: 'year', label: 'Starting year', format: String },
      { key: 'lives', label: 'Lives in the sample' },
      { key: 'short', label: 'Ran short' },
      { key: 'share', label: 'Share that ran short', format: (v) => `${(v * 100).toFixed(0)}%` },
      { key: 'wealth', label: 'Median net worth at the end', format: moneyCompact },
      { key: 'wraps', label: 'Outruns the record', align: 'left' },
    ],
    rows: cohortYears(summary).map((r) => ({ year: r.year, lives: r.lives, short: r.short, share: r.shortShare, wealth: r.medianWealth, wraps: r.wraps ? 'yes' : 'no' })),
  };
}

export function CohortLegend() {
  const theme = useChartTheme();
  return (
    <div data-chart-legend>
      <Legend
        items={[
          { key: 'held', label: 'A life whose money lasted', color: theme.baseline.line, swatch: 'line' },
          { key: 'short', label: 'A life that ran short', color: theme.status.depleted, swatch: 'line' },
          { key: 'wrap', label: 'Past the end of the record, replaying it from 1926', color: withAlpha(theme.ink.tertiary, 0.6), swatch: 'line' },
        ]}
      />
    </div>
  );
}

function CohortPaths({ summary, traces, focusYear, onFocus }: { summary: Summary; traces: CohortTraces; focusYear: number | null; onFocus: (year: number | null) => void }) {
  const time = useMemo(() => summaryTime(summary), [summary]);
  const { lives } = traces;
  const top = Math.max(1, ...lives.flatMap((l) => l.netWorth));
  const bottom = Math.min(0, ...lives.flatMap((l) => l.netWorth));
  const medians = lives.map((l) => l.netWorth[Math.floor(l.netWorth.length / 2)] ?? 0).sort((a, b) => a - b);
  const typical = medians[Math.floor(medians.length / 2)] ?? 1;
  const log = top / Math.max(1, typical) > 12;
  const focused = focusYear === null ? null : (lives.find((l) => l.year === focusYear) ?? null);

  return (
    <ChartFrame height={(w) => Math.round(Math.min(380, Math.max(240, w * 0.42)))} margin={{ top: 22, right: 12, bottom: 36, left: 52 }} aria-label={`Net worth by age for ${lives.length} simulated lives, one starting in each year of the historical record`}>
      {({ plot, theme }) => {
        const x = linearScale(time.domain, [plot.x, plot.right]);
        const y: Scale = log ? symlogScale([bottom * 1.1, top * 1.15], [plot.bottom, plot.y], Math.max(10_000, 10 ** (Math.floor(Math.log10(Math.max(top, 1e5))) - 2))) : linearScale([bottom, top * 1.05], [plot.bottom, plot.y]);
        const ticks = y.ticks(Math.max(3, Math.round(plot.height / 56)));
        const xs = time.ages.map((a) => x(a));
        const stroke = (ctx: CanvasRenderingContext2D, l: CohortLife, from: number, to: number) => {
          ctx.beginPath();
          for (let i = from; i <= to && i < l.netWorth.length; i++) {
            const px = xs[i] as number;
            const py = y(l.netWorth[i] as number);
            if (i === from) ctx.moveTo(px, py);
            else ctx.lineTo(px, py);
          }
          ctx.stroke();
        };
        const path = (l: CohortLife) => l.netWorth.map((v, i) => `${i ? 'L' : 'M'}${(xs[i] as number).toFixed(1)},${y(v).toFixed(1)}`).join('');
        return (
          <>
            <CanvasLayer
              deps={[lives, log]}
              draw={(ctx) => {
                drawGrid(ctx, plot, y, ticks, theme.grid, bottom < 0 ? theme.axis : undefined);
                const done = clipPlot(ctx, plot, 1);
                ctx.lineJoin = 'round';
                // Lives that held first, the ones that ran short on top.
                for (const held of [true, false]) {
                  for (const l of lives) {
                    if (l.held !== held) continue;
                    const color = held ? theme.baseline.line : theme.status.depleted;
                    ctx.lineWidth = held ? 1 : 1.5;
                    ctx.strokeStyle = withAlpha(color, held ? 0.42 : 0.85);
                    stroke(ctx, l, 0, Math.min(l.wrapsAt, l.netWorth.length - 1));
                    if (l.wrapsAt < l.netWorth.length - 1) {
                      ctx.lineWidth = 1;
                      ctx.strokeStyle = withAlpha(held ? theme.ink.tertiary : color, held ? 0.2 : 0.3);
                      stroke(ctx, l, l.wrapsAt, l.netWorth.length - 1);
                    }
                  }
                }
                done();
              }}
            />
            <SvgLayer>
              <ValueAxis y={y} format="money" ticks={ticks} grid={false} title="Net worth, today’s dollars" />
              <AgeAxis x={x} time={time} />
              {focused && (
                <PlotClip>
                  <path d={path(focused)} fill="none" stroke={theme.surface} strokeWidth={5} strokeLinejoin="round" />
                  <path d={path(focused)} fill="none" stroke={focused.held ? theme.ink.primary : theme.status.depleted} strokeWidth={2} strokeLinejoin="round" />
                </PlotClip>
              )}
            </SvgLayer>
            <Cursor
              count={time.length}
              xOf={(i) => xs[i] ?? 0}
              label="Net worth of one life per starting year, by age"
              describe={(i) => `Age ${Math.floor(time.ages[i] as number)}: ${lives.filter((l) => l.netWorth.length > i).length} of these lives are still going.`}
              clickable={focused !== null}
              onPointer={(i, _px, py) => {
                if (i === null) return onFocus(null);
                let best: CohortLife | null = null;
                let distance = 14;
                for (const l of lives) {
                  const v = l.netWorth[i];
                  if (v === undefined) continue;
                  const d = Math.abs(y(v) - py);
                  if (d < distance) {
                    distance = d;
                    best = l;
                  }
                }
                onFocus(best ? best.year : null);
              }}
              onSelect={() => focused && navigateToLife(focused.path, 'sample')}
              tooltip={(i) => {
                const going = lives.filter((l) => l.netWorth.length > i);
                const v = focused?.netWorth[i];
                return (
                  <>
                    <TipTitle detail={String(time.years[i])}>Age {Math.floor(time.ages[i] as number)}</TipTitle>
                    {focused && v !== undefined ? (
                      <>
                        <TipRow color={focused.held ? theme.ink.primary : theme.status.depleted} label={`Started ${startMonth(focused.year)} ${focused.year}`} value={moneyCompact(v)} strong />
                        <TipNote>
                          Life #{focused.path} {focused.held ? 'never ran short' : 'ran short of essentials'}
                          {i >= focused.wrapsAt ? '; by this age it is past the end of the record and replaying it from 1926' : ''}. Click to open it.
                        </TipNote>
                      </>
                    ) : (
                      <TipRow label="Lives still going" value={`${going.length} of ${lives.length}`} />
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

function CohortStrip({ rows, focusYear, onFocus }: { rows: CohortYear[]; focusYear: number | null; onFocus: (year: number | null) => void }) {
  const maxShare = Math.max(0.1, ...rows.map((r) => r.shortShare ?? 0));
  const firstWrap = rows.find((r) => r.wraps);
  return (
    <ChartSyncProvider>
      <ChartFrame height={150} margin={{ top: 30, right: 12, bottom: 24, left: 52 }} aria-label="Share of simulated lives that ran short, by the year of the historical record they start in">
        {({ plot, theme }) => {
          const first = rows[0]?.year ?? HISTORY.firstYear;
          const last = rows[rows.length - 1]?.year ?? first + 1;
          const x = linearScale([first - 0.5, last + 0.5], [plot.x, plot.right]);
          const y = linearScale([0, maxShare], [plot.bottom, plot.y]);
          const w = Math.max(1, x(1) - x(0) - (x(1) - x(0) > 5 ? 2 : 1));
          const bar = (r: CohortYear, fill: string, key?: number) => {
            const share = r.shortShare ?? 0;
            const h = plot.bottom - y(share);
            if (h <= 0) return <rect key={key} x={x(r.year) - w / 2} y={plot.bottom - 1.5} width={w} height={1.5} fill={withAlpha(theme.ink.tertiary, 0.5)} />;
            const radius = Math.min(4, w / 2, h);
            return <path key={key} d={`M${x(r.year) - w / 2},${plot.bottom}v${-(h - radius)}q0,${-radius} ${radius},${-radius}h${w - 2 * radius}q${radius},0 ${radius},${radius}v${h - radius}z`} fill={fill} />;
          };
          const focused = focusYear === null ? undefined : rows.find((r) => r.year === focusYear);
          return (
            <>
              <SvgLayer>
                <ValueAxis y={y} format="share100" count={2} title="Ran short, of 100 lives starting that year" />
                <XAxis x={x} format={(v) => String(v)} ticks={Array.from({ length: Math.floor((last - 1930) / 10) + 1 }, (_, i) => 1930 + i * 10)} />
                {rows.map((r, i) => bar(r, withAlpha(theme.status.depleted, r.wraps ? 0.45 : 1), i))}
                {firstWrap && x(firstWrap.year) < plot.right - 40 && (
                  <g fontSize={10.5}>
                    <line x1={x(firstWrap.year - 0.5)} x2={x(firstWrap.year - 0.5)} y1={plot.y - 12} y2={plot.bottom} stroke={theme.annotation} strokeWidth={1} />
                    <text x={x(firstWrap.year - 0.5) + 5} y={plot.y - 4} fill={theme.ink.secondary}>
                      From {firstWrap.year}, lives outrun the record
                    </text>
                  </g>
                )}
                {focused && bar(focused, theme.ink.primary)}
              </SvgLayer>
              <Cursor
                count={rows.length}
                xOf={(i) => x(rows[i]?.year ?? first)}
                label="Lives that ran short, by starting year"
                crosshair={false}
                describe={(i) => {
                  const r = rows[i];
                  if (!r) return '';
                  return r.lives === 0 ? `${r.year}: no lives in the sample.` : `Starting in ${r.year}: ${r.short} of ${r.lives} lives ran short.${r.wraps ? ' These lives outrun the record.' : ''} Press Enter to open one.`;
                }}
                clickable
                onPointer={(i) => onFocus(i === null ? null : (rows[i]?.year ?? null))}
                onSelect={(i) => {
                  const path = pathForYear(rows[i]?.year ?? 0);
                  if (path !== null) navigateToLife(path, 'sample');
                }}
                tooltip={(i) => {
                  const r = rows[i];
                  if (!r) return null;
                  return (
                    <>
                      <TipTitle detail={`${r.lives} lives`}>Starting in {r.year}</TipTitle>
                      <TipRow color={theme.status.depleted} swatch="box" label="Ran short" value={r.lives ? `${r.short} of ${r.lives}` : 'n/a'} strong />
                      {r.shortShare !== null && <TipRow label="Per 100" value={ofHundred(r.shortShare).n} />}
                      {r.medianWealth !== null && <TipRow label="Median ending" value={moneyCompact(r.medianWealth)} />}
                      <TipNote>{r.wraps ? 'These lives outrun the record and replay it from 1926. ' : ''}Click to open a life that starts this year.</TipNote>
                    </>
                  );
                }}
              />
            </>
          );
        }}
      </ChartFrame>
    </ChartSyncProvider>
  );
}

export function Cohorts({ summary, traces }: { summary: Summary; traces: CohortTraces }) {
  const rows = useMemo(() => cohortYears(summary), [summary]);
  const [focusYear, setFocusYear] = useState<number | null>(null);
  return (
    <div>
      {traces.lives.length > 0 ? (
        <ChartSyncProvider>
          <CohortPaths summary={summary} traces={traces} focusYear={focusYear} onFocus={setFocusYear} />
        </ChartSyncProvider>
      ) : (
        <div className="flex h-40 items-center justify-center rounded-md border border-dashed border-line text-sm text-ink-3" role="status">
          {traces.error ?? 'Tracing one life from each starting year…'}
        </div>
      )}
      {traces.lives.length > 0 && traces.lives.length < traces.total && !traces.error && (
        <p className="mt-1 text-2xs text-ink-3" role="status">
          Tracing lives: {traces.lives.length} of {traces.total} starting years drawn.
        </p>
      )}
      {traces.error && traces.lives.length > 0 && <p className="mt-1 text-2xs text-ink-3">Some lives could not be traced: {traces.error}</p>}
      <div className="mt-5">
        <CohortStrip rows={rows} focusYear={focusYear} onFocus={setFocusYear} />
      </div>
    </div>
  );
}
