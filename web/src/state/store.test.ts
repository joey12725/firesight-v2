// Store behaviour against the real wasm module (validate, applyPatch, migrateV1 work
// in every build). Needs `npm run wasm` to have produced src/engine/pkg.
import { readFileSync } from 'node:fs';
import { beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { from409aOps, linkJobOps, newCompany, newHolding, type HoldingType } from '@/editor/equity/model';
import { defaultProfile, equityPreview, exampleProfiles, initEngineSync } from '@/engine/api';
import type { Profile } from '@/engine/types';
import { parsePlan, planFileText } from './files';
import { append, compactPatch, remove, set } from './patches';
import { canonicalPath, getAt, uniqueId } from './pointer';
import { selectEffectiveProfile, useProfileStore } from './profile-store';
import { decodeShare, encodeShare, shareParam, shareUrl } from './share';

beforeAll(() => {
  initEngineSync(readFileSync(new URL('../engine/pkg/firesight_wasm_bg.wasm', import.meta.url)));
});

const store = () => useProfileStore.getState();
const demo = () => exampleProfiles()[0]!.profile;

beforeEach(() => {
  store().load({ meta: { id: 't', createdAt: 0, origin: 'test' }, profile: demo() });
});

describe('pointer helpers', () => {
  it('reads by id and canonicalises index segments', () => {
    const p = demo();
    const id = p.incomes[0]!.id;
    expect(getAt(p, `/incomes/@${id}/kind/type`)).toBe('job');
    expect(getAt(p, '/incomes/0/kind/type')).toBe('job');
    expect(getAt(p, '/incomes/@missing/kind')).toBeUndefined();
    expect(canonicalPath(p, '/incomes/0/kind/salary')).toBe(`/incomes/@${id}/kind/salary`);
    expect(canonicalPath(p, '/portfolio/target')).toBe('/portfolio/target');
    expect(uniqueId('New Job!', ['new_job', 'new_job_2'])).toBe('new_job_3');
  });

  it('compacts repeated sets but keeps structural edits in order', () => {
    expect(compactPatch([set('/a', 1), set('/b', 2), set('/a', 3)])).toEqual([set('/b', 2), set('/a', 3)]);
    const ops = [set('/a', 1), append('/list', { id: 'x' }), set('/a', 2), remove('/list/@x'), set('/a', 3)];
    expect(compactPatch(ops)).toEqual(ops);
  });
});

describe('profile store', () => {
  it('loads a complete, valid profile', () => {
    expect(store().report.errors).toEqual([]);
    expect(store().profile?.household.people.length).toBeGreaterThan(0);
    expect(store().revision).toBe(store().savedRevision);
  });

  it('edits through patches, revalidates, and undoes', () => {
    const id = store().profile!.incomes[0]!.id;
    const before = store().profile!.incomes[0]!.kind;
    const path = `/incomes/@${id}/kind/salary`;
    expect(store().edit([set(path, 200_000)]).ok).toBe(true);
    expect(getAt(store().profile, path)).toBe(200_000);
    expect(store().revision).not.toBe(store().savedRevision);

    // An invalid value is kept (the user is mid-edit) and reported by the engine.
    store().edit([set(path, -5)]);
    expect(store().report.errors.some((e) => canonicalPath(store().profile, e.path) === path)).toBe(true);

    store().undo();
    expect(getAt(store().profile, path)).toBe(200_000);
    expect(store().report.errors).toEqual([]);
    store().undo();
    expect(store().profile!.incomes[0]!.kind).toEqual(before);
    store().redo();
    expect(getAt(store().profile, path)).toBe(200_000);
  });

  it('coalesces rapid edits to one field into one undo step', () => {
    const path = '/spending/lifestyle_creep';
    const original = getAt(store().profile, path);
    for (const v of [0.1, 0.2, 0.3]) store().edit([set(path, v)], { coalesce: path });
    expect(store().past).toHaveLength(1);
    store().undo();
    expect(getAt(store().profile, path)).toBe(original);
  });

  it('rejects a patch that cannot apply and changes nothing', () => {
    const before = store().profile;
    const result = store().edit([remove('/events/@no_such_event')]);
    expect(result.ok).toBe(false);
    expect(store().profile).toBe(before);
    expect(store().past).toHaveLength(0);
  });

  it('keeps scenarios as patches and resolves them against the baseline', () => {
    const added = store().addScenario({ name: 'Spend less', patch: [set('/spending/lifestyle_creep', 0)] });
    expect(added.ok && added.id).toBe('spend_less');
    expect(store().resolved.spend_less?.profile?.spending.lifestyle_creep).toBe(0);
    expect(store().profile!.spending.lifestyle_creep).not.toBe(0);

    // Editing with the scenario as target changes its patch, not the baseline.
    store().setEditTarget('spend_less');
    const base = store().profile;
    store().edit([set('/retirement/bequest_target', 250_000)]);
    expect(store().profile).toBe(base);
    expect(store().scenarios[0]!.patch).toHaveLength(2);
    expect(selectEffectiveProfile(store())?.retirement.bequest_target).toBe(250_000);

    // A baseline change flows through to the resolved scenario.
    store().edit([set('/name', 'Renamed')], { target: 'base' });
    expect(store().resolved.spend_less?.profile?.name).toBe('Renamed');
    expect(store().resolved.spend_less?.profile?.retirement.bequest_target).toBe(250_000);

    store().updateScenario('spend_less', { enabled: false });
    expect(store().scenarios[0]!.enabled).toBe(false);
    store().removeScenario('spend_less');
    expect(store().scenarios).toHaveLength(0);
    expect(store().editTarget).toBe('base');
    store().undo();
    expect(store().scenarios).toHaveLength(1);
  });

  it('flags a scenario whose patch no longer applies', () => {
    const eventId = store().profile!.events[0]!.id;
    store().addScenario({ name: 'Later', patch: [set(`/events/@${eventId}/probability`, 0.5)] });
    store().edit([remove(`/events/@${eventId}`)], { target: 'base' });
    expect(store().resolved.later?.profile).toBeNull();
    expect(store().resolved.later?.error).toContain(eventId);
  });

  it('turns the last baseline edit into a scenario', () => {
    const original = store().profile!.retirement.bequest_target;
    store().edit([set('/retirement/bequest_target', 500_000)], { label: 'Leave 500k' });
    const result = store().moveLastEditToScenario();
    expect(result.ok).toBe(true);
    expect(store().profile!.retirement.bequest_target).toBe(original);
    expect(store().scenarios.at(-1)).toMatchObject({ name: 'Leave 500k', patch: [set('/retirement/bequest_target', 500_000)], enabled: true });
    expect(store().moveLastEditToScenario().ok).toBe(false);
  });

  it('promotes a scenario to the plan as one undo step', () => {
    const original = store().profile!.retirement.bequest_target;
    store().addScenario({ name: 'Leave more', patch: [set('/retirement/bequest_target', 750_000)] });
    store().addScenario({ name: 'Other', patch: [set('/spending/lifestyle_creep', 0)] });
    const before = store().past.length;
    expect(store().promoteScenario('leave_more').ok).toBe(true);
    expect(store().profile!.retirement.bequest_target).toBe(750_000);
    expect(store().scenarios.map((x) => x.id)).toEqual(['other']);
    expect(store().past.length).toBe(before + 1);
    // The remaining scenario now resolves against the new plan.
    expect(store().resolved.other?.profile?.retirement.bequest_target).toBe(750_000);
    store().undo();
    expect(store().profile!.retirement.bequest_target).toBe(original);
    expect(store().scenarios.map((x) => x.id)).toEqual(['leave_more', 'other']);
    store().redo();
    expect(store().profile!.retirement.bequest_target).toBe(750_000);
    expect(store().scenarios.map((x) => x.id)).toEqual(['other']);
    expect(store().promoteScenario('nope').ok).toBe(false);
  });

  it('previews without committing', () => {
    const before = store().profile;
    const preview = store().preview([set('/portfolio/target', { us_stocks: 50, crypto: 50 })]);
    expect('report' in preview && preview.report.warnings.some((w) => w.code === 'crypto_heavy')).toBe(true);
    expect(store().profile).toBe(before);
    expect('error' in store().preview([set('/nope', 1)])).toBe(true);
  });
});

describe('startup equity through the store', () => {
  const startup = () => exampleProfiles().find((e) => e.slug === 'startup-employee')!.profile;
  const load = (profile: Profile) => store().load({ meta: { id: 't', createdAt: 0, origin: 'test' }, profile });
  const errors = () => store().report.errors.map((e) => `${e.code} ${e.path}`);
  const KINDS: HoldingType[] = ['iso', 'nso', 'rsu', 'shares'];

  /** The editor's add path: a company, a price, then one holding of each kind. Asserts no error after each step. */
  function addPath(label: string, withJob: boolean) {
    const profile = store().profile!;
    const before = errors();
    const company = { id: 'newco', ...newCompany(withJob ? profile : { ...profile, incomes: [] }) };
    expect(company.job === null, label).toBe(!withJob);
    expect(store().edit([append('/private_equity', company)]).ok, label).toBe(true);
    expect(errors(), `${label}: add a company`).toEqual(before);
    store().edit([set('/private_equity/@newco/preferred_price', 14)]);
    expect(errors(), `${label}: price`).toEqual(before);
    for (const kind of KINDS) {
      const { name, ...value } = newHolding(kind, company.job !== null);
      expect(store().edit([append('/private_equity/@newco/holdings', { id: kind, name, ...value })]).ok, `${label}: ${kind}`).toBe(true);
      expect(errors(), `${label}: add ${kind}`).toEqual(before);
    }
    expect(store().profile!.private_equity.find((c) => c.id === 'newco')!.holdings).toHaveLength(4);
  }

  it('adds a company, a price and each kind of holding with no validation error, with and without a job', () => {
    const blank = defaultProfile();
    const withJob = { ...blank, incomes: [{ id: 'job', name: 'Job', owner: 0, kind: { type: 'job', salary: 120_000 } }] } as unknown as Profile;
    const cases: [string, Profile][] = [['blank', blank], ['blank with a job', withJob], ...exampleProfiles().map((e): [string, Profile] => [e.slug, e.profile])];
    for (const [label, profile] of cases) {
      const hasJob = profile.incomes.some((i) => i.kind.type === 'job');
      for (const linked of hasJob ? [true, false] : [false]) {
        load(profile);
        expect(store().report.errors, label).toEqual([]);
        addPath(`${label}, ${linked ? 'linked' : 'no job'}`, linked);
      }
    }
  });

  it('appends a company and a holding, undoes them, and targets a scenario', () => {
    load(startup());
    const base = store().profile!;
    store().edit([append('/private_equity', { id: 'second', ...newCompany(base) })], { label: 'Add a company' });
    expect(store().profile!.private_equity.map((c) => c.id)).toEqual(['northwind', 'second']);
    // The engine filled in everything the editor did not write.
    expect(store().profile!.private_equity[1]).toMatchObject({ job: 'priya_job', owner: 0, included: true, outlook: 'typical', outcomes: [], lockup_months: 6, policy: { exercise: 'at_liquidity', at_deadline: 'exercise', tender_sell_fraction: 0.5 } });
    const { name, ...iso } = newHolding('iso', true);
    store().edit([append('/private_equity/@second/holdings', { id: 'grant', name, ...iso })]);
    expect(getAt(store().profile, '/private_equity/@second/holdings/@grant/kind')).toEqual({ type: 'iso', strike: 1, granted_months_ago: 24 });
    store().undo();
    expect(store().profile!.private_equity[1]!.holdings).toEqual([]);
    store().undo();
    expect(store().profile).toEqual(base);
    store().redo();
    expect(store().profile!.private_equity).toHaveLength(2);
    store().undo();

    // A scenario edit leaves the baseline alone and resolves against it.
    store().addScenario({ name: 'Equity at zero', patch: [set('/private_equity/@northwind/included', false)] });
    expect(store().profile!.private_equity[0]!.included).toBe(true);
    expect(store().resolved.equity_at_zero?.profile?.private_equity[0]!.included).toBe(false);
    store().setEditTarget('equity_at_zero');
    store().edit([set('/private_equity/@northwind/outlook', 'pessimistic')]);
    expect(store().profile!.private_equity[0]!.outlook).toBe('typical');
    expect(selectEffectiveProfile(store())?.private_equity[0]).toMatchObject({ included: false, outlook: 'pessimistic' });
    expect(store().scenarios[0]!.patch).toHaveLength(2);
  });

  it('switching the linked job to None leaves no error, in one undo step', () => {
    load(startup());
    const company = store().profile!.private_equity[0]!;
    const { ops, vestedChanged } = linkJobOps('/private_equity/@northwind', company, null);
    expect(vestedChanged).toBe(true);
    expect(store().edit(ops).ok).toBe(true);
    expect(store().report.errors).toEqual([]);
    expect(store().profile!.private_equity[0]!.job).toBeNull();
    expect(store().past).toHaveLength(1);
    store().undo();
    expect(store().profile!.private_equity[0]).toEqual(company);
  });

  it('"I only know the 409A value" writes both fields in one undo step', () => {
    load(startup());
    const path = '/private_equity/@northwind';
    const before = store().profile!.private_equity[0]!;
    const ratio = equityPreview({ stage: before.stage, preferred_price: 1 }).fmv_409a;
    // Typed digit by digit: the edits coalesce like any field's.
    for (const v of [4, 4.2, 4.25]) store().edit(from409aOps(path, v, ratio), { coalesce: `${path}#from-409a` });
    const after = store().profile!.private_equity[0]!;
    expect(after.fmv_409a).toBe(4.25);
    expect(after.preferred_price).toBe(Math.round((4.25 / ratio) * 100) / 100);
    expect(store().report.errors).toEqual([]);
    expect(store().past).toHaveLength(1);
    store().undo();
    expect(store().profile!.private_equity[0]).toEqual(before);
  });
});

describe('files', () => {
  it('round-trips a plan with scenarios and run settings', () => {
    store().addScenario({ name: 'Alt', patch: [set('/spending/lifestyle_creep', 0)] });
    const text = planFileText(store().profile!, store().scenarios, { paths: 50_000, seed: 9 });
    const plan = parsePlan(text);
    expect(plan.format).toBe('v2');
    expect(plan.scenarios).toHaveLength(1);
    expect(plan.run).toEqual({ paths: 50_000, seed: 9 });
    expect(plan.profile.simulation.paths).toBe(50_000);
    expect(plan.profile.household).toEqual(store().profile!.household);
  });

  it('writes a bare profile when there are no scenarios, as the CLI expects', () => {
    const parsed = JSON.parse(planFileText(defaultProfile(), [], { paths: 2_000, seed: 3 })) as { schema_version: number; simulation: { seed: number } };
    expect(parsed.schema_version).toBe(2);
    expect(parsed.simulation.seed).toBe(3);
  });

  it('leaves the optional fields that hold their defaults out of the file, and reads them back', () => {
    const profile = store().profile!;
    // In the app every field is there...
    expect(profile.spending.vehicles).toEqual([]);
    expect(profile.retirement.home_equity_borrowing).toEqual({ type: 'none' });
    // ...in the file only what the plan uses, so a plan without optional features is written as it always was.
    const written = JSON.parse(planFileText(profile, [], { paths: 2_000, seed: 3 })) as { spending: Record<string, unknown>; retirement: Record<string, unknown> };
    expect('vehicles' in written.spending).toBe(false);
    expect('home_equity_borrowing' in written.retirement).toBe(false);
    expect(store().edit([set('/retirement/qcd', true), append('/spending/vehicles', { id: 'car', name: 'Car', price: 30_000 })]).ok).toBe(true);
    const text = planFileText(store().profile!, [], { paths: 2_000, seed: 3 });
    expect((JSON.parse(text) as { retirement: { qcd?: boolean } }).retirement.qcd).toBe(true);
    const back = parsePlan(text).profile;
    expect(back.spending.vehicles).toHaveLength(1);
    expect(back.spending.seasonality).toBe('off');
    expect(back.household).toEqual(store().profile!.household);
  });

  it('migrates a v1 file and reports what it assumed', () => {
    const v1 = readFileSync(new URL('../../../examples/v1/dual-income-family.json', import.meta.url), 'utf8');
    const plan = parsePlan(v1);
    expect(plan.format).toBe('v1');
    expect(plan.notes.length).toBeGreaterThan(3);
    expect(plan.profile.schema_version).toBe(2);
  });

  it('explains files it cannot read', () => {
    expect(() => parsePlan('not json')).toThrow(/not valid JSON/);
    expect(() => parsePlan('[1,2]')).toThrow(/does not contain/);
    expect(() => parsePlan('{"schema_version": 2, "household": {"peeple": []}}')).toThrow(/peeple/);
  });
});

describe('share links', () => {
  it('round-trips the plan through the URL fragment', async () => {
    const payload = { v: 2 as const, profile: store().profile!, scenarios: [{ id: 'a', name: 'A', patch: [set('/name', 'x')], enabled: true }], run: { paths: 10_000, seed: 4 } };
    const url = await shareUrl(payload, 'https://example.test/outcomes?x=1');
    expect(url.startsWith('https://example.test/outcomes?x=1#plan=')).toBe(true);
    const encoded = shareParam(new URL(url).hash)!;
    expect(encoded).toMatch(/^[A-Za-z0-9_-]+$/);
    expect(await decodeShare(encoded)).toEqual(payload);
    // Compression earns its keep: the link is much shorter than the JSON.
    expect(encoded.length).toBeLessThan(JSON.stringify(payload).length / 2);
    expect(await encodeShare(payload)).toBe(encoded);
  });

  it('rejects links that are not plans', async () => {
    await expect(decodeShare('AAAA')).rejects.toThrow();
    expect(shareParam('#other=1')).toBeNull();
  });
});
