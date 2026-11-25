// The Equity view's transforms and sentences on the startup demo's real output (the same
// wasm the app runs): every count that is printed adds up, the sentence and the picture
// count the same lives, and the words cope with a plan whose equity is left out or whose
// outcome classes are too small to summarise.
import { readFileSync } from 'node:fs';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { applyPatch, compare, exampleProfiles, initEngineSync } from '@/engine/api';
import { EQUITY_OUTCOMES, type Comparison, type EquityConditional, type EquitySummary, type PathTrace, type Summary } from '@/engine/output-types';
import { engineInput } from '@/engine/pool';
import * as wasm from '@/engine/pkg/firesight_wasm.js';
import type { PatchOp, Profile } from '@/engine/types';
import { equityDecisions, equityExerciseNow, equityLapse, equityTenderMax, equityTenderNone, equityZero, quickScenarios } from '../scenarios/quick';
import {
  PANELS,
  cashModel,
  companyScope,
  countsOf,
  decisionEnds,
  decisionRows,
  decisionTable,
  equityCounted,
  equityLives,
  fiAgeShift,
  firstExercise,
  hasEquity,
  nothingBand,
  outcomeCounts,
  outcomeEnds,
  paysModel,
  primaryCompany,
  primaryCounted,
  sidePaths,
  soldOrListed,
  successOf,
  usesOwnOdds,
  vestedOptions,
  visiblePanels,
  whenModel,
  withWithout,
  zeroPatch,
  type DecisionRow,
  type WithWithout,
} from './model';
import {
  DECISION_GERUND,
  OVERVIEW_EXCLUDED,
  MEAN_CARRIED_BY_FEW,
  byOutcomeTakeaway,
  cashTakeaway,
  companyScopeNote,
  decisionSentence,
  exerciseShortfall,
  meanSentence,
  points,
  equityCardSentence,
  equityHeadlineNote,
  outcomesTakeaway,
  paysCaption,
  paysTakeaway,
  whenTakeaway,
  withWithoutCaption,
  withWithoutDecimals,
  withWithoutPending,
  withWithoutTakeaway,
} from './text';

vi.setConfig({ testTimeout: 120_000, hookTimeout: 120_000 });

const PATHS = 2000;
let demo: Profile;
let others: Profile[] = [];
const runs = new Map<string, { summary: Summary; acc: Uint8Array; json: string }>();

function run(profile: Profile, paths = PATHS) {
  const { json, key } = engineInput(profile, 1);
  const id = `${key}:${paths}`;
  let hit = runs.get(id);
  if (!hit) {
    const engine = new wasm.Engine(json);
    try {
      const acc = engine.runChunk(0, paths);
      hit = { summary: JSON.parse(wasm.summarize(acc, json)) as Summary, acc, json };
    } finally {
      engine.free();
    }
    runs.set(id, hit);
  }
  return hit;
}

function paired(base: Profile, patch: PatchOp[], paths = PATHS): { a: Summary; b: Summary; c: Comparison; after: Profile } {
  const after = applyPatch(base, patch).profile;
  const a = run(base, paths);
  const b = run(after, paths);
  return { a: a.summary, b: b.summary, c: compare(a.acc, b.acc, a.json), after };
}

beforeAll(() => {
  initEngineSync(readFileSync(new URL('../../engine/pkg/firesight_wasm_bg.wasm', import.meta.url)));
  for (const e of exampleProfiles()) {
    const profile = applyPatch(e.profile, []).profile;
    if (e.slug === 'startup-employee') demo = profile;
    else others.push(profile);
  }
  others = others.slice(0, 3);
});

const equityOf = (summary: Summary) => summary.equity as EquitySummary;
const clean = (text: string) => {
  expect(text).not.toMatch(/NaN|undefined|null|Infinity|—/);
  expect(text.trim()).toMatch(/[.)]$/);
};

describe('which plan this is', () => {
  it('finds equity on the startup demo and on no other', () => {
    expect(hasEquity(demo)).toBe(true);
    expect(equityCounted(demo)).toBe(true);
    expect(usesOwnOdds(demo)).toBe(false);
    for (const p of others) {
      expect(hasEquity(p)).toBe(false);
      expect(equityCounted(p)).toBe(false);
      expect(run(p, 200).summary.equity).toBeUndefined();
      expect(visiblePanels(p, run(p, 200).summary)).toEqual([]);
      expect(equityCardSentence(run(p, 200).summary, false)).toBeNull();
      expect(equityHeadlineNote(run(p, 200).summary, false)).toBeNull();
    }
  });

  it('shows every panel when the equity is counted, and only the first two when it is not', () => {
    const summary = run(demo).summary;
    expect(visiblePanels(demo, summary)).toEqual(PANELS);
    expect(visiblePanels(demo, null)).toEqual([]);
    const zero = zeroPatch(demo) as PatchOp[];
    const { b, after } = paired(demo, zero);
    expect(equityCounted(after)).toBe(false);
    expect(hasEquity(after)).toBe(true);
    // The summary still has the block, and every dollar figure in it is zero: not a result.
    expect(b.equity?.nothing_rate.value).toBe(1);
    expect(b.equity?.net_proceeds_mean.value).toBe(0);
    expect(visiblePanels(after, b)).toEqual(['with_without', 'outcomes']);
    expect(equityCardSentence(b, false)).toBe(OVERVIEW_EXCLUDED);
    expect(equityHeadlineNote(b, false)).toBeNull();
    // The company's fate is still simulated and still described.
    clean(outcomesTakeaway(equityOf(b)));
    expect(b.equity?.outcomes.map((o) => o.count)).toEqual(run(demo).summary.equity?.outcomes.map((o) => o.count));
  });
});

describe('counts of lives', () => {
  it('rounds shares to whole counts that add up exactly', () => {
    expect(countsOf([0.4265, 0.0405, 0.364, 0.169])).toEqual([43, 4, 36, 17]);
    expect(countsOf([1, 1, 1])).toEqual([34, 33, 33]);
    expect(countsOf([0, 0])).toEqual([0, 0]);
    expect(countsOf([3, 1], 7).reduce((t, v) => t + v, 0)).toBe(7);
  });

  it('panel 2: the four outcomes add up to 100 and the sentence prints those counts', () => {
    const equity = equityOf(run(demo).summary);
    const { of, rows } = outcomeCounts(equity);
    expect(of).toBe(100);
    expect(rows.map((r) => r.outcome)).toEqual([...EQUITY_OUTCOMES]);
    expect(rows.reduce((t, r) => t + r.lives, 0)).toBe(100);
    expect(rows.reduce((t, r) => t + r.paths, 0)).toBe(PATHS);
    const text = outcomesTakeaway(equity);
    clean(text);
    const n = (o: string) => rows.find((r) => r.outcome === o)?.lives;
    expect(text).toBe(`In ${n('failed')} of 100 lives Northwind Robotics shuts down, in ${n('acquired')} it is acquired and in ${n('ipo')} it goes public. In ${n('still_private')} none of those happens within the life.`);
    const printed = [...text.matchAll(/\b[Ii]n (\d+)\b/g)].map((m) => Number(m[1]));
    expect(printed).toHaveLength(4);
    expect(printed.reduce((t, v) => t + v, 0)).toBe(100);
  });

  it('counts the lives themselves when fewer than 100 were simulated', () => {
    const equity = equityOf(run(demo, 40).summary);
    const { of, rows } = outcomeCounts(equity);
    expect(of).toBe(40);
    expect(rows.reduce((t, r) => t + r.lives, 0)).toBe(40);
    expect(outcomesTakeaway(equity)).toMatch(/of 40 lives/);
  });
});

describe('panel 3: what the equity pays', () => {
  it('partitions the dots it draws into three disjoint groups that add up', () => {
    const summary = run(demo).summary;
    const m = paysModel(summary, demo, 'net');
    expect(m.dots).toHaveLength(100);
    expect(m.loss + m.nothing + m.paid).toBe(100);
    const groups = m.dots.map((d) => m.groupOf(d));
    expect(groups.filter((g) => g === 'loss')).toHaveLength(m.loss);
    expect(groups.filter((g) => g === 'nothing')).toHaveLength(m.nothing);
    expect(groups.filter((g) => g === 'paid')).toHaveLength(m.paid);
    // Sorted dots: out of pocket, then nothing, then paid, with no interleaving.
    expect(groups.join(',')).toBe([...new Array<string>(m.loss).fill('loss'), ...new Array<string>(m.nothing).fill('nothing'), ...new Array<string>(m.paid).fill('paid')].join(','));
    // The band is small beside what is at stake, and catches the capital-loss lives (about +$1,000 with nothing received).
    expect(m.band).toBe(nothingBand(demo));
    expect(m.band).toBe(6000);
    const equity = equityOf(summary);
    expect(Math.abs(m.loss / 100 - equity.loss_rate.value)).toBeLessThan(0.05);
    // Lives that received nothing either lost money on an exercise or sit in the "nothing" stack.
    expect(m.nothing).toBeGreaterThan(10);
    expect(m.nothing / 100).toBeLessThanOrEqual(equity.nothing_rate.value + 0.03);
    expect(m.lossMedian as number).toBeLessThan(-m.band);
    expect(m.paidMedian as number).toBeGreaterThan(m.band);
  });

  it('writes the partition, the mean with its uncertainty and the exercises', () => {
    const summary = run(demo).summary;
    const equity = equityOf(summary);
    const m = paysModel(summary, demo, 'net');
    const text = paysTakeaway(m, equity);
    clean(text);
    // "No cash" is the gross count and nothing else is called nothing: the net groups are out of pocket, about even and ahead.
    const noCash = Math.round(equity.nothing_rate.value * 100);
    expect(text).toContain(`No cash ever comes back in ${noCash} of 100 lives. After the cost of exercising and the tax it caused, the household is out of pocket in ${m.loss} (median `);
    expect(text).toContain(`, ends within $6k of even in ${m.nothing} and comes out ahead in ${m.paid} (`);
    expect(text).not.toMatch(/comes to nothing|pays nothing/);
    expect(m.loss + m.nothing + m.paid).toBe(100);
    // The card on the Overview and the lead use the same count and name the lives that end out of pocket.
    expect(equityCardSentence(summary, true)).toContain(`The equity pays out no cash in ${noCash} of 100 lives, and in ${Math.round(equity.loss_rate.value * 100)} it costs more than it returns.`);
    expect(text).toMatch(/The average over all lives is \$[\d.]+[kM] \(give or take \$[\d.]+[kM]\), carried by the best few: half of all the gains come from \d+ of 2,000 lives\./);
    expect(text).toContain(`Options are exercised for cash in ${Math.round(equity.exercised_rate.value * 100)} of 100 lives, for `);
    clean(paysCaption(m));
    expect(paysCaption(m)).toMatch(/tax/);
    expect(paysCaption(m)).toContain('count as about even');
  });

  it('does not quote the mean with a "give or take" when a few lives carry it', () => {
    const equity = equityOf(run(demo).summary);
    expect(equity.net_proceeds_mean_half_from).toBeGreaterThanOrEqual(MEAN_CARRIED_BY_FEW);
    const few = meanSentence({ ...equity, net_proceeds_mean_half_from: 12 });
    expect(few).not.toMatch(/give or take/);
    expect(few).toMatch(/^The average over all lives is \$[\d.]+[kM], but half of all the gains come from the best 12 of 2,000 simulated lives, so it moves a lot from one run to the next\. Do not plan on it\.$/);
    // A small run of the demo really is carried by a handful of lives.
    const small = equityOf(run(demo, 200).summary);
    expect(small.net_proceeds_mean_half_from).toBeLessThan(MEAN_CARRIED_BY_FEW);
    expect(meanSentence(small)).not.toMatch(/give or take/);
    // No life with a gain: nothing to say about who carries it.
    expect(meanSentence({ ...equity, net_proceeds_mean_half_from: 0 })).toMatch(/\(give or take \$[\d.]+[kM]\)\.$/);
  });

  it('before cost and tax, splits lives into nothing and something', () => {
    const summary = run(demo).summary;
    const equity = equityOf(summary);
    const m = paysModel(summary, demo, 'gross');
    expect(m.band).toBe(0);
    expect(m.loss).toBe(0);
    expect(m.nothing + m.paid).toBe(100);
    expect(m.dots.filter((d) => d === 0)).toHaveLength(m.nothing);
    expect(Math.abs(m.nothing / 100 - equity.nothing_rate.value)).toBeLessThan(0.02);
    const text = paysTakeaway(m, equity);
    clean(text);
    const noCash = Math.round(equity.nothing_rate.value * 100);
    expect(text).toContain(`the equity pays out no cash in ${noCash} of 100 lives. In the other ${100 - noCash} it pays `);
  });
});

describe('panel 4: when', () => {
  it('reads both curves at the age by which half of the eventual sales and listings have happened', () => {
    const equity = equityOf(run(demo).summary);
    const m = whenModel(equity);
    expect(m.half).not.toBeNull();
    const i = m.half as number;
    const eventual = 1 - equity.liquidity_by_age.never;
    expect(m.liquidity[i]).toBeGreaterThanOrEqual(eventual / 2 - 1e-9);
    if (i > 0) expect(m.liquidity[i - 1]).toBeLessThan(eventual / 2);
    const x = (m.ages[i] as number) + 1;
    expect(whenTakeaway(m)).toBe(`By age ${x} the company has been sold or listed in ${Math.round((m.liquidity[i] as number) * 100)} of 100 lives and has failed in ${Math.round((m.failed[i] as number) * 100)}.`);
    // The demo's person is 31 and the stage resolves within about a decade in half of lives.
    expect(x).toBeGreaterThan(32);
    expect(x).toBeLessThan(45);
    // Curves are cumulative and the drawn range ends once they are flat.
    for (let k = 1; k < m.ages.length; k++) {
      expect(m.liquidity[k]).toBeGreaterThanOrEqual(m.liquidity[k - 1] as number);
      expect(m.failed[k]).toBeGreaterThanOrEqual(m.failed[k - 1] as number);
    }
    expect(m.liquidity[m.last]).toBeCloseTo(eventual, 2);
  });

  it('speaks of failure alone when the company is never sold or listed', () => {
    const doomed = applyPatch(demo, [{ op: 'set', path: '/private_equity/0/outcomes', value: [{ id: 'fails', kind: 'fails', probability: 1, years_from: 1, years_to: 6, multiple_low: 1, multiple_high: 1 }] }]);
    expect(doomed.errors).toEqual([]);
    const equity = equityOf(run(doomed.profile, 300).summary);
    const m = whenModel(equity);
    expect(m.half).toBeNull();
    expect(whenTakeaway(m)).toMatch(/^By age \d+ the company has failed in \d+ of 100 lives\. It is sold or listed in fewer than 1\.$/);
    expect(usesOwnOdds(doomed.profile)).toBe(true);
  });
});

describe('panel 5: the plan, by what happens to the company', () => {
  it('compares the lives where it fails with its best outcome that has enough lives', () => {
    const equity = equityOf(run(demo).summary);
    const { low, high } = outcomeEnds(equity.by_outcome);
    expect(low?.outcome).toBe('failed');
    expect(high?.outcome).toBe('ipo');
    const text = byOutcomeTakeaway(equity, true);
    clean(text);
    expect(text.startsWith(`If Northwind Robotics fails the plan holds in ${Math.round((low?.success_rate.value as number) * 100)} of 100 of those lives; if it goes public, in ${Math.round((high?.success_rate.value as number) * 100)}.`)).toBe(true);
    // A payout mostly buys an earlier retirement here, and the sentence says when.
    expect(text).toMatch(/work becomes optional at [\d.]+ in the middle life if it fails and at [\d.]+ if it goes public\./);
  });

  const withEnough = (rows: EquityConditional[], enough: Partial<Record<string, boolean>>) => rows.map((r) => (enough[r.outcome] === false ? { ...r, enough: false, paths: 12 } : r));

  it('falls back to the acquired class, and says so when a class is too small', () => {
    const equity = equityOf(run(demo).summary);
    const noIpo = { ...equity, by_outcome: withEnough(equity.by_outcome, { ipo: false }) };
    expect(outcomeEnds(noIpo.by_outcome).high?.outcome).toBe('acquired');
    expect(byOutcomeTakeaway(noIpo, true)).toMatch(/; if it is acquired, in \d+\./);
    const noFail = { ...equity, by_outcome: withEnough(equity.by_outcome, { failed: false }) };
    expect(outcomeEnds(noFail.by_outcome).low).toBeNull();
    expect(byOutcomeTakeaway(noFail, true)).toMatch(/^If Northwind Robotics goes public the plan holds in \d+ of 100 of those lives\. Too few lives have it fail/);
    const none = { ...equity, by_outcome: withEnough(equity.by_outcome, { failed: false, acquired: false, ipo: false }) };
    expect(byOutcomeTakeaway(none, true)).toMatch(/^Too few simulated lives/);
    for (const e of [noIpo, noFail, none]) clean(byOutcomeTakeaway(e, false));
  });

  it('the pinned note and the Overview card, with and without enough lives in a class', () => {
    const summary = run(demo).summary;
    const equity = equityOf(summary);
    const failed = equity.by_outcome.find((r) => r.outcome === 'failed') as EquityConditional;
    const x = Math.round((failed.success_rate.value as number) * 100);
    expect(equityHeadlineNote(summary, true)).toBe(`and in ${x} of 100 of the lives where Northwind Robotics fails`);
    const pooled = soldOrListed(equity.by_outcome);
    const acquired = equity.by_outcome.find((r) => r.outcome === 'acquired') as EquityConditional;
    const ipo = equity.by_outcome.find((r) => r.outcome === 'ipo') as EquityConditional;
    expect(pooled?.paths).toBe(acquired.paths + ipo.paths);
    expect(pooled?.success).toBeCloseTo(((acquired.success_rate.value as number) * acquired.paths + (ipo.success_rate.value as number) * ipo.paths) / (acquired.paths + ipo.paths), 9);
    const nothing = Math.round(equity.nothing_rate.value * 100);
    const cash = `The equity pays out no cash in ${nothing} of 100 lives, and in ${Math.round(equity.loss_rate.value * 100)} it costs more than it returns.`;
    expect(equityCardSentence(summary, true)).toBe(`If Northwind Robotics fails the plan holds in ${x} of 100 of those lives; if it is sold or listed, in ${Math.round((pooled?.success as number) * 100)}. ${cash}`);
    // The company the outcomes describe is itself left out of the plan: its clauses go, the dollars stay.
    expect(equityCardSentence(summary, true, false)).toBe(cash);
    // No life out of pocket: the clause about cost goes.
    expect(equityCardSentence({ ...summary, equity: { ...equity, loss_rate: { ...equity.loss_rate, value: 0 } } }, true, false)).toBe(`The equity pays out no cash in ${nothing} of 100 lives.`);

    // Too few lives where it fails: the clause goes, here and under the pinned figure.
    const noFail: Summary = { ...summary, equity: { ...equity, by_outcome: withEnough(equity.by_outcome, { failed: false }) } };
    expect(equityHeadlineNote(noFail, true)).toBeNull();
    expect(equityCardSentence(noFail, true)).toBe(`If Northwind Robotics is sold or listed, the plan holds in ${Math.round((pooled?.success as number) * 100)} of 100 of those lives. ${cash}`);
    // Only the public class is too small: the pooled clause is the acquired class alone.
    const noIpo: Summary = { ...summary, equity: { ...equity, by_outcome: withEnough(equity.by_outcome, { ipo: false }) } };
    expect(equityCardSentence(noIpo, true)).toContain(`if it is sold or listed, in ${Math.round((acquired.success_rate.value as number) * 100)}.`);
    const none: Summary = { ...summary, equity: { ...equity, by_outcome: withEnough(equity.by_outcome, { failed: false, acquired: false, ipo: false }) } };
    expect(equityCardSentence(none, true)).toBe(cash);
    // A tiny run has no class of 30 lives at all.
    const tiny = run(demo, 20).summary;
    expect(equityHeadlineNote(tiny, true)).toBeNull();
    expect(equityCardSentence(tiny, true)).toMatch(/^The equity pays out no cash in \d+ of 20 lives(, and in \d+ it costs more than it returns)?\.$/);
  });
});

describe('panel 6 and 7', () => {
  it('reads the lifetime means from the per-life scalars and the paper value from its series', () => {
    const summary = run(demo).summary;
    const m = cashModel(summary);
    const gross = summary.distributions.find((d) => d.scalar === 'equity_gross_proceeds')?.mean as number;
    expect(m.meanIn).toBe(gross);
    expect(m.meanOut).toBe(summary.distributions.find((d) => d.scalar === 'equity_exercise_cost')?.mean);
    expect(m.paperNow).toBe(summary.composition.find((c) => c.series === 'equity_paper_value')?.values[0]);
    expect(m.paperNow).toBeGreaterThan(500_000);
    expect(m.first).toBe(summary.axis.first_year_months < 12 ? 1 : 0);
    expect(m.last).toBeGreaterThan(m.first + 5);
    expect(m.last).toBeLessThan(summary.axis.ages.length);
    expect(Math.max(...m.proceeds)).toBeGreaterThan(Math.max(...m.exercise));
    clean(cashTakeaway(m));
    expect(cashTakeaway(m)).toMatch(/^Averaged over all lives, the equity brings in \$[\d.]+[kM] and costs \$[\d.]+[kM] to exercise; on paper the vested part is worth \$[\d.]+[kM] at the end of \d{4}\.$/);
  });

  it('offers the three lives the engine picks, each a different path', () => {
    const lives = equityLives(run(demo).summary);
    expect(lives.map((l) => l.label)).toEqual(['equity_failed', 'equity_paid_median', 'equity_paid_p90']);
    expect(new Set(lives.map((l) => l.path)).size).toBe(3);
    for (const l of lives) expect(l.path).toBeLessThan(PATHS);
  });
});

describe('side runs', () => {
  it('uses the lives of the plan\'s own run, between 500 and 10,000', () => {
    expect(sidePaths(10_000)).toBe(10_000);
    expect(sidePaths(250_000)).toBe(10_000);
    expect(sidePaths(1200)).toBe(1200);
    expect(sidePaths(100)).toBe(500);
    expect(sidePaths(0)).toBe(2000);
  });

  it('the standing comparison is the "Count the equity at zero" what-if, patch for patch', () => {
    const quick = quickScenarios(demo, run(demo).summary).find((q) => q.key === 'equity_zero');
    expect(zeroPatch(demo)).toEqual(quick?.patch);
    expect(zeroPatch(demo)).toEqual([{ op: 'set', path: '/private_equity/@northwind/included', value: false }]);
    expect(equityZero(others[0] as Profile)).toBeNull();
    expect(zeroPatch(others[0] as Profile)).toBeNull();
  });

  it('panel 1: states what the equity adds, on the live figure, with flips that match the paired table', () => {
    const live = run(demo, 4000).summary;
    const { c } = paired(demo, zeroPatch(demo) as PatchOp[]);
    const w = withWithout(live, c);
    expect(w).not.toBeNull();
    if (!w) return;
    const s = successOf(c);
    expect(w.paths).toBe(PATHS);
    // As modelled is the live run's own figure, never the side run's.
    expect(w.modelled.value).toBe(live.headline.success_rate.value);
    expect(w.difference.value).toBeCloseTo(-(s?.difference.value as number), 12);
    expect(w.difference.lo).toBeLessThanOrEqual(w.difference.value);
    expect(w.difference.hi).toBeGreaterThanOrEqual(w.difference.value);
    expect(w.zero.value).toBeCloseTo(w.modelled.value - w.difference.value, 9);
    expect(w.zero.lo).toBeLessThan(w.zero.value);
    expect(w.rescued).toBeCloseTo((s?.a_only as number) / PATHS, 12);
    expect(w.harmed).toBeCloseTo((s?.b_only as number) / PATHS, 12);
    // Of the lives the plan holds in only with the equity, those where the company failed: the engine's own count.
    const failedClass = c.by_equity_outcome?.find((o) => o.outcome === 'failed');
    expect(w.rescuedFailed).toBeCloseTo((failedClass?.success.a_only as number) / PATHS, 12);
    expect(w.rescuedFailed as number).toBeLessThanOrEqual(w.rescued);
    // The shift in the middle retirement age is stated the same way round as the difference: with the equity minus without.
    expect(w.fiShift).toBeCloseTo(-(fiAgeShift(c) as number), 12);
    const text = withWithoutTakeaway(w, true);
    clean(text);
    const d = withWithoutDecimals(w);
    const shown = (v: number) => (d === 1 ? (v * 100).toFixed(1) : String(Math.round(v * 100)));
    expect(text).toContain(`The plan holds in ${shown(w.modelled.value)} of 100 lives with the equity as modelled, and in about ${shown(w.zero.value)} with it left out (difference `);
    expect(text).toContain(`measured on 2,000 of the same lives).`);
    expect(text).not.toMatch(/The equity is the difference between holding and not|pays nothing\b(?! \()/);
    expect(text).toMatch(w.withinNoise ? /makes no measurable difference to whether the plan holds\./ : /The result differs in \d+ of 100 lives: the plan holds only with the equity in \d+/);
    clean(withWithoutPending(live, true));
    expect(withWithoutPending(live, false)).toBe(`The plan holds in ${Math.round(live.headline.success_rate.value * 100)} of 100 lives with the equity as modelled.`);
  });
});

describe('panel 1 in words', () => {
  const base: WithWithout = {
    paths: 2000,
    modelled: { value: 0.886, lo: 0.87, hi: 0.9 },
    zero: { value: 0.882, lo: 0.866, hi: 0.896 },
    difference: { value: 0.004, lo: 0.001, hi: 0.007 },
    withinNoise: false,
    rescued: 0.041,
    harmed: 0.0285,
    rescuedFailed: 0.011,
    fiShift: 1.25,
  };

  it('never credits the payout with lives in which the company failed', () => {
    const text = withWithoutTakeaway(base, true);
    // 89 and 88 with a difference of 0.4 would not add up: both figures get a decimal.
    expect(withWithoutDecimals(base)).toBe(1);
    expect(text).toContain('The plan holds in 88.6 of 100 lives with the equity as modelled, and in about 88.2 with it left out (difference 0.4, 95% interval 0.1 to 0.7, measured on 2,000 of the same lives).');
    expect(text).toContain('The result differs in 7 of 100 lives: the plan holds only with the equity in 4 (in 1 of those the company failed and paid nothing: what made the difference there is the money spent on exercising and a later retirement, not a payout) and only without it in 3.');
    // A plan with a fixed retirement date has no later retirement to point to.
    expect(withWithoutTakeaway(base, false)).toContain('(in 1 of those the company failed and paid nothing, so a payout is not what made the difference there)');
    expect(withWithoutTakeaway({ ...base, rescuedFailed: 0 }, true)).toContain('only with the equity in 4 and only without it in 3.');
    expect(withWithoutTakeaway({ ...base, rescuedFailed: null }, true)).toContain('only with the equity in 4 and only without it in 3.');
  });

  it('leads with the harm when counting the equity measurably lowers success', () => {
    const hurts: WithWithout = { ...base, modelled: { value: 0.878, lo: 0.86, hi: 0.89 }, zero: { value: 0.882, lo: 0.866, hi: 0.896 }, difference: { value: -0.004, lo: -0.007, hi: -0.001 }, rescued: 0.012, harmed: 0.016 };
    const text = withWithoutTakeaway(hurts, true);
    expect(text).toContain('(difference −0.4, 95% interval −0.7 to −0.1,');
    expect(text).toContain('Counting the equity, the plan holds in fewer lives: only without it in 2 of 100, and only with it in 1.');
    expect(text).not.toMatch(/The result differs in/);
    // Both sides round to 88: a decimal tells them apart.
    expect(text).toContain('holds in 87.8 of 100 lives with the equity as modelled, and in about 88.2 with it left out');
  });

  it('prints whole counts when they agree with the difference, and says so when it is noise', () => {
    const clear: WithWithout = { ...base, modelled: { value: 0.9, lo: 0.88, hi: 0.92 }, zero: { value: 0.84, lo: 0.82, hi: 0.86 }, difference: { value: 0.06, lo: 0.04, hi: 0.08 } };
    expect(withWithoutDecimals(clear)).toBe(0);
    expect(withWithoutTakeaway(clear)).toContain('The plan holds in 90 of 100 lives with the equity as modelled, and in about 84 with it left out (difference 6.0,');
    const noise = withWithoutTakeaway({ ...base, withinNoise: true, difference: { value: 0.004, lo: -0.002, hi: 0.01 } });
    expect(noise).toContain('Counting the equity or leaving it out makes no measurable difference to whether the plan holds. The result still differs in some lives: the plan holds only with the equity in 4 of 100, and only without it in 3.');
  });

  it('says in the caption that a plan retiring when funded has its retirement date moved', () => {
    expect(withWithoutCaption(false)).not.toMatch(/retires when the savings are enough/);
    const funded = withWithoutCaption(true, 1.25);
    expect(funded).toContain('a payout brings it forward, and money spent on exercising options that end up worthless pushes it back. A later retirement by itself makes a plan hold more often and an earlier one less often');
    expect(funded).toMatch(/Here work becomes optional 15 months later in the middle life with the equity than without\.$/);
    expect(withWithoutCaption(true, -2.5)).toMatch(/2\.5 years earlier in the middle life/);
    expect(withWithoutCaption(true, null)).toMatch(/that timing, not the money\.$/);
    for (const text of [withWithoutCaption(false), funded]) clean(text);
  });

  it('never prints a signed zero', () => {
    expect(points(-0.0004)).toBe('0.0');
    expect(points(0.0004, true)).toBe('0.0');
    expect(points(-0.0004, true)).toBe('0.0');
    expect(points(0.009, true)).toBe('+0.9');
    expect(points(-0.026, true)).toBe('−2.6');
    expect(points(0)).toBe('0.0');
  });
});

describe('several companies', () => {
  const second = (included: boolean, shares: number): PatchOp[] => [
    { op: 'append', path: '/private_equity', value: { id: 'acme', name: 'Acme Bio', stage: 'seed', preferred_price: 2, included, holdings: [{ id: 'shares', name: 'Shares', kind: { type: 'shares', basis_per_share: 0.1 }, shares }] } },
  ];

  it('says nothing for one counted company, and names the companies when outcomes and dollars part ways', () => {
    expect(companyScope(demo)).toBeNull();
    expect(companyScopeNote(companyScope(demo))).toBeNull();
    expect(primaryCompany(demo)?.name).toBe('Northwind Robotics');
    expect(primaryCounted(demo)).toBe(true);

    const two = applyPatch(demo, second(true, 1000)).profile;
    expect(companyScope(two)).toEqual({ primary: 'Northwind Robotics', primaryCounted: true, counted: ['Northwind Robotics', 'Acme Bio'] });
    expect(companyScopeNote(companyScope(two))).toBe(
      'What happens on this page (shut down, acquired, public) is what happens to Northwind Robotics, the largest position. The dollar figures total Northwind Robotics and Acme Bio, so a life where Northwind Robotics fails can still include a payout from Acme Bio.',
    );
    // The engine describes the same company.
    expect(run(two, 200).summary.equity?.company).toBe('Northwind Robotics');
  });

  it('does not describe a company as part of the plan when only another one is counted', () => {
    const profile = applyPatch(demo, [...second(true, 1000), { op: 'set', path: '/private_equity/0/included', value: false }]).profile;
    expect(equityCounted(profile)).toBe(true);
    expect(primaryCompany(profile)?.name).toBe('Northwind Robotics');
    expect(primaryCounted(profile)).toBe(false);
    const summary = run(profile, 400).summary;
    // The engine still follows the largest position, counted or not: the page has to say so.
    expect(summary.equity?.company).toBe('Northwind Robotics');
    const note = companyScopeNote(companyScope(profile)) as string;
    expect(note).toContain('is what happens to Northwind Robotics, the largest position, which is not counted in the plan. The dollar figures are for Acme Bio only');
    clean(note);
    // Nothing outside the view says "the lives where Northwind fails" about a plan it is not in.
    expect(equityHeadlineNote(summary, equityCounted(profile) && primaryCounted(profile))).toBeNull();
    expect(equityCardSentence(summary, true, primaryCounted(profile))).toMatch(/^The equity pays out no cash in \d+ of 100 lives/);
    // The larger position is the one followed, whichever comes first in the plan.
    const bigger = applyPatch(demo, second(true, 10_000_000)).profile;
    expect(primaryCompany(bigger)?.name).toBe('Acme Bio');
    expect(run(bigger, 200).summary.equity?.company).toBe('Acme Bio');
  });

  it('shows the Overview card and the tab for the same plans', () => {
    const empty = applyPatch(demo, [{ op: 'set', path: '/private_equity/0/holdings', value: [] }]).profile;
    // The engine still reports an equity block for a company with nothing in it; the view and the card are both hidden.
    expect(run(empty, 200).summary.equity).toBeDefined();
    expect(hasEquity(empty)).toBe(false);
    expect(equityCounted(empty)).toBe(false);
  });
});

describe('what "exercise now" really exercises', () => {
  it('reads the first month of a traced life, and says how far short of the wish it falls', () => {
    const q = equityExerciseNow(demo);
    const after = applyPatch(demo, q?.patch ?? []).profile;
    const engine = new wasm.Engine(engineInput(after, 1).json);
    try {
      const done = firstExercise(JSON.parse(engine.trace(0)) as PathTrace);
      // Another life exercises the same in its first month: nothing random has happened yet.
      expect(firstExercise(JSON.parse(engine.trace(7)) as PathTrace)).toEqual(done);
      const vested = vestedOptions(demo.private_equity);
      expect(vested).toBe(58_000);
      expect(done.options).toBeGreaterThan(0);
      expect(done.options).toBeLessThan(vested);
      expect(done.cost).toBeLessThan(107_000);
      // The engine's own warning says the same thing.
      expect(applyPatch(demo, q?.patch ?? []).warnings.map((w) => w.code)).toContain('exercise_unaffordable');
      const text = exerciseShortfall(done, vested, 107_000) as string;
      expect(text).toMatch(/^The plan can pay for [\d,]+ of the 58,000 vested options: \$[\d.]+k of the \$107k, plus the tax held back, is what savings above the emergency fund cover\./);
      expect(text).toContain('are not exercised later');
      clean(text);
    } finally {
      engine.free();
    }
    // The plan as it stands exercises nothing in its first month.
    const plain = new wasm.Engine(engineInput(demo, 1).json);
    try {
      expect(firstExercise(JSON.parse(plain.trace(0)) as PathTrace)).toEqual({ options: 0, cost: 0 });
    } finally {
      plain.free();
    }
  });

  it('says nothing when everything is exercised, and says so when nothing can be', () => {
    expect(exerciseShortfall({ options: 58_000, cost: 107_000 }, 58_000, 107_000)).toBeNull();
    expect(exerciseShortfall({ options: 0, cost: 0 }, 0, 0)).toBeNull();
    expect(exerciseShortfall({ options: 0, cost: 0 }, 58_000, 107_000)).toMatch(/^The plan cannot pay for any of it/);
    expect(exerciseShortfall({ options: 34_828, cost: 52_242 }, 58_000, 107_000)).toBe(
      'The plan can pay for 34,828 of the 58,000 vested options: $52k of the $107k, plus the tax held back, is what savings above the emergency fund cover. The comparison is for that smaller exercise. The other 23,172 are not exercised later: they wait for a sale or IPO, or for the deadline.',
    );
  });
});

describe('panel 8: decisions', () => {
  it('lists exactly the decisions that would change something', () => {
    const builders = [equityExerciseNow, equityTenderMax, equityTenderNone, equityLapse];
    expect(decisionRows(demo).map((q) => q.key)).toEqual(builders.map((b) => b(demo)?.key).filter(Boolean));
    expect(decisionRows(demo).map((q) => q.key)).toEqual(['equity_exercise_now', 'equity_tender_max', 'equity_tender_none', 'equity_lapse']);
    expect(decisionRows(demo)).toEqual(equityDecisions(demo));
    for (const q of decisionRows(demo)) {
      expect(DECISION_GERUND[q.key]).toBeTruthy();
      expect(applyPatch(demo, q.patch).errors).toEqual([]);
    }
    // Once a decision is the plan, its row goes: nothing left to change.
    const exercising = applyPatch(demo, [{ op: 'set', path: '/private_equity/0/policy/exercise', value: 'vested_now' }]).profile;
    expect(decisionRows(exercising).map((q) => q.key)).not.toContain('equity_exercise_now');
    const maxed = applyPatch(demo, [{ op: 'set', path: '/private_equity/0/policy/tender_sell_fraction', value: 1 }]).profile;
    expect(decisionRows(maxed).map((q) => q.key)).toEqual(['equity_exercise_now', 'equity_tender_none', 'equity_lapse']);
    const never = applyPatch(demo, [{ op: 'set', path: '/private_equity/0/tenders/annual_probability', value: 0 }]).profile;
    expect(decisionRows(never).map((q) => q.key)).toEqual(['equity_exercise_now', 'equity_lapse']);
    for (const p of others) expect(decisionRows(p)).toEqual([]);
    // A plan whose equity is left out has no decisions to compare, and no decisions panel.
    const out = applyPatch(demo, zeroPatch(demo) as PatchOp[]).profile;
    expect(decisionRows(out)).toEqual([]);
  });

  it('turns a real by-outcome comparison into the table and the sentence', () => {
    const q = equityExerciseNow(demo);
    const { c } = paired(demo, q?.patch ?? []);
    expect(c.by_equity_outcome?.map((r) => r.outcome)).toEqual([...EQUITY_OUTCOMES]);
    const rows = decisionTable(c);
    expect(rows.reduce((t, r) => t + r.paths, 0)).toBe(c.paths);
    for (const r of rows) {
      expect(r.enough).toBe(r.paths >= 30);
      if (r.paths > 0) expect(r.holds?.lo as number).toBeLessThanOrEqual(r.holds?.value as number);
    }
    const { low, high } = decisionEnds(rows);
    expect(low?.outcome).toBe('failed');
    expect(high?.outcome).toBe('ipo');
    const s = successOf(c);
    const text = decisionSentence('Exercising now', 'Northwind Robotics', rows, s ? { value: s.difference.value, withinNoise: s.within_noise } : null);
    clean(text);
    expect(text).toMatch(/^Exercising now leaves the household (\$[\d.]+[kM] (poorer|richer)|no better or worse off) in the lives where Northwind Robotics fails and (\$[\d.]+[kM] (poorer|richer)|no better or worse off) where it goes public; (the plan holds in [\d.]+ (more|fewer) of 100 lives overall|no measurable difference to whether the plan holds overall)\.$/);
    // Exercising early costs money where the company fails.
    expect(low?.wealth as number).toBeLessThan(0);
  });

  it('writes the sentence from a fixture, including classes too small to use', () => {
    const { c } = paired(demo, equityExerciseNow(demo)?.patch ?? []);
    const by = c.by_equity_outcome ?? [];
    // Every class's mean change and change in "plan holds" is put inside noise unless a test says otherwise.
    const quiet = { value: 0, std_error: 1, lo: -1, hi: 1 };
    const fixture = (wealth: Record<string, number>, paths: Record<string, number>, moved: Record<string, { mean?: [number, number, number]; holds?: [number, number, number] }> = {}): Comparison => ({
      ...c,
      by_equity_outcome: by.map((r) => {
        const m = moved[r.outcome] ?? {};
        const est = (v: [number, number, number] | undefined) => (v ? { value: v[0], std_error: 1, lo: v[1], hi: v[2] } : quiet);
        return {
          ...r,
          paths: paths[r.outcome] ?? r.paths,
          success: { ...r.success, difference: est(m.holds), within_noise: !m.holds },
          terminal_net_worth: { ...r.terminal_net_worth, mean_delta: est(m.mean), quantiles: r.terminal_net_worth.quantiles.map((v, i) => (c.quantile_levels[i] === 0.5 ? (wealth[r.outcome] ?? (m.mean || m.holds ? 0 : v)) : v)) },
        };
      }),
    });
    const rows = decisionTable(fixture({ failed: -42_000, ipo: 310_000 }, {}));
    expect(decisionSentence('Exercising now', 'Acme', rows, { value: 0.021, withinNoise: false })).toBe('Exercising now leaves the household $42k poorer in the lives where Acme fails and $310k richer where it goes public; the plan holds in 2 more of 100 lives overall.');
    expect(decisionSentence('Exercising now', 'Acme', rows, { value: -0.034, withinNoise: false })).toMatch(/; the plan holds in 3 fewer of 100 lives overall\.$/);
    expect(decisionSentence('Exercising now', 'Acme', rows, { value: 0.004, withinNoise: true })).toMatch(/; no measurable difference to whether the plan holds overall\.$/);
    // Under half a life in 100 is not written as a fraction of a life.
    expect(decisionSentence('Exercising now', 'Acme', rows, { value: 0.003, withinNoise: false })).toMatch(/; the plan holds in about 1 more life in 300 overall\.$/);
    expect(decisionSentence('Exercising now', 'Acme', rows, { value: -0.0048, withinNoise: false })).toMatch(/; the plan holds in about 1 fewer life in 200 overall\.$/);
    expect(decisionSentence('Exercising now', 'Acme', rows, { value: 0.003, withinNoise: false })).not.toMatch(/0\.3 more of 100/);
    // Too few public lives: the acquired class stands in; too few of everything: say so.
    const noIpo = decisionTable(fixture({ failed: -42_000, acquired: 90_000 }, { ipo: 12 }));
    expect(decisionSentence('Letting options lapse', 'Acme', noIpo, null)).toBe('Letting options lapse leaves the household $42k poorer in the lives where Acme fails and $90k richer where it is acquired.');
    const none = decisionTable(fixture({}, { failed: 3, still_private: 0, acquired: 5, ipo: 2 }));
    expect(decisionEnds(none)).toEqual({ low: null, high: null });
    expect(decisionSentence('Selling nothing in tenders', 'Acme', none, { value: 0, withinNoise: true })).toBe('Selling nothing in tenders: no measurable difference to whether the plan holds overall. Too few lives have any one outcome to say more.');
    expect(decisionTable({ ...c, by_equity_outcome: undefined })).toEqual([]);
    // A change of under $500 in the middle life is not a change.
    const flat = decisionTable(fixture({ failed: 120, ipo: -300, acquired: 0, still_private: 0 }, {}));
    // The middle life of another class moved (options kept where there is no outcome): that leads instead.
    const kept = decisionTable(fixture({ failed: 0, ipo: 0, acquired: 0, still_private: 192_000 }, {}));
    expect(decisionSentence('Letting options lapse', 'Acme', kept, null)).toBe(
      'Letting options lapse changes nothing in the middle life, whether Acme fails or goes public, but it does move the lives where the occasion for it arises: the middle life ends $192k richer where Acme has no outcome within the life.',
    );
    expect(decisionSentence('Selling the maximum in tenders', 'Acme', flat, null)).toBe('Selling the maximum in tenders changes nothing in the middle life, whether Acme fails or goes public: it matters only in the fewer lives where the occasion for it arises.');
    // The same flat middle, but the lives it touches moved: the sentence must not say "changes nothing" and stop.
    const lapse = decisionTable(fixture({ failed: 0, ipo: 0, acquired: 0, still_private: 0 }, {}, { failed: { holds: [-0.009, -0.014, -0.004] }, acquired: { holds: [-0.008, -0.015, -0.001], mean: [-847_000, -1_200_000, -500_000] }, ipo: { holds: [-0.026, -0.04, -0.012], mean: [-4_200_000, -5_000_000, -3_400_000] } }));
    const lapsing = decisionSentence('Letting options lapse', 'Acme', lapse, { value: -0.011, withinNoise: false });
    expect(lapsing).toBe(
      'Letting options lapse changes nothing in the middle life, whether Acme fails or goes public, but it does move the lives where the occasion for it arises: the plan holds in 2.6 fewer of 100 of the lives where Acme goes public; on average the household ends $4.2M poorer where Acme goes public (95% interval $3.4M to $5M), an average carried by the few lives it touches. The plan holds in 1 fewer of 100 lives overall.',
    );
    expect(lapse.find((r: DecisionRow) => r.outcome === 'ipo')?.mean).toEqual({ value: -4_200_000, lo: -5_000_000, hi: -3_400_000, withinNoise: false });
    // A tender decision whose medians are all zero but whose mean moved where the company goes public.
    const tender = decisionTable(fixture({ failed: 0, ipo: 0, acquired: 0, still_private: 0 }, {}, { ipo: { mean: [-115_000, -194_000, -36_000] } }));
    const selling = decisionSentence('Selling the maximum in tenders', 'Acme', tender, { value: 0.001, withinNoise: true }, { occasion: 'A tender offer pays something in about 12 of 100 lives as the plan stands.' });
    expect(selling).toBe(
      'Selling the maximum in tenders changes nothing in the middle life, whether Acme fails or goes public, but it does move the lives where the occasion for it arises: on average the household ends $115k poorer where Acme goes public (95% interval $36k to $194k), an average carried by the few lives it touches. No measurable difference to whether the plan holds overall. A tender offer pays something in about 12 of 100 lives as the plan stands.',
    );
    // What a decision does to the retirement date is said, and when that date alone would explain the change in "plan holds" the sentence says so.
    const later = decisionSentence('Exercising now', 'Acme', rows, { value: 0.021, withinNoise: false }, { fiShift: 0.25, funded: true });
    expect(later).toMatch(/overall\. Work becomes optional 3 months later in the middle life\. A later retirement by itself makes a plan hold more often, so the change in whether the plan holds reflects that date as much as the shares\.$/);
    const earlier = decisionSentence('Letting options lapse', 'Acme', rows, { value: -0.011, withinNoise: false }, { fiShift: -0.42, funded: true });
    expect(earlier).toMatch(/Work becomes optional 5 months earlier in the middle life\. An earlier retirement by itself makes a plan hold less often/);
    // A fixed retirement date, or a change inside noise, gets the shift without the caution.
    expect(decisionSentence('Exercising now', 'Acme', rows, { value: 0.021, withinNoise: false }, { fiShift: 0.25, funded: false })).toMatch(/overall\. Work becomes optional 3 months later in the middle life\.$/);
    expect(decisionSentence('Exercising now', 'Acme', rows, { value: 0.021, withinNoise: false }, { fiShift: 0, funded: true })).toMatch(/overall\.$/);
    // A single month in the median is not a shift worth a sentence.
    expect(decisionSentence('Selling nothing in tenders', 'Acme', rows, { value: 0.021, withinNoise: false }, { fiShift: 1 / 12, funded: true })).toMatch(/overall\.$/);
    expect(decisionSentence('Selling nothing in tenders', 'Acme', rows, { value: 0.021, withinNoise: false }, { fiShift: 2 / 12, funded: true })).toMatch(/Work becomes optional 2 months later/);
    const half = decisionTable(fixture({ failed: 120, ipo: 90_000 }, {}));
    expect(decisionSentence('Selling the maximum in tenders', 'Acme', half, null)).toBe('Selling the maximum in tenders leaves the household no better or worse off in the lives where Acme fails and $90k richer where it goes public.');
  });
});
