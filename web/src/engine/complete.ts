// The engine leaves the optional realism fields (docs/methodology.md, section 18) out
// of a profile it writes while they hold their defaults, so that plans which use none
// of them are byte-for-byte what they were before the features existed. The app works
// on the complete form (`types.ts`: every field present), and patches address fields by
// path, so a field that is not there could be neither read nor appended to. This puts
// the defaults back on every profile that comes out of the engine.
//
// The schema says where the fields are (which type, which variant); the table below
// says what each one's default is. `complete.test.ts` checks the table against the
// schema (a field the engine starts leaving out fails there) and against the engine
// (a completed profile normalises back to exactly what the engine wrote).

interface Node {
  $ref?: string;
  type?: string | string[];
  const?: unknown;
  properties?: Record<string, Node>;
  required?: string[];
  default?: unknown;
  items?: Node;
  oneOf?: Node[];
  anyOf?: Node[];
  $defs?: Record<string, Node>;
}

/**
 * Defaults of the fields the engine omits, keyed by schema type (and variant, for a
 * tagged union) and field name.
 */
export const OMITTED_DEFAULTS: Record<string, unknown> = {
  'Household.married': null,
  'Household.divorce_risk': { type: 'off' },
  'Household.parents': [],
  'Person.disability_risk': 'off',
  'Person.ltd_insurance': null,
  'Person.ltc_insurance': null,
  'Person.life_insurance': [],
  'SocialSecurity.claiming': { type: 'fixed' },
  'Child.arrival': { type: 'certain' },
  'Child.college_cost': { type: 'fixed' },
  'Child.adult_support': null,
  'Child.parental_leave': null,
  'Child.second_parental_leave': null,
  'Parent.care_support': null,
  'CareSupport.caregiver': null,
  'IncomeKind.self_employment.business_risk': 'off',
  'IncomeKind.self_employment.years_operating': 0,
  'IncomeKind.self_employment.fallback_wage': null,
  'CareerRisk.job_mobility': 'off',
  'CareerRisk.recession_pay': 'off',
  'CareerRisk.late_career': 'off',
  'Spending.vehicles': [],
  'Spending.seasonality': 'off',
  'Spending.category_inflation': 'off',
  'Spending.unemployment_cut': 0,
  'BudgetItem.rent_growth': 'fixed',
  'Healthcare.cover_children': false,
  'Healthcare.children_coverage_end_age': 26,
  'Healthcare.medicaid': false,
  'Healthcare.spousal_care': false,
  'PortfolioPolicy.tax_loss_harvesting': false,
  'PortfolioPolicy.behavior': { type: 'disciplined' },
  'PortfolioPolicy.pause_deferrals_in_distress': false,
  'PortfolioPolicy.distress': { keep_match: true, high_interest_rate: 0.08 },
  'Property.maintenance': 'smooth',
  'Property.assessment': { type: 'market' },
  'Property.insurance_real_growth': 'fixed',
  'PropertyUse.rental.dynamics': { type: 'simple' },
  'Mortgage.kind': { type: 'fixed' },
  'Assessment.capped.assessed_value': null,
  'DebtPolicy.cash_out_refinance': null,
  'DebtPolicy.mortgage_prepayment': { type: 'never' },
  'EventKind.buy_home.maintenance': 'smooth',
  'EventKind.buy_home.assessment': { type: 'market' },
  'EventKind.buy_home.insurance_real_growth': 'fixed',
  'EventKind.buy_home.mortgage_kind': { type: 'fixed' },
  'RetirementPlan.qcd': false,
  'RetirementPlan.home_equity_borrowing': { type: 'none' },
  'RetirementPlan.downsize': null,
  'RetirementPlan.return_to_work': null,
  'RetirementTrigger.when_funded.confirmation': null,
  'RetirementTrigger.when_funded.one_more_year': null,
  'Assumptions.housing_liquidity': 'instant',
  'Assumptions.health_model': { type: 'independent' },
  'MarketModel.bond_pricing': 'sampled',
  'MarketModel.valuation': null,
  'MarketModel.dynamic_yields': false,
  'TaxModel.chained_cpi': false,
  'TaxModel.state_detail': 'basic',
  'TaxModel.policy_risk': null,
  'Longevity.stochastic.couple_dependence': { type: 'independent' },
  'Longevity.stochastic.income_gradient': false,
  'Longevity.stochastic.income_percentile': null,
};

const refName = (ref: string) => ref.replace('#/$defs/', '');

function fillObject(root: Node, node: Node, value: Record<string, unknown>, key: string) {
  for (const [name, child] of Object.entries(node.properties ?? {})) {
    if (!(name in value)) {
      const fallback = OMITTED_DEFAULTS[`${key}.${name}`];
      if (fallback === undefined) continue;
      value[name] = typeof fallback === 'object' && fallback !== null ? structuredClone(fallback) : fallback;
    }
    fill(root, child, value[name], `${key}.${name}`);
  }
}

function fill(root: Node, node: Node | undefined, value: unknown, key: string): void {
  if (!node || value === null || typeof value !== 'object') return;
  if (node.$ref) {
    const name = refName(node.$ref);
    return fill(root, root.$defs?.[name], value, name);
  }
  if (Array.isArray(value)) {
    for (const element of value) fill(root, node.items, element, key);
    return;
  }
  const record = value as Record<string, unknown>;
  const members = (node.oneOf ?? node.anyOf)?.filter((m) => m.type !== 'null');
  if (!members) return fillObject(root, node, record, key);
  // "X or null" wraps one type; a tagged union is picked by the value's `type`.
  if (members.length === 1 && members[0]?.$ref) return fill(root, members[0], value, key);
  const variant = members.find((m) => m.properties?.type?.const === record.type);
  if (variant && typeof record.type === 'string') fillObject(root, variant, record, `${key}.${record.type}`);
}

/**
 * Puts back the fields the engine left out because they hold their defaults. Changes
 * `profile` in place (it is always a freshly parsed document) and returns it.
 */
export function completeProfile<T>(schema: unknown, profile: T): T {
  const root = schema as Node;
  if (profile !== null && typeof profile === 'object') fillObject(root, root, profile as Record<string, unknown>, 'Profile');
  return profile;
}

/** Every optional schema field that has no stated default: the fields the engine omits. For the test. */
export function omittedFields(schema: unknown): string[] {
  const root = schema as Node;
  const out: string[] = [];
  const collect = (key: string, node: Node) => {
    const required = new Set(node.required ?? []);
    for (const [name, child] of Object.entries(node.properties ?? {})) {
      if (!('default' in child) && !required.has(name) && !(name === 'type' && child.const !== undefined)) out.push(`${key}.${name}`);
    }
  };
  collect('Profile', root);
  for (const [name, def] of Object.entries(root.$defs ?? {})) {
    collect(name, def);
    for (const member of def.oneOf ?? def.anyOf ?? []) if (member.properties) collect(`${name}.${String(member.properties.type?.const)}`, member);
  }
  return out;
}
