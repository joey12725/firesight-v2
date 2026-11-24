// The optional realism features (docs/methodology.md, section 18): what the editor
// writes when one is switched on, how many are on under each "optional realism"
// disclosure, and the list of all fifty with where they live. Every feature is off in a
// new plan; the editor keeps them folded away until asked for.
import type { Profile } from '@/engine/types';

type Household = Profile['household'];
type Person = Household['people'][number];
type Child = Household['children'][number];
type Spending = Profile['spending'];
type Retirement = Profile['retirement'];
type Assumptions = Profile['assumptions'];
type Rental = Extract<Profile['properties'][number]['use'], { type: 'rental' }>;

// ---- what the editor writes ---------------------------------------------------------
//
// The engine fills in every field left out, so most of these name only the variant or
// the fields a schema marks as required. `realism.test.ts` applies each one to a real
// plan and checks the engine accepts it.

export const MAKE = {
  ltdInsurance: () => ({}),
  ltcInsurance: () => ({}),
  inflationRider: (type: string) => (type === 'compound' ? { type, rate: 0.03 } : { type }),
  lifePolicy: () => ({ kind: { type: 'term', years_remaining: 20 }, death_benefit: 500_000, annual_premium: 600 }),
  policyKind: (type: string) => (type === 'term' ? { type, years_remaining: 20 } : { type }),
  adultSupport: () => ({}),
  parentalLeave: (person: number) => ({ person }),
  parent: (birth: string) => ({ birth, estate: 200_000, share: 0.5 }),
  careSupport: () => ({ annual_cost: 7_000 }),
  fallbackWage: () => ({}),
  vehicle: () => ({ price: 35_000, replace_every_years: 10, age_years: 5 }),
  vehicleFinancing: () => ({}),
  arm: () => ({ type: 'arm', fixed_months: 60, adjust_every_months: 12, margin: 0.0275, periodic_cap: 0.02, lifetime_cap: 0.05 }),
  mortgageKind: (type: string) => (type === 'arm' ? MAKE.arm() : { type }),
  assessment: (type: string) => (type === 'capped' ? { type, annual_cap: 0.02 } : { type }),
  cashOutRefinance: () => ({}),
  confirmation: () => ({}),
  oneMoreYear: () => ({}),
  returnToWork: () => ({}),
  jobSearch: (type: string) => (type === 'fixed' ? { type, months: 6 } : { type }),
  reverseOpen: (type: string) => (type === 'savings_below' ? { type, years: 5 } : { type }),
  downsize: () => ({}),
  downsizeWhen: (type: string) => (type === 'at_age' ? { type, person: 0, age: 70 } : type === 'when_funded_ratio_below' ? { type, ratio: 0.8 } : { type }),
  valuation: () => ({}),
  policyRisk: () => ({}),
  historicalEpisode: () => ({ type: 'historical_episode', start: { type: 'retirement', offset_years: 0 }, episode: 'financial2008' }),
  lostGeneration: () => ({ type: 'lost_generation', start: { type: 'retirement', offset_years: 0 } }),
  divorce: () => ({ type: 'divorce' }),
};

// ---- how many are on ----------------------------------------------------------------

const count = (...flags: unknown[]) => flags.filter(Boolean).length;

export const personRealism = (p: Person) => count(p.disability_risk !== 'off', p.ltc_insurance, p.life_insurance.length > 0);
export const childRealism = (c: Child) => count(c.arrival.type === 'uncertain', c.college_cost.type === 'stochastic', c.adult_support, c.parental_leave ?? c.second_parental_leave);
export const familyRealism = (h: Household) => count(h.parents.length > 0, h.parents.some((p) => p.care_support), h.divorce_risk.type !== 'off');
export const careerRealism = (risk: { job_mobility: string; recession_pay: string; late_career: string }) => count(risk.job_mobility !== 'off', risk.recession_pay !== 'off', risk.late_career !== 'off');
export const claimingRealism = (people: Person[]) => count(...people.map((p) => p.social_security.claiming.type === 'adaptive'));
export const spendingRealism = (s: Spending) => count(s.seasonality !== 'off', s.category_inflation !== 'off', s.unemployment_cut > 0);
export const healthcareRealism = (h: Spending['healthcare']) => count(h.cover_children, h.medicaid, h.spousal_care);
export const portfolioRealism = (p: Profile['portfolio']) => count(p.tax_loss_harvesting, p.behavior.type === 'panic_prone', p.pause_deferrals_in_distress);
export const propertyRealism = (p: { maintenance: string; assessment: { type: string }; insurance_real_growth: string }) =>
  count(p.maintenance === 'lumpy', p.assessment.type === 'capped', p.insurance_real_growth === 'stochastic');
export const rentalRealism = (use: Rental) => count(use.dynamics.type === 'realistic');
export const debtPolicyRealism = (d: Profile['debt_policy']) => count(d.cash_out_refinance, d.mortgage_prepayment.type !== 'never');
export const timingRealism = (t: Retirement['trigger']) => (t.type === 'when_funded' ? count(t.confirmation, t.one_more_year) : 0);
export const homeRealism = (r: Retirement) => count(r.home_equity_borrowing.type !== 'none', r.downsize);
export const marketRealism = (m: Assumptions['market']) => count(m.bond_pricing === 'from_yield', m.valuation, m.dynamic_yields);
export const taxRealism = (t: Assumptions['taxes']) => count(t.chained_cpi, t.state_detail === 'full', t.policy_risk);
export const lifeRealism = (a: Assumptions) =>
  count(a.longevity.type === 'stochastic' && a.longevity.couple_dependence.type === 'linked', a.longevity.type === 'stochastic' && a.longevity.income_gradient, a.health_model.type === 'states', a.housing_liquidity === 'realistic');

// ---- the fifty ----------------------------------------------------------------------

export type SectionId = 'household' | 'income' | 'spending' | 'accounts' | 'property' | 'events' | 'retirement' | 'assumptions';

export interface Feature {
  id: string;
  /** The schema field, variant or value its control writes: the word to look for in the section's source. */
  field: string;
  label: string;
  /** The editor section its control is in. */
  section: SectionId;
  /** Whether the plan uses it. */
  on: (p: Profile) => boolean;
}

const jobs = (p: Profile) => p.incomes.flatMap((i) => (i.kind.type === 'job' ? [i.kind] : []));
const businesses = (p: Profile) => p.incomes.flatMap((i) => (i.kind.type === 'self_employment' ? [i.kind] : []));
const rentals = (p: Profile) => p.properties.flatMap((x) => (x.use.type === 'rental' ? [x.use] : []));
const homes = (p: Profile) => [...p.properties, ...p.events.flatMap((e) => (e.kind.type === 'buy_home' ? [e.kind] : []))];
const mortgageKinds = (p: Profile) => [...p.properties.flatMap((x) => (x.mortgage ? [x.mortgage.kind] : [])), ...p.events.flatMap((e) => (e.kind.type === 'buy_home' ? [e.kind.mortgage_kind] : []))];
const stresses = (p: Profile) => p.assumptions.market.stress;

export const FEATURES: Feature[] = [
  { id: 'F01', field: 'vehicles', label: 'Vehicle replacement cycles', section: 'spending', on: (p) => p.spending.vehicles.length > 0 },
  { id: 'F02', field: 'seasonality', label: 'Seasonal spending', section: 'spending', on: (p) => p.spending.seasonality !== 'off' },
  { id: 'F03', field: 'category_inflation', label: 'Personal inflation by category', section: 'spending', on: (p) => p.spending.category_inflation !== 'off' },
  { id: 'F04', field: 'detailed', label: 'Detailed unplanned expenses', section: 'spending', on: (p) => p.spending.shocks === 'detailed' },
  { id: 'F05', field: 'unemployment_cut', label: 'Belt-tightening when unemployed', section: 'spending', on: (p) => p.spending.unemployment_cut > 0 },
  { id: 'F06', field: 'cover_children', label: 'Children on the health plan', section: 'spending', on: (p) => p.spending.healthcare.cover_children },
  { id: 'F07', field: 'arrival', label: 'Uncertain arrival of planned children', section: 'household', on: (p) => p.household.children.some((c) => c.arrival.type === 'uncertain') },
  { id: 'F08', field: 'college_cost', label: 'College cost uncertainty', section: 'household', on: (p) => p.household.children.some((c) => c.college_cost.type === 'stochastic') },
  { id: 'F09', field: 'adult_support', label: 'Adult children who need support', section: 'household', on: (p) => p.household.children.some((c) => c.adult_support) },
  { id: 'F10', field: 'parental_leave', label: 'Parental leave', section: 'household', on: (p) => p.household.children.some((c) => c.parental_leave ?? c.second_parental_leave) },
  { id: 'F11', field: 'maintenance', label: 'Lumpy home maintenance', section: 'property', on: (p) => homes(p).some((h) => h.maintenance === 'lumpy') },
  { id: 'F12', field: 'assessment', label: 'Assessment caps and insurance drift', section: 'property', on: (p) => homes(p).some((h) => h.assessment.type === 'capped' || h.insurance_real_growth === 'stochastic') },
  { id: 'F13', field: 'rent_growth', label: 'Market rents', section: 'spending', on: (p) => p.spending.items.some((i) => i.rent_growth === 'market') },
  { id: 'F14', field: 'housing_liquidity', label: 'Home sales take time', section: 'assumptions', on: (p) => p.assumptions.housing_liquidity === 'realistic' },
  { id: 'F15', field: 'arm', label: 'Adjustable-rate mortgages', section: 'property', on: (p) => mortgageKinds(p).some((k) => k.type === 'arm') },
  { id: 'F16', field: 'job_mobility', label: 'Job changes with pay jumps', section: 'income', on: (p) => jobs(p).some((j) => j.risk.job_mobility !== 'off') },
  { id: 'F17', field: 'recession_pay', label: 'Recession pay freezes and cuts', section: 'income', on: (p) => jobs(p).some((j) => j.risk.recession_pay !== 'off') },
  { id: 'F18', field: 'disability_risk', label: 'Disability', section: 'household', on: (p) => p.household.people.some((x) => x.disability_risk !== 'off') },
  { id: 'F19', field: 'late_career', label: 'Involuntary retirement after 55', section: 'income', on: (p) => jobs(p).some((j) => j.risk.late_career !== 'off') },
  { id: 'F20', field: 'business_risk', label: 'Business cycles and failure', section: 'income', on: (p) => businesses(p).some((b) => b.business_risk !== 'off') },
  { id: 'F21', field: 'chained_cpi', label: 'Chained-CPI indexing', section: 'assumptions', on: (p) => p.assumptions.taxes.chained_cpi },
  { id: 'F22', field: 'state_detail', label: 'Full state tax detail', section: 'assumptions', on: (p) => p.assumptions.taxes.state_detail === 'full' },
  { id: 'F23', field: 'policy_risk', label: 'Policy risk', section: 'assumptions', on: (p) => p.assumptions.taxes.policy_risk !== null },
  { id: 'F24', field: 'tax_loss_harvesting', label: 'Tax-loss harvesting', section: 'accounts', on: (p) => p.portfolio.tax_loss_harvesting },
  { id: 'F25', field: 'qcd', label: 'Qualified charitable distributions', section: 'retirement', on: (p) => p.retirement.qcd },
  { id: 'F26', field: 'heloc', label: 'Home-equity line of credit', section: 'retirement', on: (p) => p.retirement.home_equity_borrowing.type === 'heloc' },
  { id: 'F27', field: 'reverse_mortgage', label: 'Reverse mortgage', section: 'retirement', on: (p) => p.retirement.home_equity_borrowing.type === 'reverse_mortgage' },
  { id: 'F28', field: 'downsize', label: 'Planned downsizing', section: 'retirement', on: (p) => p.retirement.downsize !== null },
  { id: 'F29', field: 'cash_out_refinance', label: 'Cash-out refinancing and rate-aware prepayment', section: 'property', on: (p) => debtPolicyRealism(p.debt_policy) > 0 },
  { id: 'F30', field: 'dynamics', label: 'Realistic rentals', section: 'property', on: (p) => rentals(p).some((r) => r.dynamics.type === 'realistic') },
  { id: 'F31', field: 'health_model', label: 'Health states', section: 'assumptions', on: (p) => p.assumptions.health_model.type === 'states' },
  { id: 'F32', field: 'couple_dependence', label: 'Linked lifespans', section: 'assumptions', on: (p) => p.assumptions.longevity.type === 'stochastic' && p.assumptions.longevity.couple_dependence.type === 'linked' },
  { id: 'F33', field: 'income_gradient', label: 'Longevity rises with income', section: 'assumptions', on: (p) => p.assumptions.longevity.type === 'stochastic' && p.assumptions.longevity.income_gradient },
  { id: 'F34', field: 'medicaid', label: 'Medicaid for long-term care', section: 'spending', on: (p) => p.spending.healthcare.medicaid },
  { id: 'F35', field: 'ltc_insurance', label: 'Long-term-care insurance', section: 'household', on: (p) => p.household.people.some((x) => x.ltc_insurance) },
  { id: 'F36', field: 'bond_pricing', label: 'Bond returns from the yield path', section: 'assumptions', on: (p) => p.assumptions.market.bond_pricing === 'from_yield' },
  { id: 'F37', field: 'valuation', label: 'Valuation-aware returns', section: 'assumptions', on: (p) => p.assumptions.market.valuation !== null },
  { id: 'F38', field: 'historical_episode', label: 'Historical-episode and lost-generation stresses', section: 'assumptions', on: (p) => stresses(p).some((s) => s.type === 'historical_episode' || s.type === 'lost_generation') },
  { id: 'F39', field: 'fat_tailed', label: 'Fat-tailed market model', section: 'assumptions', on: (p) => p.assumptions.market.kind.type === 'fat_tailed' },
  { id: 'F40', field: 'dynamic_yields', label: 'Income yields that move with the market', section: 'assumptions', on: (p) => p.assumptions.market.dynamic_yields },
  { id: 'F41', field: 'return_to_work', label: 'Going back to work', section: 'retirement', on: (p) => p.retirement.return_to_work !== null },
  { id: 'F42', field: 'panic_prone', label: 'Panic selling', section: 'accounts', on: (p) => p.portfolio.behavior.type === 'panic_prone' },
  { id: 'F43', field: 'claiming', label: 'Adaptive Social Security claiming', section: 'income', on: (p) => claimingRealism(p.household.people) > 0 },
  { id: 'F44', field: 'pause_deferrals_in_distress', label: 'Pausing saving in distress', section: 'accounts', on: (p) => p.portfolio.pause_deferrals_in_distress },
  { id: 'F45', field: 'confirmation', label: 'Not retiring into a crash', section: 'retirement', on: (p) => timingRealism(p.retirement.trigger) > 0 },
  { id: 'F46', field: 'divorce_risk', label: 'Divorce', section: 'household', on: (p) => p.household.divorce_risk.type !== 'off' || p.events.some((e) => e.kind.type === 'divorce') },
  { id: 'F47', field: 'parents', label: 'Inheritance from parents', section: 'household', on: (p) => p.household.parents.length > 0 },
  { id: 'F48', field: 'care_support', label: 'Caring for a parent', section: 'household', on: (p) => p.household.parents.some((x) => x.care_support) },
  { id: 'F49', field: 'spousal_care', label: 'Spousal caregiving', section: 'spending', on: (p) => p.spending.healthcare.spousal_care },
  { id: 'F50', field: 'life_insurance', label: 'Life insurance', section: 'household', on: (p) => p.household.people.some((x) => x.life_insurance.length > 0) },
];

/** The optional features a plan has switched on. */
export const featuresOn = (profile: Profile): Feature[] => FEATURES.filter((f) => f.on(profile));
