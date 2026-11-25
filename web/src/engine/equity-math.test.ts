// The paper-value and exercise-cost formulas are written twice: in the engine's preview
// (core `equity.rs`) and in `equity-math.ts`. This runs the real wasm engine in Node and
// holds the two to the same numbers, on the demo company and on a fixed random set.
import { readFileSync } from 'node:fs';
import { beforeAll, describe, expect, it } from 'vitest';
import { equityPreview, exampleProfiles, initEngineSync } from './api';
import { EngineError } from './errors';
import { exerciseCostTotal, exerciseCostVested, includedCompanies, paperValue } from './equity-math';
import type { EquityHolding, HoldingKind, PrivateCompany, Profile } from './types';

let demo: Profile;
let company: PrivateCompany;

beforeAll(() => {
  initEngineSync(readFileSync(new URL('./pkg/firesight_wasm_bg.wasm', import.meta.url)));
  const example = exampleProfiles().find((e) => e.slug === 'startup-employee');
  if (!example?.profile.private_equity[0]) throw new Error('the startup-employee demo is missing');
  demo = example.profile;
  company = example.profile.private_equity[0];
});

/** A small fixed-sequence generator (mulberry32), so the random set is the same on every run. */
function sequence(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function randomCompany(next: () => number, index: number): PrivateCompany {
  const cents = (max: number) => Math.round(next() * max * 100) / 100;
  const kinds: HoldingKind['type'][] = ['iso', 'nso', 'rsu', 'shares'];
  const holdings: EquityHolding[] = [];
  const n = Math.floor(next() * 5);
  for (let i = 0; i < n; i++) {
    const type = kinds[Math.floor(next() * kinds.length)] ?? 'iso';
    const shares = Math.round(next() * 200_000);
    // Now and then nothing is vested or everything is. (More vested than the grant holds is an error the preview refuses.)
    const roll = next();
    const vested = roll < 0.15 ? 0 : roll < 0.4 ? shares : Math.round(next() * shares);
    const kind: HoldingKind =
      type === 'iso' || type === 'nso'
        ? { type, strike: cents(40), granted_months_ago: Math.floor(next() * 100) }
        : type === 'rsu'
          ? { type, must_be_employed_at_event: next() < 0.5 }
          : { type, basis_per_share: cents(10), months_held: Math.floor(next() * 60), from_iso: next() < 0.5, fmv_at_exercise: cents(20) };
    holdings.push({ id: `h${i}`, name: `Holding ${i}`, kind, shares, vested, vest_months_left: vested >= shares ? 0 : 24, cliff_months: 0 });
  }
  return { ...company, id: `c${index}`, preferred_price: Math.max(0.01, cents(60)), holdings };
}

describe('equity-math agrees with the engine preview', () => {
  it('on the demo company', () => {
    const preview = equityPreview(company);
    expect(paperValue(company)).toEqual({ vested: preview.paper_value_vested, total: preview.paper_value_total });
    expect(exerciseCostVested(company)).toBe(preview.exercise_cost_vested);
    expect(exerciseCostTotal(company)).toBe(preview.exercise_cost_total);
    // The figures the engine's own test pins for the demo.
    expect(paperValue(company).total).toBe(1_458_000);
    expect(exerciseCostVested(company)).toBe(50_000 * 1.5 + 8_000 * 4);
    expect(paperValue(company).vested).toBeLessThan(paperValue(company).total);
    expect(exerciseCostVested(company)).toBeLessThan(exerciseCostTotal(company));
  });

  it('on a random set of companies', () => {
    const next = sequence(0x45515559);
    let withOptions = 0;
    let underWater = 0;
    for (let i = 0; i < 300; i++) {
      const c = randomCompany(next, i);
      // 100 draws: the figures compared here do not depend on the draws.
      const preview = equityPreview(c, 100);
      const paper = paperValue(c);
      expect(paper.vested, `company ${i} vested`).toBe(preview.paper_value_vested);
      expect(paper.total, `company ${i} total`).toBe(preview.paper_value_total);
      expect(exerciseCostVested(c), `company ${i} cost vested`).toBe(preview.exercise_cost_vested);
      expect(exerciseCostTotal(c), `company ${i} cost total`).toBe(preview.exercise_cost_total);
      const options = c.holdings.filter((h) => h.kind.type === 'iso' || h.kind.type === 'nso');
      if (options.length > 0) withOptions++;
      if (options.some((h) => 'strike' in h.kind && h.kind.strike > c.preferred_price)) underWater++;
    }
    // The set really covers options, and options whose strike is above the price.
    expect(withOptions).toBeGreaterThan(100);
    expect(underWater).toBeGreaterThan(30);
  });

  it('values an option under water at nothing and still counts its strike', () => {
    const option: EquityHolding = { id: 'o', name: 'Options', kind: { type: 'nso', strike: 12, granted_months_ago: 6 }, shares: 1000, vested: 400, vest_months_left: 30, cliff_months: 0 };
    const c: PrivateCompany = { ...company, preferred_price: 10, holdings: [option] };
    expect(paperValue(c)).toEqual({ vested: 0, total: 0 });
    expect(exerciseCostVested(c)).toBe(4800);
    expect(exerciseCostTotal(c)).toBe(12_000);
  });

  it('counts shares already owned as vested and RSUs by their vested units', () => {
    const shares: EquityHolding = { id: 's', name: 'Shares', kind: { type: 'shares', basis_per_share: 1, months_held: 20, from_iso: false, fmv_at_exercise: 0 }, shares: 100, vested: 0, vest_months_left: 0, cliff_months: 0 };
    const rsu: EquityHolding = { id: 'r', name: 'RSUs', kind: { type: 'rsu', must_be_employed_at_event: true }, shares: 200, vested: 50, vest_months_left: 36, cliff_months: 0 };
    const c: PrivateCompany = { ...company, preferred_price: 10, holdings: [shares, rsu] };
    expect(paperValue(c)).toEqual({ vested: 1500, total: 3000 });
    expect(exerciseCostVested(c)).toBe(0);
    expect(exerciseCostTotal(c)).toBe(0);
    const preview = equityPreview(c, 100);
    expect([preview.paper_value_vested, preview.paper_value_total]).toEqual([1500, 3000]);
  });
});

describe('includedCompanies', () => {
  it('keeps the companies the plan counts, in order', () => {
    expect(includedCompanies(demo)).toEqual([company]);
    const off = { ...company, id: 'off', included: false };
    const second = { ...company, id: 'second' };
    expect(includedCompanies({ ...demo, private_equity: [off, company, second] }).map((c) => c.id)).toEqual([company.id, 'second']);
    expect(includedCompanies({ ...demo, private_equity: [off] })).toEqual([]);
  });

  it('is empty for a household without private equity', () => {
    for (const example of exampleProfiles().filter((e) => e.slug !== 'startup-employee')) expect(includedCompanies(example.profile)).toEqual([]);
    const { private_equity: _dropped, ...older } = demo;
    expect(includedCompanies(older as Profile)).toEqual([]);
  });
});

describe('equityPreview', () => {
  it('takes an object or JSON and gives the same preview every time', () => {
    const a = equityPreview(company);
    expect(equityPreview(JSON.stringify(company), 0)).toEqual(a);
    expect(a.draws).toBe(4000);
    expect(equityPreview(company, 5).draws).toBe(100);
    expect(equityPreview(company, 1_000_000).draws).toBe(50_000);
  });

  it('fills in everything but the stage and the price', () => {
    const preview = equityPreview({ stage: 'series_b', preferred_price: 1 });
    expect(preview.fmv_409a).toBeGreaterThan(0);
    expect(preview.fmv_409a).toBeLessThan(1);
    expect(preview.paper_value_total).toBe(0);
    expect(preview.table).toEqual(preview.typical);
  });

  it('throws an EngineError that names the offending field', () => {
    expect(() => equityPreview('{"stage":"series_z"}')).toThrow(EngineError);
    expect(() => equityPreview('{"stage":"series_z"}')).toThrow(/stage/);
    expect(() => equityPreview({ colour: 'red' } as never)).toThrow(/colour/);
  });
});
