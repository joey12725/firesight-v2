// Every tool executor against the real engine (the wasm module, run in this thread) and
// the real profile store. Runs use a few hundred lives where the number is not the point.
import { readFileSync } from 'node:fs';
import { beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { applyPatch, compare, digest, equityPreview, exampleProfiles, initEngineSync, planPreview, schema, validate } from '@/engine/api';
import { EngineError } from '@/engine/errors';
import type { City } from '@/editor/cities';
import { EQUITY_OUTCOMES, STATUSES, type DigestEquity, type PathTrace } from '@/engine/output-types';
import { engineInput } from '@/engine/pool';
import type { OutcomeRow, Profile } from '@/engine/types';
import { useProfileStore } from '@/state/profile-store';
import { EXAMPLE_INTAKE_PATCH, EXAMPLE_SCENARIO_PATCH } from '../prompt';
import { nodeEngine, type NodeEngine } from '../testing/node-engine';
import { executeTool, MAX_RESULT_CHARS, RunRegistry, TOOLS, TOOL_NAMES, toolDefinitions, type Decision, type PlanStore, type Proposal, type ToolContext, type ToolRun } from './index';
import { SCHEMA_TYPES } from './plan';
import { oddsOf100, pruneDefaults, typeNames, type OddsOf100, type SchemaNode } from './plan-view';
import { dollarsPerShare, resolveVariable } from './variables';

beforeAll(() => {
  initEngineSync(readFileSync(new URL('../../engine/pkg/firesight_wasm_bg.wasm', import.meta.url)));
});

const state = () => useProfileStore.getState();
const store: PlanStore = {
  profile: () => state().profile,
  report: () => state().report,
  scenarios: () => state().scenarios,
  run: () => state().run,
  preview: (ops) => state().preview(ops),
  edit: (ops, label) => state().edit(ops, { label, target: 'base' }),
  addScenario: (input) => state().addScenario(input),
};

const city = (name: string, country: string, code: string, level: number): City => ({
  city: name,
  country,
  countryCode: code,
  quality: 1,
  overallSingle: level,
  overallFamily: level,
  rent: level,
  homePrice: level,
  groceries: level,
  dining: level,
  transport: level,
  utilities: level,
  childcare: level,
  goodsServices: level,
  search: `${name}, ${country}`.toLowerCase(),
});
const CITIES = [city('Denver', 'United States', 'US', 100), city('Lisbon', 'Portugal', 'PT', 60), city('Portland', 'United States', 'US', 95), city('Portland', 'Jamaica', 'JM', 50)];

let engine: NodeEngine;
let ctx: ToolContext;
let decide: (proposal: Proposal) => Decision;
let proposals: Proposal[];

function makeContext(overrides: Partial<ToolContext> = {}): ToolContext {
  return {
    store,
    engine,
    runs: new RunRegistry(),
    requestApproval: async (_id, proposal) => {
      proposals.push(proposal);
      return decide(proposal);
    },
    autoAccept: () => false,
    cities: async () => CITIES,
    today: () => '2026-11',
    ...overrides,
  };
}

let n = 0;
async function call(name: string, input: unknown, context: ToolContext = ctx): Promise<ToolRun & { json: Record<string, unknown> }> {
  const run = await executeTool(context, { id: `call_${++n}`, name, input }, new AbortController().signal);
  let json: Record<string, unknown>;
  try {
    json = JSON.parse(run.reply.content) as Record<string, unknown>;
  } catch {
    json = { text: run.reply.content };
  }
  return { ...run, json };
}

const demo = (): Profile => exampleProfiles()[0]!.profile;
const RETIRE_AT_55 = [{ op: 'set', path: '/retirement/trigger', value: { type: 'at_age', person: 0, age: 55 } }] as const;
/** The record behind a run id in a tool result. */
const record = (id: unknown) => ctx.runs.get(id as string)!;
/** Simulating in this thread takes about half a second per thousand lives. */
const SLOW = 60_000;
/**
 * The example plan retires when funded, so spending more also means working longer and
 * success does not fall with spending. With a fixed retirement age the levers pull the
 * way intuition says.
 */
const retireAtFixedAge = () => state().edit([{ op: 'set', path: '/retirement/trigger', value: { type: 'at_age', person: 0, age: 42 } }], { label: 'Retire at 42', target: 'base' });

beforeEach(() => {
  state().load({ meta: { id: 't', createdAt: 0, origin: 'test' }, profile: demo() });
  state().setRun({ paths: 2000, seed: 7 });
  engine = nodeEngine();
  proposals = [];
  decide = () => ({ type: 'accept' });
  ctx = makeContext();
});

describe('tool definitions', () => {
  it('are name-sorted, schema-closed and described for the model', () => {
    const defs = toolDefinitions();
    expect(defs.map((d) => d.name)).toEqual([...TOOL_NAMES].sort());
    expect(TOOL_NAMES).toEqual(['explain_tax', 'get_plan', 'get_schema', 'inspect_life', 'lookup_cost_of_living', 'propose_changes', 'rank_levers', 'run_simulation', 'show_chart', 'solve', 'sweep', 'try_scenario']);
    for (const def of defs) {
      expect(def.input_schema.type).toBe('object');
      expect((def.input_schema as { additionalProperties?: boolean }).additionalProperties).toBe(false);
      expect(def.description!.length).toBeGreaterThan(120);
      expect(def.eager_input_streaming).toBe(true);
    }
    // Stable bytes: the definitions are part of the cached prompt prefix.
    expect(JSON.stringify(toolDefinitions())).toBe(JSON.stringify(defs));
    expect(JSON.stringify(defs).length).toBeLessThan(27_000);
  });

  it('keep the schema type list in step with the engine schema', () => {
    expect([...SCHEMA_TYPES].slice(1).sort()).toEqual(typeNames(schema() as SchemaNode).sort());
  });

  it('reject malformed input before running anything', async () => {
    const r = await call('try_scenario', { name: 'x' });
    expect(r.reply.isError).toBe(true);
    expect(r.json.problems).toEqual(['input.ops is required']);
    expect(engine.simulated).toBe(0);
    expect((await call('launch_rocket', {})).reply.content).toMatch(/no tool named/);
  });
});

describe('get_plan and get_schema', () => {
  it('renders the plan compactly, with ids, totals, validation and scenarios', async () => {
    state().addScenario({ name: 'Retire at 55', patch: [{ op: 'set', path: '/retirement/trigger', value: { type: 'at_age', person: 0, age: 55 } }] });
    const r = await call('get_plan', {});
    expect(r.reply.isError).toBe(false);
    const plan = r.json.plan as Profile;
    const profile = demo();
    expect(plan.incomes.map((i) => i.id)).toEqual(profile.incomes.map((i) => i.id));
    expect(plan.accounts.map((a) => a.balance)).toEqual(profile.accounts.map((a) => a.balance));
    // The plan's facts are the engine's own preview, rounded, not a recomputation.
    const core = planPreview(profile);
    expect(r.json.preview).toMatchObject({
      annual_spending: Math.round(core.annual_spending),
      annual_spending_retired: Math.round(core.annual_spending_retired),
      annual_income: Math.round(core.annual_income),
      funded_requirement: Math.round(core.funded_requirement),
      horizon_years: core.horizon_years,
    });
    expect((r.json.preview as { savings_rate: number }).savings_rate).toBeCloseTo(core.savings_rate, 3);
    expect(Object.keys(r.json.preview as object).sort()).toEqual(Object.keys(core).sort());
    expect((r.json.derived as { accounts_by_kind: Record<string, number> }).accounts_by_kind).toBeTypeOf('object');
    expect((r.json.validation as { can_run: boolean }).can_run).toBe(true);
    expect(r.json.scenarios).toEqual([expect.objectContaining({ id: 'retire_at_55', name: 'Retire at 55' })]);
    expect(r.json.run_settings).toMatchObject({ paths: 2000, seed: 7 });
    // Much shorter than the raw profile, and within the result budget.
    expect(r.reply.content.length).toBeLessThan(JSON.stringify(profile).length * 0.7);
    expect(r.reply.content.length).toBeLessThan(MAX_RESULT_CHARS);
  });

  it('shows a plan the engine cannot compile without a preview', async () => {
    state().load({ meta: { id: 'blank', createdAt: 0, origin: 'test' }, profile: applyPatch('{}', []).profile });
    const r = await call('get_plan', {});
    expect(r.reply.isError).toBe(false);
    expect((r.json.validation as { can_run: boolean }).can_run).toBe(false);
    expect(r.json.preview).toBeUndefined();
  });

  it('prunes only values that equal their schema default, so the plan can be rebuilt from it', () => {
    const root = schema() as SchemaNode;
    for (const example of exampleProfiles()) {
      const pruned = pruneDefaults(root, { properties: root.properties }, example.profile);
      // Feeding the pruned plan back through the engine restores every default.
      expect(applyPatch(JSON.stringify(pruned), []).profile).toEqual(example.profile);
    }
  });

  it('looks up a schema type with its variants, and nested types on request', async () => {
    const kind = await call('get_schema', { section: 'IncomeKind' });
    const variants = kind.json.variants as { type: string; fields: { name: string }[] }[];
    expect(variants.map((v) => v.type)).toEqual(['job', 'self_employment', 'pension', 'annuity', 'other']);
    expect(variants[0]!.fields.map((f) => f.name)).toContain('salary');

    const income = await call('get_schema', { section: 'Income', with_nested: true });
    expect((income.json.fields as { name: string }[]).map((f) => f.name)).toEqual(['id', 'name', 'owner', 'kind', 'start', 'end']);
    expect((income.json.nested as { name: string }[]).map((x) => x.name)).toContain('IncomeKind');
    expect(income.reply.content.length).toBeLessThan(MAX_RESULT_CHARS);

    const root = await call('get_schema', { section: 'Profile' });
    expect((root.json.fields as { name: string }[]).map((f) => f.name)).toContain('retirement');
    expect((await call('get_schema', { section: 'Nope' })).reply.isError).toBe(true);
  });
});

describe('propose_changes', () => {
  const salaryPath = () => `/incomes/@${demo().incomes[0]!.id}/kind/salary`;

  it('shows a diff, applies on acceptance as one undoable edit, and reports back', async () => {
    const before = (state().profile!.incomes[0]!.kind as { salary: number }).salary;
    const r = await call('propose_changes', {
      summary: 'Raise the salary',
      ops: [{ op: 'set', path: salaryPath(), value: before + 10_000 }],
      assumptions: [{ field: 'Salary', value: 'as you said', basis: 'stated', confidence: 'high' }],
    });
    expect(r.json.status).toBe('accepted');
    expect(proposals[0]!.entries[0]).toMatchObject({ op: 'set', before, after: before + 10_000 });
    expect(proposals[0]!.entries[0]!.label).toMatch(/salary$/);
    expect((state().profile!.incomes[0]!.kind as { salary: number }).salary).toBe(before + 10_000);
    expect(r.view.display).toMatchObject({ type: 'change', state: 'accepted' });
    state().undo();
    expect((state().profile!.incomes[0]!.kind as { salary: number }).salary).toBe(before);
  });

  it('returns the patch engine error verbatim and does not bother the user', async () => {
    const r = await call('propose_changes', { summary: 'Typo', ops: [{ op: 'set', path: '/incomes/@nobody/kind/salary', value: 1 }] });
    expect(r.reply.isError).toBe(true);
    expect(r.reply.content).toMatch(/patch operation 0/);
    expect(r.reply.content).toContain(demo().incomes[0]!.id);
    expect(proposals).toHaveLength(0);
  });

  it('refuses a patch that would make the plan invalid', async () => {
    const r = await call('propose_changes', { summary: 'Remove everyone', ops: [{ op: 'set', path: '/household/people', value: [] }] });
    expect(r.reply.isError).toBe(true);
    expect(r.reply.content).toMatch(/would then have errors/);
    expect(state().profile!.household.people.length).toBeGreaterThan(0);
  });

  it('leaves the plan alone when declined, and applies the user\'s edits when edited', async () => {
    const before = state().profile;
    decide = () => ({ type: 'decline' });
    const declined = await call('propose_changes', { summary: 'Raise', ops: [{ op: 'set', path: salaryPath(), value: 1 }] });
    expect(declined.json.status).toBe('declined');
    expect(state().profile).toBe(before);

    decide = (p) => ({ type: 'accept_edited', ops: [{ ...p.ops[0]!, value: 150_000 } as never] });
    const edited = await call('propose_changes', { summary: 'Raise', ops: [{ op: 'set', path: salaryPath(), value: 1 }] });
    expect(edited.json.status).toBe('accepted_with_edits');
    expect((edited.json.final_ops as { value: number }[])[0]!.value).toBe(150_000);
    expect((state().profile!.incomes[0]!.kind as { salary: number }).salary).toBe(150_000);
  });

  it('applies without asking when auto-accept is on', async () => {
    const auto = makeContext({ autoAccept: () => true });
    const r = await call('propose_changes', { summary: 'Raise', ops: [{ op: 'set', path: salaryPath(), value: 99_000 }] }, auto);
    expect(r.json).toMatchObject({ status: 'accepted', automatically: true });
    expect(proposals).toHaveLength(0);
    expect((state().profile!.incomes[0]!.kind as { salary: number }).salary).toBe(99_000);
  });

  it('builds a plan from nothing with the worked example in the prompt', async () => {
    state().load({ meta: { id: 'blank', createdAt: 0, origin: 'test' }, profile: applyPatch('{}', []).profile });
    const r = await call('propose_changes', { summary: 'Add Maya and her job', ops: EXAMPLE_INTAKE_PATCH });
    expect(r.json.status).toBe('accepted');
    expect(validate(state().profile!).errors).toEqual([]);
    // The scenario example applies on top of it.
    expect(applyPatch(state().profile!, EXAMPLE_SCENARIO_PATCH).errors).toEqual([]);
  });
});

describe('run_simulation and try_scenario', () => {
  it("returns the engine's digest of the run, on the session seed", async () => {
    const r = await call('run_simulation', {});
    expect(r.reply.isError).toBe(false);
    const run = record(r.json.run_id);
    // Core's digest of the run's accumulator, plus the id, the seed and the pairing note.
    const core = digest(run.acc, engineInput(run.profile, 7).json);
    // ...and the three headline figures the digest lacks: the home sale that success counts as holding, the capped share of plan delivered, and the starting withdrawal rate.
    const h = run.summary.headline;
    const r3 = (v: number) => Math.round(v * 1000) / 1000;
    expect(r.json).toEqual({
      run_id: run.id,
      seed: 7,
      ...core,
      home_sale: { p: r3(h.home_sale_rate.value), lo: r3(h.home_sale_rate.lo), hi: r3(h.home_sale_rate.hi) },
      plan_delivered: h.plan_delivered_share.value === null ? null : r3(h.plan_delivered_share.value),
      initial_withdrawal_rate_p50: h.initial_withdrawal_rate.p50 === null ? null : Math.round(h.initial_withdrawal_rate.p50 * 10000) / 10000,
      common_random_numbers: true,
    });
    expect((r.json.plan_delivered as number | null) ?? 0).toBeLessThanOrEqual(1);
    expect(r.json).toMatchObject({ paths: 2000, seed: 7 });
    const success = r.json.success as { p: number; lo: number; hi: number };
    expect(success.p).toBeCloseTo(run.summary.headline.success_rate.value, 3);
    expect(success.lo).toBeLessThan(success.p);
    expect(success.hi).toBeGreaterThan(success.p);
    expect((r.json.by_age as { ages: number[]; spending_p10: number[] }).ages).toHaveLength(6);
    expect((r.json.by_age as { spending_p10: number[] }).spending_p10).toHaveLength(6);
    expect(r.json.notes).toMatch(/today's dollars/);
    for (const cause of r.json.failure_causes as { cause: string; share: number }[]) expect(cause.share).toBeGreaterThan(0);
    expect((r.json.trace_paths as { median: number }).median).toBe(run.summary.picks.find((p) => p.label === 'median')!.path_index);
    expect(r.reply.content.length).toBeLessThan(2_700);
    expect(r.view).toMatchObject({ title: 'Simulated 2,000 lives of the current plan', meta: { paths: 2000, seed: 7 } });
    // The same inputs are not simulated twice.
    await call('run_simulation', {});
    expect(engine.simulated).toBe(1);
  }, SLOW);

  it("compares a scenario with the plan on the same lives from the engine's paired comparison, without touching the plan", async () => {
    const before = state().profile;
    const r = await call('try_scenario', { name: 'Retire at 55', ops: RETIRE_AT_55 });
    expect(r.reply.isError).toBe(false);
    expect(state().profile).toBe(before);
    expect(state().scenarios).toHaveLength(0);
    type Outcome = { a: number; b: number; delta: number; std_error: number; lo: number; hi: number; rescued: number; harmed: number; unchanged: number; within_noise: boolean; mcnemar_z: number };
    const c = r.json.comparison as { paired: boolean; paired_paths: number; seed: number; success: Outcome; depleted: Outcome; scalars: { scalar: string; n: number; mean_delta: number; b_higher: number }[]; headline: { metric: string; a: number; b: number; delta: number }[] };
    expect(c).toMatchObject({ paired: true, paired_paths: 2000, seed: 7 });

    // Every figure is core's, from the two accumulators.
    const base = record(r.json.base_run_id);
    const scenario = record(r.json.scenario_run_id);
    const core = compare(base.acc, scenario.acc, engineInput(base.profile, 7).json);
    const success = core.outcomes.find((o) => o.outcome === 'success')!;
    expect(c.success.delta).toBeCloseTo(success.difference.value, 4);
    expect(c.success.std_error).toBeCloseTo(success.difference.std_error, 4);
    expect(c.success).toMatchObject({ rescued: success.b_only, harmed: success.a_only, unchanged: success.both + success.neither, within_noise: success.within_noise });
    // The flips account for the whole difference: this is a paired experiment.
    expect(c.success.rescued + c.success.harmed + c.success.unchanged).toBe(2000);
    expect((c.success.rescued - c.success.harmed) / 2000).toBeCloseTo(c.success.delta, 4);
    expect(c.success.b - c.success.a).toBeCloseTo(c.success.delta, 2);
    expect(c.success.lo).toBeLessThan(c.success.delta);
    expect(c.success.hi).toBeGreaterThan(c.success.delta);
    // Working to 55 instead of stopping when funded rescues far more lives than it harms.
    expect(c.success.rescued).toBeGreaterThan(c.success.harmed);
    expect(c.success.within_noise).toBe(false);
    // A life rescued from depletion is one that depleted under the plan only.
    const depleted = core.outcomes.find((o) => o.outcome === 'depleted')!;
    expect(c.depleted).toMatchObject({ rescued: depleted.a_only, harmed: depleted.b_only });
    expect(c.depleted.delta).toBeLessThan(0);

    const worth = core.scalars.find((x) => x.scalar === 'terminal_net_worth')!;
    const shown = c.scalars.find((x) => x.scalar === 'terminal_net_worth')!;
    expect(shown.n).toBe(worth.both);
    expect(shown.mean_delta).toBe(Math.round(worth.mean_delta.value! / 1000) * 1000);
    expect(shown.b_higher).toBeCloseTo(worth.b_higher!, 3);
    expect(c.scalars.map((x) => x.scalar)).toEqual(expect.arrayContaining(['fi_age', 'retirement_age', 'terminal_net_worth']));
    expect(c.headline.map((h) => h.metric)).toEqual(core.headline.map((h) => h.metric));
    expect(c.headline.find((h) => h.metric === 'success_rate')).toMatchObject({ a: c.success.a, b: c.success.b });

    // The scenario's own digest is core's too.
    expect((r.json.scenario_detail as { fi_age: unknown }).fi_age).toEqual(digest(scenario.acc, engineInput(scenario.profile, 7).json).fi_age);
    expect(r.view.meta).toMatchObject({ paired: true, paths: 2000, runs: 2 });
    expect(engine.batches).toEqual([2]);
    expect(engine.compared).toBe(1);
    expect(r.reply.content.length).toBeLessThan(MAX_RESULT_CHARS);
  }, SLOW);

  it('saves the scenario when asked to keep it, and reuses the baseline run', async () => {
    await call('run_simulation', { paths: 400 });
    const r = await call('try_scenario', { name: 'Work to 55', ops: RETIRE_AT_55, keep: true, paths: 400 });
    expect(r.json.saved_scenario).toEqual({ id: 'work_to_55' });
    expect(state().scenarios.map((s) => s.name)).toEqual(['Work to 55']);
    expect(engine.simulated).toBe(2);
  }, SLOW);

  it('flags a difference that cannot be told from zero', async () => {
    const r = await call('try_scenario', { name: 'Rename', ops: [{ op: 'set', path: '/name', value: 'Renamed' }], paths: 400 });
    const c = r.json.comparison as { success: { delta: number; rescued: number; harmed: number; within_noise: boolean } };
    expect(c.success).toMatchObject({ delta: 0, rescued: 0, harmed: 0, within_noise: true });
  }, SLOW);

  it('reports a failed comparison instead of inventing one', async () => {
    engine.compare = async () => {
      throw new EngineError('worker', 'The two runs do not cover the same paths.');
    };
    const r = await call('try_scenario', { name: 'Retire at 55', ops: RETIRE_AT_55, paths: 400 });
    expect(r.reply.isError).toBe(true);
    expect(r.reply.content).toMatch(/do not cover the same paths/);
  }, SLOW);

  it('passes patch errors back verbatim and blocks an invalid plan', async () => {
    const bad = await call('try_scenario', { name: 'Bad', ops: [{ op: 'set', path: '/retirement/triger', value: 1 }] });
    expect(bad.reply.isError).toBe(true);
    expect(bad.reply.content).toMatch(/patch/);
    expect(engine.simulated).toBe(0);

    state().load({ meta: { id: 'blank', createdAt: 0, origin: 'test' }, profile: applyPatch('{}', []).profile });
    const blocked = await call('run_simulation', {});
    expect(blocked.reply.isError).toBe(true);
    expect(blocked.reply.content).toMatch(/cannot be simulated until/);
  });

  it('says plainly when the engine cannot simulate yet', async () => {
    engine = nodeEngine({ fail: () => new EngineError('not_implemented', 'not implemented: Engine::run_chunk') });
    const r = await call('run_simulation', {}, makeContext());
    expect(r.reply.isError).toBe(true);
    expect(r.reply.content).toMatch(/no results/);
    expect(r.reply.content).toMatch(/do not estimate/);
  });
});

describe('rank_levers, solve and sweep', () => {
  it('ranks the engine levers and custom levers as one paired batch', async () => {
    retireAtFixedAge();
    const r = await call('rank_levers', {
      metric: 'success_rate',
      custom_levers: [
        { label: 'Work to 55', ops: RETIRE_AT_55 },
        { label: 'Rename the plan', ops: [{ op: 'set', path: '/name', value: 'Renamed' }] },
      ],
      paths: 300,
    });
    expect(r.reply.isError).toBe(false);
    const ranked = r.json.levers as { id: string; label: string; swing: number; settings: { label: string; value: number; delta: number; std_error: number | null; within_noise: boolean | null }[] }[];
    expect(engine.batches).toHaveLength(1);
    // One batch for the plan and every setting; a setting identical to the plan (Social Security paid in full) is not simulated twice.
    const settings = ranked.reduce((sum, l) => sum + l.settings.length, 0);
    expect(engine.batches[0]).toBeGreaterThanOrEqual(settings);
    expect(engine.batches[0]).toBeLessThanOrEqual(settings + 1);
    // Every setting is compared with the plan by the engine, life by life.
    expect(engine.compared).toBe(settings);
    expect(ranked.map((l) => l.id)).toEqual(expect.arrayContaining(['spending', 'salary', 'stock_returns', 'custom_1', 'custom_2']));
    const swings = ranked.map((l) => l.swing);
    expect(swings).toEqual([...swings].sort((a, b) => b - a));
    // Spending less helps and spending more hurts.
    const spending = ranked.find((l) => l.id === 'spending')!;
    expect(spending.settings.map((s) => Math.sign(s.delta))).toEqual([1, -1]);
    for (const s of ranked.flatMap((l) => l.settings)) {
      expect(s.value - (r.json.plan_value as number)).toBeCloseTo(s.delta, 2);
      expect(typeof s.within_noise).toBe('boolean');
      expect(s.std_error).toBeGreaterThanOrEqual(0);
    }
    // Working to 55 is a real effect; a rename changes no life, and is reported as noise.
    expect(ranked.find((l) => l.id === 'custom_1')!.settings[0]).toMatchObject({ within_noise: false });
    expect(ranked.find((l) => l.id === 'custom_2')!.settings[0]).toMatchObject({ delta: 0, std_error: 0, within_noise: true });
    expect(r.json).toMatchObject({ paired: true, paths: 300, seed: 7 });
    expect(typeof r.json.ranking_id).toBe('string');
    expect(r.reply.content.length).toBeLessThan(MAX_RESULT_CHARS);
  }, SLOW);

  it('solves for the spending that meets a success target by bisection', async () => {
    retireAtFixedAge();
    const r = await call('solve', { variable: 'annual_spending', target_metric: 'success_rate', target_value: 0.9, paths: 200 });
    expect(r.json.status).toBe('converged');
    const bracket = r.json.bracket as { below: { value: number; metric: number }; above: { value: number; metric: number } };
    expect(bracket.below.metric).toBeGreaterThanOrEqual(0.9);
    expect(bracket.above.metric).toBeLessThanOrEqual(0.9);
    expect(bracket.below.value).toBeLessThan(bracket.above.value);
    expect(r.json.value as number).toBeGreaterThanOrEqual(bracket.below.value);
    expect(r.json.value as number).toBeLessThanOrEqual(bracket.above.value);
    expect(r.json.metric_direction).toBe('falls as the quantity grows');
    const evaluations = r.json.evaluations as { value: number; metric: number }[];
    expect(evaluations.length).toBeLessThanOrEqual(9);
    // The bracket is two of the engine runs made, and the answer agrees with an independent run of the plan.
    expect(evaluations).toEqual(expect.arrayContaining([bracket.below, bracket.above]));
    const plan = await call('run_simulation', { paths: 200 });
    const now = (plan.json.success as { p: number }).p;
    expect(Math.sign((r.json.value as number) - (r.json.current_value as number))).toBe(now < bracket.below.metric ? -1 : 1);
    expect(r.view.meta).toMatchObject({ paths: 200, seed: 7, paired: true });
  }, SLOW);

  it('means the same all-in spending as the plan view', async () => {
    retireAtFixedAge();
    const plan = await call('get_plan', {});
    const allIn = (plan.json.preview as { annual_spending: number }).annual_spending;
    const r = await call('solve', { variable: 'annual_spending', target_metric: 'success_rate', target_value: 0.9, paths: 200 });
    expect(r.json.current_value).toBe(allIn);
    expect(r.json.variable).toMatch(/all-in/);
    expect(String(r.json.variable_note)).toMatch(/same figure as preview\.annual_spending/);
    // A value below the costs the variable cannot change is refused with the reason, not simulated.
    const low = await call('sweep', { x: { variable: 'annual_spending', values: [1] }, metric: 'success_rate', paths: 200 });
    const budget = state().profile!.spending.items.length;
    expect(budget).toBeGreaterThan(0);
    expect(low.reply.isError).toBe(true);
  }, SLOW);

  it('reports the direction the metric really moves, and brackets the target either way', async () => {
    // Retiring when funded: a bigger budget raises the funded number, so the household works longer.
    // (With the audited engine the rate rises from about 0.79 to 0.86 over the search range, so the target sits inside it.)
    const r = await call('solve', { variable: 'annual_spending', target_metric: 'success_rate', target_value: 0.82, paths: 200 });
    expect(r.json.status).toBe('converged');
    expect(r.json.metric_direction).toBe('rises as the quantity grows');
    const bracket = r.json.bracket as { below: { value: number; metric: number }; above: { value: number; metric: number } };
    expect(bracket.below.metric).toBeLessThanOrEqual(0.82);
    expect(bracket.above.metric).toBeGreaterThanOrEqual(0.82);
  }, SLOW);

  it('reports out of range with the closest end instead of inventing an answer', async () => {
    const r = await call('solve', { variable: 'retirement_age', target_metric: 'success_rate', target_value: 0.2, lo: 50, hi: 55, paths: 200 });
    expect(r.json.status).toBe('out_of_range');
    const evaluations = r.json.evaluations as { value: number; metric: number }[];
    expect(evaluations.map((e) => e.value)).toEqual([50, 55]);
    const closest = [...evaluations].sort((a, b) => Math.abs(a.metric - 0.2) - Math.abs(b.metric - 0.2))[0]!;
    expect((r.json.closest as { metric: number }).metric).toBe(closest.metric);
    expect([50, 55]).toContain((r.json.closest as { value: number }).value);
  }, SLOW);

  it('solves along a custom numeric path and rejects a non-numeric one', async () => {
    const ok = await call('solve', { path: '/assumptions/market/equity_premium_adjustment', target_metric: 'median_terminal_net_worth', target_value: 3_000_000, lo: -0.05, hi: 0.02, paths: 200 });
    expect(ok.json.status).toBe('converged');
    expect(ok.json.metric_direction).toBe('rises as the quantity grows');
    const bad = await call('solve', { path: '/retirement/trigger', target_metric: 'success_rate', target_value: 0.9 });
    expect(bad.reply.isError).toBe(true);
    expect(bad.reply.content).toMatch(/not a number/);
  }, SLOW);

  it('sweeps a grid of two quantities at reduced paths', async () => {
    const r = await call('sweep', { x: { variable: 'retirement_age', values: [55, 60, 65] }, y: { variable: 'annual_spending', lo: 40_000, hi: 70_000, steps: 4 }, metric: 'median_terminal_net_worth', paths: 200 });
    expect(r.reply.isError).toBe(false);
    const cells = r.json.cells as number[][];
    expect(cells).toHaveLength(4);
    expect(cells[0]).toHaveLength(3);
    // Working longer leaves more behind; spending more leaves less.
    expect(cells[0]![2]).toBeGreaterThan(cells[0]![0]!);
    expect(cells[3]![0]).toBeLessThan(cells[0]![0]!);
    expect(engine.batches).toEqual([12]);
    expect(r.json).toMatchObject({ paths: 200, paired: true, cells_layout: 'cells[y index][x index]' });
    // Grid cells are not kept as runs.
    expect(ctx.runs.ids()).toEqual([]);
    const tooBig = await call('sweep', { x: { variable: 'retirement_age', values: [1, 2, 3, 4, 5, 6, 7, 8, 9] }, metric: 'success_rate' });
    expect(tooBig.reply.isError).toBe(true);
  }, SLOW);

  it('runs at the reduced number of lives unless told otherwise', () => {
    const paths = (name: string) => (toolDefinitions().find((d) => d.name === name)!.input_schema as { properties: { paths: { description: string } } }).properties.paths.description;
    expect(paths('rank_levers')).toMatch(/default \(2,000\)/);
    expect(paths('solve')).toMatch(/default \(2,000\)/);
    expect(paths('sweep')).toMatch(/default \(1,000 per cell\)/);
  });
});

describe('inspect_life and explain_tax', () => {
  const column = (trace: PathTrace, key: string) => trace.annual.columns.find((c) => c.key === key)!.values.map((v) => Math.round((v ?? 0) / 1000));

  it('traces a picked life as a year table with its events', async () => {
    const none = await call('inspect_life', {});
    expect(none.reply.content).toMatch(/run_simulation first/);
    const run = await call('run_simulation', {});
    const r = await call('inspect_life', { pick: 'failing' });
    expect(r.reply.isError).toBe(false);
    const failing = record(run.json.run_id).summary.picks.find((p) => p.label === 'failing')!.path_index;
    expect(r.json).toMatchObject({ path: failing, run_id: run.json.run_id, one_life_of: 2000, seed: 7 });
    expect(failing).toBe((run.json.trace_paths as { failing: number }).failing);
    // The table is the engine's ledger of that life, in thousands.
    const trace = await engine.pool.trace(demo(), failing, 7);
    const table = r.json.table as Record<string, unknown[]>;
    expect(table.year).toEqual(trace.annual.years);
    expect(table.income).toEqual(column(trace, 'gross_income'));
    expect(table.spending).toEqual(column(trace, 'spending'));
    expect(table.net_worth).toEqual(column(trace, 'net_worth'));
    expect(table.status).toHaveLength(trace.annual.years.length);
    for (const status of table.status!) expect(STATUSES).toContain(status);
    // A failing life runs out: the ledger and the event log both say so.
    expect(table.status).toContain('depleted');
    const events = r.json.events as { month: string; kind: string; label: string }[];
    expect(events.length).toBeGreaterThan(0);
    expect(events.map((e) => e.kind)).toEqual(trace.events.slice(0, 60).map((e) => e.kind));
    expect(r.json.scalars).toMatchObject({ success: 0 });
    expect((r.json.scalars as { depletion_age: number }).depletion_age).toBeGreaterThan(0);
    expect(r.reply.content.length).toBeLessThan(MAX_RESULT_CHARS);
  }, SLOW);

  it('picks a life by quantile of a per-life quantity, or by path id', async () => {
    await call('run_simulation', {});
    const low = await call('inspect_life', { quantile: 0.05, by: 'terminal_net_worth' });
    const high = await call('inspect_life', { quantile: 0.95, by: 'terminal_net_worth' });
    expect(low.json.path).not.toBe(high.json.path);
    expect((low.json.scalars as { terminal_net_worth: number }).terminal_net_worth).toBeLessThan((high.json.scalars as { terminal_net_worth: number }).terminal_net_worth);
    expect((await call('inspect_life', { path: 12 })).json.path).toBe(12);
    expect((await call('inspect_life', { path: 99_999 })).reply.content).toMatch(/does not exist/);
  }, SLOW);

  it('returns the 1040-style lines of one simulated year', async () => {
    await call('run_simulation', {});
    const r = await call('explain_tax', { path: 7, year: 2027 });
    const trace = await engine.pool.trace(demo(), 7, 7);
    const core = trace.tax_returns.find((t) => t.year === 2027)!;
    expect(r.json).toMatchObject({ year: 2027, path: 7, price_level: Number(core.price_level.toPrecision(5)), dollars: 'nominal dollars of the tax year' });
    const lines = r.json.lines as { key: string; amount: number }[];
    expect(lines.map((l) => l.key)).toEqual(core.lines.map((l) => l.key));
    expect(lines.find((l) => l.key === 'wages')!.amount).toBe(Math.round(core.lines.find((l) => l.key === 'wages')!.amount));
    expect((await call('explain_tax', { path: 7, year: 1999 })).reply.content).toMatch(new RegExp(`${trace.tax_returns[0]!.year} to ${trace.tax_returns.at(-1)!.year}`));
  }, SLOW);
});

describe('lookup_cost_of_living and show_chart', () => {
  it('returns category factors ready for a relocate event', async () => {
    const r = await call('lookup_cost_of_living', { from: 'Denver', to: 'Lisbon' });
    expect(r.json).toMatchObject({ found: true, from: 'Denver, United States', to: 'Lisbon, Portugal', country_code: 'PT' });
    expect((r.json.factors as { rent: number }).rent).toBe(0.6);
    const value = r.json.relocate_cost_of_living;
    // The value drops straight into a relocate event the engine accepts.
    const patched = applyPatch(state().profile!, [
      { op: 'append', path: '/events', value: { id: 'move', name: 'Move', when: { type: 'date', at: '2030-06' }, kind: { type: 'relocate', country: 'PT', city: 'Lisbon', cost_of_living: value } } },
    ]);
    expect(patched.errors).toEqual([]);
  });

  it('asks which city when the name is ambiguous or unknown', async () => {
    const ambiguous = await call('lookup_cost_of_living', { from: 'Denver', to: 'Portland' });
    expect(ambiguous.reply.isError).toBe(true);
    expect(ambiguous.json.to_candidates).toEqual(['Portland, United States', 'Portland, Jamaica']);
    expect((await call('lookup_cost_of_living', { from: 'Denver', to: 'Portland, Jamaica' })).json.found).toBe(true);
    expect((await call('lookup_cost_of_living', { from: 'Denver', to: 'Atlantis' })).json.found).toBe(false);
  });

  it('shows a chart only for results that exist', async () => {
    expect((await call('show_chart', { kind: 'fan', caption: 'Net worth' })).reply.content).toMatch(/run_simulation first/);
    const run = await call('run_simulation', {});
    const fan = await call('show_chart', { kind: 'fan', caption: 'Net worth by age' });
    expect(fan.json).toEqual({ shown: true, kind: 'fan' });
    expect(fan.view.display).toEqual({ type: 'chart', kind: 'fan', caption: 'Net worth by age', runId: run.json.run_id, compareRunId: null, dataId: null });
    expect((await call('show_chart', { kind: 'tornado', caption: 'Levers' })).reply.content).toMatch(/ranking_id/);
    const ranking = await call('rank_levers', { metric: 'success_rate', paths: 200 });
    const tornado = await call('show_chart', { kind: 'tornado', caption: 'Levers', data_id: ranking.json.ranking_id });
    expect(tornado.view.display).toMatchObject({ type: 'chart', kind: 'tornado', dataId: ranking.json.ranking_id });
    expect((await call('show_chart', { kind: 'heatmap', caption: 'x', data_id: ranking.json.ranking_id })).reply.isError).toBe(true);
    expect((await call('show_chart', { kind: 'compare_delta', caption: 'x' })).reply.content).toMatch(/try_scenario/);
  }, SLOW);

  it('computes the comparison behind a chart once per pair of runs', async () => {
    const r = await call('try_scenario', { name: 'Retire at 55', ops: RETIRE_AT_55, paths: 400 });
    const base = record(r.json.base_run_id);
    const scenario = record(r.json.scenario_run_id);
    const again = await ctx.runs.compare(engine, base, scenario);
    expect(engine.compared).toBe(1);
    expect(again).toEqual(compare(base.acc, scenario.acc, engineInput(base.profile, 7).json));
    // The other direction is a different comparison.
    expect((await ctx.runs.compare(engine, scenario, base)).outcomes[0]!.difference.value).toBeCloseTo(-again.outcomes[0]!.difference.value, 6);
    expect(engine.compared).toBe(2);
  }, SLOW);
});

describe('private-company equity', () => {
  const startup = (): Profile => exampleProfiles().find((e) => e.slug === 'startup-employee')!.profile;
  const loadStartup = () => state().load({ meta: { id: 's', createdAt: 0, origin: 'test' }, profile: startup() });
  const ZERO = [{ op: 'set', path: '/private_equity/@northwind/included', value: false }] as const;
  /** Median final net worth is about $4.2 million when the company lists at its last price and $11 million at five times it (200 lives). */
  const SOLVE_TARGET = 8_000_000;
  type CompanyView = {
    id: string;
    included: boolean;
    paper_value_vested: number;
    paper_value_total: number;
    exercise_cost_vested: number;
    share_of_net_worth: number;
    odds_in_force: OddsOf100;
    typical_odds: OddsOf100;
    typical_rows: OutcomeRow[];
    implied_multiple: number;
    typical_implied_multiple: number;
    nothing_share: number;
    fmv_409a: number;
    fmv_fraction: number;
    fmv_409a_basis: string;
    tender_annual_probability: number;
  };
  const r3 = (v: number) => Math.round(v * 1000) / 1000;
  const total = (odds: OddsOf100) => odds.fails + odds.acquired + odds.ipo + odds.still_private;

  it('shows each company with its paper value and the odds in force, from the engine preview', async () => {
    loadStartup();
    const r = await call('get_plan', {});
    expect(r.reply.isError).toBe(false);
    const derived = r.json.derived as { private_equity: CompanyView[]; private_equity_note: string };
    expect(derived.private_equity).toHaveLength(1);
    const view = derived.private_equity[0]!;
    const company = startup().private_equity[0]!;
    const core = equityPreview(company);
    expect(view).toMatchObject({ id: 'northwind', included: true, paper_value_vested: core.paper_value_vested, paper_value_total: core.paper_value_total, exercise_cost_vested: core.exercise_cost_vested });
    // Every figure the prompt asks the assistant to state about odds is here, and is the engine's.
    expect(total(view.odds_in_force)).toBeCloseTo(100, 9);
    expect(total(view.typical_odds)).toBeCloseTo(100, 9);
    expect(view.odds_in_force).toEqual({ fails: 30, acquired: 27, ipo: 12, still_private: 31 });
    expect(view.typical_odds).toEqual(view.odds_in_force);
    // The stage's rows with their timing and prices: a custom table inherits nothing, so this is what it is built from.
    expect(view.typical_rows.map((r) => r.kind)).toEqual(['fails', 'acquired', 'ipo', 'still_private']);
    expect(view.typical_rows.map((r) => r.probability)).toEqual([0.3, 0.27, 0.12, 0.31]);
    for (const row of view.typical_rows.filter((r) => r.kind === 'acquired' || r.kind === 'ipo')) {
      expect(row.years_to).toBeGreaterThan(row.years_from);
      expect(row.multiple_high).toBeGreaterThan(row.multiple_low);
    }
    // Copied into the plan as it stands, it is the typical table: valid, and the same picture as leaving outcomes empty.
    const copied = applyPatch(startup(), [{ op: 'set', path: '/private_equity/@northwind/outcomes', value: view.typical_rows }]);
    expect(copied.errors).toEqual([]);
    const fromRows = equityPreview(copied.profile.private_equity[0]!);
    expect(Math.abs(fromRows.implied_multiple / core.implied_multiple - 1)).toBeLessThan(0.03);
    expect(Math.abs(fromRows.nothing_share - core.nothing_share)).toBeLessThan(0.03);
    expect(fromRows.acquisition_zero_share).toBeGreaterThan(0.1);
    // A table with the same odds and nothing else is what the prompt warns against: every sale and listing at exactly the last round's price.
    const bare = applyPatch(startup(), [{ op: 'set', path: '/private_equity/@northwind/outcomes', value: view.typical_rows.map((r) => ({ id: r.id, kind: r.kind, probability: r.probability })) }]);
    expect(bare.errors).toEqual([]);
    const flat = equityPreview(bare.profile.private_equity[0]!);
    expect(flat.acquisition_zero_share).toBe(0);
    expect(flat.quantiles[2]).toBeGreaterThan(fromRows.quantiles[2] + 100_000);
    expect(derived.private_equity_note).toMatch(/typical_rows is the stage's typical table with each row's timing/);
    expect(view.implied_multiple).toBe(r3(core.implied_multiple));
    expect(view.typical_implied_multiple).toBe(r3(core.typical_implied_multiple));
    expect(view.nothing_share).toBe(r3(core.nothing_share));
    expect(view.tender_annual_probability).toBe(r3(core.tender_annual_probability));
    expect(view).toMatchObject({ fmv_409a: 5, fmv_fraction: r3(5 / 14), fmv_409a_basis: 'stated in the plan' });
    // The position against everything else the household owns, paper value included in the total.
    const p = startup();
    const other = p.accounts.reduce((n, a) => n + a.balance, 0) + p.properties.reduce((n, x) => n + x.value - (x.mortgage?.balance ?? 0), 0) - p.debts.reduce((n, d) => n + d.balance, 0);
    expect(view.share_of_net_worth).toBe(r3(core.paper_value_total / (other + core.paper_value_total)));
    expect(view.share_of_net_worth).toBeGreaterThan(0.5);
    expect(derived.private_equity_note).toMatch(/in no balance/);
    expect(r.reply.content.length).toBeLessThan(MAX_RESULT_CHARS);

    // A custom table shows beside the typical one, and a stage's own 409A fraction when none is stated.
    state().edit(
      [
        { op: 'set', path: '/private_equity/@northwind/outlook', value: 'pessimistic' },
        { op: 'set', path: '/private_equity/@northwind/fmv_409a', value: null },
      ],
      { label: 'Lean the odds', target: 'base' },
    );
    const leaned = ((await call('get_plan', {})).json.derived as { private_equity: CompanyView[] }).private_equity[0]!;
    expect(leaned.odds_in_force.fails).toBe(40);
    expect(leaned.typical_odds.fails).toBe(30);
    expect(total(leaned.odds_in_force)).toBeCloseTo(100, 9);
    expect(leaned.implied_multiple).toBeLessThan(leaned.typical_implied_multiple);
    expect(leaned).toMatchObject({ fmv_fraction: 0.35, fmv_409a_basis: 'typical for the stage' });

    // A company with no price yet has no entry, and a plan without companies no key.
    state().edit([{ op: 'set', path: '/private_equity/@northwind/holdings', value: [] }, { op: 'set', path: '/private_equity/@northwind/preferred_price', value: 0 }], { label: 'No price', target: 'base' });
    expect((await call('get_plan', {})).json.derived).not.toHaveProperty('private_equity');
    state().load({ meta: { id: 't', createdAt: 0, origin: 'test' }, profile: demo() });
    expect((await call('get_plan', {})).json.derived).not.toHaveProperty('private_equity');
  });

  it('states odds in futures of 100 that add up, whatever the rounding', () => {
    const row = (kind: string, probability: number) => ({ id: kind, kind, probability, years_from: 1, years_to: 5, multiple_low: 1, multiple_high: 1 }) as never;
    expect(oddsOf100(equityPreview({ stage: 'seed', preferred_price: 1 }).typical)).toEqual({ fails: 60, acquired: 22, ipo: 1.5, still_private: 16.5 });
    const thirds = oddsOf100([row('fails', 1 / 3), row('acquired', 1 / 3), row('ipo', 1 / 3)]);
    expect(thirds).toEqual({ fails: 33.4, acquired: 33.3, ipo: 33.3, still_private: 0 });
    // Several rows of one kind are summed, and a table that is slightly off 1 is normalised as the engine does.
    const split = oddsOf100([row('fails', 0.2), row('ipo', 0.05), row('ipo', 0.151), row('still_private', 0.6)]);
    expect(split.ipo).toBeCloseTo(20.1, 1);
    expect(split.fails + split.acquired + split.ipo + split.still_private).toBeCloseTo(100, 9);
    expect(oddsOf100([])).toEqual({ fails: 0, acquired: 0, ipo: 0, still_private: 0 });
  });

  it('looks up the company schema, leaving what does not fit to be asked for separately', async () => {
    const r = await call('get_schema', { section: 'PrivateCompany', with_nested: true });
    expect(r.reply.isError).toBe(false);
    expect((r.json.fields as { name: string }[]).map((f) => f.name)).toEqual(expect.arrayContaining(['stage', 'preferred_price', 'fmv_409a', 'outcomes', 'holdings', 'policy', 'included']));
    const nested = ((r.json.nested ?? []) as { name: string }[]).map((x) => x.name);
    const omitted = (r.json.not_included_ask_separately ?? []) as string[];
    expect([...nested, ...omitted].sort()).toEqual(['CompanyStage', 'EquityHolding', 'EquityPolicy', 'OutcomeRow', 'Outlook', 'TenderOffers']);
    expect(r.reply.content.length).toBeLessThan(8_000);
    for (const section of omitted) expect((await call('get_schema', { section })).reply.isError, section).toBe(false);
    const kind = await call('get_schema', { section: 'HoldingKind' });
    expect((kind.json.variants as { type: string }[]).map((v) => v.type)).toEqual(['iso', 'nso', 'rsu', 'shares']);
  });

  it("carries the engine's equity block in the digest of a run", async () => {
    loadStartup();
    const r = await call('run_simulation', { paths: 1000 });
    expect(r.reply.isError).toBe(false);
    const run = record(r.json.run_id);
    const core = digest(run.acc, engineInput(run.profile, 7).json);
    const equity = r.json.equity as DigestEquity;
    expect(equity).toEqual(core.equity);
    expect(equity.company).toBe('Northwind Robotics');
    expect(equity.failed + equity.still_private + equity.acquired + equity.ipo).toBeCloseTo(1, 2);
    expect(equity.nothing.p).toBeGreaterThan(0.3);
    expect(equity.nothing.lo).toBeLessThan(equity.nothing.p);
    // The mean is carried by the best lives: far above the median, and uncertain.
    expect(equity.net_proceeds_mean!).toBeGreaterThan(equity.net_proceeds.p50!);
    expect(equity.net_proceeds_mean_se!).toBeGreaterThan(0);
    expect(equity.success_by_outcome).toHaveLength(EQUITY_OUTCOMES.length);
    expect(r.reply.content.length).toBeLessThan(3_400);
    // A plan without private equity has no such block.
    state().load({ meta: { id: 't', createdAt: 0, origin: 'test' }, profile: demo() });
    expect((await call('run_simulation', { paths: 400 })).json).not.toHaveProperty('equity');
  }, SLOW);

  it('reports the plan both ways: as modelled and with the equity counted at zero, split by outcome', async () => {
    loadStartup();
    const r = await call('try_scenario', { name: 'Equity pays nothing', ops: ZERO, paths: 1000 });
    expect(r.reply.isError).toBe(false);
    type Class = { outcome: string; paths: number; success_a: number; success_b: number; delta: number; lo: number; hi: number; within_noise: boolean; net_worth_median_change: number };
    const c = r.json.comparison as { paired_paths: number; success: { a: number; b: number; delta: number }; by_equity_outcome: Class[] };
    expect(c.by_equity_outcome.map((k) => k.outcome)).toEqual([...EQUITY_OUTCOMES]);
    expect(c.by_equity_outcome.reduce((n, k) => n + k.paths, 0)).toBe(c.paired_paths);
    // Every figure is core's.
    const base = record(r.json.base_run_id);
    const scenario = record(r.json.scenario_run_id);
    const core = compare(base.acc, scenario.acc, engineInput(base.profile, 7).json);
    c.by_equity_outcome.forEach((k, i) => {
      const from = core.by_equity_outcome![i]!;
      expect(k).toMatchObject({ outcome: from.outcome, paths: from.paths, within_noise: from.success.within_noise });
      expect(k.success_a).toBeCloseTo(from.success.a.value, 3);
      expect(k.delta).toBe(Math.round(from.success.difference.value * 10_000) / 10_000);
      expect(k.lo).toBeLessThanOrEqual(k.delta);
      expect(k.hi).toBeGreaterThanOrEqual(k.delta);
    });
    // The class changes add up to the whole change.
    expect(c.by_equity_outcome.reduce((n, k) => n + k.delta * k.paths, 0) / c.paired_paths).toBeCloseTo(c.success.delta, 3);
    // Where the company lists, losing the proceeds costs the household money.
    expect(c.by_equity_outcome.find((k) => k.outcome === 'ipo')!.net_worth_median_change).toBeLessThan(0);
    // Each side's own equity block: as modelled it sometimes pays, at zero it never does.
    const detail = (key: string) => (r.json[key] as { equity: { nothing: { p: number }; net_proceeds: { p90: number } } }).equity;
    expect(detail('base_detail').nothing.p).toBeLessThan(0.9);
    expect(detail('base_detail').net_proceeds.p90).toBeGreaterThan(0);
    expect(detail('scenario_detail').nothing.p).toBe(1);
    expect(detail('scenario_detail').net_proceeds.p90).toBe(0);
    expect(r.reply.content.length).toBeLessThan(MAX_RESULT_CHARS);

    // A plan without private equity has no split.
    state().load({ meta: { id: 't', createdAt: 0, origin: 'test' }, profile: demo() });
    const plain = await call('try_scenario', { name: 'Retire at 55', ops: RETIRE_AT_55, paths: 400 });
    expect(plain.json.comparison).not.toHaveProperty('by_equity_outcome');
    expect(plain.json.base_detail).not.toHaveProperty('equity');
  }, SLOW);

  it('turns an exit price into a patch the engine accepts, and solves for the exit a goal needs', async () => {
    loadStartup();
    const profile = startup();
    const variable = resolveVariable(profile, { variable: 'exit_price_multiple' }, null, equityPreview);
    expect(variable).toMatchObject({ unit: 'factor', current: null, bounds: [0.25, 20], tolerance: 0.05 });
    // The time is the middle of the stage's typical years to a listing, and the price is stated in dollars.
    const ipo = equityPreview(profile.private_equity[0]!).typical.find((row) => row.kind === 'ipo')!;
    const years = (ipo.years_from + ipo.years_to) / 2;
    expect(variable.note).toContain('$14.00 a share');
    // The dollar price of a value is in the result, so the model never multiplies.
    expect(variable.per_share_dollars).toBe(14);
    expect(dollarsPerShare(variable, 3.3)).toBe(46.2);
    expect(dollarsPerShare(resolveVariable(profile, { variable: 'tender_sell_fraction' }), 0.5)).toBeNull();
    expect(variable.note).toContain(`${years} years from now`);
    for (const m of [0.25, 1, 3.3, 20]) {
      const ops = variable.ops(m);
      expect(ops).toEqual([{ op: 'set', path: '/private_equity/@northwind/outcomes', value: [{ id: 'ipo', kind: 'ipo', probability: 1, years_from: years, years_to: years, multiple_low: m, multiple_high: m }] }]);
      expect(applyPatch(profile, ops).errors, String(m)).toEqual([]);
    }
    // In the engine's own preview of the patched company, every future is a listing.
    const certain = equityPreview(applyPatch(profile, variable.ops(2)).profile.private_equity[0]!);
    expect(certain.outcome_shares).toEqual([0, 0, 0, 1]);

    const r = await call('solve', { variable: 'exit_price_multiple', target_metric: 'median_terminal_net_worth', target_value: SOLVE_TARGET, paths: 200 });
    expect(r.reply.isError).toBe(false);
    expect(r.json.status).toBe('converged');
    expect(r.json).toMatchObject({ unit: 'factor', current_value: null, metric_direction: 'rises as the quantity grows' });
    expect(String(r.json.variable_note)).toMatch(/certain listing/);
    const bracket = r.json.bracket as { below: { value: number; metric: number }; above: { value: number; metric: number } };
    expect(bracket.below.metric).toBeLessThanOrEqual(SOLVE_TARGET);
    expect(bracket.above.metric).toBeGreaterThanOrEqual(SOLVE_TARGET);
    expect(r.json.value as number).toBeGreaterThan(0.25);
    expect(r.json.value as number).toBeLessThan(20);
    expect(bracket.above.value - bracket.below.value).toBeLessThan(0.2);
    // The exit price in dollars per share is stated in the result itself, for the value and for each side of the bracket.
    expect(r.json.value_dollars_per_share).toBe(Math.round((r.json.value as number) * 14 * 100) / 100);
    const sides = r.json.bracket as { below: { value: number; dollars_per_share: number }; above: { value: number; dollars_per_share: number } };
    expect(sides.below.dollars_per_share).toBe(Math.round(sides.below.value * 14 * 100) / 100);
    expect(sides.above.dollars_per_share).toBe(Math.round(sides.above.value * 14 * 100) / 100);
    expect(String(r.json.variable_note)).toMatch(/quote that, do not multiply/);
    const swept = await call('sweep', { x: { variable: 'exit_price_multiple', values: [1, 3] }, metric: 'median_terminal_net_worth', paths: 200 });
    expect((swept.json.x as { values_dollars_per_share: number[] }).values_dollars_per_share).toEqual([14, 42]);
  }, SLOW);

  it('varies the share sold in tender offers, and refuses equity variables on a plan without a company', async () => {
    loadStartup();
    const variable = resolveVariable(startup(), { variable: 'tender_sell_fraction' });
    expect(variable).toMatchObject({ unit: 'rate', current: 0.5, bounds: [0, 1] });
    expect(variable.ops(1)).toEqual([{ op: 'set', path: '/private_equity/@northwind/policy/tender_sell_fraction', value: 1 }]);
    const r = await call('sweep', { x: { variable: 'tender_sell_fraction', values: [0, 0.5, 1] }, metric: 'median_terminal_net_worth', paths: 200 });
    expect(r.reply.isError).toBe(false);
    expect((r.json.cells as number[][])[0]).toHaveLength(3);
    expect((r.json.x as { current: number }).current).toBe(0.5);

    // Counted at zero, there is no company for the variables to act on.
    state().edit([...ZERO], { label: 'Zero', target: 'base' });
    const none = await call('solve', { variable: 'exit_price_multiple', target_metric: 'success_rate', target_value: 0.9, paths: 200 });
    expect(none.reply.isError).toBe(true);
    expect(none.reply.content).toMatch(/needs a private company with holdings that is counted/);
    state().load({ meta: { id: 't', createdAt: 0, origin: 'test' }, profile: demo() });
    expect((await call('sweep', { x: { variable: 'tender_sell_fraction', values: [0, 1] }, metric: 'success_rate' })).reply.isError).toBe(true);
    expect(engine.simulated).toBe(3);
  }, SLOW);

  it('ranks the equity levers with the others', async () => {
    loadStartup();
    const r = await call('rank_levers', { metric: 'median_terminal_net_worth', paths: 200 });
    expect(r.reply.isError).toBe(false);
    const ids = (r.json.levers as { id: string }[]).map((l) => l.id);
    expect(ids).toEqual(expect.arrayContaining(['spending', 'equity_outlook', 'equity_exercise', 'equity_tender', 'equity_sell_down']));
    expect(r.reply.content.length).toBeLessThan(MAX_RESULT_CHARS);
  }, SLOW);

  it('traces the lives the equity picks name, with the outcome in words and the equity events', async () => {
    loadStartup();
    const run = await call('run_simulation', { paths: 1000 });
    const picks = record(run.json.run_id).summary.picks;
    const failed = await call('inspect_life', { pick: 'equity_failed' });
    expect(failed.reply.isError).toBe(false);
    expect(failed.json.path).toBe(picks.find((p) => p.label === 'equity_failed')!.path_index);
    expect((failed.json.scalars as { equity_outcome: string }).equity_outcome).toBe('failed');
    expect((failed.json.events as { kind: string }[]).map((e) => e.kind)).toContain('company_failed');

    const paid = await call('inspect_life', { pick: 'equity_paid_p90' });
    const scalars = paid.json.scalars as { equity_outcome: string; equity_gross_proceeds: number; equity_net_proceeds: number; equity_event_age: number };
    expect(['acquired', 'ipo']).toContain(scalars.equity_outcome);
    expect(scalars.equity_gross_proceeds).toBeGreaterThan(0);
    expect(scalars.equity_net_proceeds).toBeLessThan(scalars.equity_gross_proceeds);
    expect(scalars.equity_event_age).toBeGreaterThan(30);
    const kinds = (paid.json.events as { kind: string }[]).map((e) => e.kind);
    expect(kinds.some((k) => k === 'company_acquired' || k === 'company_ipo')).toBe(true);
    expect(paid.reply.content.length).toBeLessThan(MAX_RESULT_CHARS);

    // The middle of the lives that were paid did worse than the 90th percentile of them.
    const middle = await call('inspect_life', { pick: 'equity_paid_median' });
    expect((middle.json.scalars as { equity_net_proceeds: number }).equity_net_proceeds).toBeLessThan(scalars.equity_net_proceeds);
    expect((middle.json.scalars as { equity_gross_proceeds: number }).equity_gross_proceeds).toBeGreaterThan(0);

    // A plan without a company has no such picks, and says which exist.
    state().load({ meta: { id: 't', createdAt: 0, origin: 'test' }, profile: demo() });
    await call('run_simulation', { paths: 400 });
    const none = await call('inspect_life', { pick: 'equity_failed' });
    expect(none.reply.isError).toBe(true);
    expect(none.reply.content).toMatch(/no "equity_failed" pick/);
    expect((await call('inspect_life', { pick: 'median' })).json.scalars).not.toHaveProperty('equity_outcome');
  }, SLOW);
});

describe('result discipline', () => {
  it('keeps every tool result compact', async () => {
    await call('run_simulation', {});
    const inputs: Record<string, unknown> = {
      get_plan: {},
      get_schema: { section: 'EventKind', with_nested: true },
      run_simulation: {},
      try_scenario: { name: 'x', ops: [{ op: 'set', path: '/spending/lifestyle_creep', value: 0.1 }] },
      rank_levers: { metric: 'median_fi_age', paths: 200 },
      solve: { variable: 'salary_scale', target_metric: 'success_rate', target_value: 0.85, paths: 200 },
      sweep: { x: { variable: 'retirement_age', lo: 50, hi: 68, steps: 8 }, y: { variable: 'annual_spending', steps: 8 }, metric: 'median_terminal_net_worth', paths: 200 },
      inspect_life: { pick: 'median' },
      explain_tax: { path: 7, year: 2026 },
      lookup_cost_of_living: { from: 'Denver', to: 'Lisbon' },
      show_chart: { kind: 'status_stack', caption: 'Life stages' },
    };
    for (const tool of TOOLS) {
      if (tool.name === 'propose_changes') continue;
      const r = await call(tool.name, inputs[tool.name]);
      expect(r.reply.isError, `${tool.name}: ${r.reply.content.slice(0, 200)}`).toBe(false);
      expect(r.reply.content.length, tool.name).toBeLessThan(8_000);
      expect(r.reply.content, tool.name).not.toContain('"quantiles"');
      expect(r.view.title.length, tool.name).toBeGreaterThan(8);
    }
  }, 2 * SLOW);
});
