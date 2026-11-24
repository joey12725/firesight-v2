// The optional realism controls, checked against the real engine: every value the editor
// writes when a feature is switched on is accepted and leaves the plan valid, each
// feature's "on" test sees exactly its own switch, nothing is on in a new plan, and each
// has a control in the section it claims.
import { readFileSync } from 'node:fs';
import { beforeAll, describe, expect, it } from 'vitest';
import { applyPatch, defaultProfile, exampleProfiles, initEngineSync, validate } from '@/engine/api';
import type { PatchOp, Profile } from '@/engine/types';
import { append, set } from '@/state/patches';
import {
  FEATURES,
  MAKE,
  careerRealism,
  childRealism,
  claimingRealism,
  debtPolicyRealism,
  familyRealism,
  featuresOn,
  healthcareRealism,
  homeRealism,
  lifeRealism,
  marketRealism,
  personRealism,
  portfolioRealism,
  propertyRealism,
  spendingRealism,
  taxRealism,
  timingRealism,
  type SectionId,
} from './realism';

let examples = new Map<string, Profile>();

beforeAll(() => {
  initEngineSync(readFileSync(new URL('../engine/pkg/firesight_wasm_bg.wasm', import.meta.url)));
  examples = new Map(exampleProfiles().map((e) => [e.slug, e.profile]));
});

const FAMILY = 'dual-income-family';
const LANDLORD = 'side-hustle-landlord';
const person = '/household/people/0';
const job = '/incomes/@priya_job/kind/risk';
const duplex = '/properties/@duplex';
const purchase = (p: Profile) => `/events/@${p.events.find((e) => e.kind.type === 'buy_home')?.id}/kind`;

/** What the editor does to switch each feature on: the plan it is tried on, and the edits. `also` lists features that come with it. */
const ENABLE: Record<string, { on: string; ops: (p: Profile) => PatchOp[]; also?: string[] }> = {
  F01: { on: FAMILY, ops: () => [append('/spending/vehicles', { id: 'car', name: 'Car', ...MAKE.vehicle() }), set('/spending/vehicles/@car/financing', MAKE.vehicleFinancing())] },
  F02: { on: FAMILY, ops: () => [set('/spending/seasonality', 'typical')] },
  F03: { on: FAMILY, ops: () => [set('/spending/category_inflation', 'stochastic')] },
  F04: { on: FAMILY, ops: () => [set('/spending/shocks', 'detailed')] },
  F05: { on: FAMILY, ops: () => [set('/spending/unemployment_cut', 0.1)] },
  F06: { on: FAMILY, ops: () => [set('/spending/healthcare/cover_children', true), set('/spending/healthcare/children_coverage_end_age', 24)] },
  F07: { on: FAMILY, ops: () => [set('/household/children/@second_child/arrival', { type: 'uncertain' })] },
  F08: { on: FAMILY, ops: () => [set('/household/children/@anika/college_cost', { type: 'stochastic' })] },
  F09: { on: FAMILY, ops: () => [set('/household/children/@anika/adult_support', MAKE.adultSupport())] },
  F10: { on: FAMILY, ops: () => [set('/household/children/@second_child/parental_leave', MAKE.parentalLeave(0)), set('/household/children/@second_child/second_parental_leave', MAKE.parentalLeave(1))] },
  F11: { on: LANDLORD, ops: () => [set(`${duplex}/maintenance`, 'lumpy')] },
  F12: { on: LANDLORD, ops: () => [set(`${duplex}/assessment`, MAKE.assessment('capped')), set(`${duplex}/insurance_real_growth`, 'stochastic')] },
  F13: { on: FAMILY, ops: () => [set('/spending/items/@rent/rent_growth', 'market')] },
  F14: { on: FAMILY, ops: () => [set('/assumptions/housing_liquidity', 'realistic')] },
  F15: { on: LANDLORD, ops: () => [set(`${duplex}/mortgage/kind`, MAKE.mortgageKind('arm'))] },
  F16: { on: FAMILY, ops: () => [set(`${job}/job_mobility`, 'high')] },
  F17: { on: FAMILY, ops: () => [set(`${job}/recession_pay`, 'typical')] },
  F18: { on: FAMILY, ops: () => [set(`${person}/disability_risk`, 'typical'), set(`${person}/ltd_insurance`, MAKE.ltdInsurance())] },
  F19: { on: FAMILY, ops: () => [set(`${job}/late_career`, 'realistic')] },
  F20: { on: LANDLORD, ops: () => [set('/incomes/@delivery/kind/business_risk', 'typical'), set('/incomes/@delivery/kind/fallback_wage', MAKE.fallbackWage())] },
  F21: { on: FAMILY, ops: () => [set('/assumptions/taxes/chained_cpi', true)] },
  F22: { on: FAMILY, ops: () => [set('/assumptions/taxes/state_detail', 'full')] },
  F23: { on: FAMILY, ops: () => [set('/assumptions/taxes/policy_risk', MAKE.policyRisk())] },
  F24: { on: FAMILY, ops: () => [set('/portfolio/tax_loss_harvesting', true)] },
  F25: { on: FAMILY, ops: () => [set('/retirement/qcd', true)] },
  F26: { on: FAMILY, ops: () => [set('/retirement/home_equity_borrowing', { type: 'heloc' })] },
  F27: { on: FAMILY, ops: () => [set('/retirement/home_equity_borrowing', { type: 'reverse_mortgage' }), set('/retirement/home_equity_borrowing/open_when', MAKE.reverseOpen('savings_below'))] },
  F28: { on: FAMILY, ops: () => [set('/retirement/downsize', MAKE.downsize()), set('/retirement/downsize/when', MAKE.downsizeWhen('at_age')), set('/retirement/downsize/when', MAKE.downsizeWhen('when_funded_ratio_below'))] },
  F29: { on: FAMILY, ops: () => [set('/debt_policy/cash_out_refinance', MAKE.cashOutRefinance()), set('/debt_policy/mortgage_prepayment', { type: 'when_rate_above' })] },
  F30: { on: LANDLORD, ops: () => [set(`${duplex}/use/dynamics`, { type: 'realistic' })] },
  F31: { on: FAMILY, ops: () => [set('/assumptions/health_model', { type: 'states' })] },
  F32: { on: FAMILY, ops: () => [set('/assumptions/longevity/couple_dependence', { type: 'linked' })] },
  F33: { on: FAMILY, ops: () => [set('/assumptions/longevity/income_gradient', true)] },
  F34: { on: FAMILY, ops: () => [set('/spending/healthcare/medicaid', true)] },
  F35: { on: FAMILY, ops: () => [set(`${person}/ltc_insurance`, MAKE.ltcInsurance()), set(`${person}/ltc_insurance/inflation_rider`, MAKE.inflationRider('compound'))] },
  F36: { on: FAMILY, ops: () => [set('/assumptions/market/bond_pricing', 'from_yield')] },
  F37: { on: FAMILY, ops: () => [set('/assumptions/market/valuation', MAKE.valuation())] },
  F38: { on: FAMILY, ops: () => [append('/assumptions/market/stress', MAKE.historicalEpisode()), append('/assumptions/market/stress', MAKE.lostGeneration())] },
  F39: { on: FAMILY, ops: () => [set('/assumptions/market/kind', { type: 'fat_tailed' })] },
  F40: { on: FAMILY, ops: () => [set('/assumptions/market/dynamic_yields', true)] },
  F41: { on: FAMILY, ops: () => [set('/retirement/return_to_work', MAKE.returnToWork()), set('/retirement/return_to_work/search', MAKE.jobSearch('fixed'))] },
  F42: { on: FAMILY, ops: () => [set('/portfolio/behavior', { type: 'panic_prone' })] },
  F43: { on: FAMILY, ops: () => [set(`${person}/social_security/claiming`, { type: 'adaptive' })] },
  F44: { on: FAMILY, ops: () => [set('/portfolio/pause_deferrals_in_distress', true)] },
  F45: { on: FAMILY, ops: () => [set('/retirement/trigger/confirmation', MAKE.confirmation()), set('/retirement/trigger/one_more_year', MAKE.oneMoreYear())] },
  F46: { on: FAMILY, ops: () => [set('/household/married', '2019-06'), set('/household/divorce_risk', { type: 'typical' })] },
  F47: { on: FAMILY, ops: () => [append('/household/parents', { id: 'mom', name: 'Mom', ...MAKE.parent('1956-03') })] },
  F48: { on: FAMILY, ops: () => [append('/household/parents', { id: 'mom', name: 'Mom', ...MAKE.parent('1956-03') }), set('/household/parents/@mom/care_support', MAKE.careSupport())], also: ['F47'] },
  F49: { on: FAMILY, ops: () => [set('/spending/healthcare/spousal_care', true)] },
  F50: { on: FAMILY, ops: () => [append(`${person}/life_insurance`, { id: 'term', name: 'Term life', ...MAKE.lifePolicy() }), set(`${person}/life_insurance/@term/kind`, MAKE.policyKind('permanent'))] },
};

const base = (slug: string) => examples.get(slug) as Profile;
const ids = (profile: Profile) => featuresOn(profile).map((f) => f.id);

describe('optional realism features', () => {
  it('are fifty, each with a section', () => {
    expect(FEATURES.map((f) => f.id)).toEqual(Array.from({ length: 50 }, (_, i) => `F${String(i + 1).padStart(2, '0')}`));
    expect(Object.keys(ENABLE).sort()).toEqual(FEATURES.map((f) => f.id));
  });

  it('are all off in a new plan and in the three everyday demo households', () => {
    expect(ids(defaultProfile())).toEqual([]);
    for (const slug of ['dual-income-family', 'early-career-saver', 'side-hustle-landlord']) expect(ids(base(slug)), slug).toEqual([]);
  });

  it('are widely used by the full-life demo household', () => {
    const on = ids(base('full-life'));
    expect(on.length).toBeGreaterThanOrEqual(30);
    expect(validate(base('full-life')).errors).toEqual([]);
  });

  for (const feature of FEATURES) {
    it(`${feature.id} ${feature.label}: the editor's edits are accepted and switch on only it`, () => {
      const how = ENABLE[feature.id]!;
      const before = base(how.on);
      const result = applyPatch(before, how.ops(before));
      expect(result.errors, JSON.stringify(result.errors)).toEqual([]);
      // The engine has filled in every field the editor left out, and the plan is still one it can simulate.
      expect(validate(result.profile).errors).toEqual([]);
      expect(ids(result.profile).sort()).toEqual([feature.id, ...(how.also ?? [])].sort());
    });
  }

  it('also cover a planned purchase and a dated divorce', () => {
    const family = base(FAMILY);
    const bought = applyPatch(family, [
      set(`${purchase(family)}/mortgage_kind`, MAKE.mortgageKind('arm')),
      set(`${purchase(family)}/maintenance`, 'lumpy'),
      set(`${purchase(family)}/assessment`, MAKE.assessment('capped')),
    ]);
    expect(bought.errors).toEqual([]);
    expect(validate(bought.profile).errors).toEqual([]);
    expect(ids(bought.profile)).toEqual(['F11', 'F12', 'F15']);

    const divorced = applyPatch(family, [append('/events', { id: 'divorce', name: 'Divorce', when: { type: 'date', at: '2036-01' }, kind: MAKE.divorce() })]);
    expect(divorced.errors).toEqual([]);
    expect(validate(divorced.profile).errors).toEqual([]);
    expect(ids(divorced.profile)).toEqual(['F46']);
    // Back to a fixed rate: the variant is written whole, so nothing of the ARM is left behind.
    const fixed = applyPatch(bought.profile, [set(`${purchase(family)}/mortgage_kind`, MAKE.mortgageKind('fixed'))]);
    expect(ids(fixed.profile)).toEqual(['F11', 'F12']);
  });
});

describe('the counts shown on each folded group', () => {
  it('are zero for an everyday plan', () => {
    const p = base(FAMILY);
    const counts = [
      ...p.household.people.map(personRealism),
      ...p.household.children.map(childRealism),
      familyRealism(p.household),
      ...p.incomes.map((i) => (i.kind.type === 'job' ? careerRealism(i.kind.risk) : 0)),
      claimingRealism(p.household.people),
      spendingRealism(p.spending),
      healthcareRealism(p.spending.healthcare),
      portfolioRealism(p.portfolio),
      debtPolicyRealism(p.debt_policy),
      timingRealism(p.retirement.trigger),
      homeRealism(p.retirement),
      marketRealism(p.assumptions.market),
      taxRealism(p.assumptions.taxes),
      lifeRealism(p.assumptions),
    ];
    expect(counts.every((n) => n === 0)).toBe(true);
  });

  it('count what is switched on beneath them', () => {
    const p = applyPatch(base(FAMILY), [
      ...ENABLE.F02!.ops(base(FAMILY)),
      ...ENABLE.F05!.ops(base(FAMILY)),
      ...ENABLE.F18!.ops(base(FAMILY)),
      ...ENABLE.F50!.ops(base(FAMILY)),
      ...ENABLE.F16!.ops(base(FAMILY)),
      ...ENABLE.F21!.ops(base(FAMILY)),
      ...ENABLE.F23!.ops(base(FAMILY)),
      ...ENABLE.F45!.ops(base(FAMILY)),
      ...ENABLE.F32!.ops(base(FAMILY)),
      ...ENABLE.F14!.ops(base(FAMILY)),
    ]).profile;
    expect(spendingRealism(p.spending)).toBe(2);
    expect(personRealism(p.household.people[0]!)).toBe(2);
    expect(personRealism(p.household.people[1]!)).toBe(0);
    expect(careerRealism((p.incomes[0]!.kind as { risk: Parameters<typeof careerRealism>[0] }).risk)).toBe(1);
    expect(taxRealism(p.assumptions.taxes)).toBe(2);
    expect(timingRealism(p.retirement.trigger)).toBe(2);
    expect(lifeRealism(p.assumptions)).toBe(2);
    const landlord = applyPatch(base(LANDLORD), [...ENABLE.F11!.ops(base(LANDLORD)), ...ENABLE.F12!.ops(base(LANDLORD))]).profile;
    expect(propertyRealism(landlord.properties[0]!)).toBe(3);
  });
});

describe('every feature has a control where a user would look', () => {
  const FILE: Record<SectionId, string> = {
    household: 'Household',
    income: 'Income',
    spending: 'Spending',
    accounts: 'Accounts',
    property: 'Property',
    events: 'Events',
    retirement: 'Retirement',
    assumptions: 'Assumptions',
  };
  const source = (section: SectionId) => readFileSync(new URL(`./sections/${FILE[section]}.tsx`, import.meta.url), 'utf8');

  for (const feature of FEATURES) {
    it(`${feature.id} ${feature.label} is in the ${feature.section} section`, () => {
      // The section's form names the field in a patch path or as a choice value (or builds the value through MAKE).
      const text = source(feature.section);
      const camel = feature.field.replace(/_(\w)/g, (_, c: string) => c.toUpperCase());
      expect(text.includes(`/${feature.field}`) || text.includes(`'${feature.field}'`) || text.includes(`MAKE.${camel}`), feature.field).toBe(true);
    });
  }

  it('cash-out refinancing is offered to an owner with no mortgage', () => {
    const text = source('property');
    // The group shows for any main home, owned or planned; only the mortgage's own controls need a mortgage.
    expect(text).toMatch(/\{\(mortgaged \|\| hasHome\) && \(/);
    const cashOut = text.indexOf('path="/debt_policy/cash_out_refinance"');
    const gated = [...text.matchAll(/\{mortgaged && \(/g)].map((m) => m.index);
    expect(gated).toHaveLength(2);
    // The cash-out control sits between the two gated blocks (refinancing above it, prepayment below), inside neither.
    expect(cashOut).toBeGreaterThan(text.indexOf(')}', gated[0]));
    expect(cashOut).toBeLessThan(gated[1] as number);
  });

  it('care for a parent that costs nothing and takes no time is not counted as on', () => {
    const withParent = applyPatch(base(FAMILY), [append('/household/parents', { id: 'mom', name: 'Mom', ...MAKE.parent('1956-03') })]).profile;
    const idle = applyPatch(withParent, [set('/household/parents/@mom/care_support', { annual_cost: 0, hours: 0 })]).profile;
    expect(ids(idle)).not.toContain('F48');
    expect(familyRealism(idle.household)).toBe(1);
    const hours = applyPatch(withParent, [set('/household/parents/@mom/care_support', { annual_cost: 0, hours: 0.2 })]).profile;
    expect(ids(hours)).toContain('F48');
    // The full-life example's parent care is real: it is counted, and it costs something.
    const full = examples.get('full-life') as Profile;
    expect(ids(full)).toContain('F48');
    expect(full.household.parents.some((x) => x.care_support && x.care_support.annual_cost > 0 && x.care_support.hours > 0)).toBe(true);
  });

  it('the divorce life event and an adjustable-rate purchase are in the events section', () => {
    expect(source('events')).toContain("type: 'divorce'");
    expect(source('events')).toContain('/mortgage_kind');
  });
});
