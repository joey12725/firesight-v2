// The Scenarios view's transforms and words, on real engine output: the three demo
// households, a few hundred lives each, compared with real what-ifs.
import { readFileSync } from 'node:fs';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { applyPatch, compare, exampleProfiles, initEngineSync } from '@/engine/api';
import type { Comparison, Summary } from '@/engine/output-types';
import { engineInput } from '@/engine/pool';
import * as wasm from '@/engine/pkg/firesight_wasm.js';
import type { PatchOp, Profile } from '@/engine/types';
import { describePatch, describeValue, fieldLabel, lineText, pathPattern } from './describe';
import { metricRows, metricTableData, metricTakeaway } from './MetricTable';
import { METRICS, binDeltas, chooseLives, divergence, flipCounts, flipsOf, formatDelta, rankRows, scalarOf, stageDiffs, type Flips } from './model';
import { quickScenarios, samePatch, stockShare } from './quick';
import { deltaTakeaway, fanTakeaway, flipNote, flipSentence, plain, provisionalSentence, rankTakeaway, stageTakeaway } from './text';

// Real simulations on a machine that may be busy with other work.
vi.setConfig({ testTimeout: 120_000, hookTimeout: 120_000 });

const PATHS = 400;
const demos: { slug: string; profile: Profile }[] = [];
const runs = new Map<string, { summary: Summary; acc: Uint8Array; json: string }>();

function run(profile: Profile): { summary: Summary; acc: Uint8Array; json: string } {
  const { json, key } = engineInput(profile, 1);
  let hit = runs.get(key);
  if (!hit) {
    const engine = new wasm.Engine(json);
    try {
      const acc = engine.runChunk(0, PATHS);
      hit = { summary: JSON.parse(wasm.summarize(acc, json)) as Summary, acc, json };
    } finally {
      engine.free();
    }
    runs.set(key, hit);
  }
  return hit;
}

function paired(base: Profile, patch: PatchOp[]): { a: Summary; b: Summary; c: Comparison; after: Profile } {
  const after = applyPatch(base, patch).profile;
  const a = run(base);
  const b = run(after);
  return { a: a.summary, b: b.summary, c: compare(a.acc, b.acc, a.json), after };
}

beforeAll(() => {
  initEngineSync(readFileSync(new URL('../../engine/pkg/firesight_wasm_bg.wasm', import.meta.url)));
  for (const e of exampleProfiles()) demos.push({ slug: e.slug, profile: applyPatch(e.profile, []).profile });
});

const demo = (slug: string) => demos.find((d) => d.slug === slug)?.profile as Profile;

describe('quick what-ifs', () => {
  it('builds patches the engine accepts without errors, for every demo household', () => {
    expect(demos.length).toBeGreaterThanOrEqual(3);
    for (const { slug, profile } of demos) {
      const quick = quickScenarios(profile, run(profile).summary);
      expect(quick.length, slug).toBeGreaterThanOrEqual(8);
      expect(new Set(quick.map((q) => q.key)).size).toBe(quick.length);
      for (const q of quick) {
        const r = applyPatch(profile, q.patch);
        expect(r.errors, `${slug} ${q.key}`).toEqual([]);
        expect(JSON.stringify(r.profile), `${slug} ${q.key} changes the plan`).not.toBe(JSON.stringify(profile));
        expect(q.label.length).toBeLessThanOrEqual(48);
      }
    }
  });

  it('turns "when funded" into a fixed age two years either side of the middle life', () => {
    const profile = demo('dual-income-family');
    const summary = run(profile).summary;
    const median = Math.round(summary.headline.retirement_age.p50 as number);
    const quick = quickScenarios(profile, summary);
    expect(quick.find((q) => q.key === 'retire_earlier')?.patch).toEqual([{ op: 'set', path: '/retirement/trigger', value: { type: 'at_age', person: 0, age: median - 2 } }]);
    expect(quick.find((q) => q.key === 'retire_later')?.name).toBe(`Retire at ${median + 2}`);
    // Without a result there is no middle life to anchor on.
    expect(quickScenarios(profile, null).some((q) => q.key.startsWith('retire_'))).toBe(false);
  });

  it('moves a fixed retirement age directly and offers the home what-if only when a purchase is planned', () => {
    const fixed = applyPatch(demo('dual-income-family'), [{ op: 'set', path: '/retirement/trigger', value: { type: 'at_age', person: 0, age: 60 } }]).profile;
    expect(quickScenarios(fixed, null).find((q) => q.key === 'retire_earlier')?.patch).toEqual([{ op: 'set', path: '/retirement/trigger/age', value: 58 }]);
    expect(quickScenarios(demo('dual-income-family'), null).some((q) => q.key === 'skip_home')).toBe(true);
    expect(quickScenarios(demo('side-hustle-landlord'), null).some((q) => q.key === 'skip_home')).toBe(false);
  });

  it('scales retirement spending line by line and keeps the stock split when changing the mix', () => {
    const profile = demo('early-career-saver');
    const less = quickScenarios(profile, null).find((q) => q.key === 'spend_less');
    expect(less?.patch.length).toBe(profile.spending.items.filter((i) => i.in_retirement.type !== 'stops').length);
    const all = quickScenarios(profile, null).find((q) => q.key === 'all_stocks');
    const target = applyPatch(profile, all?.patch ?? []).profile.portfolio.target;
    expect(stockShare(target)).toBe(1);
    expect((target.us_stocks ?? 0) / (target.intl_stocks ?? 1)).toBeCloseTo(70 / 20, 1);
    expect(samePatch(all?.patch ?? [], JSON.parse(JSON.stringify(all?.patch ?? [])) as PatchOp[])).toBe(true);
  });
});

describe('describing a patch', () => {
  it('names the field, the thing it belongs to and the values before and after', () => {
    const profile = demo('dual-income-family');
    const lines = describePatch(profile, [{ op: 'set', path: '/household/people/0/social_security/claim_age', value: 70 }], null);
    expect(lines.map(lineText)).toEqual(['Priya: Social Security claim age: 67 → 70']);
    expect(pathPattern('/incomes/@priya_job/kind/salary')).toBe('/incomes/*/kind/salary');
  });

  it('merges the same change made to several people into one line', () => {
    const profile = demo('dual-income-family');
    const quick = quickScenarios(profile, null).find((q) => q.key === 'delay_social_security');
    const lines = describePatch(profile, quick?.patch ?? [], null);
    expect(lines).toHaveLength(1);
    expect(lineText(lines[0] as (typeof lines)[number])).toBe('Priya and Daniel: Social Security claim age: 67 → 70');
    expect(lines[0]?.ops).toEqual([0, 1]);
  });

  it('reads a rescaled budget as one line with totals', () => {
    const profile = demo('dual-income-family');
    const quick = quickScenarios(profile, null).find((q) => q.key === 'spend_less');
    const after = applyPatch(profile, quick?.patch ?? []).profile;
    const lines = describePatch(profile, quick?.patch ?? [], after);
    expect(lines).toHaveLength(1);
    expect(lines[0]?.label).toMatch(/^Budget in retirement, across \d+ budget lines$/);
    expect(lines[0]?.before).toMatch(/^\$\d+k a year$/);
    expect(lines[0]?.after).toMatch(/\(−10%\)$/);
    expect(lines[0]?.ops.length).toBe(quick?.patch.length);
  });

  it('says so when the plan already has what the scenario sets', () => {
    // After a scenario is made the plan, a copy of it changes nothing: no "68k → 68k (+0%)".
    const profile = demo('dual-income-family');
    const quick = quickScenarios(profile, null).find((q) => q.key === 'spend_less');
    const promoted = applyPatch(profile, quick?.patch ?? []).profile;
    const budget = describePatch(promoted, quick?.patch ?? [], applyPatch(promoted, quick?.patch ?? []).profile);
    expect(budget).toHaveLength(1);
    expect(budget[0]?.before).toBeNull();
    expect(budget[0]?.after).toMatch(/^\$\d+k a year in retirement, which the plan already has$/);
    const claim = describePatch(profile, [{ op: 'set', path: '/household/people/0/social_security/claim_age', value: 67 }], null);
    expect(claim.map(lineText)).toEqual(['Priya: Social Security claim age: 67, which the plan already has']);
  });

  it('puts tagged values, removals and additions into words', () => {
    const profile = demo('dual-income-family');
    const quick = quickScenarios(profile, run(profile).summary);
    const text = (key: string) => {
      const q = quick.find((x) => x.key === key);
      return describePatch(profile, q?.patch ?? [], applyPatch(profile, q?.patch ?? []).profile).map(lineText);
    };
    expect(text('retire_earlier')[0]).toMatch(/^Retirement: when savings cover spending at a 3\.5% withdrawal rate → at age \d+$/);
    expect(text('skip_home')).toEqual(['Event “Buy a house”: removed']);
    expect(text('crash_at_retirement')).toEqual(['Stress test: stocks fall 40% at retirement and take 5 years to recover']);
    expect(text('social_security_haircut')).toEqual(['Social Security benefits: paid in full → cut 21% from 2033']);
    expect(text('sixty_forty')[0]).toMatch(/^Investment mix: 55% US stocks, 20% .* → \d+% US stocks, \d+% international stocks, 40% bonds$/);
    expect(text('part_time')[0]).toMatch(/^New income: Part-time work after retiring, \$\d+k a year, from retirement until 5 years after retirement$/);
    expect(text('save_more')).toEqual(['Product manager: pay into the workplace plan: 10% → 15%', 'Registered nurse: pay into the workplace plan: 8% → 13%']);
  });

  it('falls back to the schema description, then to the field name', () => {
    const profile = demo('dual-income-family');
    expect(fieldLabel(profile, '/portfolio/emergency_months')).toBe('Emergency fund target in months of current spending');
    expect(fieldLabel(profile, '/nowhere/odd_field')).toBe('Odd field');
    expect(describeValue(0.035, '/retirement/trigger/withdrawal_rate', profile)).toBe('3.5%');
    expect(describeValue(null, '/retirement/trigger/min_age', profile)).toBe('none');
    expect(describeValue('2031-09', '/events/*/when/at', profile)).toBe('Sep 2031');
  });
});

describe('paired statistics', () => {
  it('counts flips in one denominator that adds up and shows every part that exists', () => {
    const f = (rescued: number, harmed: number, paths: number): Flips => ({ paths, rescued, harmed, both: paths - rescued - harmed, neither: 0, net: { value: (rescued - harmed) / paths, std_error: 0, lo: 0, hi: 0 }, withinNoise: false });
    expect(flipCounts(f(900, 100, 10_000))).toEqual({ rescued: 9, harmed: 1, unchanged: 90, of: 100 });
    expect(flipCounts(f(900, 20, 10_000))).toEqual({ rescued: 9, harmed: 0, unchanged: 91, of: 100 });
    expect(plain(flipSentence(f(900, 20, 10_000)))).toBe('In 9 of 100 lives this change is the difference between the plan falling short and holding; it is the other way round in only 20 of 10,000. In 91 it changes nothing about whether the plan holds.');
    expect(plain(flipSentence(f(100, 900, 10_000)))).toBe('In 9 of 100 lives this change is the difference between the plan holding and falling short; in 1 it is the other way round, and the change rescues the plan. In 90 it changes nothing about whether the plan holds.');
    expect(flipCounts(f(30, 2, 10_000))).toEqual({ rescued: 3, harmed: 0, unchanged: 997, of: 1000 });
    expect(flipCounts(f(4, 0, 10_000))).toEqual({ rescued: 4, harmed: 0, unchanged: 9996, of: 10_000 });
    expect(flipCounts(f(3, 1, 40))).toEqual({ rescued: 3, harmed: 1, unchanged: 36, of: 40 });
    expect(flipCounts(f(0, 0, 2000))).toEqual({ rescued: 0, harmed: 0, unchanged: 100, of: 100 });
  });

  it('writes the flip headline from the engine’s table, for a change that helps and one that hurts', () => {
    const profile = demo('dual-income-family');
    const quick = quickScenarios(profile, run(profile).summary);
    const helps = paired(profile, [{ op: 'set', path: '/assumptions/market/equity_premium_adjustment', value: 0.02 }]);
    const flips = flipsOf(helps.c) as Flips;
    expect(flips.rescued + flips.harmed + flips.both + flips.neither).toBe(PATHS);
    expect(flips.rescued).toBeGreaterThan(flips.harmed);
    const sentence = plain(flipSentence(flips));
    expect(sentence).toMatch(/^In \d+ of [\d,]+ lives this change is the difference between the plan falling short and holding/);
    expect(sentence).toMatch(/In \d+ it changes nothing about whether the plan holds\.$/);
    expect(flipNote(flips, 'Kinder markets')).toContain(`same ${PATHS} simulated futures`);

    const hurts = paired(profile, quick.find((q) => q.key === 'crash_at_retirement')?.patch ?? []);
    const worse = flipsOf(hurts.c) as Flips;
    expect(worse.harmed).toBeGreaterThan(worse.rescued);
    expect(plain(flipSentence(worse))).toMatch(worse.rescued === 0 ? /it rescues none/ : /it is the other way round/);
    expect(plain(flipSentence(worse))).toMatch(/between the plan holding and falling short/);

    const nothing: Flips = { ...flips, rescued: 0, harmed: 0 };
    expect(plain(flipSentence(nothing))).toMatch(/^This change does not alter whether the plan holds in any of 400 simulated lives/);
    expect(plain(provisionalSentence(helps.a, helps.b))).toMatch(/^So far the plan holds in \d+ of 100 lives with this change and \d+ without it\./);
  });

  it('bins per-life differences with zero as an edge and unchanged lives apart', () => {
    const dots = [-250, -120, -90, -10, 0, 0, 0, 40, 60, 180, 420, null];
    const m = binDeltas(dots, 8);
    expect(m.total).toBe(11);
    expect(m.unchanged).toBe(3);
    expect(m.bins.reduce((n, b) => n + b.count, 0)).toBe(8);
    expect(m.domain[0]).toBeLessThan(0);
    expect(m.domain[1]).toBeGreaterThan(0);
    for (const b of m.bins) expect(b.lo * b.hi, 'no bin straddles zero').toBeGreaterThanOrEqual(0);
    expect(binDeltas([0, 0, null], 8)).toMatchObject({ bins: [], unchanged: 2, total: 2 });
    // A tolerance folds changes too small to matter into "unchanged".
    expect(binDeltas([-200, 3, 900], 8, 500).unchanged).toBe(2);
    // The engine's exact share of unchanged lives decides how many dots are hollow (the ones nearest zero), so the plot agrees with the sentence above it.
    const forced = binDeltas([-300, -2, 0, 0, 1, 700, 900, 1200, 1500, 2000], 8, 500, 0.2);
    expect(forced.unchanged).toBe(2);
    expect(forced.bins.reduce((n, b) => n + b.count, 0)).toBe(8);
    expect(binDeltas([0, 0, 0, 50], 8, 0, 0.5).unchanged).toBe(2);
    // An extreme life is gathered into an open end bin, not allowed to stretch the axis.
    const skewed = binDeltas([...Array.from({ length: 99 }, (_, i) => i + 1), 1_000_000], 10);
    expect(skewed.bins.reduce((n, b) => n + b.count, 0)).toBe(100);
    expect(skewed.domain[1]).toBeLessThan(1000);
    expect(skewed.bins[skewed.bins.length - 1]?.open).toBe('above');
  });

  it('bins the engine’s 100 dots without losing any', () => {
    const profile = demo('early-career-saver');
    const quick = quickScenarios(profile, run(profile).summary);
    const { c } = paired(profile, quick.find((q) => q.key === 'retire_later')?.patch ?? []);
    for (const scalar of ['terminal_net_worth', 'lifetime_spending', 'fi_age'] as const) {
      const s = scalarOf(c, scalar);
      if (!s || s.dots.length === 0) continue;
      const m = binDeltas(s.dots, 20);
      expect(m.bins.reduce((n, b) => n + b.count, 0) + m.unchanged, scalar).toBe(s.dots.filter((d) => d !== null).length);
      if (s.equal !== null) expect(binDeltas(s.dots, 20, 500, s.equal).unchanged, `${scalar} hollow dots match the engine's share`).toBe(Math.round(s.equal * s.dots.filter((d) => d !== null).length));
      expect(deltaTakeaway(s)).toMatch(/^(With the change|The change leaves|No simulated life)/);
    }
  });

  it('ranks scenarios best first on each measure, with pending ones last', () => {
    const profile = demo('side-hustle-landlord');
    const quick = quickScenarios(profile, run(profile).summary);
    const made = ['retire_earlier', 'spend_more', 'crash_at_retirement'].map((key) => {
      const q = quick.find((x) => x.key === key);
      const r = paired(profile, q?.patch ?? []);
      return { id: key, name: q?.name ?? key, profile: r.after, summary: r.b, paired: r.c as Comparison | null };
    });
    const pending = { id: 'pending', name: 'Still running', profile, summary: null, paired: null };
    for (const metric of METRICS) {
      const rows = rankRows([pending, ...made], metric);
      expect(rows[rows.length - 1]?.id).toBe('pending');
      const values = rows.filter((r) => r.value !== null).map((r) => r.value as number);
      const sorted = [...values].sort((a, b) => (metric.higherIsBetter ? b - a : a - b));
      expect(values, metric.key).toEqual(sorted);
      for (const r of rows) if (r.value !== null && r.lo !== null && r.hi !== null) expect(r.withinNoise).toBe(r.lo <= 0 && r.hi >= 0);
      expect(rankTakeaway(rows, metric).length).toBeGreaterThan(20);
    }
    const success = rankRows(made, METRICS[0] as (typeof METRICS)[number]);
    expect(success.map((r) => r.id).sort()).toEqual(['crash_at_retirement', 'retire_earlier', 'spend_more']);
    expect(success.every((r) => !r.pending && r.paired === PATHS)).toBe(true);
    expect(rankTakeaway([], METRICS[0] as (typeof METRICS)[number])).toMatch(/still running/);
  });

  it('formats differences in the measure’s own terms', () => {
    expect(formatDelta(0.041, 'share')).toBe('+4 of 100');
    expect(formatDelta(-0.004, 'share')).toBe('−0.4 of 100');
    expect(formatDelta(0, 'share')).toBe('no change');
    expect(formatDelta(-1.84, 'age')).toBe('1.8 years earlier');
    expect(formatDelta(0.25, 'age')).toBe('3 months later');
    expect(formatDelta(212_000, 'dollars')).toBe('+$212k');
    expect(formatDelta(null, 'dollars')).toBe('n/a');
  });

  it('builds the measure table from the comparison, with a verdict per row', () => {
    const profile = demo('dual-income-family');
    const quick = quickScenarios(profile, run(profile).summary);
    const { c } = paired(profile, quick.find((q) => q.key === 'retire_later')?.patch ?? []);
    const rows = metricRows(c);
    expect(rows.slice(0, 3).map((r) => r.key)).toEqual(['success', 'depleted', 'fi']);
    expect(rows.some((r) => r.key === 'terminal_net_worth')).toBe(true);
    const table = metricTableData(c, 'Retire later');
    expect(table.rows).toHaveLength(rows.length);
    expect(table.rows.every((r) => typeof r.verdict === 'string')).toBe(true);
    expect(metricTakeaway(c)).toMatch(/^(With the change, things are|The change moves|None of these)/);
  });
});

describe('over a lifetime and life by life', () => {
  it('finds the stages a change moves lives between, summing to zero each year', () => {
    const profile = demo('dual-income-family');
    const quick = quickScenarios(profile, run(profile).summary);
    const { a, b } = paired(profile, quick.find((q) => q.key === 'retire_later')?.patch ?? []);
    const all = stageDiffs(a, b, 0);
    for (let y = 0; y < a.axis.ages.length; y++) expect(Math.abs(all.reduce((n, d) => n + (d.values[y] ?? 0), 0))).toBeLessThan(1e-6);
    const diffs = stageDiffs(a, b);
    expect(diffs.some((d) => d.status === 'working' && d.peak > 0), 'retiring later keeps more lives working').toBe(true);
    expect(diffs.every((d) => Math.abs(d.peak) >= 0.005)).toBe(true);
    expect(stageTakeaway(diffs, a)).toMatch(/^With the change, [\d.]+ (more|fewer) of 100 lives are /);
    expect(stageTakeaway([], a)).toMatch(/fewer than 1 life in 200/);
    expect(fanTakeaway(a, b, 'net_worth', 'Retire later')).toMatch(/median net worth/i);
    expect(fanTakeaway(a, a, 'spending', 'Same')).toMatch(/indistinguishable/);
  });

  it('picks lives to replay by joining the two runs on path index', () => {
    const profile = demo('dual-income-family');
    const quick = quickScenarios(profile, run(profile).summary);
    const { a, b, c } = paired(profile, [{ op: 'set', path: '/assumptions/market/equity_premium_adjustment', value: 0.02 }]);
    expect(quick.length).toBeGreaterThan(0);
    const lives = chooseLives(a, b);
    expect(lives.length).toBeGreaterThan(0);
    expect(new Set(lives.map((l) => l.pathIndex)).size).toBe(lives.length);
    const flips = flipsOf(c) as Flips;
    const rescued = lives.find((l) => l.why === 'rescued');
    if (flips.rescued > 0) {
      // Every life is in the per-life sample at this run size, so the counts must agree.
      expect(rescued?.among).toBe(flips.rescued);
      const index = a.scatter.paths.indexOf(rescued?.pathIndex ?? -1);
      const success = (s: Summary) => s.scatter.columns.find((col) => col.scalar === 'success')?.values[index];
      expect([success(a), success(b)]).toEqual([0, 1]);
    }
    expect(lives.some((l) => l.why === 'typical')).toBe(true);
    expect(chooseLives(a, a).map((l) => l.why)).toEqual(['typical']);
  });

  it('finds where two versions of one life part ways', () => {
    expect(divergence([100_000, 110_000, 120_000], [100_000, 110_400, 150_000])).toBe(2);
    expect(divergence([100_000, 110_000], [100_000, 110_000])).toBeNull();
    expect(divergence([0, 0, 500], [0, 0, 900])).toBeNull();
    expect(divergence([0, null, 5000], [0, 7000, 9000])).toBe(2);
    expect(divergence([1, 2, 3], [1, 2])).toBe(1);
  });
});
