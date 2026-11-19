// Store behaviour against the real wasm module (validate, applyPatch, migrateV1 work
// in every build). Needs `npm run wasm` to have produced src/engine/pkg.
import { readFileSync } from 'node:fs';
import { beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { defaultProfile, exampleProfiles, initEngineSync } from '@/engine/api';
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
