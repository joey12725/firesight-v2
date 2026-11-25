// Runs the real wasm engine in Node and checks the preview it returns against the
// hand-written `EquityPreview` interface: the same keys, no more and no fewer, and the
// value kinds the interface promises.
import { readFileSync } from 'node:fs';
import { beforeAll, describe, expect, it } from 'vitest';
import { equityPreview, exampleProfiles, initEngineSync } from './api';
import { PREVIEW_OUTCOMES, type EquityPreview } from './equity-types';
import type { OutcomeRow, PrivateCompany } from './types';

/** Every key of the interface. The compiler rejects this object when the interface gains or loses a field. */
const KEYS: Record<keyof EquityPreview, true> = {
  draws: true,
  table: true,
  typical: true,
  fmv_409a: true,
  preference_overhang: true,
  tender_annual_probability: true,
  paper_value_vested: true,
  paper_value_total: true,
  exercise_cost_vested: true,
  exercise_cost_total: true,
  spread_iso_vested: true,
  spread_nso_vested: true,
  nothing_share: true,
  outcome_shares: true,
  acquisition_zero_share: true,
  ipo_below_last_round_share: true,
  tender_before_share: true,
  option_deadline_months: true,
  option_deadline_cost: true,
  deadline_before_outcome_share: true,
  dots: true,
  quantiles: true,
  if_paid: true,
  years_to_payout_p50: true,
  implied_multiple: true,
  typical_implied_multiple: true,
};

const ROW_KEYS: Record<keyof OutcomeRow, true> = { id: true, kind: true, probability: true, years_from: true, years_to: true, multiple_low: true, multiple_high: true };

const ARRAYS = ['table', 'typical', 'outcome_shares', 'dots', 'quantiles', 'if_paid'] as const;
const NULLABLE = ['years_to_payout_p50', 'option_deadline_months'] as const;

let company: PrivateCompany;

beforeAll(() => {
  initEngineSync(readFileSync(new URL('./pkg/firesight_wasm_bg.wasm', import.meta.url)));
  const found = exampleProfiles().find((e) => e.slug === 'startup-employee')?.profile.private_equity[0];
  if (!found) throw new Error('the startup-employee demo is missing');
  company = found;
});

function check(preview: EquityPreview) {
  const raw = preview as unknown as Record<string, unknown>;
  expect(Object.keys(raw).sort()).toEqual(Object.keys(KEYS).sort());
  for (const key of Object.keys(KEYS)) {
    if ((ARRAYS as readonly string[]).includes(key)) expect(Array.isArray(raw[key]), key).toBe(true);
    else if ((NULLABLE as readonly string[]).includes(key)) expect(raw[key] === null || Number.isFinite(raw[key]), key).toBe(true);
    else expect(Number.isFinite(raw[key]), key).toBe(true);
  }
  expect(preview.outcome_shares).toHaveLength(PREVIEW_OUTCOMES.length);
  expect(preview.quantiles).toHaveLength(5);
  expect(preview.if_paid).toHaveLength(3);
  expect(preview.dots).toHaveLength(100);
  for (const v of [...preview.outcome_shares, ...preview.quantiles, ...preview.dots]) expect(Number.isFinite(v)).toBe(true);
  for (const v of preview.if_paid) expect(v === null || Number.isFinite(v)).toBe(true);
  for (const row of [...preview.table, ...preview.typical]) {
    expect(Object.keys(row).sort()).toEqual(Object.keys(ROW_KEYS).sort());
    expect(['fails', 'acquired', 'ipo', 'still_private']).toContain(row.kind);
  }
}

describe('EquityPreview matches the engine', () => {
  it('on the demo company', () => {
    const preview = equityPreview(company);
    check(preview);
    expect(preview.outcome_shares.reduce((a, b) => a + b, 0)).toBeCloseTo(1, 9);
    expect(preview.table.reduce((a, row) => a + row.probability, 0)).toBeCloseTo(1, 9);
    expect(preview.if_paid.every((v) => v !== null)).toBe(true);
    expect(preview.years_to_payout_p50).not.toBeNull();
    expect([...preview.dots].sort((a, b) => a - b)).toEqual(preview.dots);
  });

  it('on every stage with nothing but a price', () => {
    for (const stage of ['seed', 'series_a', 'series_b', 'series_c', 'late', 'unicorn'] as const) check(equityPreview({ stage, preferred_price: 1 }));
  });

  it('gives nulls, not NaN, when no future pays', () => {
    const certainFailure: OutcomeRow = { id: 'fails', kind: 'fails', probability: 1, years_from: 1, years_to: 3, multiple_low: 0, multiple_high: 0 };
    const preview = equityPreview({ ...company, outcomes: [certainFailure] });
    check(preview);
    expect(preview.nothing_share).toBe(1);
    expect(preview.if_paid).toEqual([null, null, null]);
    expect(preview.years_to_payout_p50).toBeNull();
    // In the order of PREVIEW_OUTCOMES: every draw fails.
    expect(preview.outcome_shares).toEqual([1, 0, 0, 0]);
  });

  it('lists the outcome shares in the order of PREVIEW_OUTCOMES', () => {
    expect(PREVIEW_OUTCOMES).toEqual(['fails', 'written_off', 'acquired', 'ipo']);
    const certainIpo: OutcomeRow = { id: 'ipo', kind: 'ipo', probability: 1, years_from: 2, years_to: 2, multiple_low: 2, multiple_high: 2 };
    expect(equityPreview({ ...company, outcomes: [certainIpo] }).outcome_shares).toEqual([0, 0, 0, 1]);
    const certainSale: OutcomeRow = { id: 'sale', kind: 'acquired', probability: 1, years_from: 2, years_to: 2, multiple_low: 2, multiple_high: 2 };
    expect(equityPreview({ ...company, outcomes: [certainSale] }).outcome_shares).toEqual([0, 0, 1, 0]);
    const neverExits: OutcomeRow = { id: 'private', kind: 'still_private', probability: 1, years_from: 0, years_to: 0, multiple_low: 0, multiple_high: 0 };
    expect(equityPreview({ ...company, outcomes: [neverExits] }).outcome_shares).toEqual([0, 1, 0, 0]);
  });
});
