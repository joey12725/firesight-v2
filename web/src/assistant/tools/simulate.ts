import type { PatchOp } from '@/engine/types';
import { compareRuns, digestSummary } from './digest';
import { asOps, count, explainEngineError, fullPaths, PATCH_OP_SCHEMA, PATHS_SCHEMA, requireRunnable, variant } from './shared';
import { ToolError, type ToolSpec } from './types';

export const runSimulation: ToolSpec<{ paths?: number }> = {
  name: 'run_simulation',
  description:
    "Simulate the user's current plan and get the digest: probability of success, of reaching financial independence and of depletion (each with a 95% interval), FI and retirement age quantiles, net worth and spending bands at six ages, spending delivered against plan, the leading causes of failure, and the path ids of representative lives to inspect. Uses the session's fixed seed, so results are reproducible and comparable with every other run in this conversation. Identical inputs are served from memory.",
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
        result: { ...digestSummary(run.summary, run), common_random_numbers: true },
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
    "Answer a what-if without touching the user's plan: apply edits to a copy, simulate the plan and the copy on identical futures (same seed, same lives), and get the paired comparison. The comparison reports B (the scenario) minus A (the plan): the change in success with its paired standard error, how many lives are rescued and how many harmed, the per-life change in FI age, retirement age and terminal net worth, and the headline figures side by side. within_noise: true means the difference cannot be told from zero at this number of lives. Set keep to save it as a named scenario the user can see in the Scenarios view (do this when they ask to keep or compare it, not for every probe).",
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
    let saved: { id?: string; error?: string } | null = null;
    if (input.keep) {
      const added = ctx.store.addScenario({ name: input.name, patch: ops });
      saved = added.ok ? { id: added.id } : { error: added.error };
    }
    const comparison = compareRuns(base, changed, ctx.engine.compare);
    const a = digestSummary(base.summary, base);
    const b = digestSummary(changed.summary, changed);
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
