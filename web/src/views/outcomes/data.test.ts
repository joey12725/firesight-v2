import { readFileSync } from 'node:fs';
import { beforeAll, describe, expect, it } from 'vitest';
import { exampleProfiles, initEngineSync, planPreview } from '@/engine/api';
import type { AgeCurve, PathTrace, Summary } from '@/engine/output-types';
import { engineInput } from '@/engine/pool';
import * as wasm from '@/engine/pkg/firesight_wasm.js';
import type { Profile } from '@/engine/types';
import { layoutDots } from '@/ui/charts/ext/outcomes-QuantileDots';
import { ageReachedBy, brackets, filingStatus, longevity, modelRanges, modelTakeaway, placeOnLadder, taxPoints } from '../assumptions/data';
import { cohortOf, cohortYears, contrasts, curveDots, depletedBy, distribution, historyLastYear, lastingModel, livesDots, luckSplit, pathForYear, quantileSorted, resample, returnBins, scatterLives, selectLives, type ScatterLife } from './data';
import { agesTakeaway, cohortTakeaway, contrastTakeaway, cutTakeaway, lastingTakeaway, luckTakeaway, rangeSentence, shortYearsTakeaway, taxTakeaway, triangulationTakeaway, wealthTakeaway } from './text';
import { withMarket } from './Triangulation';

const runs = new Map<string, { profile: Profile; summary: Summary; trace: PathTrace }>();

function simulate(profile: Profile, paths: number, tracePath = 3) {
  const { json } = engineInput(profile, 7);
  const engine = new wasm.Engine(json);
  try {
    return { profile, summary: JSON.parse(wasm.summarize(engine.runChunk(0, paths), json)) as Summary, trace: JSON.parse(engine.trace(tracePath)) as PathTrace };
  } finally {
    engine.free();
  }
}

beforeAll(() => {
  initEngineSync(readFileSync(new URL('../../engine/pkg/firesight_wasm_bg.wasm', import.meta.url)));
  for (const example of exampleProfiles()) runs.set(example.slug, simulate(example.profile, 400));
}, 120_000);

const each = (check: (run: { profile: Profile; summary: Summary; trace: PathTrace }, slug: string) => void) => runs.forEach(check);

describe('dots', () => {
  it('resamples evenly and keeps order', () => {
    expect(resample([1, 2, 3, 4], 2)).toEqual([2, 4]);
    expect(resample([1, 2, 3], 3)).toEqual([1, 2, 3]);
    expect(resample([], 5)).toEqual([]);
  });

  it('stands for every life: lives without a value become nulls in proportion', () => {
    each(({ summary }) => {
      for (const scalar of ['worst_spending_ratio', 'terminal_net_worth', 'years_depleted', 'lifetime_tax'] as const) {
        const d = distribution(summary, scalar);
        const dots = livesDots(d, summary.paths);
        expect(dots).toHaveLength(100);
        const missing = dots.filter((v) => v === null).length;
        expect(missing).toBe(Math.round(((d?.missing ?? 0) / summary.paths) * 100));
      }
      expect(livesDots(distribution(summary, 'terminal_net_worth'), 40)).toHaveLength(40);
      expect(curveDots(summary.fi, 40)).toHaveLength(40);
    });
  });

  it('places every dot exactly once, within the height', () => {
    each(({ summary }) => {
      const dots = livesDots(distribution(summary, 'terminal_net_worth'), summary.paths);
      for (const width of [300, 560, 900]) {
        const layout = layoutDots({ dots, scale: 'log', groupOf: (v) => (v === null || v <= 0 ? 'none' : null), groups: [{ key: 'none', label: 'Nothing', side: 'left' }], width, height: 110 });
        expect(layout.circles).toHaveLength(dots.length);
        expect(layout.bins.reduce((n, b) => n + b.count, 0) + layout.groups.reduce((n, g) => n + g.count, 0)).toBe(dots.length);
        expect(Math.max(...layout.circles.map((c) => c.row + 1)) * layout.diameter).toBeLessThanOrEqual(110 + 1e-6);
        for (const c of layout.circles) {
          expect(c.cx).toBeGreaterThanOrEqual(0);
          expect(c.cx).toBeLessThanOrEqual(width);
        }
        const last = [...layout.bins].pop();
        expect(Math.max(last?.cumulative ?? 0, ...layout.groups.map((g) => g.cumulative))).toBe(dots.length);
      }
    });
  });

  it('keeps a spike inside the plot by setting dots side by side in whole-year bins', () => {
    const dots = [...new Array<number>(60).fill(50), ...Array.from({ length: 38 }, (_, i) => 50.2 + i * 0.4), null, null];
    const layout = layoutDots({ dots, scale: 'linear', steps: [1, 2, 5, 10], groups: [{ key: 'never', label: 'Never', side: 'right', hollow: true }], width: 560, height: 110 });
    expect(Math.max(...layout.circles.filter((c) => c.group === null).map((c) => c.row + 1)) * layout.diameter).toBeLessThanOrEqual(110);
    expect(layout.bins[0]?.lo).toBe(50);
    expect(layout.bins[0]?.count).toBeGreaterThanOrEqual(60);
    expect(layout.groups[0]?.count).toBe(2);
    expect(layout.diameter).toBeGreaterThanOrEqual(4);
  });
});

describe('early returns', () => {
  const lives: ScatterLife[] = Array.from({ length: 200 }, (_, i) => ({ path: i, ret: -0.05 + i * 0.001, wealth: 100_000 + i * 10_000, depletionAge: i < 30 ? 70 + (i % 10) : null, retirementAge: 60, deathAge: 90, held: i >= 30 }));

  it('bins equal numbers of lives and counts the ones that ran short', () => {
    const bins = returnBins(lives, 4);
    expect(bins.map((b) => b.lives)).toEqual([50, 50, 50, 50]);
    expect(bins.map((b) => b.short)).toEqual([30, 0, 0, 0]);
    expect(bins[0]?.shortShare).toBeCloseTo(0.6);
    expect(bins[0]?.medianWealth).toBeLessThan(bins[3]?.medianWealth ?? 0);
    expect(returnBins(lives.slice(0, 10))).toEqual([]);
  });

  it('splits off the unluckiest fifth', () => {
    const split = luckSplit(lives);
    expect(split?.unlucky.lives).toBe(40);
    expect(split?.unlucky.shortShare).toBeCloseTo(0.75);
    expect(split?.rest.shortShare).toBe(0);
    expect(luckTakeaway(split, lives.length)).toContain('75 of 100 lives run short');
    expect(luckTakeaway(null, 0)).toContain('No simulated life has retired');
  });

  it('selects a range, worst endings first', () => {
    const picked = selectLives(lives, -0.05, -0.01);
    expect(picked.lives).toHaveLength(41);
    expect(picked.lives[0]?.held).toBe(false);
    expect(picked.lives[picked.lives.length - 1]?.held).toBe(true);
    expect(picked.shortShare).toBeCloseTo(30 / 41);
  });

  it('reads the engine scatter', () => {
    each(({ summary }) => {
      const rows = scatterLives(summary);
      expect(rows.length).toBeLessThanOrEqual(summary.scatter.paths.length);
      for (const l of rows) expect(Number.isFinite(l.ret) && Number.isFinite(l.wealth)).toBe(true);
      expect(rows.filter((l) => !l.held).length).toBeLessThanOrEqual(summary.failure.depleted);
    });
  });
});

describe('money and lives', () => {
  it('interpolates the share depleted by an exact age', () => {
    const curve = { ages: [60, 61, 62], cumulative: [0.1, 0.3, 0.3] } as AgeCurve;
    expect(depletedBy(curve, 59)).toBe(0);
    expect(depletedBy(curve, 60.5)).toBeCloseTo(0.05);
    expect(depletedBy(curve, 61)).toBeCloseTo(0.1);
    expect(depletedBy(curve, 61.5)).toBeCloseTo(0.2);
    expect(depletedBy(curve, 80)).toBeCloseTo(0.3);
    expect(quantileSorted([1, 2, 3, 4], 0.5)).toBe(2.5);
  });

  it('never has more lives alive and short than alive, and money that lasted never rises', () => {
    each(({ summary }) => {
      const m = lastingModel(summary);
      m.short.forEach((s, y) => expect(s).toBeLessThanOrEqual((m.alive[y] ?? 0) + 1e-9));
      m.lasted.forEach((v, y) => y > 0 && expect(v).toBeLessThanOrEqual((m.lasted[y - 1] ?? 1) + 1e-9));
      expect(m.lasted[m.lasted.length - 1]).toBeCloseTo(1 - summary.headline.depletion_rate.value, 1);
    });
  });

  it('orders the contrasts by how far apart the groups sit', () => {
    each(({ summary }) => {
      const rows = contrasts(summary);
      rows.forEach((r, i) => i > 0 && expect(r.separation).toBeLessThanOrEqual(rows[i - 1]?.separation ?? Infinity));
      for (const r of rows) {
        expect(r.domain[0]).toBeLessThanOrEqual(Math.min(r.failed, r.succeeded));
        expect(r.domain[1]).toBeGreaterThanOrEqual(Math.max(r.failed, r.succeeded));
      }
    });
  });
});

describe('historical cohorts', () => {
  it('maps a path to its starting month and back', () => {
    expect(cohortOf(0)).toEqual({ year: 1926, month: 7 });
    expect(cohortOf(6)).toEqual({ year: 1927, month: 1 });
    expect(cohortOf(1202)).toEqual({ year: 2026, month: 9 });
    expect(cohortOf(1203)).toEqual({ year: 1926, month: 7 });
    expect(historyLastYear()).toBe(2026);
    expect(pathForYear(1926)).toBe(0);
    expect(pathForYear(1927)).toBe(6);
    expect(cohortOf(pathForYear(1966) as number)).toEqual({ year: 1966, month: 1 });
    expect(pathForYear(2027)).toBeNull();
  });

  it('counts every sampled life once, by starting year', () => {
    const base = runs.get('early-career-saver') as { profile: Profile };
    const { summary } = simulate(withMarket(base.profile, 'historical'), 1300);
    const rows = cohortYears(summary);
    expect(rows[0]?.year).toBe(1926);
    expect(rows.reduce((n, r) => n + r.lives, 0)).toBe(summary.scatter.paths.length);
    expect(rows.reduce((n, r) => n + r.short, 0)).toBe(summary.scatter.columns.find((c) => c.scalar === 'depletion_age')?.values.filter((v) => v !== null).length);
    expect(rows.find((r) => r.year === 1930)?.wraps).toBe(false);
    expect(rows.find((r) => r.year === 2020)?.wraps).toBe(true);
    expect(cohortTakeaway(rows)).not.toMatch(/NaN|undefined|null/);
    // A worse year past the end of the record is reported as a caveat, never as the headline.
    const year = (y: number, short: number, wraps: boolean) => ({ year: y, lives: 10, short, shortShare: short / 10, medianWealth: 1, wraps });
    const made = cohortTakeaway([year(1926, 0, false), year(1927, 1, false), year(1928, 4, false), year(1929, 5, false), year(1930, 0, false), year(1931, 0, false), year(1990, 9, true)]);
    expect(made).toContain('between 1928 and 1929 fare worst: of those starting in 1929, 5 of 10 run short');
    expect(made).toContain('In 3 of 6 such starting years, none do.');
    expect(made).toContain('Later starts look worse (1990: 9 of 10)');
  }, 60_000);

  it('changes only the market model', () => {
    const base = (runs.get('dual-income-family') as { profile: Profile }).profile;
    const next = withMarket(base, 'regime');
    expect(next.assumptions.market.kind).toEqual({ type: 'regime' });
    expect(next.assumptions.market.stress).toBe(base.assumptions.market.stress);
    expect(withMarket(base, base.assumptions.market.kind.type).assumptions.market.kind).toBe(base.assumptions.market.kind);
    expect(next.incomes).toBe(base.incomes);
  });
});

describe('sentences', () => {
  it('are complete for every demo household', () => {
    each(({ summary }) => {
      const sentences = [
        rangeSentence(summary).map((s) => s.text).join(''),
        agesTakeaway(summary, true),
        agesTakeaway(summary, false),
        wealthTakeaway(summary),
        cutTakeaway(summary),
        taxTakeaway(summary),
        shortYearsTakeaway(summary),
        lastingTakeaway(summary),
        luckTakeaway(luckSplit(scatterLives(summary)), scatterLives(summary).length),
        contrastTakeaway(summary, contrasts(summary)),
      ];
      for (const s of sentences) {
        expect(s.length).toBeGreaterThan(20);
        expect(s).not.toMatch(/NaN|undefined|null|Infinity|−0\.0%|-0/);
      }
    });
  });

  it('says how far the market model moves the answer', () => {
    const r = (kind: string, label: string, value: number) => ({ kind, label, success: { value, lo: value - 0.02, hi: value + 0.02 } });
    const wide = triangulationTakeaway([r('bootstrap', 'History reshuffled', 0.83), r('regime', 'Calm and turbulent regimes', 0.74), r('lognormal', 'Textbook random walk', 0.86), r('historical', 'History as it happened', 0.9)], 'regime');
    expect(wide).toContain('74 of 100');
    expect(wide).toContain('by 16 lives in 100');
    expect(wide).toContain('most cautious');
    expect(triangulationTakeaway([r('bootstrap', 'A', 0.83), r('regime', 'B', 0.84)], 'bootstrap')).toContain('barely depends');
  });
});

describe('assumptions', () => {
  it('finds the age a survival curve falls to a share', () => {
    expect(ageReachedBy([1, 0.8, 0.4, 0.1], [60, 70, 80, 90], 0.5)).toBeCloseTo(77.5);
    expect(ageReachedBy([1, 0.9], [60, 70], 0.5)).toBeNull();
  });

  it('reads longevity for each person from the summary and the preview', () => {
    each(({ profile, summary }) => {
      const people = longevity(profile, summary, planPreview(profile));
      expect(people).toHaveLength(profile.household.people.length);
      for (const p of people) {
        expect(p.expected).toBeGreaterThan(70);
        expect(p.survival[0]).toBeGreaterThan(0.9);
        if (p.reach.half !== null) expect(p.reach.half).toBeGreaterThan(70);
      }
      // The ages outlived are the engine's own quantiles of age at death, not a reading off the yearly curve.
      const first = people[0];
      const deaths = distribution(summary, 'death_age_0');
      if (first && deaths && (first.survival.at(-1) ?? 0) < 0.25) {
        expect(first.reach.half).toBe(deaths.quantiles[4]);
        expect(first.reach.quarter).toBe(deaths.quantiles[5]);
      }
    });
  });

  it('builds the bracket ladder from the published table and files as the engine does', () => {
    const ladder = brackets('married_joint');
    expect(ladder[0]).toEqual({ rate: 0.1, from: 0, to: 24800 });
    expect(ladder[ladder.length - 1]?.to).toBeNull();
    ladder.forEach((b, i) => i > 0 && expect(b.from).toBe(ladder[i - 1]?.to));
    expect(filingStatus((runs.get('dual-income-family') as { profile: Profile }).profile)).toBe('married_joint');
    expect(filingStatus((runs.get('early-career-saver') as { profile: Profile }).profile)).toBe('single');
  });

  it('places a traced year in the bracket the engine reports', () => {
    each(({ profile, trace }) => {
      const { now } = taxPoints(trace);
      expect(now).not.toBeNull();
      if (!now || now.foreign || now.marginal <= 0) return;
      const ladder = brackets(filingStatus(profile));
      const place = placeOnLadder(now, ladder);
      expect(place.bracket?.rate).toBeCloseTo(now.marginal);
      expect(place.x).toBeGreaterThanOrEqual(place.bracket?.from ?? 0);
      // The deflated taxable income should already sit in or near that bracket: the clamp is a guard, not a correction.
      expect(Math.abs(place.x - now.taxable) / Math.max(1, now.taxable)).toBeLessThan(0.25);
    });
  });

  it('describes the model in force against the record', () => {
    const rows = modelRanges((runs.get('dual-income-family') as { profile: Profile }).profile);
    expect(rows.filter((r) => r.current)).toHaveLength(1);
    expect(rows.filter((r) => r.record)).toHaveLength(1);
    // The plan's row carries its stock-return adjustment (the default is -1.5 points), and says so; the others do not move.
    const profile = (runs.get('dual-income-family') as { profile: Profile }).profile;
    const adjustment = profile.assumptions.market.equity_premium_adjustment;
    expect(rows.find((r) => r.current)?.shift).toBe(adjustment);
    expect(rows.filter((r) => !r.current).every((r) => r.shift === 0)).toBe(true);
    expect(modelTakeaway(rows)).toMatch(adjustment === 0 ? /7\.3% a year/ : /with its −1\.5-point adjustment, US stocks return about 5\.8% a year/);
    const plain = modelRanges({ ...profile, assumptions: { ...profile.assumptions, market: { ...profile.assumptions.market, equity_premium_adjustment: 0 } } });
    expect(modelTakeaway(plain)).toMatch(/Under this plan’s model, US stocks return about 7\.3% a year/);
  });
});
