// Every tool executor against a fake simulation pool and the real profile store, patch
// engine, validator, levers and schema (which work in every wasm build).
import { readFileSync } from 'node:fs';
import { beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { applyPatch, exampleProfiles, initEngineSync, levers, schema, validate } from '@/engine/api';
import { EngineError } from '@/engine/errors';
import type { City } from '@/editor/cities';
import type { Profile } from '@/engine/types';
import { useProfileStore } from '@/state/profile-store';
import { EXAMPLE_INTAKE_PATCH, EXAMPLE_SCENARIO_PATCH } from '../prompt';
import { defaultKnobs, fakePool, type FakePool } from '../testing/fake-engine';
import { executeTool, MAX_RESULT_CHARS, RunRegistry, TOOLS, TOOL_NAMES, toolDefinitions, type Decision, type PlanStore, type Proposal, type ToolContext, type ToolRun } from './index';
import { SCHEMA_TYPES } from './plan';
import { pruneDefaults, typeNames, type SchemaNode } from './plan-view';

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

let pool: FakePool;
let ctx: ToolContext;
let decide: (proposal: Proposal) => Decision;
let proposals: Proposal[];

function makeContext(overrides: Partial<ToolContext> = {}): ToolContext {
  return {
    store,
    engine: { pool, applyPatch, levers, schema },
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

beforeEach(() => {
  state().load({ meta: { id: 't', createdAt: 0, origin: 'test' }, profile: demo() });
  state().setRun({ paths: 10_000, seed: 7 });
  pool = fakePool();
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
    expect(JSON.stringify(defs).length).toBeLessThan(24_000);
  });

  it('keep the schema type list in step with the engine schema', () => {
    expect([...SCHEMA_TYPES].slice(1).sort()).toEqual(typeNames(schema() as SchemaNode).sort());
  });

  it('reject malformed input before running anything', async () => {
    const r = await call('try_scenario', { name: 'x' });
    expect(r.reply.isError).toBe(true);
    expect(r.json.problems).toEqual(['input.ops is required']);
    expect(pool.simulated).toBe(0);
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
    expect((r.json.derived as { annual_budget_now: number }).annual_budget_now).toBeGreaterThan(0);
    expect((r.json.validation as { can_run: boolean }).can_run).toBe(true);
    expect(r.json.scenarios).toEqual([expect.objectContaining({ id: 'retire_at_55', name: 'Retire at 55' })]);
    expect(r.json.run_settings).toMatchObject({ paths: 10_000, seed: 7 });
    // Much shorter than the raw profile, and within the result budget.
    expect(r.reply.content.length).toBeLessThan(JSON.stringify(profile).length * 0.7);
    expect(r.reply.content.length).toBeLessThan(MAX_RESULT_CHARS);
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
  it('returns a compact digest with intervals, on the session seed', async () => {
    const r = await call('run_simulation', {});
    expect(r.reply.isError).toBe(false);
    expect(r.json).toMatchObject({ paths: 10_000, seed: 7, common_random_numbers: true });
    const success = r.json.success as { p: number; lo: number; hi: number };
    expect(success.lo).toBeLessThan(success.p);
    expect(success.hi).toBeGreaterThan(success.p);
    expect((r.json.by_age as { ages: number[] }).ages).toHaveLength(6);
    expect((r.json.trace_paths as { median: number }).median).toBe(7);
    expect(r.reply.content.length).toBeLessThan(2_500);
    expect(r.view).toMatchObject({ title: 'Simulated 10,000 lives of the current plan', meta: { paths: 10_000, seed: 7 } });
    // The same inputs are not simulated twice.
    await call('run_simulation', {});
    expect(pool.simulated).toBe(1);
  });

  it('compares a scenario with the plan on the same lives, without touching the plan', async () => {
    const before = state().profile;
    const r = await call('try_scenario', { name: 'Retire at 55', ops: [{ op: 'set', path: '/retirement/trigger', value: { type: 'at_age', person: 0, age: 55 } }], paths: 4000 });
    expect(r.reply.isError).toBe(false);
    expect(state().profile).toBe(before);
    expect(state().scenarios).toHaveLength(0);
    const c = r.json.comparison as { paired: boolean; paired_paths: number; seed: number; success: { delta: number; std_error: number; rescued: number; harmed: number; within_noise: boolean; mcnemar_z: number } };
    expect(c).toMatchObject({ paired: true, paired_paths: 4000, seed: 7 });
    // Retiring earlier than the default costs success in the fake engine: lives are harmed, none rescued.
    const expected = defaultKnobs(applyPatch(before!, [{ op: 'set', path: '/retirement/trigger', value: { type: 'at_age', person: 0, age: 55 } }]).profile).success - defaultKnobs(before!).success;
    expect(c.success.delta).toBeCloseTo(expected, 2);
    expect(c.success.rescued).toBe(0);
    expect(c.success.harmed).toBeGreaterThan(0);
    // The paired standard error is far smaller than that of two independent runs.
    expect(c.success.std_error).toBeLessThan(0.01);
    expect(c.success.within_noise).toBe(false);
    expect(r.view.meta).toMatchObject({ paired: true, paths: 4000, runs: 2 });
    expect(pool.batches).toEqual([2]);
    expect(r.reply.content.length).toBeLessThan(MAX_RESULT_CHARS);
  });

  it('saves the scenario when asked to keep it, and reuses the baseline run', async () => {
    await call('run_simulation', { paths: 4000 });
    const r = await call('try_scenario', { name: 'Spend less', ops: [{ op: 'set', path: '/spending/lifestyle_creep', value: 0 }], keep: true, paths: 4000 });
    expect(r.json.saved_scenario).toEqual({ id: 'spend_less' });
    expect(state().scenarios.map((s) => s.name)).toEqual(['Spend less']);
    expect(pool.simulated).toBe(2);
  });

  it('flags a difference that cannot be told from zero', async () => {
    const r = await call('try_scenario', { name: 'Rename', ops: [{ op: 'set', path: '/name', value: 'Renamed' }], paths: 2000 });
    const c = r.json.comparison as { success: { delta: number; within_noise: boolean } };
    expect(c.success.delta).toBe(0);
    expect(c.success.within_noise).toBe(true);
  });

  it('passes patch errors back verbatim and blocks an invalid plan', async () => {
    const bad = await call('try_scenario', { name: 'Bad', ops: [{ op: 'set', path: '/retirement/triger', value: 1 }] });
    expect(bad.reply.isError).toBe(true);
    expect(bad.reply.content).toMatch(/patch/);
    expect(pool.simulated).toBe(0);

    state().load({ meta: { id: 'blank', createdAt: 0, origin: 'test' }, profile: applyPatch('{}', []).profile });
    const blocked = await call('run_simulation', {});
    expect(blocked.reply.isError).toBe(true);
    expect(blocked.reply.content).toMatch(/cannot be simulated until/);
  });

  it('says plainly when the engine cannot simulate yet', async () => {
    pool = fakePool(defaultKnobs, { fail: () => new EngineError('not_implemented', 'not implemented: Engine::run_chunk') });
    const r = await call('run_simulation', {}, makeContext());
    expect(r.reply.isError).toBe(true);
    expect(r.reply.content).toMatch(/no results/);
    expect(r.reply.content).toMatch(/do not estimate/);
  });
});

describe('rank_levers, solve and sweep', () => {
  it('ranks the engine levers and custom levers as one paired batch', async () => {
    const r = await call('rank_levers', {
      metric: 'success_rate',
      custom_levers: [{ label: 'Retire at 65', ops: [{ op: 'set', path: '/retirement/trigger', value: { type: 'at_age', person: 0, age: 65 } }] }],
    });
    expect(r.reply.isError).toBe(false);
    const ranked = r.json.levers as { id: string; label: string; swing: number; settings: { label: string; delta: number; within_noise: boolean | null }[] }[];
    expect(pool.batches).toHaveLength(1);
    // One batch for the plan and every setting; a setting identical to the plan (Social Security paid in full) is not simulated twice.
    const settings = ranked.reduce((sum, l) => sum + l.settings.length, 0);
    expect(pool.batches[0]).toBeGreaterThanOrEqual(settings);
    expect(pool.batches[0]).toBeLessThanOrEqual(settings + 1);
    expect(ranked.map((l) => l.id)).toContain('spending');
    expect(ranked.map((l) => l.id)).toContain('custom_1');
    const swings = ranked.map((l) => l.swing);
    expect(swings).toEqual([...swings].sort((a, b) => b - a));
    const spending = ranked.find((l) => l.id === 'spending')!;
    expect(spending.settings.map((s) => Math.sign(s.delta))).toEqual([1, -1]);
    // Fees do nothing in the fake engine: reported, and marked as noise.
    expect(ranked.find((l) => l.id === 'fees')!.settings.every((s) => s.delta === 0 && s.within_noise === true)).toBe(true);
    expect(r.json).toMatchObject({ paired: true, paths: 2000, seed: 7 });
    expect(typeof r.json.ranking_id).toBe('string');
    expect(r.reply.content.length).toBeLessThan(MAX_RESULT_CHARS);
  });

  it('solves for the spending that meets a success target by bisection', async () => {
    const r = await call('solve', { variable: 'annual_spending', target_metric: 'success_rate', target_value: 0.9 });
    expect(r.json.status).toBe('converged');
    // Check against the fake engine's known response: success falls 1 point per $1,500 a year.
    const profile = state().profile!;
    const check = applyPatch(profile, []).profile;
    const now = (r.json.current_value as number) ?? 0;
    const scale = (r.json.value as number) / now;
    check.spending.items.forEach((item) => (item.amount *= scale));
    expect(defaultKnobs(check).success).toBeCloseTo(0.9, 1);
    const bracket = r.json.bracket as { below: { value: number; metric: number }; above: { value: number; metric: number } };
    expect(bracket.below.metric).toBeGreaterThanOrEqual(0.9);
    expect(bracket.above.metric).toBeLessThanOrEqual(0.9);
    expect(r.json.metric_direction).toBe('falls as the quantity grows');
    expect((r.json.evaluations as unknown[]).length).toBeLessThanOrEqual(9);
    expect(r.view.meta).toMatchObject({ paths: 2000, seed: 7, paired: true });
  });

  it('reports out of range with the closest end instead of inventing an answer', async () => {
    const r = await call('solve', { variable: 'retirement_age', target_metric: 'success_rate', target_value: 0.999, lo: 50, hi: 55 });
    expect(r.json.status).toBe('out_of_range');
    expect((r.json.closest as { value: number }).value).toBe(55);
    expect(r.json.evaluations).toHaveLength(2);
  });

  it('solves along a custom numeric path and rejects a non-numeric one', async () => {
    const ok = await call('solve', { path: '/assumptions/market/equity_premium_adjustment', target_metric: 'success_rate', target_value: 0.7, lo: -0.05, hi: 0.02 });
    expect(ok.json.status).toBe('converged');
    const bad = await call('solve', { path: '/retirement/trigger', target_metric: 'success_rate', target_value: 0.9 });
    expect(bad.reply.isError).toBe(true);
    expect(bad.reply.content).toMatch(/not a number/);
  });

  it('sweeps a grid of two quantities at reduced paths', async () => {
    const r = await call('sweep', { x: { variable: 'retirement_age', values: [55, 60, 65] }, y: { variable: 'annual_spending', lo: 50_000, hi: 80_000, steps: 4 }, metric: 'success_rate' });
    expect(r.reply.isError).toBe(false);
    const cells = r.json.cells as number[][];
    expect(cells).toHaveLength(4);
    expect(cells[0]).toHaveLength(3);
    // Later retirement helps; more spending hurts.
    expect(cells[0]![2]).toBeGreaterThan(cells[0]![0]!);
    expect(cells[3]![0]).toBeLessThan(cells[0]![0]!);
    expect(pool.batches).toEqual([12]);
    expect(r.json).toMatchObject({ paths: 1000, paired: true, cells_layout: 'cells[y index][x index]' });
    // Grid cells are not kept as runs.
    expect(ctx.runs.ids()).toEqual([]);
    const tooBig = await call('sweep', { x: { variable: 'retirement_age', values: [1, 2, 3, 4, 5, 6, 7, 8, 9] }, metric: 'success_rate' });
    expect(tooBig.reply.isError).toBe(true);
  });
});

describe('inspect_life and explain_tax', () => {
  it('traces a picked life as a year table with its events', async () => {
    const none = await call('inspect_life', {});
    expect(none.reply.content).toMatch(/run_simulation first/);
    const run = await call('run_simulation', { paths: 2000 });
    const r = await call('inspect_life', { pick: 'failing' });
    expect(r.reply.isError).toBe(false);
    expect(r.json).toMatchObject({ path: 17, run_id: run.json.run_id, one_life_of: 2000, seed: 7 });
    const table = r.json.table as Record<string, unknown[]>;
    expect(table.year).toEqual([2026, 2027, 2028, 2029]);
    expect(table.income).toEqual([128, 132, 0, 0]);
    expect(table.status).toEqual(['working', 'working', 'retired_on_plan', 'retired_cutting']);
    expect((r.json.events as { kind: string }[]).map((e) => e.kind)).toEqual(['market', 'laid_off', 'retired', 'spending_cut']);
    expect(r.json.scalars).toEqual({ fi_age: 37, terminal_net_worth: 198_700, success: 0 });
  });

  it('picks a life by quantile of a per-life quantity, or by path id', async () => {
    await call('run_simulation', { paths: 2000 });
    const low = await call('inspect_life', { quantile: 0.05, by: 'terminal_net_worth' });
    const high = await call('inspect_life', { quantile: 0.95, by: 'terminal_net_worth' });
    expect(low.json.path).not.toBe(high.json.path);
    expect((await call('inspect_life', { path: 12 })).json.path).toBe(12);
    expect((await call('inspect_life', { path: 99_999 })).reply.content).toMatch(/does not exist/);
  });

  it('returns the 1040-style lines of one simulated year', async () => {
    await call('run_simulation', { paths: 2000 });
    const r = await call('explain_tax', { path: 7, year: 2027 });
    expect(r.json).toMatchObject({ year: 2027, path: 7, price_level: 1.03, dollars: 'nominal dollars of the tax year' });
    expect((r.json.lines as { key: string }[]).map((l) => l.key)).toEqual(['wages', 'standard_deduction', 'total_tax']);
    expect((await call('explain_tax', { path: 7, year: 1999 })).reply.content).toMatch(/2026 to 2029/);
  });
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
    const run = await call('run_simulation', { paths: 2000 });
    const fan = await call('show_chart', { kind: 'fan', caption: 'Net worth by age' });
    expect(fan.json).toEqual({ shown: true, kind: 'fan' });
    expect(fan.view.display).toEqual({ type: 'chart', kind: 'fan', caption: 'Net worth by age', runId: run.json.run_id, compareRunId: null, dataId: null });
    expect((await call('show_chart', { kind: 'tornado', caption: 'Levers' })).reply.content).toMatch(/ranking_id/);
    const ranking = await call('rank_levers', { metric: 'success_rate' });
    const tornado = await call('show_chart', { kind: 'tornado', caption: 'Levers', data_id: ranking.json.ranking_id });
    expect(tornado.view.display).toMatchObject({ type: 'chart', kind: 'tornado', dataId: ranking.json.ranking_id });
    expect((await call('show_chart', { kind: 'heatmap', caption: 'x', data_id: ranking.json.ranking_id })).reply.isError).toBe(true);
    expect((await call('show_chart', { kind: 'compare_delta', caption: 'x' })).reply.content).toMatch(/try_scenario/);
  });
});

describe('result discipline', () => {
  it('keeps every tool result compact', async () => {
    await call('run_simulation', { paths: 2000 });
    const inputs: Record<string, unknown> = {
      get_plan: {},
      get_schema: { section: 'EventKind', with_nested: true },
      run_simulation: {},
      try_scenario: { name: 'x', ops: [{ op: 'set', path: '/spending/lifestyle_creep', value: 0.1 }] },
      rank_levers: { metric: 'median_fi_age' },
      solve: { variable: 'salary_scale', target_metric: 'success_rate', target_value: 0.85 },
      sweep: { x: { variable: 'retirement_age', lo: 50, hi: 68, steps: 8 }, y: { variable: 'annual_spending', steps: 8 }, metric: 'median_terminal_net_worth' },
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
  });
});
