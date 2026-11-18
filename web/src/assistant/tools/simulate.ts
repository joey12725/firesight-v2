import type { PatchOp } from '@/engine/types';
import { digestComparison, digestRun } from './digest';
import { asOps, count, explainEngineError, fullPaths, PATCH_OP_SCHEMA, PATHS_SCHEMA, requireRunnable, variant } from './shared';
import { ToolError, type ToolSpec } from './types';

export const runSimulation: ToolSpec<{ paths?: number }> = {
  name: 'run_simulation',
  description:
    "Simulate the user's current plan and get the engine's digest of the run: success, fi and depletion as {p, lo, hi} (a probability with its 95% interval); fi_age, retirement_age and terminal_net_worth as {p10, p50, p90}; fi_by_age and depletion_by_age (the cumulative share of all lives, p, by each age in ages); by_age (the share of households alive and the p10, p50 and p90 of net worth and of spending at six ages); spending_delivery (delivered_ratio: retirement spending delivered over planned; paths_below_90 and paths_below_75: the share of retired lives with at least one year below 90% or 75% of planned spending; years_below_90: the share of retirement years below 90%; worst_cut_p90: the single-year cut, as a fraction of plan, that 90% of retired lives never exceed); failure_causes (the top first causes as {cause, share}, share of the failed lives); trace_paths (path ids of representative lives for inspect_life); and notes, which states the units and definitions. Uses the session's fixed seed, so results are reproducible and comparable with every other run in this conversation. Identical inputs are served from memory.",
  input_schema: { type: 'object', properties: { paths: PATHS_SCHEMA("the plan's setting, at most 10,000") }, additionalProperties: false },
  pending: (input) => `Simulating ${input.paths ? `${count(input.paths)} lives` : 'the plan'}`,
  async run(input, ctx, call) {
    const profile = requireRunnable(ctx);
    const paths = fullPaths(ctx, input.paths);
    const { seed } = ctx.store.run();
    try {
      const [run] = await ctx.runs.simulate(ctx.engine.pool, [{ label: 'Current plan', profile }], { paths, seed, signal: call.signal });
      if (!run) throw new ToolError('The simulation returned nothing.');
      ctx.runs.latestBaseline = run.id;
      return {
        result: { ...digestRun(ctx.engine, run), common_random_numbers: true },
        view: { title: `Simulated ${count(paths)} lives of the current plan`, meta: { paths, seed, runs: 1 } },
      };
    } catch (error) {
      throw explainEngineError(error);
    }
  },
};

interface TryScenarioInput {
  name: string;
  ops: PatchOp[];
  keep?: boolean;
  paths?: number;
}

export const tryScenario: ToolSpec<TryScenarioInput> = {
  name: 'try_scenario',
  description:
    "Answer a what-if without touching the user's plan: apply edits to a copy, simulate the plan and the copy on identical futures (same seed, same lives), and get the engine's paired comparison. The comparison reports B (the scenario) minus A (the plan): for success and for depleted, the change (delta) with its paired standard error and 95% interval, and how many lives are rescued (fail or deplete under the plan only) and how many harmed; under scalars, the per-life change in FI age, retirement age, terminal net worth, lifetime tax and lifetime spending over the n lives that have a value in both runs (mean_delta with its interval, delta_p10/p50/p90, and b_higher and a_higher, the shares of those lives where the scenario or the plan is higher); and the headline figures side by side. scenario_detail and base_detail carry the same fields as a run_simulation digest. within_noise: true means the difference cannot be told from zero at this number of lives. Set keep to save it as a named scenario the user can see in the Scenarios view (do this when they ask to keep or compare it, not for every probe).",
  input_schema: {
    type: 'object',
    properties: {
      name: { type: 'string', minLength: 1, maxLength: 60, description: 'Short name for the scenario: "Retire at 55", "Move to Lisbon in 2030".' },
      ops: { type: 'array', minItems: 1, maxItems: 40, items: PATCH_OP_SCHEMA, description: 'Patch operations applied to a copy of the current plan.' },
      keep: { type: 'boolean', description: 'Save as a named scenario in the plan. Default false.' },
      paths: PATHS_SCHEMA("the plan's setting, at most 10,000"),
    },
    required: ['name', 'ops'],
    additionalProperties: false,
  },
  pending: (input) => `Simulating "${input.name}" against the plan`,
  async run(input, ctx, call) {
    const profile = requireRunnable(ctx);
    const ops = asOps(input.ops);
    const scenario = variant(ctx, profile, ops, `Scenario "${input.name}"`);
    const paths = fullPaths(ctx, input.paths);
    const { seed } = ctx.store.run();
    let base, changed;
    try {
      [base, changed] = await ctx.runs.simulate(
        ctx.engine.pool,
        [
          { label: 'Current plan', profile },
          { label: input.name, profile: scenario },
        ],
        { paths, seed, signal: call.signal },
      );
    } catch (error) {
      throw explainEngineError(error);
    }
    if (!base || !changed) throw new ToolError('The simulation returned nothing.');
    ctx.runs.latestBaseline = base.id;
    let comparison, a, b;
    try {
      comparison = digestComparison(await ctx.runs.compare(ctx.engine, base, changed), seed);
      a = ctx.engine.digest(base);
      b = ctx.engine.digest(changed);
    } catch (error) {
      throw explainEngineError(error);
    }
    // Saved only once there is a result to show for it.
    let saved: { id?: string; error?: string } | null = null;
    if (input.keep) {
      const added = ctx.store.addScenario({ name: input.name, patch: ops });
      saved = added.ok ? { id: added.id } : { error: added.error };
    }
    return {
      result: {
        scenario: input.name,
        a_is: 'the current plan',
        b_is: `the plan with the scenario's edits`,
        paths,
        base_run_id: base.id,
        scenario_run_id: changed.id,
        comparison,
        scenario_detail: { fi_age: b.fi_age, retirement_age: b.retirement_age, terminal_net_worth: b.terminal_net_worth, spending_delivery: b.spending_delivery, failure_causes: b.failure_causes, trace_paths: b.trace_paths },
        base_detail: { fi_age: a.fi_age, retirement_age: a.retirement_age, terminal_net_worth: a.terminal_net_worth, spending_delivery: a.spending_delivery },
        ...(saved ? { saved_scenario: saved } : {}),
      },
      view: {
        title: `Simulated ${count(paths)} lives of "${input.name}", paired against the plan${saved?.id ? ', and saved it' : ''}`,
        meta: { paths, seed, paired: true, runs: 2 },
      },
    };
  },
};
