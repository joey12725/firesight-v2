// The editor's rules against the real engine. Needs `npm run wasm` to have produced
// src/engine/pkg.
import { readFileSync } from 'node:fs';
import { beforeAll, describe, expect, it } from 'vitest';
import { applyPatch, defaultProfile, equityPreview, exampleProfiles, initEngineSync } from '@/engine/api';
import type { EquityHolding, OutcomeRow, PrivateCompany, Profile } from '@/engine/types';
import * as wasm from '@/engine/pkg/firesight_wasm.js';
import { append, set } from '@/state/patches';
import {
  LIMITS,
  STAGE_ORDER,
  commonMultiple,
  currentJob,
  estimateGrantAge,
  estimateVestMonthsLeft,
  followCountOps,
  followMonthsLeftOps,
  from409aOps,
  grantAgeBasis,
  grantAgeFromMonthsLeft,
  isoYearlyAmount,
  linkJobOps,
  monthsLeftIsEstimate,
  newCompany,
  newHolding,
  newOutcomeRow,
  priceIsEstimated,
  seedCustomTable,
  splitIsoAtLimit,
  splitIsoOps,
  stageOps,
  switchKindOps,
  tenderSale,
  vestedBy,
} from './model';

beforeAll(() => {
  initEngineSync(readFileSync(new URL('../../engine/pkg/firesight_wasm_bg.wasm', import.meta.url)));
});

const startup = (): Profile => exampleProfiles().find((e) => e.slug === 'startup-employee')!.profile;
const demoCompany = (): PrivateCompany => startup().private_equity[0]!;
const codes = (list: { code: string }[]) => list.map((i) => i.code);

describe('seedCustomTable', () => {
  it('validates through applyPatch on the demo', () => {
    const profile = startup();
    const company = demoCompany();
    const seeded = seedCustomTable(equityPreview(company).table);
    expect(seeded.map((r) => r.id)).toEqual(['fails', 'acquired', 'ipo', 'still_private']);
    const result = applyPatch(profile, [set(`/private_equity/@${company.id}/outcomes`, seeded)]);
    expect(result.errors).toEqual([]);
    expect(codes(result.warnings)).not.toContain('outcomes_optimistic');
    expect(result.profile.private_equity[0]!.outcomes).toEqual(seeded);
  });

  it('seeds price multiples at two significant figures, as the typical table shows them', () => {
    const seeded = seedCustomTable(equityPreview(demoCompany()).table);
    for (const row of seeded) {
      expect(row.multiple_low, row.id).toBe(Number(row.multiple_low.toPrecision(2)));
      expect(row.multiple_high, row.id).toBe(Number(row.multiple_high.toPrecision(2)));
      expect(row.multiple_high).toBeGreaterThanOrEqual(row.multiple_low);
    }
    // Series C: not 0.1666, 2.161, 1.223, 7.357.
    expect(seeded.find((r) => r.kind === 'acquired')).toMatchObject({ multiple_low: 0.17, multiple_high: 2.2 });
    expect(seeded.find((r) => r.kind === 'ipo')).toMatchObject({ multiple_low: 1.2, multiple_high: 7.4 });
  });

  it('leaves the outcome shares unchanged and the implied multiple within 3%, for every stage and lean', () => {
    for (const stage of STAGE_ORDER) {
      for (const outlook of ['pessimistic', 'typical', 'optimistic'] as const) {
        const company = { ...demoCompany(), stage, outlook, fmv_409a: null };
        const preset = equityPreview(company);
        const seeded = equityPreview({ ...company, outcomes: seedCustomTable(preset.table) });
        expect(seeded.outcome_shares, `${stage} ${outlook}`).toEqual(preset.outcome_shares);
        // Seeding is not exact: years go to the half year and multiples to two figures. It must not move the picture.
        expect(Math.abs(seeded.implied_multiple / preset.implied_multiple - 1), `${stage} ${outlook}`).toBeLessThan(0.03);
        // The engine normalises a custom table, which can move the last bit.
        seeded.table.forEach((r, i) => expect(r.probability).toBeCloseTo(preset.table[i]!.probability, 12));
      }
    }
  });
});

describe('commonMultiple', () => {
  // A table with one certain sale at exactly `m` times the last valuation: every draw
  // pays one share's worth, so the largest dot over the price is the engine's rule.
  const engineRule = (m: number, overhang: number): number => {
    const row: OutcomeRow = { id: 'sale', kind: 'acquired', probability: 1, years_from: 0, years_to: 0, multiple_low: m, multiple_high: m };
    const company = { stage: 'series_c' as const, preferred_price: 10, preference_overhang: overhang, outcomes: [row], holdings: [{ id: 's', name: 'Shares', kind: { type: 'shares' as const, basis_per_share: 1 }, shares: 1 }] };
    const preview = equityPreview(company, 100);
    return preview.dots.at(-1) ?? 0;
  };
  // The preview deflates each payoff by the same month of inflation, so compare against a sale at exactly the last valuation.
  const engineMultiple = (m: number, overhang: number) => engineRule(m, overhang) / engineRule(1, overhang);

  it('equals the engine below the overhang, between it and 1, and above 1', () => {
    for (const overhang of [0, 0.2, 0.3, 0.5]) {
      for (const m of [0.1, 0.25, 0.3, 0.45, 0.8, 0.999, 1, 1.5, 4]) {
        expect(commonMultiple(m, overhang), `m ${m}, overhang ${overhang}`).toBeCloseTo(engineMultiple(m, overhang), 9);
      }
    }
    expect(commonMultiple(0.2, 0.3)).toBe(0);
    expect(commonMultiple(0.65, 0.3)).toBeCloseTo(0.5, 12);
    expect(commonMultiple(2.2, 0.3)).toBe(2.2);
  });
});

describe('splitIsoAtLimit', () => {
  const grant = (over: Partial<EquityHolding> = {}): EquityHolding => ({ id: 'big', name: 'Big grant', kind: { type: 'iso', strike: 5, granted_months_ago: 24 }, shares: 200_000, vested: 100_000, vest_months_left: 24, cliff_months: 0, ...over });

  it('yields an ISO part at or under the limit and preserves the counts', () => {
    const h = grant();
    expect(isoYearlyAmount(h)).toBe(250_000);
    const parts = splitIsoAtLimit(h)!;
    expect(isoYearlyAmount(parts.iso)).toBeLessThanOrEqual(LIMITS.isoYearly);
    expect(parts.iso.shares).toBe(80_000);
    expect(parts.iso.vested).toBe(40_000);
    expect(parts.iso.shares + parts.nso.shares).toBe(h.shares);
    expect(parts.iso.vested + parts.nso.vested).toBe(h.vested);
    expect(parts.nso).toMatchObject({ id: 'big_nso', kind: { type: 'nso', strike: 5, granted_months_ago: 24 }, vest_months_left: 24 });
    expect(parts.iso.id).toBe('big');
  });

  it('works on a fully vested grant and on awkward counts', () => {
    const full = splitIsoAtLimit(grant({ vested: 200_000, vest_months_left: 0, kind: { type: 'iso', strike: 5, granted_months_ago: 60 } }))!;
    expect(full.iso.vested).toBe(full.iso.shares);
    expect(full.nso.vested).toBe(full.nso.shares);
    expect(full.iso.shares + full.nso.shares).toBe(200_000);
    for (const [shares, vested] of [[33_333, 33_332], [100_001, 1], [70_007, 35_003.5], [50_000, 0]] as const) {
      const h = grant({ shares, vested, kind: { type: 'iso', strike: 37, granted_months_ago: 7 } });
      const parts = splitIsoAtLimit(h)!;
      expect(parts.iso.shares + parts.nso.shares).toBe(shares);
      expect(parts.iso.vested + parts.nso.vested).toBe(vested);
      for (const part of [parts.iso, parts.nso]) {
        expect(part.vested).toBeGreaterThanOrEqual(0);
        expect(part.vested).toBeLessThanOrEqual(part.shares);
      }
      expect(isoYearlyAmount(parts.iso)).toBeLessThanOrEqual(LIMITS.isoYearly);
    }
  });

  it('is null within the limit and for other kinds', () => {
    expect(splitIsoAtLimit(demoCompany().holdings[0]!)).toBeNull();
    expect(splitIsoAtLimit(grant({ kind: { type: 'nso', strike: 5, granted_months_ago: 24 } }))).toBeNull();
  });

  it('clears the engine warning in one edit and takes an unused id', () => {
    const profile = startup();
    const company = demoCompany();
    const path = `/private_equity/@${company.id}`;
    const big = applyPatch(profile, [append(`${path}/holdings`, grant()), append(`${path}/holdings`, { id: 'big_nso', name: 'Taken', kind: { type: 'nso', strike: 1 }, shares: 10, vested: 10 })]);
    expect(codes(big.warnings)).toContain('iso_over_100k_limit');
    const ops = splitIsoOps(path, big.profile.private_equity[0]!, 'big');
    expect(ops.map((o) => o.op)).toEqual(['set', 'append']);
    const split = applyPatch(big.profile, ops);
    expect(split.errors).toEqual([]);
    expect(codes(split.warnings)).not.toContain('iso_over_100k_limit');
    expect(split.profile.private_equity[0]!.holdings.map((h) => h.id)).toContain('big_nso_2');
    expect(splitIsoOps(path, split.profile.private_equity[0]!, 'big')).toEqual([]);
  });

  it('measures the yearly amount as the engine does', () => {
    // The warning appears exactly when the editor's figure is over the limit.
    const profile = startup();
    const path = `/private_equity/@${demoCompany().id}/holdings`;
    for (const [strike, shares, age, left] of [[5, 80_000, 24, 24], [5, 80_004, 24, 24], [2, 60_000, 0, 6], [2, 40_000, 0, 6], [10, 30_000, 100, 0], [3, 100_000, 6, 30]] as const) {
      const h: EquityHolding = { id: 'probe', name: 'Probe', kind: { type: 'iso', strike, granted_months_ago: age }, shares, vested: left === 0 ? shares : 0, vest_months_left: left, cliff_months: 0 };
      const report = applyPatch(profile, [append(path, h)]);
      expect(codes(report.warnings).includes('iso_over_100k_limit'), JSON.stringify(h)).toBe(isoYearlyAmount(h) > LIMITS.isoYearly);
    }
  });
});

describe('the estimates', () => {
  it('estimates the grant age from the vested share of four years, at least a year', () => {
    expect(estimateGrantAge(50_000, 200_000)).toBe(12);
    expect(estimateGrantAge(100_000, 200_000)).toBe(24);
    expect(estimateGrantAge(150_000, 200_000)).toBe(36);
    expect(estimateGrantAge(200_000, 200_000)).toBe(48);
    expect(estimateGrantAge(300_000, 200_000)).toBe(48);
    expect(estimateGrantAge(1, 200_000)).toBe(12);
    expect(estimateGrantAge(0, 200_000)).toBeNull();
    expect(estimateGrantAge(10, 0)).toBeNull();
  });

  it('estimates the months left to vest, never 0 while something is unvested', () => {
    expect(estimateVestMonthsLeft(0, 200_000)).toBe(48);
    expect(estimateVestMonthsLeft(150_000, 200_000)).toBe(12);
    expect(estimateVestMonthsLeft(199_999, 200_000)).toBe(1);
    expect(estimateVestMonthsLeft(200_000, 200_000)).toBe(0);
    expect(estimateVestMonthsLeft(0, 0)).toBeNull();
  });

  it('estimates the round price from a 409A value at the stage fraction', () => {
    const ratio = equityPreview({ stage: 'series_c', preferred_price: 1 }).fmv_409a;
    expect(ratio).toBeGreaterThan(0);
    expect(ratio).toBeLessThan(1);
    const ops = from409aOps('/private_equity/@x', 4.9, ratio);
    expect(ops).toEqual([set('/private_equity/@x/fmv_409a', 4.9), set('/private_equity/@x/preferred_price', Math.round((4.9 / ratio) * 100) / 100)]);
    const price = ops[1]!.op === 'set' ? (ops[1]!.value as number) : 0;
    expect(priceIsEstimated({ preferred_price: price, fmv_409a: 4.9 }, ratio)).toBe(true);
    expect(priceIsEstimated({ preferred_price: 14.5, fmv_409a: 4.9 }, ratio)).toBe(false);
    expect(priceIsEstimated({ preferred_price: price, fmv_409a: null }, ratio)).toBe(false);
  });
});

describe('LIMITS', () => {
  const holding = (i: number) => ({ id: `h${i}`, name: `H${i}`, kind: { type: 'shares', basis_per_share: 1 }, shares: 10 });

  it('match the engine: at the limit is accepted, one more is an error', () => {
    const base = defaultProfile();
    const company = (i: number) => ({ id: `c${i}`, name: `C${i}`, preferred_price: 1 });
    const many = (n: number) => applyPatch(base, Array.from({ length: n }, (_, i) => append('/private_equity', company(i))));
    expect(codes(many(LIMITS.companies).errors)).toEqual([]);
    expect(codes(many(LIMITS.companies + 1).errors)).toContain('too_many_companies');

    const withHoldings = (n: number) => applyPatch(base, [append('/private_equity', { ...company(0), holdings: Array.from({ length: n }, (_, i) => holding(i)) })]);
    expect(codes(withHoldings(LIMITS.holdings).errors)).toEqual([]);
    expect(codes(withHoldings(LIMITS.holdings + 1).errors)).toContain('too_many_holdings');

    const rows = (n: number): OutcomeRow[] => Array.from({ length: n }, (_, i) => ({ id: `r${i}`, kind: 'fails', probability: 1 / n, years_from: 1, years_to: 5, multiple_low: 1, multiple_high: 1 }));
    const withRows = (n: number) => applyPatch(base, [append('/private_equity', { ...company(0), outcomes: rows(n) })]);
    expect(codes(withRows(LIMITS.outcomes).errors)).toEqual([]);
    expect(codes(withRows(LIMITS.outcomes + 1).errors)).toContain('too_many_outcomes');

    const sell = (years: number) => applyPatch(base, [append('/private_equity', { ...company(0), policy: { sell_down: { type: 'over_years', years } } })]);
    expect(codes(sell(LIMITS.sellDownYears).errors)).toEqual([]);
    expect(codes(sell(LIMITS.sellDownYears + 1).errors)).toContain('out_of_range');
  });
});

describe('adding and changing', () => {
  it('links a new company to the current job and its owner', () => {
    const family = exampleProfiles().find((e) => e.slug === 'dual-income-family')!.profile;
    const job = currentJob(family)!;
    const first = family.incomes.find((i) => i.kind.type === 'job' && i.owner === 0)!;
    expect(job).toEqual({ id: first.id, owner: 0 });
    expect(newCompany(family)).toEqual({ name: 'Company', stage: 'series_b', preferred_price: 0, job: first.id, owner: 0 });
    // Only the second person has a job: the company is theirs.
    const partnerOnly = { ...family, incomes: family.incomes.filter((i) => i.kind.type === 'job' && i.owner === 1) };
    expect(partnerOnly.incomes.length).toBeGreaterThan(0);
    expect(newCompany(partnerOnly)).toMatchObject({ job: partnerOnly.incomes[0]!.id, owner: 1 });
    expect(newCompany({ ...family, incomes: [] })).toMatchObject({ job: null, owner: 0 });
  });

  it('makes holdings of an unlinked company fully vested', () => {
    expect(newHolding('iso', true)).toMatchObject({ kind: { type: 'iso', strike: 1 }, shares: 10_000, vested: 0, vest_months_left: 48 });
    expect(newHolding('nso', false)).toMatchObject({ kind: { type: 'nso', strike: 1 }, shares: 10_000, vested: 10_000, vest_months_left: 0 });
    expect(newHolding('rsu', false)).toMatchObject({ shares: 5_000, vested: 5_000, vest_months_left: 0 });
    expect(newHolding('shares', true)).toEqual({ name: 'Shares', kind: { type: 'shares', basis_per_share: 1 }, shares: 5_000 });
  });

  it('switches a holding between every pair of types without an error', () => {
    const profile = startup();
    const company = demoCompany();
    const types = ['iso', 'nso', 'rsu', 'shares'] as const;
    for (const h of company.holdings) {
      for (const to of types) {
        const path = `/private_equity/@${company.id}/holdings/@${h.id}`;
        const ops = switchKindOps(path, h, to);
        if (h.kind.type === to) {
          expect(ops).toEqual([]);
          continue;
        }
        const result = applyPatch(profile, ops);
        expect(result.errors, `${h.id} to ${to}`).toEqual([]);
        expect(result.profile.private_equity[0]!.holdings.find((x) => x.id === h.id)!.kind.type).toBe(to);
      }
    }
    // Strike and grant age carry between the two kinds of option.
    const iso = company.holdings.find((h) => h.kind.type === 'iso')!;
    expect(switchKindOps('/p', iso, 'nso')).toEqual([set('/p/kind', { type: 'nso', strike: 1.5, granted_months_ago: 40 })]);
  });

  it('unlinking the job leaves no error and says whether vested counts changed', () => {
    const profile = startup();
    const company = demoCompany();
    const path = `/private_equity/@${company.id}`;
    const bare = applyPatch(profile, [set(`${path}/job`, null)]);
    expect(codes(bare.errors)).toContain('equity_unvested_without_job');
    const { ops, vestedChanged } = linkJobOps(path, company, null);
    expect(vestedChanged).toBe(true);
    const result = applyPatch(profile, ops);
    expect(result.errors).toEqual([]);
    for (const h of result.profile.private_equity[0]!.holdings) {
      if (h.kind.type !== 'shares') expect([h.vested, h.vest_months_left, h.cliff_months]).toEqual([h.shares, 0, 0]);
    }
    const again = linkJobOps(path, result.profile.private_equity[0]!, null);
    expect(again.vestedChanged).toBe(false);
    expect(linkJobOps(path, company, { id: 'priya_job', owner: 0 }).ops).toEqual([set(`${path}/job`, 'priya_job'), set(`${path}/owner`, 0)]);
  });

  it('shapes a new outcome row like the typical row of its kind, at 0 of 100', () => {
    const preview = equityPreview(demoCompany());
    const seeded = seedCustomTable(preview.table);
    const row = newOutcomeRow('ipo', seeded, preview.typical);
    expect(row).toMatchObject({ id: 'ipo_2', kind: 'ipo', probability: 0 });
    expect(row.multiple_low).toBe(seeded.find((r) => r.kind === 'ipo')!.multiple_low);
    expect(newOutcomeRow('fails', [], [])).toMatchObject({ id: 'fails', years_from: 1, years_to: 5 });
  });
});

describe('estimates that follow the vested count', () => {
  const path = '/private_equity/@northwind/holdings/@g';
  const grant = (over: Partial<EquityHolding> = {}, age = 24): EquityHolding => ({ id: 'g', name: 'Grant', kind: { type: 'iso', strike: 1, granted_months_ago: age }, shares: 200_000, vested: 0, vest_months_left: 48, cliff_months: 0, ...over });
  const apply = (h: EquityHolding, ops: ReturnType<typeof followCountOps>, own: Partial<EquityHolding>): EquityHolding => {
    let next: EquityHolding = { ...h, ...own };
    for (const op of ops) {
      if (op.op !== 'set') continue;
      if (op.path === `${path}/kind/granted_months_ago`) next = { ...next, kind: { ...next.kind, granted_months_ago: op.value as number } as EquityHolding['kind'] };
      else next = { ...next, [op.path.slice(path.length + 1)]: op.value };
    }
    return next;
  };
  const setVested = (h: EquityHolding, vested: number) => apply(h, followCountOps(path, h, true, h.shares, vested), { vested });

  it('are recognised from the holding itself, so undo cannot switch them off', () => {
    const fresh = grant();
    expect(grantAgeBasis(fresh)).toBe('vested');
    expect(monthsLeftIsEstimate(fresh)).toBe(true);
    // Vested 150,000 of 200,000: three years in, one left.
    const edited = setVested(fresh, 150_000);
    expect(edited).toMatchObject({ vested: 150_000, vest_months_left: 12, kind: { granted_months_ago: 36 } });
    // Undo puts the holding back exactly as it was. Nothing outside the plan remembers the edit, so the estimates go on.
    const undone = fresh;
    const again = setVested(undone, 50_000);
    expect(again).toMatchObject({ vested: 50_000, vest_months_left: 36, kind: { granted_months_ago: 12 } });
    expect(grantAgeBasis(again)).toBe('vested');
    // The same holding in another plan, or after a reload, behaves the same: there is no remembered state to lose.
    expect(setVested(JSON.parse(JSON.stringify(edited)) as EquityHolding, 100_000)).toMatchObject({ vest_months_left: 24, kind: { granted_months_ago: 24 } });
  });

  it('stop for a figure the household typed, and only for that figure', () => {
    // Their own grant age: left alone when the vested count changes; the months left still follow.
    const own = grant({ vested: 100_000, vest_months_left: 24 }, 40);
    expect(grantAgeBasis(own)).toBeNull();
    expect(setVested(own, 150_000)).toMatchObject({ vest_months_left: 12, kind: { granted_months_ago: 40 } });
    // Their own months left: left alone; the grant age still follows.
    const ownLeft = grant({ vested: 100_000, vest_months_left: 30 }, 24);
    expect(monthsLeftIsEstimate(ownLeft)).toBe(false);
    expect(setVested(ownLeft, 150_000)).toMatchObject({ vest_months_left: 30, kind: { granted_months_ago: 36 } });
    // RSUs have no grant age.
    const rsu: EquityHolding = { ...grant(), kind: { type: 'rsu', must_be_employed_at_event: false } };
    expect(grantAgeBasis(rsu)).toBeNull();
    expect(followCountOps(path, rsu, true, 200_000, 100_000)).toEqual([set(`${path}/vest_months_left`, 24)]);
  });

  it('re-estimate the grant age from the months left when those are typed, so the two cannot contradict', () => {
    // 120,000 of 200,000 vested estimates 29 months since the grant and 19 left. The household knows 24 are left.
    const h = setVested(grant(), 120_000);
    expect(h).toMatchObject({ vest_months_left: 19, kind: { granted_months_ago: 29 } });
    const ops = followMonthsLeftOps(path, h, 24);
    expect(ops).toEqual([set(`${path}/kind/granted_months_ago`, 24)]);
    const typed = apply(h, ops, { vest_months_left: 24 });
    // 24 since the grant and 24 left make the four years; not 29 and 24.
    expect((typed.kind as { granted_months_ago: number }).granted_months_ago + typed.vest_months_left).toBe(48);
    expect(grantAgeBasis(typed)).toBe('months_left');
    // It keeps following the months left, and a later change of the vested count does not pull it back.
    expect(followMonthsLeftOps(path, typed, 12)).toEqual([set(`${path}/kind/granted_months_ago`, 36)]);
    expect(setVested(typed, 130_000)).toMatchObject({ vest_months_left: 24, kind: { granted_months_ago: 24 } });
    // A grant age the household typed is never overwritten.
    expect(followMonthsLeftOps(path, grant({ vested: 100_000, vest_months_left: 24 }, 40), 12)).toEqual([]);
    expect(grantAgeFromMonthsLeft(0)).toBe(48);
    expect(grantAgeFromMonthsLeft(60)).toBe(0);
  });

  it('keep a holding fully vested when its count changes at a company with no job', () => {
    const h = grant({ vested: 200_000, vest_months_left: 0 }, 48);
    expect(followCountOps(path, h, false, 250_000, 200_000)).toContainEqual(set(`${path}/vested`, 250_000));
  });

  it('stay inside what the engine accepts for the grant age', () => {
    const profile = startup();
    const company = demoCompany();
    const iso = company.holdings.find((h) => h.kind.type === 'iso')!;
    const at = (age: number) => applyPatch(profile, [set(`/private_equity/@${company.id}/holdings/@${iso.id}/kind/granted_months_ago`, age)]);
    expect(codes(at(LIMITS.grantAgeMonths).errors)).toEqual([]);
    expect(codes(at(LIMITS.grantAgeMonths + 1).errors)).toContain('option_term_passed');
    const fraction = (v: number) => codes(applyPatch(profile, [set(`/private_equity/@${company.id}/tenders/price_fraction`, v)]).errors);
    expect(fraction(LIMITS.tenderPriceMin)).toEqual([]);
    expect(fraction(LIMITS.tenderPriceMax)).toEqual([]);
    expect(fraction(LIMITS.tenderPriceMin - 0.01)).toContain('out_of_range');
    const strike = (v: number) => codes(applyPatch(profile, [set(`/private_equity/@${company.id}/holdings/@${iso.id}/kind/strike`, v)]).errors);
    expect(strike(LIMITS.perShare + 1)).toContain('out_of_range');
  });
});

describe('a change of stage', () => {
  // The engine's typical 409A fractions: 35% at Series C, 20% at seed.
  const ratio = (stage: string) => equityPreview({ stage: stage as PrivateCompany['stage'], preferred_price: 1 }, 100).fmv_409a;
  const path = '/private_equity/@c';

  it('moves a price that was estimated from the 409A value, so the note about it stays true', () => {
    const [, priceOp] = from409aOps(path, 2, ratio('series_c'));
    const company = { stage: 'series_c' as const, fmv_409a: 2, preferred_price: (priceOp as { value: number }).value };
    expect(company.preferred_price).toBe(5.71);
    expect(stageOps(path, company, 'seed', ratio)).toEqual([set(`${path}/stage`, 'seed'), set(`${path}/preferred_price`, 10)]);
    // After the change the price is still the estimate, at the new stage's fraction.
    expect(priceIsEstimated({ fmv_409a: 2, preferred_price: 10 }, ratio('seed'))).toBe(true);
  });

  it('leaves a price the household entered alone', () => {
    expect(stageOps(path, { stage: 'series_c', fmv_409a: 2, preferred_price: 14 }, 'seed', ratio)).toEqual([set(`${path}/stage`, 'seed')]);
    expect(stageOps(path, { stage: 'series_c', fmv_409a: null, preferred_price: 14 }, 'late', ratio)).toEqual([set(`${path}/stage`, 'late')]);
  });
});

describe('what a tender sells', () => {
  it('counts shares and vested options above their strike, sells shares first, and never RSUs', () => {
    const company = demoCompany();
    // 10,000 shares, 50,000 vested ISOs at $1.50, 8,000 vested NSOs at $4.00; the RSUs do not count.
    // As holdings stand today (month -1: before this month's vesting).
    const sale = tenderSale(company, -1);
    expect(sale.base).toBe(68_000);
    expect(sale.price).toBe(14);
    // Half of the 20% limit: 6,800 units, all of them shares.
    expect(sale.sold).toBe(6_800);
    expect(sale.proceeds).toBe(6_800 * 14);
    // Selling the most a 100% limit allows takes the shares, then the NSOs (the spread only), then the ISOs.
    const all = tenderSale({ ...company, tenders: { ...company.tenders, max_fraction: 1 }, policy: { ...company.policy, tender_sell_fraction: 1 } }, -1);
    expect(all.sold).toBe(68_000);
    expect(all.proceeds).toBe(10_000 * 14 + 8_000 * (14 - 4) + 50_000 * (14 - 1.5));
    // A tender priced under a strike leaves those options out of the base.
    const cheap = tenderSale({ ...company, tenders: { ...company.tenders, price_fraction: 0.25 } }, -1);
    expect(cheap.price).toBe(3.5);
    expect(cheap.base).toBe(60_000);
    expect(tenderSale({ ...company, policy: { ...company.policy, tender_sell_fraction: 0 } }).sold).toBe(0);
  });

  it('agrees with what the engine sells in a tender planned for the first months', () => {
    const profile = startup();
    const company = demoCompany();
    const planned = applyPatch(profile, [set(`/private_equity/@${company.id}/tenders/next_in_months`, 1)]);
    expect(planned.errors).toEqual([]);
    const engine = new wasm.Engine(JSON.stringify(planned.profile));
    try {
      const trace = JSON.parse(engine.trace(0)) as { events: { kind: string; label: string; amount: number | null }[] };
      const tender = trace.events.find((e) => e.kind === 'tender_offer');
      expect(tender).toBeDefined();
      const sold = Number(/sold ([\d,]+)/.exec(tender?.label ?? '')?.[1]?.replace(/,/g, ''));
      // What has vested by the tender's month counts; the unit count is the engine's exactly.
      const sale = tenderSale(planned.profile.private_equity[0]!, 1);
      expect(Math.round(sale.sold)).toBe(sold);
      // The engine prices it at the company's simulated price that month, a little off the last round's.
      expect(Math.abs((tender?.amount as number) / sale.proceeds - 1)).toBeLessThan(0.1);
      expect(vestedBy(company.holdings.find((h) => h.kind.type === 'rsu')!, 5, true)).toBe(0);
      expect(vestedBy(company.holdings.find((h) => h.kind.type === 'iso')!, 200, true)).toBe(60_000);
      expect(vestedBy(company.holdings.find((h) => h.kind.type === 'iso')!, 200, false)).toBe(50_000);
    } finally {
      engine.free();
    }
  });
});
