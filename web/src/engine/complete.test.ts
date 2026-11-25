// The table of omitted defaults, checked against the schema and the engine.
import { readFileSync } from 'node:fs';
import { beforeAll, describe, expect, it } from 'vitest';
import * as wasm from './pkg/firesight_wasm.js';
import { OMITTED_DEFAULTS, completeProfile, omittedFields } from './complete';
import type { Profile } from './types';

let schema: unknown;
let raw: { slug: string; profile: Record<string, unknown> }[] = [];

beforeAll(() => {
  wasm.initSync({ module: readFileSync(new URL('./pkg/firesight_wasm_bg.wasm', import.meta.url)) });
  schema = JSON.parse(wasm.schema());
  raw = JSON.parse(wasm.exampleProfiles()) as typeof raw;
});

const clone = <T>(v: T): T => JSON.parse(JSON.stringify(v)) as T;
/** The profile as the engine writes it back. */
const normalised = (profile: unknown, ops: unknown[] = []) => (JSON.parse(wasm.applyPatch(JSON.stringify(profile), JSON.stringify(ops))) as { profile: Record<string, unknown> }).profile;

describe('completing a profile', () => {
  it('knows a default for every field the engine omits, and no others', () => {
    // Allocation weights are the one deliberate exception: a missing asset class is a zero weight (see types.ts).
    const omitted = omittedFields(schema).filter((key) => !key.startsWith('Allocation.'));
    expect(Object.keys(OMITTED_DEFAULTS).sort()).toEqual(omitted.sort());
  });

  it('fills the omitted fields of every demo household with values the engine reads as defaults', () => {
    expect(raw.length).toBe(4);
    for (const { slug, profile } of raw) {
      const completed = completeProfile(schema, clone(profile)) as unknown as Profile;
      // Every field the editor reads is there...
      expect(completed.spending.vehicles, slug).toBeInstanceOf(Array);
      expect(completed.household.parents, slug).toBeInstanceOf(Array);
      expect(completed.household.divorce_risk.type, slug).toBeTypeOf('string');
      expect(completed.portfolio.distress.keep_match, slug).toBeTypeOf('boolean');
      expect(completed.retirement.home_equity_borrowing.type, slug).toBeTypeOf('string');
      expect(completed.assumptions.health_model.type, slug).toBeTypeOf('string');
      for (const person of completed.household.people) {
        expect(person.life_insurance, slug).toBeInstanceOf(Array);
        expect(person.social_security.claiming.type, slug).toBeTypeOf('string');
        expect(person.disability_risk, slug).toBeTypeOf('string');
      }
      for (const child of completed.household.children) expect(child.arrival.type, slug).toBeTypeOf('string');
      for (const income of completed.incomes) {
        if (income.kind.type === 'job') expect(income.kind.risk.job_mobility, slug).toBeTypeOf('string');
        if (income.kind.type === 'self_employment') expect(income.kind.business_risk, slug).toBeTypeOf('string');
      }
      for (const property of completed.properties) {
        expect(property.assessment.type, slug).toBeTypeOf('string');
        if (property.mortgage) expect(property.mortgage.kind.type, slug).toBeTypeOf('string');
        if (property.use.type === 'rental') expect(property.use.dynamics.type, slug).toBeTypeOf('string');
      }
      for (const event of completed.events) if (event.kind.type === 'buy_home') expect(event.kind.mortgage_kind.type, slug).toBeTypeOf('string');
      // ...and the engine writes the completed profile back exactly as it wrote the original: every filled value is the default.
      expect(normalised(completed), slug).toEqual(profile);
      // Nothing is left to fill a second time.
      expect(completeProfile(schema, clone(completed))).toEqual(completed);
    }
  });

  it('fills the fields inside optional parts once those are switched on', () => {
    const landlord = raw.find((e) => e.slug === 'side-hustle-landlord')!.profile;
    const written = normalised(landlord, [
      { op: 'set', path: '/properties/@duplex/assessment', value: { type: 'capped', annual_cap: 0.02 } },
      { op: 'set', path: '/household/parents', value: [{ id: 'mom', name: 'Mom', birth: '1956-03', care_support: { annual_cost: 5000 } }] },
    ]);
    const completed = completeProfile(schema, clone(written)) as unknown as Profile;
    expect(completed.properties[0]!.assessment).toEqual({ type: 'capped', annual_cap: 0.02, assessed_value: null });
    expect(completed.household.parents[0]!.care_support?.caregiver).toBeNull();
    expect(normalised(completed)).toEqual(written);
  });

  it('lets a patch append to a list the engine had left out', () => {
    const family = raw.find((e) => e.slug === 'dual-income-family')!.profile;
    const op = [{ op: 'append', path: '/spending/vehicles', value: { id: 'car', name: 'Car', price: 30000 } }];
    expect(() => normalised(family, op)).toThrow(/no field "vehicles"/);
    const completed = completeProfile(schema, clone(family));
    expect((normalised(completed, op).spending as { vehicles: unknown[] }).vehicles).toHaveLength(1);
  });
});
