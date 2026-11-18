// The same life, two decisions: one simulated future (the same markets, career, health
// and lifespan) replayed under the plan and under the scenario, with the year the two
// part ways marked. The engine re-simulates the one path for each plan on request.
import { useEffect, useMemo, useState } from 'react';
import type { EnginePool } from '@/engine';
import type { PathScalar, PathTrace, Summary } from '@/engine/output-types';
import type { Profile } from '@/engine/types';
import { navigateToLife } from '@/state/route';
import { Icon, SegmentedControl, Skeleton, moneyCompact } from '@/ui';
import {
  AgeAxis,
  CanvasLayer,
  ChartCard,
  ChartFrame,
  Cursor,
  EndLabels,
  EventMarker,
  Legend,
  RuleAnnotations,
  STATUS_LABEL,
  SvgLayer,
  TipNote,
  TipRow,
  TipTitle,
  ValueAxis,
  clipPlot,
  createTimeAxis,
  drawGrid,
  drawLine,
  linearScale,
  rulesHeight,
  useChartTheme,
  type EndLabel,
  type TableData,
} from '@/ui/charts';
import { chooseLives, divergence, type LifeChoice, type LifeWhy } from './model';

const WHY_LABEL: Record<LifeWhy, string> = { rescued: 'A life it rescues', harmed: 'A life it harms', typical: 'A typical change', gain: 'The biggest gain', loss: 'The biggest loss' };

function whyText(c: LifeChoice, kept: number): string {
  switch (c.why) {
    case 'rescued':
      return `Picked because the plan falls short in this life and the change makes it hold: the middle one, by change in final wealth, of ${c.among} such lives among the first ${kept.toLocaleString('en-US')} simulated.`;
    case 'harmed':
      return `Picked because the plan holds in this life and the change makes it fall short: the middle one, by change in final wealth, of ${c.among} such lives among the first ${kept.toLocaleString('en-US')} simulated.`;
    case 'typical':
      return `Picked because its change in wealth at the end is the median one among the first ${kept.toLocaleString('en-US')} simulated lives.`;
    case 'gain':
      return `Picked because it gains the most wealth at the end from the change among the first ${kept.toLocaleString('en-US')} simulated lives: an extreme, not a typical life.`;
    case 'loss':
      return `Picked because it loses the most wealth at the end from the change among the first ${kept.toLocaleString('en-US')} simulated lives: an extreme, not a typical life.`;
  }
}

const scalar = (t: PathTrace, key: PathScalar) => t.scalars.find((s) => s.scalar === key)?.value ?? null;
const column = (t: PathTrace, key: string) => t.annual.columns.find((c) => c.key === key)?.values ?? [];

interface Pair {
  a: PathTrace;
  b: PathTrace;
}

type Loaded = { key: string; status: 'ready'; pair: Pair } | { key: string; status: 'error'; message: string };

/** One sentence on what the decision did to this life. */
export function lifeTakeaway(pair: Pair, name: string, ages: number[]): string {
  const { a, b } = pair;
  const okA = scalar(a, 'success') === 1;
  const okB = scalar(b, 'success') === 1;
  const endA = scalar(a, 'terminal_net_worth');
  const endB = scalar(b, 'terminal_net_worth');
  const shortA = scalar(a, 'depletion_age');
  const shortB = scalar(b, 'depletion_age');
  const at = divergence(column(a, 'net_worth'), column(b, 'net_worth'));
  const parted = at === null ? ' The two versions of this life never differ by more than 1%.' : ` The two part ways at ${Math.floor(ages[at] ?? 0)}.`;
  const ends = endA !== null && endB !== null ? `it ends with ${moneyCompact(endB)} instead of ${moneyCompact(endA)}` : '';
  const lead = `In life #${a.path_index}`;
  if (!okA && okB) return `${lead} the plan ${shortA !== null ? `runs short at ${Math.floor(shortA)}` : 'falls short'}; with “${name}” the same life holds${ends ? `, and ${ends}` : ''}.${parted}`;
  if (okA && !okB) return `${lead} the plan holds; with “${name}” the same life ${shortB !== null ? `runs short at ${Math.floor(shortB)}` : 'falls short'}${ends ? `, and ${ends}` : ''}.${parted}`;
  const same = okA ? 'the plan holds either way' : 'the plan falls short either way';
  return `${lead} ${same}${ends ? `; with “${name}” ${ends}` : ''}.${parted}`;
}

export function lifeTable(pair: Pair, ages: number[]): TableData {
  const nwA = column(pair.a, 'net_worth');
  const nwB = column(pair.b, 'net_worth');
  const spA = column(pair.a, 'spending');
  const spB = column(pair.b, 'spending');
  const fmt = (v: number) => moneyCompact(v);
  const n = Math.max(pair.a.annual.years.length, pair.b.annual.years.length);
  return {
    caption: `Simulated life #${pair.a.path_index} under the plan and under the scenario, by year, today's dollars`,
    columns: [
      { key: 'age', label: 'Age' },
      { key: 'year', label: 'Year', format: String },
      { key: 'a', label: 'Net worth, plan', format: fmt },
      { key: 'b', label: 'Net worth, scenario', format: fmt },
      { key: 'gap', label: 'Scenario minus plan', format: fmt },
      { key: 'sa', label: 'Spending, plan', format: fmt },
      { key: 'sb', label: 'Spending, scenario', format: fmt },
      { key: 'stage_a', label: 'Stage, plan', align: 'left' },
      { key: 'stage_b', label: 'Stage, scenario', align: 'left' },
    ],
    rows: Array.from({ length: n }, (_, i) => {
      const a = nwA[i] ?? null;
      const b = nwB[i] ?? null;
      const stage = (t: PathTrace) => {
        const s = t.statuses[t.annual.status[i] ?? -1];
        return s ? STATUS_LABEL[s] : null;
      };
      return { age: Math.floor(ages[i] ?? 0), year: pair.a.annual.years[i] ?? pair.b.annual.years[i] ?? null, a, b, gap: a !== null && b !== null ? b - a : null, sa: spA[i] ?? null, sb: spB[i] ?? null, stage_a: stage(pair.a), stage_b: stage(pair.b) };
    }),
  };
}

function LifeChart({ pair, ages, name }: { pair: Pair; ages: number[]; name: string }) {
  const years = pair.a.annual.years.length >= pair.b.annual.years.length ? pair.a.annual.years : pair.b.annual.years;
  const time = useMemo(() => createTimeAxis({ ages: ages.slice(0, years.length), years }), [ages, years]);
  const nwA = column(pair.a, 'net_worth');
  const nwB = column(pair.b, 'net_worth');
  const at = divergence(nwA, nwB);
  const short = name.length > 14 ? 'Scenario' : name;
  const yearIndex = (month: string) => years.indexOf(Number(month.slice(0, 4)));
  const moment = (t: PathTrace, kind: 'retired' | 'depleted') => {
    const e = t.events.find((x) => x.kind === kind);
    const i = e ? yearIndex(e.month) : -1;
    return i >= 0 ? i : null;
  };
  const retiredAt = [moment(pair.a, 'retired'), moment(pair.b, 'retired')];
  const atRetirement = at !== null && retiredAt.some((i) => i !== null && Math.abs(i - at) <= 1);
  const rules = at === null ? [] : [{ index: at, label: 'The two part ways', detail: `age ${Math.floor(time.ages[at] ?? 0)}${atRetirement ? ', at retirement' : ''}` }];

  return (
    <ChartFrame height={(w) => Math.round(Math.min(320, Math.max(230, w * 0.42)))} margin={{ top: rules.length ? rulesHeight(1) + 14 : 22, right: Math.max(58, Math.min(110, short.length * 6.2 + 30)), bottom: 36, left: 52 }} aria-label={`Net worth by age in one simulated life, under the plan and under ${name}`}>
      {({ plot, theme }) => {
        const x = linearScale(time.domain, [plot.x, plot.right]);
        const xs = time.ages.map((a) => x(a));
        const all = [...nwA, ...nwB].filter((v): v is number => v !== null);
        const lo = Math.min(0, ...all);
        const hi = Math.max(1, ...all);
        const y = linearScale([lo - (lo < 0 ? (hi - lo) * 0.05 : 0), hi + (hi - lo) * 0.06], [plot.bottom, plot.y]);
        const ticks = y.ticks(Math.max(2, Math.round(plot.height / 56)));
        const last = (values: (number | null)[]) => {
          let k = values.length - 1;
          while (k >= 0 && values[k] === null) k--;
          return k;
        };
        const labels: EndLabel[] = [];
        const kA = last(nwA);
        const kB = last(nwB);
        if (kA >= 0) labels.push({ key: 'a', y: y(nwA[kA] as number), text: 'Plan', color: theme.baseline.median, strong: true });
        if (kB >= 0) labels.push({ key: 'b', y: y(nwB[kB] as number), text: short, color: theme.scenario.line, strong: true });
        const marks = [
          { key: 'ra', i: moment(pair.a, 'retired'), values: nwA, other: nwB, label: 'Retires' },
          { key: 'rb', i: moment(pair.b, 'retired'), values: nwB, other: nwA, label: 'Retires' },
          { key: 'da', i: moment(pair.a, 'depleted'), values: nwA, other: nwB, label: 'Runs short' },
          { key: 'db', i: moment(pair.b, 'depleted'), values: nwB, other: nwA, label: 'Runs short' },
        ].filter((m): m is typeof m & { i: number } => m.i !== null && m.values[m.i] !== null && m.values[m.i] !== undefined);
        // When both plans retire (or run short) in the same year, one label does for both.
        const shown = marks.filter((m, _, list) => !(m.key.endsWith('b') && list.some((o) => o.key === `${m.key[0]}a` && o.i === m.i)));
        const stage = (t: PathTrace, i: number) => {
          const s = t.statuses[t.annual.status[i] ?? -1];
          return s ? STATUS_LABEL[s] : 'n/a';
        };
        return (
          <>
            <CanvasLayer
              deps={[pair]}
              draw={(ctx) => {
                drawGrid(ctx, plot, y, ticks, theme.grid, lo < 0 ? theme.axis : undefined);
                const done = clipPlot(ctx, plot, 1);
                drawLine(ctx, xs, nwA, y, { color: theme.baseline.median, width: 2 });
                drawLine(ctx, xs, nwB, y, { color: theme.scenario.line, width: 2 });
                done();
              }}
            />
            <SvgLayer>
              <ValueAxis y={y} format="money" title="Net worth in this one life, today's dollars" titleY={rules.length ? 10 : undefined} ticks={ticks} grid={false} />
              <AgeAxis x={x} time={time} />
              <RuleAnnotations items={rules.map((r) => ({ x: xs[r.index] as number, label: r.label, detail: r.detail, tone: 'event' as const }))} />
              {shown.map((m) => (
                <EventMarker key={m.key} x={xs[m.i] as number} y={y(m.values[m.i] as number)} label={at !== null && Math.abs(m.i - at) <= 1 ? undefined : m.label} color={m.key.endsWith('a') ? theme.baseline.median : theme.scenario.line} side={(m.values[m.i] as number) >= (m.other[m.i] ?? -Infinity) ? 'above' : 'below'} />
              ))}
              <EndLabels labels={labels} gap={14} />
            </SvgLayer>
            <Cursor
              count={time.length}
              xOf={(i) => xs[i] ?? 0}
              label={`Net worth by age in life #${pair.a.path_index}`}
              describe={(i) => `Age ${Math.floor(time.ages[i] ?? 0)}, ${time.years[i]}: plan ${nwA[i] === null || nwA[i] === undefined ? 'has ended' : moneyCompact(nwA[i] as number)}, ${name} ${nwB[i] === null || nwB[i] === undefined ? 'has ended' : moneyCompact(nwB[i] as number)}.`}
              marks={(i) => (
                <g>
                  {nwA[i] !== null && nwA[i] !== undefined && <circle cx={xs[i]} cy={y(nwA[i] as number)} r={4} fill={theme.baseline.median} stroke={theme.surface} strokeWidth={2} />}
                  {nwB[i] !== null && nwB[i] !== undefined && <circle cx={xs[i]} cy={y(nwB[i] as number)} r={4} fill={theme.scenario.line} stroke={theme.surface} strokeWidth={2} />}
                </g>
              )}
              tooltip={(i) => {
                const a = nwA[i];
                const b = nwB[i];
                return (
                  <>
                    <TipTitle detail={String(time.years[i])}>Age {Math.floor(time.ages[i] ?? 0)}</TipTitle>
                    <TipRow color={theme.scenario.line} label={short} value={b === null || b === undefined ? 'n/a' : moneyCompact(b)} strong />
                    <TipRow label="stage" value={stage(pair.b, i)} />
                    <TipRow color={theme.baseline.median} label="Plan" value={a === null || a === undefined ? 'n/a' : moneyCompact(a)} strong />
                    <TipRow label="stage" value={stage(pair.a, i)} />
                    {a !== null && a !== undefined && b !== null && b !== undefined && <TipRow label="Difference" value={`${b - a > 0 ? '+' : ''}${moneyCompact(b - a)}`} />}
                    <TipNote>One simulated life, not a forecast.</TipNote>
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

export interface SameLifeProps {
  pool: EnginePool;
  seed: number;
  /** Trace only when the summaries describe the current plan and both runs have finished. */
  ready: boolean;
  dimmed: boolean;
  base: { profile: Profile; summary: Summary };
  scenario: { id: string; name: string; profile: Profile; summary: Summary };
}

export function SameLife({ pool, seed, ready, dimmed, base, scenario }: SameLifeProps) {
  const theme = useChartTheme();
  const choices = useMemo(() => chooseLives(base.summary, scenario.summary), [base.summary, scenario.summary]);
  const [why, setWhy] = useState<LifeWhy | null>(null);
  const choice = choices.find((c) => c.why === why) ?? choices[0] ?? null;
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  const kept = base.summary.scatter.paths.length;
  const key = choice && ready ? `${scenario.id}:${choice.pathIndex}:${seed}:${base.summary.paths}` : null;

  useEffect(() => {
    if (!choice || !key) return;
    let cancelled = false;
    Promise.all([pool.trace(base.profile, choice.pathIndex, seed), pool.trace(scenario.profile, choice.pathIndex, seed)])
      .then(([a, b]) => {
        if (!cancelled) setLoaded({ key, status: 'ready', pair: { a, b } });
      })
      .catch((error: unknown) => {
        if (!cancelled) setLoaded({ key, status: 'error', message: error instanceof Error ? error.message : 'The engine could not replay this life.' });
      });
    return () => {
      cancelled = true;
    };
    // The profiles are captured with the key: a new key is a new pair of plans or a new life.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, pool]);

  // Keep the last life on screen (dimmed) while the next one is being replayed.
  const current = loaded && loaded.key === key ? loaded : null;
  const pair = loaded?.status === 'ready' && loaded.key.startsWith(`${scenario.id}:`) ? loaded.pair : null;
  const ages = base.summary.axis.ages;

  return (
    <ChartCard
      title="The same life, two decisions"
      takeaway={pair ? lifeTakeaway(pair, scenario.name, ages) : choice ? 'Replaying one simulated life under both plans…' : 'No life could be matched between the two runs.'}
      caption={
        choice && pair ? (
          <>
            {whyText(choice, kept)} Both lines are the same simulated future: the same markets, jobs, health and lifespan. Only the decision differs. A single life is an anecdote, not a forecast.{' '}
            <button type="button" className="inline-flex items-center gap-1 rounded-xs font-medium text-accent-ink hover:underline" onClick={() => navigateToLife(choice.pathIndex, 'scatter')}>
              Read this life year by year
              <Icon name="arrowRight" size={12} />
            </button>
          </>
        ) : undefined
      }
      captionText={choice ? `${whyText(choice, kept)} Both lines are the same simulated future; only the decision differs. Today's dollars.` : undefined}
      controls={
        choices.length > 1 ? (
          <SegmentedControl aria-label="Which life" size="sm" value={choice?.why ?? 'typical'} onChange={setWhy} options={choices.map((c) => ({ value: c.why, label: WHY_LABEL[c.why] }))} className="max-w-full flex-wrap" />
        ) : undefined
      }
      table={pair ? () => lifeTable(pair, ages) : undefined}
      filename={choice ? `life-${choice.pathIndex}-two-decisions` : 'same-life'}
      explain={pair && choice ? { context: () => ({ path_index: choice.pathIndex, why: choice.why, scenario: scenario.name, plan: Object.fromEntries(pair.a.scalars.filter((s) => ['success', 'retirement_age', 'depletion_age', 'terminal_net_worth', 'lifetime_spending'].includes(s.scalar)).map((s) => [s.scalar, s.value])), with_change: Object.fromEntries(pair.b.scalars.filter((s) => ['success', 'retirement_age', 'depletion_age', 'terminal_net_worth', 'lifetime_spending'].includes(s.scalar)).map((s) => [s.scalar, s.value])) }) } : undefined}
      dimmed={dimmed || (pair !== null && current === null)}
    >
      {pair ? (
        <>
          <div data-chart-legend className="mb-2">
            <Legend
              items={[
                { key: 'a', label: 'Under the plan', color: theme.baseline.median, swatch: 'line' },
                { key: 'b', label: `With “${scenario.name}”`, color: theme.scenario.line, swatch: 'line' },
              ]}
            />
          </div>
          <LifeChart pair={pair} ages={ages} name={scenario.name} />
        </>
      ) : loaded?.status === 'error' && current ? (
        <p className="flex items-start gap-2 py-6 text-sm text-ink-2">
          <Icon name="alertCircle" className="mt-0.5 text-bad" />
          <span>This life could not be replayed: {loaded.message}</span>
        </p>
      ) : choice ? (
        <div aria-busy="true">
          <Skeleton className="h-56 w-full" />
          {!ready && <p className="mt-2 text-xs text-ink-3">A life is picked and replayed as soon as both runs finish.</p>}
        </div>
      ) : (
        <p className="py-6 text-sm text-ink-2">The engine keeps per-life values for the first {kept.toLocaleString('en-US')} lives of each run; none of them appears in both.</p>
      )}
    </ChartCard>
  );
}
