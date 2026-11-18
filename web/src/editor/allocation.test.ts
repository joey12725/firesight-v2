import { describe, expect, it } from 'vitest';
import { equityShare, setWeight, shiftBoundary, toAllocation, toPercents, type Weights } from './allocation';

const total = (w: Weights) => w.reduce((n, x) => n + x.pct, 0);

describe('allocation arithmetic', () => {
  it('normalises any relative weights to whole percents summing to 100', () => {
    expect(toPercents({ us_stocks: 0.6, bonds: 0.4 })).toEqual([
      { key: 'us_stocks', pct: 60 },
      { key: 'bonds', pct: 40 },
    ]);
    const thirds = toPercents({ us_stocks: 1, intl_stocks: 1, bonds: 1 });
    expect(total(thirds)).toBe(100);
    expect(thirds.map((w) => w.pct).sort()).toEqual([33, 33, 34]);
    expect(toPercents({})).toEqual([]);
  });

  it('keeps the total at 100 when one weight changes', () => {
    const start = toPercents({ us_stocks: 60, intl_stocks: 20, bonds: 10, real_estate: 5, crypto: 5 });
    for (const pct of [0, 1, 37, 60, 99, 100]) {
      const next = setWeight(start, 'us_stocks', pct);
      expect(total(next)).toBe(100);
      expect(next.find((w) => w.key === 'us_stocks')?.pct).toBe(pct);
      expect(next.every((w) => w.pct >= 0)).toBe(true);
    }
    // Others keep their proportions: intl is twice bonds before and after.
    const next = setWeight(start, 'us_stocks', 40);
    const pctOf = (k: string) => next.find((w) => w.key === k)?.pct ?? 0;
    expect(pctOf('intl_stocks')).toBe(30);
    expect(pctOf('bonds')).toBe(15);
  });

  it('adds a new asset and gives the remainder somewhere to go', () => {
    const only = toPercents({ us_stocks: 1 });
    const withGold = setWeight(only, 'gold', 10);
    expect(toAllocation(withGold)).toEqual({ us_stocks: 90, gold: 10 });
    const less = setWeight(only, 'us_stocks', 70);
    expect(toAllocation(less)).toEqual({ us_stocks: 70, bonds: 30 });
  });

  it('moves weight across a boundary without going negative', () => {
    const w = toPercents({ us_stocks: 60, bonds: 40 });
    expect(shiftBoundary(w, 0, 5).map((x) => x.pct)).toEqual([65, 35]);
    expect(shiftBoundary(w, 0, -80).map((x) => x.pct)).toEqual([0, 100]);
    expect(shiftBoundary(w, 0, 80).map((x) => x.pct)).toEqual([100, 0]);
    expect(equityShare(w)).toBe(60);
  });
});
