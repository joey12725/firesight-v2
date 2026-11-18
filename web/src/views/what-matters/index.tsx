// What matters: "what does my result depend on most, and what should I change?"
// Everything on this page is a paired experiment. Each variant of the plan lives
// through the same simulated futures as the plan itself (same seed, same lives), so a
// difference is caused by the change, and each difference is shown with the sampling
// error of the pairing. Four parts: the biggest wins in words, the tornado, the
// trade-off map and the solver.
import { useEffect, useMemo, useRef, useState } from 'react';
import { levers as engineLevers } from '@/engine';
import type { PatchOp } from '@/engine/types';
import { compactPatch } from '@/state/patches';
import { useProfileStore } from '@/state/profile-store';
import { navigateToView } from '@/state/route';
import { toast } from '@/state/ui-store';
import { Button, Icon, SegmentedControl, Select, Skeleton, Table, Tag, Td, Th, count } from '@/ui';
import { ChartCard } from '@/ui/charts';
import type { ViewProps } from '../types';
import { ProbeChart, probeTable } from './Solver';
import { Tornado, TornadoLegend, tornadoGroups, tornadoTable } from './Tornado';
import { HeatScale, TradeoffMap, mapModel, mapTable } from './TradeoffMap';
import {
  METRICS,
  METRIC_IDS,
  ageNow,
  biggestWins,
  buildLevers,
  defaultAxes,
  effectOf,
  formatDelta,
  formatLevel,
  formatRange,
  makeGoal,
  readLevel,
  shortLabel,
  variablesFor,
  type Effect,
  type GoalId,
  type LeverDef,
  type LeverSide,
  type MetricId,
  type VarDef,
  type VarId,
} from './model';
import { GRID_STAGES, GridSweeper, LEVER_STAGES, LeverSweeper, SOLVER_PATHS, Solver, planKeyOf, useStore, type LeverRun, type Progress } from './sweeps';
import { intervalWords, leadSentence, mapTakeaway, riskSentence, solverSentence, tornadoTakeaway } from './text';

const rate = (p: Progress) => (p.perSecond >= 1000 ? `${Math.round(p.perSecond / 1000)}k` : String(Math.round(p.perSecond)));

/** "23,100 of 110,250 lives simulated · 28k lives a second" while a sweep runs; the total once it is done. */
function progressNote(progress: Progress | null, busy: boolean): string {
  if (!progress) return '';
  if (busy) return ` ${count(progress.lives)} of ${count(progress.total)} lives simulated for this chart so far${progress.lives > 0 ? `, ${rate(progress)} a second` : ''}.`;
  return ` ${count(progress.lives)} lives simulated for this chart, ${rate(progress)} a second.`;
}

function SweepTag({ stale, busy, paths, unit, error }: { stale: boolean; busy: boolean; paths: number | null; unit: string; error: string | null }) {
  if (error) return <Tag tone="bad" icon="alertTriangle">Could not finish</Tag>;
  if (stale) return <Tag>Updating…</Tag>;
  if (busy && paths === null) return <Tag>Simulating…</Tag>;
  if (busy && paths !== null) return <Tag tone={paths < 1000 ? 'warn' : 'neutral'}>{paths < 1000 ? `Early picture: ${count(paths)} lives per ${unit}` : `Sharpening: ${count(paths)} lives per ${unit} so far`}</Tag>;
  return null;
}

function WhatMattersSkeleton() {
  return (
    <div className="mx-auto max-w-[76rem] space-y-10 py-8" aria-busy="true" aria-label="Simulating">
      <div className="space-y-3">
        <Skeleton className="h-3 w-28" />
        <Skeleton className="h-8 w-full max-w-2xl" />
        <Skeleton className="h-8 w-2/3 max-w-xl" />
      </div>
      <Skeleton className="h-24 w-full" />
      <Skeleton className="h-96 w-full" />
    </div>
  );
}

/** Adds a change as a scenario through the profile store, and says so. */
function addScenario(name: string, patch: PatchOp[]) {
  const result = useProfileStore.getState().addScenario({ name, patch });
  if (result.ok) toast({ title: `Added the scenario “${name}”`, description: 'It is simulated beside the plan from now on, on the same lives.', tone: 'good', action: { label: 'Open Scenarios', run: () => navigateToView('scenarios') } });
  else toast({ title: 'The scenario could not be added', description: result.error, tone: 'bad' });
}

/** True when a scenario with exactly this patch already exists. */
function useHasScenario(patch: PatchOp[] | null): boolean {
  const scenarios = useProfileStore((s) => s.scenarios);
  return useMemo(() => {
    if (!patch) return false;
    const wanted = JSON.stringify(compactPatch(patch));
    return scenarios.some((s) => JSON.stringify(s.patch) === wanted);
  }, [scenarios, patch]);
}

function TryButton({ name, patch, label = 'Try this change' }: { name: string; patch: PatchOp[]; label?: string }) {
  const has = useHasScenario(patch);
  if (has) {
    return (
      <button type="button" className="inline-flex items-center gap-1 rounded-xs text-xs font-medium text-accent-ink hover:underline" onClick={() => navigateToView('scenarios')}>
        <Icon name="check" size={12} />
        In your scenarios
        <Icon name="arrowRight" size={12} />
      </button>
    );
  }
  return (
    <Button size="sm" icon="plus" onClick={() => addScenario(name, patch)}>
      {label}
    </Button>
  );
}

const scenarioName = (lever: LeverDef, side: LeverSide) => `${lever.label}: ${shortLabel(side.label)}`;

// ---- the biggest wins ---------------------------------------------------------------

function Wins({ levers, effects, metric, run }: { levers: LeverDef[]; effects: ReadonlyMap<string, Effect>; metric: MetricId; run: LeverRun }) {
  const wins = useMemo(() => biggestWins(levers, effects, metric), [levers, effects, metric]);
  const risk = useMemo(() => riskSentence(levers, effects, metric), [levers, effects, metric]);
  const unit = METRICS[metric].unit;
  return (
    <div>
      {wins.length > 0 && (
        <ol className="mt-6 grid grid-cols-1 gap-x-10 gap-y-5 border-t border-line pt-5 @lg:grid-cols-3 @lg:gap-x-6" data-testid="wins">
          {wins.map((w, i) => (
            <li key={w.side.key} className="flex min-w-0 flex-col">
              <p className="eyebrow">{['Largest', 'Second', 'Third'][i]}</p>
              <p className="mt-1.5 text-sm font-medium text-ink-1">
                {w.lever.label} <span className="font-normal text-ink-2">{shortLabel(w.side.label)}</span>
              </p>
              <p className="mt-1 flex items-baseline gap-1.5">
                <span className="num text-2xl font-semibold tracking-tight text-ink-1">{formatDelta(metric, w.effect.delta).replace(/ (yr|pts)$/, '')}</span>
                <span className="text-xs text-ink-2">{unit === 'share' ? 'of 100 lives' : unit === 'age' ? 'years, middle life' : unit === 'ratio' ? 'points of plan' : 'at the end, middle life'}</span>
              </p>
              <p className="mt-0.5 text-xs text-pretty text-ink-3">95% interval {intervalWords(metric, w.effect.lo, w.effect.hi)}</p>
              <div className="mt-2.5">
                <TryButton name={scenarioName(w.lever, w.side)} patch={w.side.ops} />
              </div>
            </li>
          ))}
        </ol>
      )}
      <p className="mt-5 max-w-[46rem] text-xs leading-relaxed text-pretty text-ink-3">
        {risk && <span className="text-ink-2">{risk} </span>}
        Each change was simulated on the same {count(run.paths)} lives as the plan, one change at a time, so a difference comes from the change and not from luck; the intervals cover sampling error only. These are
        associations inside this model, not advice: changes made together do not simply add up, and the ranges tested (10% of spending, a point of return) are not equally likely.
      </p>
    </div>
  );
}

// ---- paired detail of one lever setting -----------------------------------------------

function LeverDetail({ lever, side, run }: { lever: LeverDef; side: LeverSide; run: LeverRun }) {
  const entry = run.sides[side.key];
  if (!entry) {
    return <p className="mt-4 rounded-md border border-line bg-surface-1 px-4 py-3 text-sm text-ink-2">{run.skipped[side.key] ? `This setting could not be simulated: ${run.skipped[side.key]}` : 'This setting has not been simulated yet.'}</p>;
  }
  const rows = METRIC_IDS.map((m) => ({ m, effect: effectOf(m, entry.paired, run.base, entry.headline) }));
  const holds = rows[0]?.effect;
  return (
    <div className="mt-4 rounded-md border border-line bg-surface-1 px-4 py-3.5" data-testid="lever-detail">
      <div className="flex flex-wrap items-start justify-between gap-x-4 gap-y-2">
        <div className="min-w-0">
          <p className="eyebrow">Paired detail</p>
          <p className="mt-1 text-sm font-medium text-ink-1">
            {lever.label}: <span className="font-normal text-ink-2">{side.label}</span>
          </p>
        </div>
        <TryButton name={scenarioName(lever, side)} patch={side.ops} />
      </div>
      <div className="scroll-thin mt-3 overflow-x-auto">
        <Table className="min-w-[30rem] text-xs">
          <caption className="sr-only">
            Effect of {lever.label}, {side.label}, on each measure, against the plan on the same {count(run.paths)} lives
          </caption>
          <thead>
            <tr>
              <Th>Measure</Th>
              <Th numeric>Plan</Th>
              <Th numeric>With the change</Th>
              <Th numeric>Paired change</Th>
              <Th>95% interval</Th>
            </tr>
          </thead>
          <tbody>
            {rows.map(({ m, effect }) => (
              <tr key={m}>
                <Td className="text-ink-1">{METRICS[m].label}</Td>
                <Td numeric>{formatLevel(m, effect.base)}</Td>
                <Td numeric>{formatLevel(m, effect.value)}</Td>
                <Td numeric className={effect.state === 'effect' ? 'font-medium text-ink-1' : 'text-ink-3'}>
                  {effect.state === 'none' ? <span className="font-sans">no change</span> : formatDelta(m, effect.delta)}
                </Td>
                <Td className="text-ink-2">{effect.state === 'unknown' ? 'not available' : effect.state === 'none' ? '' : `${intervalWords(m, effect.lo, effect.hi)}${effect.state === 'noise' ? ' · within noise' : ''}`}</Td>
              </tr>
            ))}
          </tbody>
        </Table>
      </div>
      {holds && holds.state !== 'unknown' && (
        <p className="mt-3 text-xs leading-relaxed text-pretty text-ink-2">
          Life by life: of {count(holds.n)} lives, <b className="font-medium text-ink-1">{count(holds.gained)}</b> hold only with the change, <b className="font-medium text-ink-1">{count(holds.lost)}</b> hold only without it, and{' '}
          {count(holds.n - holds.gained - holds.lost)} end the same way either way. “Paired change” for ages and wealth is the middle per-life change, which is why it can differ a little from the gap between the two
          columns of medians.
        </p>
      )}
    </div>
  );
}

// ---- trade-off map section ------------------------------------------------------------

function useInView<T extends Element>(): [React.RefObject<T | null>, boolean] {
  const ref = useRef<T>(null);
  const [seen, setSeen] = useState(false);
  useEffect(() => {
    const el = ref.current;
    if (!el || seen) return;
    const observer = new IntersectionObserver((entries) => entries.some((e) => e.isIntersecting) && setSeen(true), { rootMargin: '200px' });
    observer.observe(el);
    return () => observer.disconnect();
  }, [seen]);
  return [ref, seen];
}

function MapSection({ profile, seed, pool, vars, metric, runnable }: Pick<ViewProps, 'profile' | 'pool'> & { seed: number; vars: VarDef[]; metric: MetricId; runnable: boolean }) {
  const [sweeper] = useState(() => new GridSweeper(pool));
  const state = useStore(sweeper);
  const [ref, seen] = useInView<HTMLDivElement>();
  const defaults = useMemo(() => defaultAxes(vars), [vars]);
  const [chosen, setChosen] = useState<{ x: VarId; y: VarId } | null>(null);
  const [picked, setPicked] = useState<{ key: string; col: number; row: number } | null>(null);
  const ids = chosen && vars.some((v) => v.id === chosen.x) && vars.some((v) => v.id === chosen.y) ? chosen : defaults;
  const x = vars.find((v) => v.id === ids?.x);
  const y = vars.find((v) => v.id === ids?.y);

  useEffect(() => {
    if (seen && runnable && x && y) sweeper.request(profile, seed, x, y);
    else sweeper.stop();
  }, [sweeper, seen, runnable, profile, seed, x, y]);
  useEffect(() => () => sweeper.stop(), [sweeper]);

  const run = state.run && x && y && state.run.x === x.id && state.run.y === y.id ? state.run : null;
  const model = useMemo(() => (run ? mapModel(run, metric) : null), [run, metric]);
  const selected = run && picked && picked.key === run.key ? picked : null;
  const cell = run && selected ? run.cells[selected.row]?.[selected.col] : null;
  const def = METRICS[metric];
  const options = vars.map((v) => ({ value: v.id, label: v.label }));

  return (
    <div ref={ref}>
      <ChartCard
        className="mt-14 border-t border-line pt-5"
        title="Trade-off map"
        takeaway={run && model && x && y ? mapTakeaway({ metric, x, y, values: model.values, paths: model.paths }) : 'How two decisions trade against each other, every combination simulated on the same lives.'}
        caption={
          x && y ? (
            <>
              Each cell is the plan with those two values changed and everything else as entered, simulated on the same lives as every other cell, so neighbours differ because of the values and not by luck. The number
              in a cell is {def.unit === 'share' ? 'lives in 100' : def.unit === 'ratio' ? 'percent of plan' : def.unit === 'age' ? 'the age' : "today's dollars"}; lines join equal values, interpolated between cells.
              {model && model.paths > 0 && ` ${count(model.paths)} lives per cell${model.complete ? '' : ' in the coarse pass'}: a cell is good to about ±${Math.max(1, Math.round(100 / Math.sqrt(model.paths)))} in 100, so do not read much into neighbours a point or two apart.`}
              {progressNote(state.progress, state.busy)}
            </>
          ) : undefined
        }
        controls={
          x && y ? (
            <>
              <label className="flex items-center gap-2 text-xs text-ink-2">
                Across
                <Select aria-label="Variable across" className="w-52" value={x.id} onChange={(id) => setChosen({ x: id, y: id === y.id ? x.id : y.id })} options={options} />
              </label>
              <label className="flex items-center gap-2 text-xs text-ink-2">
                Up
                <Select aria-label="Variable up" className="w-52" value={y.id} onChange={(id) => setChosen({ x: id === x.id ? y.id : x.id, y: id })} options={options} />
              </label>
              <SweepTag stale={state.stale} busy={state.busy} paths={model && model.paths > 0 ? model.paths : null} unit="cell" error={state.error} />
            </>
          ) : undefined
        }
        table={run && x && y ? () => mapTable(run, x, y, metric) : undefined}
        filename="trade-off-map"
        explain={run && model && x && y ? { context: () => ({ measure: def.axis, across: { variable: x.label, values: run.xs, plan: x.current }, up: { variable: y.label, values: run.ys, plan: y.current }, cells_by_row: model.values, lives_per_cell: model.paths, seed }) } : undefined}
        dimmed={state.stale}
      >
        {state.error ? (
          <p className="flex items-start gap-2 py-6 text-sm text-ink-2">
            <Icon name="alertTriangle" className="mt-0.5 shrink-0 text-bad" />
            The map could not be simulated: {state.error}
          </p>
        ) : !x || !y ? (
          <p className="py-6 text-sm text-ink-2">This plan has fewer than two quantities that can be varied, so there is no trade-off to map. Add spending or an income first.</p>
        ) : run && model ? (
          <>
            <div className="mb-2 flex flex-wrap items-center justify-between gap-x-4 gap-y-1">
              <HeatScale lo={model.lo} hi={model.hi} unit={metric} tone={def.better > 0 ? 'cool' : 'warm'} note={def.unit === 'share' ? `lives ${metric === 'holds' ? 'in which the plan holds' : 'that run short'}` : def.label.toLowerCase()} />
            </div>
            <TradeoffMap run={run} x={x} y={y} metric={metric} selected={selected} onSelect={(col, row) => setPicked({ key: run.key, col, row })} />
            {cell && selected && (
              <div className="mt-3 flex flex-wrap items-start justify-between gap-x-4 gap-y-2 rounded-md border border-line bg-surface-1 px-4 py-3" data-testid="cell-detail">
                <div className="min-w-0 flex-1 basis-64">
                  <p className="text-sm font-medium text-ink-1">
                    {x.short} {x.format(run.xs[selected.col] as number)} · {y.short} {y.format(run.ys[selected.row] as number)}
                  </p>
                  <p className="mt-1 text-xs leading-relaxed text-pretty text-ink-2">
                    {METRIC_IDS.map((m) => {
                      const l = cell.levels[m];
                      return `${METRICS[m].label} ${formatLevel(m, l.value)}${l.lo !== null && l.hi !== null && l.value !== null ? ` (${formatRange(m, l.lo, l.hi)})` : ''}`;
                    }).join(' · ')}
                    . {count(cell.paths)} lives; brackets are 95% sampling intervals.
                  </p>
                </div>
                <TryButton name={`${x.short} ${x.format(run.xs[selected.col] as number)}, ${y.short.toLowerCase()} ${y.format(run.ys[selected.row] as number)}`} patch={[...x.ops(run.xs[selected.col] as number), ...y.ops(run.ys[selected.row] as number)]} label="Adopt as a scenario" />
              </div>
            )}
          </>
        ) : (
          <Skeleton className="h-80 w-full" />
        )}
      </ChartCard>
    </div>
  );
}

// ---- solver section -------------------------------------------------------------------

const GOAL_OPTIONS: { value: GoalId; label: string }[] = [
  { value: 'holds', label: 'the plan holds in at least' },
  { value: 'short', label: 'money runs short in at most' },
  { value: 'fi_by', label: 'work is optional in half of lives by age' },
];
const TARGETS: Record<Exclude<GoalId, 'fi_by'>, number[]> = { holds: [0.75, 0.8, 0.85, 0.9, 0.95, 0.99], short: [0.01, 0.02, 0.05, 0.1, 0.15, 0.2] };

function SolverSection({ profile, seed, pool, vars, runnable, medianFiAge }: Pick<ViewProps, 'profile' | 'pool'> & { seed: number; vars: VarDef[]; runnable: boolean; medianFiAge: number | null }) {
  const [solver] = useState(() => new Solver(pool));
  const state = useStore(solver);
  const solvable = useMemo(() => vars.filter((v) => v.bounds !== null), [vars]);
  const [varId, setVarId] = useState<VarId | null>(null);
  const [goalId, setGoalId] = useState<GoalId>('holds');
  const [targets, setTargets] = useState<Partial<Record<GoalId, number>>>({});
  const variable = solvable.find((v) => v.id === varId) ?? solvable.find((v) => v.id === 'spending') ?? solvable[0];
  const ages = useMemo(() => {
    const from = Math.ceil(ageNow(profile)) + 1;
    return Array.from({ length: Math.max(1, 71 - from) }, (_, i) => from + i);
  }, [profile]);
  const defaultAge = Math.min(ages[ages.length - 1] ?? 60, Math.max(ages[0] ?? 40, Math.round((medianFiAge ?? 55) - 2)));
  const target = targets[goalId] ?? (goalId === 'holds' ? 0.9 : goalId === 'short' ? 0.05 : defaultAge);
  const planKey = useMemo(() => planKeyOf(profile, seed), [profile, seed]);

  // A search belongs to the plan it ran on: stop it the moment the plan changes or the view closes.
  useEffect(() => () => solver.stop(), [solver, planKey]);

  const outdated = state.planKey !== null && state.planKey !== planKey;
  const shown = state.goal && state.variable && state.probes.length > 0 ? { goal: state.goal, variable: state.variable } : null;
  const words = shown && state.outcome ? solverSentence(shown.goal, shown.variable, state.outcome, SOLVER_PATHS) : null;
  const answer = state.outcome?.status === 'converged' && shown ? { x: state.outcome.safe.x, variable: shown.variable } : null;

  return (
    <ChartCard
      className="mt-14 border-t border-line pt-5"
      title="Find the number"
      takeaway={words ? words.answer : state.status === 'running' && shown ? `Searching: ${state.probes.length} ${state.probes.length === 1 ? 'value' : 'values'} of ${shown.variable.label.toLowerCase()} tested so far.` : 'Turn a goal into a number: the most you can spend, the earliest you can stop, the salary the plan needs.'}
      caption={
        shown ? (
          <>
            {words?.detail} Dots are the values tested, numbered in order (filled where the goal is met), with the 95% sampling interval of each run of {count(SOLVER_PATHS)} lives; every test uses the same lives, so their
            order is more reliable than the intervals alone suggest. {state.outcome?.status === 'converged' && state.outcome.band ? 'The shaded band is where the boundary could fall on a different draw of lives. ' : ''}
            {state.lives > 0 && `${count(state.lives)} lives simulated for this search${state.perSecond > 0 ? `, ${state.perSecond >= 1000 ? `${Math.round(state.perSecond / 1000)}k` : Math.round(state.perSecond)} a second` : ''}.`}
          </>
        ) : (
          `A search halves the range up to eight times, about ten runs of ${count(SOLVER_PATHS)} lives on the same futures as the plan. It assumes the measure moves in one direction as the quantity grows.`
        )
      }
      table={shown ? () => probeTable(state.probes, shown.goal, shown.variable) : undefined}
      filename="find-the-number"
      explain={shown && state.outcome ? { context: () => ({ goal: shown.goal.id, target: shown.goal.target, variable: shown.variable.label, plan_value: shown.variable.current, outcome: state.outcome, tests: state.probes.map((p) => ({ x: p.x, value: p.level.value, lo: p.level.lo, hi: p.level.hi, met: p.met })), lives_per_test: SOLVER_PATHS, seed }) } : undefined}
      dimmed={outdated}
    >
      {solvable.length === 0 || !variable ? (
        <p className="py-4 text-sm text-ink-2">This plan has nothing the solver can vary yet. Add spending or a job first.</p>
      ) : (
        <form
          className="flex flex-wrap items-center gap-x-2 gap-y-2 text-sm text-ink-2"
          data-testid="solver-form"
          onSubmit={(e) => {
            e.preventDefault();
            if (runnable) void solver.start(profile, seed, makeGoal(goalId, target), variable);
          }}
        >
          <span>Find the</span>
          <Select aria-label="Quantity to solve for" className="w-60" value={variable.id} onChange={setVarId} options={solvable.map((v) => ({ value: v.id, label: v.label.toLowerCase() }))} />
          <span>at which</span>
          <Select aria-label="Goal" className="w-64" value={goalId} onChange={setGoalId} options={GOAL_OPTIONS} />
          {goalId === 'fi_by' ? (
            <Select aria-label="Age" className="w-20" value={String(target)} onChange={(v) => setTargets((t) => ({ ...t, fi_by: Number(v) }))} options={ages.map((a) => ({ value: String(a), label: String(a) }))} />
          ) : (
            <>
              <Select aria-label="Lives in 100" className="w-20" value={String(target)} onChange={(v) => setTargets((t) => ({ ...t, [goalId]: Number(v) }))} options={TARGETS[goalId].map((t) => ({ value: String(t), label: String(Math.round(t * 100)) }))} />
              <span>of 100 lives</span>
            </>
          )}
          <span className="flex items-center gap-2">
            <Button type="submit" variant="primary" size="md" icon="search" disabled={!runnable || state.status === 'running'}>
              {state.status === 'running' ? 'Searching…' : 'Find it'}
            </Button>
            {state.status === 'running' && (
              <Button size="md" variant="ghost" onClick={() => solver.stop()}>
                Stop
              </Button>
            )}
          </span>
        </form>
      )}
      {outdated && (
        <p className="mt-3 flex items-center gap-2 text-xs text-ink-2">
          <Icon name="info" size={14} className="text-ink-3" />
          The plan has changed since this search ran. Search again for the current plan.
        </p>
      )}
      {state.status === 'error' && state.error && (
        <p className="mt-3 flex items-start gap-2 text-sm text-ink-2">
          <Icon name="alertTriangle" className="mt-0.5 shrink-0 text-bad" />
          {state.error}
        </p>
      )}
      {shown && (
        <div className="mt-4">
          <ProbeChart probes={state.probes} goal={shown.goal} variable={shown.variable} outcome={state.outcome} />
        </div>
      )}
      {answer && !outdated && answer.variable.current !== answer.x && (
        <div className="mt-3">
          <TryButton name={`${answer.variable.short} ${answer.variable.format(answer.x)}`} patch={answer.variable.ops(answer.x)} label={`Apply ${answer.variable.format(answer.x)} as a scenario`} />
        </div>
      )}
    </ChartCard>
  );
}

// ---- the view -----------------------------------------------------------------------

export default function WhatMattersView({ profile, summary, run, pool }: ViewProps) {
  const seed = useProfileStore((s) => s.run.seed);
  const [metric, setMetric] = useState<MetricId>('holds');
  const [picked, setPicked] = useState<string | null>(null);
  const [sweeper] = useState(() => new LeverSweeper(pool));
  const sweep = useStore(sweeper);
  const runnable = run.status !== 'blocked' && run.status !== 'idle' && run.status !== 'error';

  const defs = useMemo(() => {
    try {
      return buildLevers(profile, engineLevers(profile));
    } catch {
      return [];
    }
  }, [profile]);
  const vars = useMemo(() => variablesFor(profile), [profile]);

  useEffect(() => {
    if (runnable && defs.length > 0) sweeper.request(profile, seed, defs);
    else sweeper.stop();
  }, [sweeper, runnable, profile, seed, defs]);
  useEffect(() => () => sweeper.stop(), [sweeper]);

  const result = sweep.run;
  const effects = useMemo(() => {
    const map = new Map<string, Effect>();
    if (!result) return map;
    for (const lever of defs) {
      for (const side of lever.sides) {
        const entry = result.sides[side.key];
        if (entry) map.set(side.key, effectOf(metric, entry.paired, result.base, entry.headline));
      }
    }
    return map;
  }, [result, defs, metric]);
  const groups = useMemo(() => tornadoGroups(defs, effects, metric, result?.skipped), [defs, effects, metric, result]);
  const selectedLever = defs.find((l) => l.sides.some((x) => x.key === picked));
  const selectedSide = selectedLever?.sides.find((x) => x.key === picked);
  const selected = selectedLever && selectedSide ? { lever: selectedLever, side: selectedSide } : null;

  if (!summary) return run.status === 'running' || run.status === 'queued' ? <WhatMattersSkeleton /> : null;

  const def = METRICS[metric];
  const base = result ? readLevel(result.base, metric) : null;
  const lead = result ? leadSentence(defs, effects, metric, result.paths) : null;
  const dimmed = sweep.stale || run.stale;

  return (
    <div className="@container mx-auto max-w-[76rem] pb-8 pt-7">
      <div className="flex min-h-5 flex-wrap items-center gap-2">
        <p className="eyebrow">What the result depends on</p>
        <SweepTag stale={sweep.stale} busy={sweep.busy} paths={result?.paths ?? null} unit="test" error={sweep.error} />
      </div>
      <div className={dimmed ? 'opacity-60 transition-opacity duration-[var(--duration-slow)]' : 'transition-opacity duration-[var(--duration-slow)]'}>
        {lead ? (
          <p data-testid="lead-sentence" className="mt-3 max-w-[40rem] text-2xl font-medium tracking-tight text-pretty text-ink-1 @4xl:text-[1.75rem] @4xl:leading-[2.25rem]">
            {lead.map((s, i) =>
              s.strong ? (
                <b key={i} className="font-semibold">
                  {s.text}
                </b>
              ) : (
                <span key={i} className="text-ink-2">
                  {s.text}
                </span>
              ),
            )}
          </p>
        ) : sweep.error ? (
          <p className="mt-3 flex max-w-[40rem] items-start gap-2 text-base text-ink-2">
            <Icon name="alertTriangle" className="mt-1 shrink-0 text-bad" />
            The levers could not be tested: {sweep.error}
          </p>
        ) : (
          <div className="mt-3 space-y-3" aria-busy="true" aria-label="Testing the levers">
            <Skeleton className="h-8 w-full max-w-2xl" />
            <Skeleton className="h-8 w-2/3 max-w-xl" />
          </div>
        )}
      </div>

      <div className="mt-6 flex flex-wrap items-center gap-x-3 gap-y-2">
        <span id="measure-label" className="text-xs font-medium text-ink-2">
          Measured by
        </span>
        <div className="hidden @xl:block">
          <SegmentedControl aria-labelledby="measure-label" value={metric} onChange={setMetric} options={METRIC_IDS.map((m) => ({ value: m, label: METRICS[m].label }))} />
        </div>
        <div className="@xl:hidden">
          <Select className="w-48" aria-labelledby="measure-label" value={metric} onChange={setMetric} options={METRIC_IDS.map((m) => ({ value: m, label: METRICS[m].label }))} />
        </div>
        <p className="hidden basis-full text-xs text-ink-3 @2xl:block">The choice applies to everything below. Money is in today’s dollars.</p>
      </div>

      {result && (
        <div className={dimmed ? 'opacity-60 transition-opacity duration-[var(--duration-slow)]' : 'transition-opacity duration-[var(--duration-slow)]'}>
          <Wins levers={defs} effects={effects} metric={metric} run={result} />
        </div>
      )}

      <ChartCard
        className="mt-12 border-t border-line pt-5"
        title={`What moves it: ${def.label.toLowerCase()}`}
        takeaway={result ? tornadoTakeaway(defs, effects, result.paths) : 'Testing each lever on the same simulated lives as the plan.'}
        caption={
          result ? (
            <>
              Each bar runs from the plan as entered to the same plan with one setting changed, on the same {count(result.paths)} lives. Its length is {def.paired}; the whisker is the 95% interval of that paired
              difference. A hollow bar has an interval that includes zero. Levers are ordered by the size of their largest effect; the ranges are conventions, not forecasts. Click a row for the detail and to try the change.
              {progressNote(sweep.progress, sweep.busy)}
            </>
          ) : undefined
        }
        captionText={result ? `Bars: paired change against the plan on the same ${count(result.paths)} lives. Whiskers: 95% interval of the paired difference. Hollow: interval includes zero.` : undefined}
        table={result && base ? () => tornadoTable(groups, def, base.value, result.paths) : undefined}
        filename={`what-moves-${metric}`}
        explain={
          result && base
            ? {
                context: () => ({
                  measure: def.axis,
                  plan: base.value,
                  lives_per_test: result.paths,
                  seed: result.seed,
                  paired: true,
                  levers: groups.flatMap((g) => g.levers.map((l) => ({ kind: g.id, lever: l.label, settings: l.bars.map((b) => ({ setting: b.label, change: b.delta, lo: b.lo, hi: b.hi, reading: b.state })) }))),
                }),
              }
            : undefined
        }
        dimmed={dimmed}
      >
        {result && base && base.value !== null ? (
          <>
            <TornadoLegend className="mb-3" />
            <Tornado groups={groups} metric={metric} base={base.value} selected={picked} onSelect={(key) => setPicked((p) => (p === key ? null : key))} />
            {selected && <LeverDetail lever={selected.lever} side={selected.side} run={result} />}
          </>
        ) : result && base ? (
          <p className="py-6 text-sm text-ink-2">
            {metric === 'fi_age' ? 'Work becomes optional in fewer than half of the simulated lives, so there is no middle age to compare. Choose another measure.' : 'This measure does not exist for the plan as entered (no simulated life reaches retirement). Choose another measure.'}
          </p>
        ) : sweep.error ? (
          <p className="flex items-start gap-2 py-6 text-sm text-ink-2">
            <Icon name="alertTriangle" className="mt-0.5 shrink-0 text-bad" />
            {sweep.error}
          </p>
        ) : defs.length === 0 ? (
          <p className="py-6 text-sm text-ink-2">The engine offers no levers for this plan yet. Add spending, an income or savings and they will appear.</p>
        ) : (
          <Skeleton className="h-96 w-full" />
        )}
      </ChartCard>

      <MapSection profile={profile} seed={seed} pool={pool} vars={vars} metric={metric} runnable={runnable} />
      <SolverSection profile={profile} seed={seed} pool={pool} vars={vars} runnable={runnable} medianFiAge={summary.headline.median_fi_age} />

      <p className="mt-12 max-w-[46rem] border-t border-line pt-4 text-xs leading-relaxed text-pretty text-ink-3">
        How to read this page: the plan’s own figures here come from the first {count(result?.paths ?? LEVER_STAGES[LEVER_STAGES.length - 1] ?? 0)} of the lives behind the Overview, so they can differ from it by a point. The tornado
        tests {defs.reduce((n, l) => n + l.sides.length, 0)} settings at up to {count(LEVER_STAGES[LEVER_STAGES.length - 1] ?? 0)} lives each; the map tests 81 combinations at up to {count(GRID_STAGES[GRID_STAGES.length - 1]?.paths ?? 0)}. All of
        it stops and starts again when the plan changes.
      </p>
    </div>
  );
}
