// Searching for what matters: lever ranking, goal seek and small grids. All of them
// simulate many variants of the plan on the same seed and lives as one batch, at a
// reduced number of lives, and say so in their results.
import type { PatchOp, Profile } from '@/engine/types';
import { digestComparison, METRICS, METRIC_UNIT, readMetric, roundMetric, type ComparisonDigest, type Metric } from './digest';
import type { RunRecord } from './runs';
import { asOps, clampPaths, count, explainEngineError, PATCH_OP_SCHEMA, PATHS_SCHEMA, REDUCED_PATHS, requireRunnable, SWEEP_PATHS } from './shared';
import { ToolError, type ToolCallInfo, type ToolContext, type ToolSpec } from './types';
import { resolveVariable, roundValue, VARIABLE_SCHEMA_PROPERTIES, type Variable, type VariableInput } from './variables';

const METRIC_SCHEMA = {
  type: 'string',
  enum: METRICS,
  description:
    'success_rate and depletion_rate are shares of lives (0 to 1). median_fi_age and median_retirement_age are ages (undefined when fewer than half of lives get there). The net worth and tax metrics are today\'s dollars. delivered_spending_ratio is retirement spending delivered over planned.',
} as const;

async function simulate(ctx: ToolContext, call: ToolCallInfo, jobs: { label: string; profile: Profile }[], paths: number, remember: boolean): Promise<RunRecord[]> {
  try {
    return await ctx.runs.simulate(ctx.engine.pool, jobs, { paths, seed: ctx.store.run().seed, signal: call.signal, remember });
  } catch (error) {
    throw explainEngineError(error);
  }
}

/** Applies ops to a copy; null (with the reason) when the result is not a runnable plan. */
function tryVariant(ctx: ToolContext, profile: Profile, ops: PatchOp[]): { profile: Profile } | { skipped: string } {
  try {
    const r = ctx.engine.applyPatch(profile, ops);
    if (r.errors.length > 0) return { skipped: r.errors[0]?.message ?? 'not a valid plan' };
    return { profile: r.profile };
  } catch (error) {
    return { skipped: error instanceof Error ? error.message : String(error) };
  }
}

/** How sure the paired difference in `metric` is, from the comparison of two runs. */
function noiseFor(metric: Metric, c: ComparisonDigest): { std_error: number | null; within_noise: boolean } | null {
  if (metric === 'success_rate' && c.success) return { std_error: c.success.std_error, within_noise: c.success.within_noise };
  if (metric === 'depletion_rate' && c.depleted) return { std_error: c.depleted.std_error, within_noise: c.depleted.within_noise };
  const scalar = { median_fi_age: 'fi_age', median_retirement_age: 'retirement_age', median_terminal_net_worth: 'terminal_net_worth', p10_terminal_net_worth: 'terminal_net_worth', median_lifetime_tax: 'lifetime_tax', delivered_spending_ratio: 'delivered_spending_ratio' }[
    metric as string
  ];
  const s = c.scalars.find((x) => x.scalar === scalar);
  // For medians and tails this is the noise of the mean per-life change: a guide, not the metric's own interval.
  return s ? { std_error: null, within_noise: s.within_noise } : null;
}

export interface LeverRanking {
  metric: Metric;
  base: number | null;
  paths: number;
  seed: number;
  levers: {
    id: string;
    label: string;
    settings: { label: string; value: number | null; delta: number | null; std_error: number | null; within_noise: boolean | null }[];
    /** Largest absolute change across the settings. */
    swing: number | null;
  }[];
}

interface RankInput {
  metric: Metric;
  custom_levers?: { label: string; ops: PatchOp[] }[];
  paths?: number;
}

export const rankLevers: ToolSpec<RankInput> = {
  name: 'rank_levers',
  description:
    "Find what moves the result. Runs the engine's standard sensitivity levers (each a low and a high setting of one decision or assumption: spending, salaries, withdrawal rate target, stock returns, inflation, Social Security, fees) plus any levers you add as patches, all on the same simulated lives as the plan, and returns them ranked by the size of their effect on one metric. Each effect is the paired change against the plan. Add custom levers for the concrete options the user is weighing (retire two years later, a half-time job from 50, moving). Runs at a reduced number of lives, so treat small effects as indistinguishable and confirm the ones you rely on with try_scenario.",
  input_schema: {
    type: 'object',
    properties: {
      metric: METRIC_SCHEMA,
      custom_levers: {
        type: 'array',
        maxItems: 8,
        items: {
          type: 'object',
          properties: {
            label: { type: 'string', minLength: 1, maxLength: 60 },
            ops: { type: 'array', minItems: 1, maxItems: 20, items: PATCH_OP_SCHEMA },
          },
          required: ['label', 'ops'],
          additionalProperties: false,
        },
        description: 'Extra one-sided levers of your own, each a patch against the current plan.',
      },
      paths: PATHS_SCHEMA(`${count(REDUCED_PATHS)}`),
    },
    required: ['metric'],
    additionalProperties: false,
  },
  pending: () => 'Testing which levers move the result',
  async run(input, ctx, call) {
    const profile = requireRunnable(ctx);
    const paths = clampPaths(input.paths, REDUCED_PATHS);
    const seed = ctx.store.run().seed;
    const skipped: { lever: string; reason: string }[] = [];
    const plan: { id: string; label: string; setting: string; profile: Profile }[] = [];
    for (const lever of ctx.engine.levers(profile)) {
      for (const [setting, ops] of [[lever.low_label, lever.low], [lever.high_label, lever.high]] as const) {
        const v = tryVariant(ctx, profile, ops);
        if ('skipped' in v) skipped.push({ lever: `${lever.label} (${setting})`, reason: v.skipped });
        else plan.push({ id: lever.id, label: lever.label, setting, profile: v.profile });
      }
    }
    (input.custom_levers ?? []).forEach((lever, i) => {
      const v = tryVariant(ctx, profile, asOps(lever.ops));
      if ('skipped' in v) skipped.push({ lever: lever.label, reason: v.skipped });
      else plan.push({ id: `custom_${i + 1}`, label: lever.label, setting: 'as described', profile: v.profile });
    });
    if (plan.length === 0) throw new ToolError(`No lever could be applied to this plan. ${skipped.map((s) => `${s.lever}: ${s.reason}`).join(' | ')}`);

    const records = await simulate(ctx, call, [{ label: 'Current plan', profile }, ...plan.map((p) => ({ label: `${p.label}: ${p.setting}`, profile: p.profile }))], paths, false);
    const base = records[0] as RunRecord;
    const baseValue = readMetric(base.summary, input.metric);
    let comparisons: ComparisonDigest[];
    try {
      comparisons = (await Promise.all(plan.map((_, i) => ctx.engine.compare(base, records[i + 1] as RunRecord)))).map((c) => digestComparison(c, seed));
    } catch (error) {
      throw explainEngineError(error);
    }
    const byLever = new Map<string, LeverRanking['levers'][number]>();
    plan.forEach((p, i) => {
      const record = records[i + 1] as RunRecord;
      const value = readMetric(record.summary, input.metric);
      const noise = noiseFor(input.metric, comparisons[i] as ComparisonDigest);
      const entry = byLever.get(p.id) ?? { id: p.id, label: p.label, settings: [], swing: null };
      const delta = value === null || baseValue === null ? null : value - baseValue;
      entry.settings.push({ label: p.setting, value: roundMetric(input.metric, value), delta: roundMetric(input.metric, delta), std_error: noise?.std_error ?? null, within_noise: noise?.within_noise ?? null });
      if (delta !== null) entry.swing = Math.max(entry.swing ?? 0, Math.abs(delta));
      byLever.set(p.id, entry);
    });
    const levers = [...byLever.values()].sort((a, b) => (b.swing ?? -1) - (a.swing ?? -1)).map((l) => ({ ...l, swing: roundMetric(input.metric, l.swing) }));
    const ranking: LeverRanking = { metric: input.metric, base: roundMetric(input.metric, baseValue), paths, seed, levers };
    const rankingId = ctx.runs.putData('ranking', ranking);
    return {
      result: {
        ranking_id: rankingId,
        metric: input.metric,
        unit: METRIC_UNIT[input.metric],
        plan_value: ranking.base,
        paired: true,
        paths,
        seed,
        levers,
        ...(skipped.length ? { skipped } : {}),
        note: 'Each delta is the setting minus the plan on the same simulated lives. within_noise: true means the change cannot be told from zero at this number of lives.',
      },
      view: { title: `Tested ${levers.length} levers on ${count(paths)} lives each, paired against the plan`, meta: { paths, seed, paired: true, runs: records.length } },
    };
  },
};

// ---- solve --------------------------------------------------------------------------

interface SolveInput extends VariableInput {
  target_metric: Metric;
  target_value: number;
  lo?: number;
  hi?: number;
  paths?: number;
}

/** Ends plus this many halvings: the interval shrinks 128-fold. */
const MAX_EVALUATIONS = 9;

export const solve: ToolSpec<SolveInput> = {
  name: 'solve',
  description:
    'Goal seek: find the value of one quantity at which a metric reaches a target, by bisection on the same simulated lives. For "what spending gives 90% success" use variable annual_spending, target_metric success_rate, target_value 0.9. For "earliest retirement age with at most 5% depletion risk" use variable retirement_age, target_metric depletion_rate, target_value 0.05. Assumes the metric moves in one direction as the quantity grows, which holds for these questions. Returns the solution, the two evaluations that bracket it (so you can state the safe side), every evaluation made, and out_of_range with the closest end when the target is not reached anywhere between the bounds. Runs at a reduced number of lives: quote the answer with the tolerance given, and confirm with try_scenario before the user relies on it.',
  input_schema: {
    type: 'object',
    properties: {
      ...VARIABLE_SCHEMA_PROPERTIES,
      target_metric: METRIC_SCHEMA,
      target_value: { type: 'number', description: 'The value the metric should reach, in its own unit (0.9 for 90%).' },
      lo: { type: 'number', description: 'Lower end of the search. Omit for a sensible default (half of current spending, age 40 or next year, 2% rate, half salary). Required with path if the default of plus or minus 50% does not suit.' },
      hi: { type: 'number', description: 'Upper end of the search. Omit for a sensible default.' },
      paths: PATHS_SCHEMA(`${count(REDUCED_PATHS)}`),
    },
    required: ['target_metric', 'target_value'],
    additionalProperties: false,
  },
  pending: () => 'Searching for the value that meets the target',
  async run(input, ctx, call) {
    const profile = requireRunnable(ctx);
    const variable = resolveVariable(profile, input);
    let lo = input.lo ?? variable.bounds[0];
    let hi = input.hi ?? variable.bounds[1];
    if (!(lo < hi)) throw new ToolError(`lo (${lo}) must be below hi (${hi}).`);
    const paths = clampPaths(input.paths, REDUCED_PATHS);
    const seed = ctx.store.run().seed;
    const evaluations: { value: number; metric: number | null }[] = [];

    const evaluate = async (value: number): Promise<number> => {
      const v = tryVariant(ctx, profile, variable.ops(value));
      if ('skipped' in v) throw new ToolError(`${variable.label} = ${roundValue(variable, value)} does not give a valid plan: ${v.skipped}. Narrow the bounds.`);
      const [record] = await simulate(ctx, call, [{ label: `${variable.label} = ${roundValue(variable, value)}`, profile: v.profile }], paths, false);
      const metric = readMetric((record as RunRecord).summary, input.target_metric);
      evaluations.push({ value: roundValue(variable, value), metric: roundMetric(input.target_metric, metric) });
      if (metric === null) {
        throw new ToolError(
          `${input.target_metric} is undefined at ${variable.label} = ${roundValue(variable, value)} (fewer than half of the simulated lives get there), so the search cannot continue. Narrow the bounds or solve for a rate metric instead. Evaluations so far: ${JSON.stringify(evaluations)}`,
        );
      }
      return metric;
    };

    const target = input.target_value;
    let fLo = await evaluate(lo);
    let fHi = await evaluate(hi);
    const base = { variable: variable.label, unit: variable.unit, current_value: variable.current, target_metric: input.target_metric, target_value: target, paths, seed, paired: true };
    const view = (title: string) => ({ title, meta: { paths, seed, paired: true, runs: evaluations.length } });
    if ((fLo - target) * (fHi - target) > 0) {
      const closest = Math.abs(fLo - target) <= Math.abs(fHi - target) ? { value: lo, metric: fLo } : { value: hi, metric: fHi };
      return {
        result: {
          ...base,
          status: 'out_of_range',
          closest: { value: roundValue(variable, closest.value), metric: roundMetric(input.target_metric, closest.metric) },
          searched: [roundValue(variable, lo), roundValue(variable, hi)],
          evaluations,
          note: 'The metric does not cross the target anywhere between the bounds. Report the range it does cover, or search again with wider bounds if that makes sense for the user.',
        },
        view: view(`Searched ${variable.label.toLowerCase()}: the target is out of reach between the bounds`),
      };
    }
    while (hi - lo > variable.tolerance && evaluations.length < MAX_EVALUATIONS) {
      const mid = (lo + hi) / 2;
      const fMid = await evaluate(mid);
      if ((fMid - target) * (fLo - target) > 0) {
        lo = mid;
        fLo = fMid;
      } else {
        hi = mid;
        fHi = fMid;
      }
    }
    const value = roundValue(variable, (lo + hi) / 2);
    return {
      result: {
        ...base,
        status: 'converged',
        value,
        tolerance: roundValue(variable, Math.max(variable.tolerance, (hi - lo) / 2)),
        bracket: {
          below: { value: roundValue(variable, lo), metric: roundMetric(input.target_metric, fLo) },
          above: { value: roundValue(variable, hi), metric: roundMetric(input.target_metric, fHi) },
        },
        metric_direction: fHi > fLo ? 'rises as the quantity grows' : 'falls as the quantity grows',
        evaluations,
        note: 'The target is met between the two bracket values; pick the side that satisfies the user\'s condition. On a fixed set of lives the metric moves in small steps, so the answer is good to the tolerance, not beyond.',
      },
      view: view(`Searched ${variable.label.toLowerCase()} over ${evaluations.length} runs of ${count(paths)} lives`),
    };
  },
};

// ---- sweep --------------------------------------------------------------------------

interface AxisInput extends VariableInput {
  values?: number[];
  lo?: number;
  hi?: number;
  steps?: number;
}

export interface SweepGrid {
  metric: Metric;
  x: { label: string; unit: string; values: number[] };
  y: { label: string; unit: string; values: number[] } | null;
  /** `cells[yi][xi]`; one row when there is no y axis. */
  cells: (number | null)[][];
  paths: number;
  seed: number;
}

const AXIS_SCHEMA = (which: string) =>
  ({
    type: 'object',
    properties: {
      ...VARIABLE_SCHEMA_PROPERTIES,
      values: { type: 'array', minItems: 2, maxItems: 8, items: { type: 'number' }, description: 'The values to try, at most 8. Or give lo, hi and steps.' },
      lo: { type: 'number' },
      hi: { type: 'number' },
      steps: { type: 'integer', minimum: 2, maximum: 8, description: 'Evenly spaced values from lo to hi. Default 5.' },
    },
    additionalProperties: false,
    description: `The ${which} axis: what varies and over which values.`,
  }) as const;

function axisValues(variable: Variable, axis: AxisInput): number[] {
  if (axis.values) return axis.values.map((v) => roundValue(variable, v));
  const lo = axis.lo ?? variable.bounds[0];
  const hi = axis.hi ?? variable.bounds[1];
  if (!(lo < hi)) throw new ToolError(`For ${variable.label}, lo (${lo}) must be below hi (${hi}).`);
  const steps = axis.steps ?? 5;
  return Array.from({ length: steps }, (_, i) => roundValue(variable, lo + ((hi - lo) * i) / (steps - 1)));
}

export const sweep: ToolSpec<{ x: AxisInput; y?: AxisInput; metric: Metric; paths?: number }> = {
  name: 'sweep',
  description:
    'Map a trade-off: simulate a small grid of one or two quantities (at most 8 by 8) on the same simulated lives and get one metric in every cell. Use it for questions of the form "how do retirement age and spending trade off", or to show how a result responds across a range instead of at one point. Runs at a reduced number of lives per cell, so neighbouring cells that differ by a point or two of probability are not distinguishable.',
  input_schema: {
    type: 'object',
    properties: { x: AXIS_SCHEMA('horizontal'), y: AXIS_SCHEMA('vertical (optional)'), metric: METRIC_SCHEMA, paths: PATHS_SCHEMA(`${count(SWEEP_PATHS)} per cell`) },
    required: ['x', 'metric'],
    additionalProperties: false,
  },
  pending: () => 'Simulating a grid of options',
  async run(input, ctx, call) {
    const profile = requireRunnable(ctx);
    const x = resolveVariable(profile, input.x);
    const y = input.y ? resolveVariable(profile, input.y) : null;
    const xs = axisValues(x, input.x);
    const ys = y && input.y ? axisValues(y, input.y) : [null];
    const paths = clampPaths(input.paths, SWEEP_PATHS);
    const seed = ctx.store.run().seed;
    const jobs: { label: string; profile: Profile }[] = [];
    const slots: (number | null)[][] = ys.map(() => xs.map(() => null));
    const invalid: string[] = [];
    ys.forEach((yv, yi) =>
      xs.forEach((xv, xi) => {
        const ops = [...x.ops(xv), ...(y && yv !== null ? y.ops(yv) : [])];
        const v = tryVariant(ctx, profile, ops);
        if ('skipped' in v) invalid.push(`${x.label}=${xv}${yv !== null ? `, ${y?.label}=${yv}` : ''}: ${v.skipped}`);
        else {
          (slots[yi] as (number | null)[])[xi] = jobs.length;
          jobs.push({ label: `${xv}${yv !== null ? ` / ${yv}` : ''}`, profile: v.profile });
        }
      }),
    );
    if (jobs.length === 0) throw new ToolError(`No cell of the grid is a valid plan. ${invalid.slice(0, 4).join(' | ')}`);
    const records = await simulate(ctx, call, jobs, paths, false);
    const cells = slots.map((row) => row.map((slot) => (slot === null ? null : roundMetric(input.metric, readMetric((records[slot] as RunRecord).summary, input.metric)))));
    const grid: SweepGrid = {
      metric: input.metric,
      x: { label: x.label, unit: x.unit, values: xs },
      y: y ? { label: y.label, unit: y.unit, values: ys as number[] } : null,
      cells,
      paths,
      seed,
    };
    const gridId = ctx.runs.putData('grid', grid);
    return {
      result: {
        grid_id: gridId,
        metric: input.metric,
        unit: METRIC_UNIT[input.metric],
        x: { ...grid.x, current: x.current },
        ...(grid.y ? { y: { ...grid.y, current: y?.current ?? null } } : {}),
        cells,
        cells_layout: grid.y ? 'cells[y index][x index]' : 'cells[0][x index]',
        paired: true,
        paths,
        seed,
        ...(invalid.length ? { invalid_cells: invalid.slice(0, 6) } : {}),
      },
      view: { title: `Simulated a ${xs.length}${y ? ` by ${ys.length}` : '-point'} grid, ${count(paths)} lives per cell`, meta: { paths, seed, paired: true, runs: jobs.length } },
    };
  },
};
