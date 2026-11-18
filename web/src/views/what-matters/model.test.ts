// The view's arithmetic against the real engine (the wasm module, in this thread) and
// on small hand-made cases.
import { readFileSync } from 'node:fs';
import { beforeAll, describe, expect, it } from 'vitest';
import { applyPatch, compare, exampleProfiles, initEngineSync, levers } from '@/engine/api';
import type { Summary } from '@/engine/output-types';
import * as wasm from '@/engine/pkg/firesight_wasm.js';
import { engineInput } from '@/engine/pool';
import type { Profile } from '@/engine/types';
import { contourLines, joinSegments, polylineAt } from '@/ui/charts/ext/levers-heatmap';
import { rankingGroups } from './compact/Tornado';
import {
  METRIC_IDS,
  allocationOps,
  biggestRisk,
  biggestWins,
  buildLevers,
  contourLevels,
  defaultAxes,
  effectOf,
  equityShare,
  formatDelta,
  formatRange,
  gridPosition,
  makeGoal,
  medianInterval,
  nextProbe,
  quantileOfDots,
  rankLevers,
  readLevels,
  samplingBand,
  trimComparison,
  variablesFor,
  type Effect,
  type Probe,
} from './model';
import { changeClause, crossingAlong, effectStatement, intervalWords, leadSentence, mapTakeaway, solverSentence, tornadoTakeaway } from './text';

const PATHS = 200;
let profiles: Profile[] = [];

function simulate(profile: Profile): { summary: Summary; acc: Uint8Array; json: string } {
  const { json } = engineInput(profile, 1);
  const engine = new wasm.Engine(json);
  try {
    const acc = engine.runChunk(0, PATHS);
    return { summary: JSON.parse(wasm.summarize(acc, json)) as Summary, acc, json };
  } finally {
    engine.free();
  }
}

beforeAll(() => {
  initEngineSync(readFileSync(new URL('../../engine/pkg/firesight_wasm_bg.wasm', import.meta.url)));
  profiles = exampleProfiles().map((e) => applyPatch(e.profile, []).profile);
});

describe('quantiles of the paired difference', () => {
  const dots = Array.from({ length: 100 }, (_, i) => i + 0.5); // the quantile function of U(0, 100)
  it('reads a 100-dot quantile function', () => {
    expect(quantileOfDots(dots, 0.5)).toBeCloseTo(50, 6);
    expect(quantileOfDots(dots, 0.105)).toBeCloseTo(10.5, 6);
    expect(quantileOfDots(dots, 0)).toBe(0.5);
    expect(quantileOfDots(dots, 1)).toBe(99.5);
    expect(quantileOfDots([], 0.5)).toBeNull();
  });
  it('gives an interval for the median that narrows as lives grow', () => {
    const few = medianInterval(dots, 100);
    const many = medianInterval(dots, 10_000);
    expect(few?.median).toBeCloseTo(50, 6);
    expect(few?.lo).toBeCloseTo(40.2, 1);
    expect(few?.hi).toBeCloseTo(59.8, 1);
    expect((many?.hi ?? 0) - (many?.lo ?? 0)).toBeCloseTo(1.96, 1);
    expect(medianInterval(dots, 0)).toBeNull();
  });
});

describe('levers and paired effects on real engine output', () => {
  it('offers engine levers plus the decisions it leaves out, each side a valid plan', () => {
    for (const profile of profiles) {
      const defs = buildLevers(profile, levers(profile));
      expect(defs.some((l) => l.group === 'control')).toBe(true);
      expect(defs.some((l) => l.group === 'world')).toBe(true);
      expect(defs.find((l) => l.id === 'stock_returns')?.group).toBe('world');
      expect(defs.find((l) => l.id === 'claim_age')?.group).toBe('control');
      const keys = defs.flatMap((l) => l.sides.map((s) => s.key));
      expect(new Set(keys).size).toBe(keys.length);
      for (const side of defs.flatMap((l) => l.sides)) expect(applyPatch(profile, side.ops).errors, side.key).toEqual([]);
    }
  });

  it('moves the target mix by the stated amount and keeps it summing to 100', () => {
    const profile = profiles[0] as Profile;
    const g = equityShare(profile) as number;
    const ops = allocationOps(profile, g - 0.1) ?? [];
    const next = applyPatch(profile, ops).profile;
    expect(equityShare(next)).toBeCloseTo(g - 0.1, 2);
    expect(Object.values(next.portfolio.target).reduce((n: number, w) => n + (w ?? 0), 0)).toBeCloseTo(100, 0);
  });

  it('turns a comparison into effects whose bars agree with the two runs', () => {
    const profile = profiles.find((p) => p.name?.includes('Dual')) ?? (profiles[0] as Profile);
    const defs = buildLevers(profile, levers(profile));
    const base = simulate(profile);
    const effects = new Map<string, Effect>();
    for (const side of defs.flatMap((l) => l.sides)) {
      const variant = simulate(applyPatch(profile, side.ops).profile);
      const paired = trimComparison(compare(base.acc, variant.acc, base.json));
      expect(paired.outcomes.map((o) => o.outcome).sort()).toEqual(['depleted', 'success']);
      for (const metric of METRIC_IDS) {
        const e = effectOf(metric, paired, base.summary.headline, variant.summary.headline);
        if (metric === 'holds') effects.set(side.key, e);
        if (e.state === 'unknown') continue;
        expect(e.lo as number).toBeLessThanOrEqual((e.delta as number) + 1e-9);
        expect(e.hi as number).toBeGreaterThanOrEqual((e.delta as number) - 1e-9);
        if (e.state === 'noise') expect((e.lo as number) <= 0 && (e.hi as number) >= 0).toBe(true);
        if (e.state === 'effect') expect((e.lo as number) * (e.hi as number)).toBeGreaterThan(0);
        if (metric === 'holds' || metric === 'short') {
          // For a share the paired difference is exactly the gap between the two runs.
          expect((e.base as number) + (e.delta as number)).toBeCloseTo(e.value as number, 4);
          expect((e.gained - e.lost) / e.n).toBeCloseTo(e.delta as number, 4);
        }
      }
    }
    // No direction is asserted: with a funded trigger, spending less also means retiring sooner, so even the sign is the engine's to say.
    expect(effects.size).toBe(defs.flatMap((l) => l.sides).length);
    expect([...effects.values()].some((e) => e.state === 'effect')).toBe(true);

    const ranked = rankLevers(defs, effects, 'control');
    const loud = ranked.filter((r) => !r.quiet);
    expect(loud.map((r) => r.swing)).toEqual([...loud.map((r) => r.swing)].sort((a, b) => b - a));
    expect(ranked.findIndex((r) => r.quiet)).toBe(loud.length === ranked.length ? -1 : loud.length);

    const wins = biggestWins(defs, effects, 'holds');
    expect(wins.length).toBeLessThanOrEqual(3);
    expect(new Set(wins.map((w) => w.lever.id)).size).toBe(wins.length);
    for (const w of wins) {
      expect(w.lever.group).toBe('control');
      expect(w.effect.state).toBe('effect');
      expect(w.effect.delta as number).toBeGreaterThan(0);
    }
    const risk = biggestRisk(defs, effects, 'holds');
    if (risk) expect(risk.effect.delta as number).toBeLessThan(0);
    expect(leadSentence(defs, effects, 'holds', PATHS).map((s) => s.text).join('')).toMatch(/\.$/);
    expect(tornadoTakeaway(defs, effects, PATHS)).toMatch(/\.$/);
  }, 60_000);

  it('calls an identical plan no change, never a small effect', () => {
    const profile = profiles[0] as Profile;
    const a = simulate(profile);
    const paired = trimComparison(compare(a.acc, a.acc, a.json));
    for (const metric of METRIC_IDS) {
      const e = effectOf(metric, paired, a.summary.headline, a.summary.headline);
      expect(['none', 'unknown']).toContain(e.state);
      if (e.state === 'none') expect(e.delta).toBe(0);
    }
  });
});

describe('variables', () => {
  it('gives nine ascending values, the plan on the grid, and valid plans at the ends', () => {
    for (const profile of profiles) {
      const vars = variablesFor(profile);
      expect(defaultAxes(vars)).not.toBeNull();
      for (const v of vars) {
        expect(v.values).toHaveLength(9);
        expect([...v.values].sort((a, b) => a - b)).toEqual(v.values);
        if (v.current !== null && v.id !== 'retirement_age') expect(gridPosition(v.values, v.current), v.id).not.toBeNull();
        for (const x of [v.values[0] as number, v.values[8] as number]) expect(applyPatch(profile, v.ops(x)).errors, `${v.id} ${x}`).toEqual([]);
        if (v.bounds) for (const x of v.bounds) expect(applyPatch(profile, v.ops(x)).errors, `${v.id} ${x}`).toEqual([]);
      }
      const spending = vars.find((v) => v.id === 'spending');
      expect(spending?.values[4]).toBe(spending?.current);
    }
  });
  it('places a value between grid nodes', () => {
    expect(gridPosition([1, 2, 3], 2.5)).toBeCloseTo(1.5);
    expect(gridPosition([1, 2, 3], 1)).toBe(0);
    expect(gridPosition([1, 2, 3], 3.5)).toBeNull();
    expect(gridPosition([1, 2, 3], null)).toBeNull();
  });
  it('summarises a run as five levels with their intervals', () => {
    const levels = readLevels(simulate(profiles[0] as Profile).summary.headline);
    expect(levels.holds.value).toBeGreaterThan(0);
    expect(levels.holds.lo as number).toBeLessThanOrEqual(levels.holds.value as number);
    expect((levels.holds.value as number) + (levels.short.value as number)).toBeLessThanOrEqual(1 + 1e-9);
  });
});

describe('contours', () => {
  const ramp = [0, 1, 2, 3].map((r) => [0, 1, 2, 3].map((c) => c * 10 + r)); // rises 10 per column
  it('draws one line where a plane crosses the level, at the interpolated position', () => {
    const lines = contourLines(ramp, 15);
    expect(lines).toHaveLength(1);
    const line = lines[0] ?? [];
    expect(line).toHaveLength(4);
    for (const [x, y] of line) expect(x).toBeCloseTo(1.5 - y / 10, 6);
    expect(polylineAt(line).length).toBeCloseTo(Math.hypot(3, 0.3), 6);
  });
  it('closes a ring around a peak and skips squares with a missing corner', () => {
    const peak = [
      [0, 0, 0],
      [0, 1, 0],
      [0, 0, 0],
    ];
    const ring = contourLines(peak, 0.5);
    expect(ring).toHaveLength(1);
    expect(ring[0]?.[0]).toEqual(ring[0]?.[ring[0].length - 1]);
    expect(contourLines([[0, null], [1, 1]], 0.5)).toEqual([]);
    expect(contourLines(ramp, 99)).toEqual([]);
  });
  it('joins segments given in any order and direction', () => {
    const lines = joinSegments([
      [[2, 0], [1, 0]],
      [[0, 0], [1, 0]],
      [[2, 0], [3, 0]],
    ]);
    expect(lines).toHaveLength(1);
    expect(lines[0]?.map((p) => p[0]).sort()).toEqual([0, 1, 2, 3]);
    expect(Math.abs((lines[0]?.[0]?.[0] ?? 0) - (lines[0]?.[3]?.[0] ?? 0))).toBe(3);
  });
  it('picks the customary probability levels, and round values otherwise', () => {
    expect(contourLevels('holds', 0.5, 1)).toEqual([0.75, 0.9, 0.95]);
    const ages = contourLevels('fi_age', 45, 62);
    expect(ages.length).toBeGreaterThan(0);
    for (const a of ages) expect(a % 5).toBe(0);
    expect(contourLevels('wealth', 5, 5)).toEqual([]);
  });
});

describe('solver', () => {
  const level = (x: number) => 1 - x / 200; // falls as x grows: 0.9 at x = 20
  const run = (goal = makeGoal('holds', 0.9), bounds: [number, number] = [0, 100], f = level) => {
    const probes: Probe[] = [];
    for (;;) {
      const step = nextProbe(probes, bounds, 0.5, (v) => Math.round(v * 4) / 4, goal);
      if ('outcome' in step) return { probes, outcome: step.outcome };
      const v = f(step.x);
      probes.push({ order: probes.length + 1, x: step.x, level: { value: v, lo: v - 0.01, hi: v + 0.01 }, met: goal.met(v) });
      if (probes.length > 20) throw new Error('did not stop');
    }
  };
  it('tests both ends first, then halves the bracket to the tolerance', () => {
    const { probes, outcome } = run();
    expect(probes.slice(0, 3).map((p) => p.x)).toEqual([0, 100, 50]);
    expect(probes.length).toBeLessThanOrEqual(10);
    expect(outcome.status).toBe('converged');
    if (outcome.status !== 'converged') return;
    expect(outcome.safe.met).toBe(true);
    expect(outcome.unsafe.met).toBe(false);
    expect(outcome.safe.x).toBeLessThanOrEqual(20);
    expect(outcome.unsafe.x).toBeGreaterThan(20);
    expect(outcome.unsafe.x - outcome.safe.x).toBeLessThanOrEqual(0.5 + 1e-9);
    // The interval of each run is +/- 0.01, which is +/- 2 on x.
    expect(outcome.band?.[0]).toBeCloseTo(18, 0);
    expect(outcome.band?.[1]).toBeCloseTo(22, 0);
  });
  it('reports a goal met everywhere or nowhere instead of inventing a boundary', () => {
    expect(run(makeGoal('holds', 0.4)).outcome.status).toBe('always');
    const never = run(makeGoal('holds', 1.5));
    expect(never.outcome.status).toBe('never');
    if (never.outcome.status === 'never') expect(never.outcome.closest.x).toBe(0);
    expect(never.probes).toHaveLength(2);
  });
  it('searches the other way for a goal that is met at the high end', () => {
    const { outcome } = run(makeGoal('short', 0.05), [0, 100], (x) => 0.3 - x / 400); // 0.05 at x = 100... met from x >= 100
    expect(['converged', 'never', 'always']).toContain(outcome.status);
    const up = run(makeGoal('short', 0.1), [0, 100], (x) => 0.3 - x / 400); // 0.1 at x = 80
    expect(up.outcome.status).toBe('converged');
    if (up.outcome.status === 'converged') {
      expect(up.outcome.safe.x).toBeGreaterThanOrEqual(80);
      expect(up.outcome.unsafe.x).toBeLessThan(80);
    }
  });
  it('treats a level that does not exist as not meeting the goal', () => {
    expect(makeGoal('fi_by', 55).met(null)).toBe(false);
    expect(makeGoal('fi_by', 55).met(54.9)).toBe(true);
    expect(samplingBand([], 0.9)).toBeNull();
  });
  it('words the answer on the safe side', () => {
    const profile = profiles[0] as Profile;
    const spending = variablesFor(profile).find((v) => v.id === 'spending');
    if (!spending?.bounds) throw new Error('no spending variable');
    const [lo, hi] = spending.bounds;
    const { outcome } = run(makeGoal('holds', 0.9), spending.bounds, (x) => 1 - (0.3 * (x - lo)) / (hi - lo));
    const words = solverSentence(makeGoal('holds', 0.9), spending, outcome, 4000);
    expect(words.answer).toMatch(/^The plan holds in at least 90 of 100 lives when the annual budget is at most \$[\d,]+ a year\.$/);
    expect(words.detail).toContain('assuming the measure moves in one direction');
  });
});

describe('words', () => {
  it('states changes as lives, years and dollars with the right direction', () => {
    expect(changeClause('holds', 0.062)).toBe('the plan holds in 6 more of 100 lives');
    expect(changeClause('holds', -0.004)).toBe('the plan holds in 4 fewer of 1,000 lives');
    expect(changeClause('fi_age', -1.25)).toBe('work becomes optional 1.3 years sooner in the middle life');
    expect(changeClause('wealth', 310_000)).toBe('the middle life ends with $310k more');
    expect(formatDelta('holds', 0.0631)).toBe('+6.3');
    expect(formatDelta('short', -0.17)).toBe('−17');
    expect(formatDelta('fi_age', 0.01)).toBe('0');
    expect(intervalWords('holds', 0.051, 0.075)).toBe('5.1 to 7.5 of 100');
    expect(intervalWords('holds', -0.012, 0.004)).toBe('−1.2 to +0.4 of 100');
    expect(intervalWords('short', -0.19, -0.16)).toBe('16 to 19 of 100');
    expect(formatRange('holds', 0.83, 0.87)).toBe('83 to 87 of 100');
    expect(formatRange('holds', 0.985, 0.998)).toBe('99 of 100 to 998 of 1,000');
  });
  it('never words noise as an effect', () => {
    const noise: Effect = { state: 'noise', delta: 0.004, lo: -0.003, hi: 0.011, base: 0.8, value: 0.804, n: 1000, gained: 9, lost: 5 };
    expect(effectStatement('holds', noise)).toMatch(/^No change that can be told from chance/);
    const none: Effect = { ...noise, state: 'none', delta: 0, lo: 0, hi: 0 };
    expect(effectStatement('holds', none)).toBe('No simulated life changes on this measure.');
  });
  it('reads the map from the plan along each axis', () => {
    expect(crossingAlong([0.8, 0.85, 0.95], [1, 2, 3], 0.9)).toBeCloseTo(2.5);
    expect(crossingAlong([0.8, null, 0.95], [1, 2, 3], 0.9)).toBeNull();
    const profile = profiles[0] as Profile;
    const vars = variablesFor(profile);
    const axes = defaultAxes(vars);
    const x = vars.find((v) => v.id === axes?.x);
    const y = vars.find((v) => v.id === axes?.y);
    if (!x || !y) throw new Error('no axes');
    const values = y.values.map((_, r) => x.values.map((_, c) => 0.99 - c * 0.03 - r * 0.02));
    const text = mapTakeaway({ metric: 'holds', x, y, values, paths: 400 });
    expect(text).toMatch(/90 of 100 lives/);
    expect(mapTakeaway({ metric: 'wealth', x, y, values: values.map((r) => r.map((v) => v * 1e6)), paths: 400 })).toMatch(/runs from \$/);
  });
});

describe('the assistant ranking as a tornado', () => {
  it('keeps the largest levers, derives whiskers from the standard error and marks noise', () => {
    const { groups, hidden } = rankingGroups(
      {
        metric: 'success_rate',
        base: 0.83,
        paths: 2000,
        seed: 1,
        levers: [
          { id: 'spending', label: 'Spending', swing: 0.06, settings: [{ label: '10% lower', value: 0.89, delta: 0.06, std_error: 0.005, within_noise: false }, { label: '10% higher', value: 0.78, delta: -0.05, std_error: 0.005, within_noise: false }] },
          { id: 'stock_returns', label: 'Stock returns', swing: 0.04, settings: [{ label: '1 point lower', value: 0.79, delta: -0.04, std_error: null, within_noise: null }] },
          { id: 'fees', label: 'Fees', swing: 0.002, settings: [{ label: '1% a year', value: 0.828, delta: -0.002, std_error: 0.003, within_noise: true }] },
          ...['a', 'b', 'c'].map((id) => ({ id: `custom_${id}`, label: id, swing: 0.001, settings: [{ label: 'as described', value: 0.83, delta: 0, std_error: 0, within_noise: false }] })),
        ],
      },
      { axis: 'Lives', unit: 'share', better: 1 },
    );
    expect(hidden).toBe(1);
    expect(groups.map((g) => g.id)).toEqual(['control', 'world']);
    const spending = groups[0]?.levers[0]?.bars[0];
    expect(spending?.state).toBe('effect');
    expect(spending?.lo).toBeCloseTo(0.06 - 1.96 * 0.005, 6);
    expect(groups[1]?.levers[0]?.bars[0]?.lo).toBeNull();
    expect(groups[0]?.levers.find((l) => l.id === 'fees')?.bars[0]?.state).toBe('noise');
    expect(groups[0]?.levers.find((l) => l.id.startsWith('custom'))?.bars[0]?.state).toBe('none');
  });
});
