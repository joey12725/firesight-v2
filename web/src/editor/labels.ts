// Display names for schema enums, and small derived figures used in summaries.
import type { AccountKind, AssetClass, BudgetItem, Category, Frequency, Profile } from '@/engine/types';

export const US_STATES: { value: string; label: string }[] = (
  'AL:Alabama,AK:Alaska,AZ:Arizona,AR:Arkansas,CA:California,CO:Colorado,CT:Connecticut,DE:Delaware,DC:District of Columbia,' +
  'FL:Florida,GA:Georgia,HI:Hawaii,ID:Idaho,IL:Illinois,IN:Indiana,IA:Iowa,KS:Kansas,KY:Kentucky,LA:Louisiana,ME:Maine,MD:Maryland,' +
  'MA:Massachusetts,MI:Michigan,MN:Minnesota,MS:Mississippi,MO:Missouri,MT:Montana,NE:Nebraska,NV:Nevada,NH:New Hampshire,' +
  'NJ:New Jersey,NM:New Mexico,NY:New York,NC:North Carolina,ND:North Dakota,OH:Ohio,OK:Oklahoma,OR:Oregon,PA:Pennsylvania,' +
  'RI:Rhode Island,SC:South Carolina,SD:South Dakota,TN:Tennessee,TX:Texas,UT:Utah,VT:Vermont,VA:Virginia,WA:Washington,' +
  'WV:West Virginia,WI:Wisconsin,WY:Wyoming'
)
  .split(',')
  .map((s) => {
    const [value = '', label = ''] = s.split(':');
    return { value, label };
  });

export const CATEGORIES: { value: Category; label: string }[] = [
  { value: 'rent', label: 'Rent' },
  { value: 'utilities', label: 'Utilities' },
  { value: 'groceries', label: 'Groceries' },
  { value: 'dining', label: 'Dining out' },
  { value: 'transport', label: 'Transport' },
  { value: 'healthcare', label: 'Healthcare' },
  { value: 'insurance', label: 'Insurance' },
  { value: 'childcare', label: 'Childcare' },
  { value: 'education', label: 'Education' },
  { value: 'travel', label: 'Travel' },
  { value: 'entertainment', label: 'Entertainment' },
  { value: 'personal', label: 'Personal' },
  { value: 'giving', label: 'Giving' },
  { value: 'other', label: 'Other' },
];

export const categoryLabel = (c: Category) => CATEGORIES.find((x) => x.value === c)?.label ?? c;

export const FREQUENCIES: { value: Frequency; label: string; short: string; perYear: number }[] = [
  { value: 'weekly', label: 'Weekly', short: '/wk', perYear: 52 },
  { value: 'biweekly', label: 'Every two weeks', short: '/2wk', perYear: 26 },
  { value: 'monthly', label: 'Monthly', short: '/mo', perYear: 12 },
  { value: 'quarterly', label: 'Quarterly', short: '/qtr', perYear: 4 },
  { value: 'annual', label: 'Yearly', short: '/yr', perYear: 1 },
];

export const frequency = (f: Frequency) => FREQUENCIES.find((x) => x.value === f) ?? (FREQUENCIES[2] as (typeof FREQUENCIES)[number]);

export const ACCOUNT_KINDS: { value: AccountKind; label: string; plural: string; note: string }[] = [
  { value: 'cash', label: 'Cash', plural: 'Cash', note: 'Checking, savings, money market, CDs' },
  { value: 'taxable', label: 'Taxable brokerage', plural: 'Taxable', note: 'Gains are taxed when sold' },
  { value: 'traditional', label: 'Traditional (pre-tax)', plural: 'Pre-tax', note: '401(k), 403(b), traditional IRA' },
  { value: 'roth', label: 'Roth', plural: 'Roth', note: 'Roth IRA, Roth 401(k)' },
  { value: 'hsa', label: 'HSA', plural: 'HSA', note: 'Health savings account' },
];

export const ASSETS: { value: AssetClass; label: string; short: string; color: string }[] = [
  { value: 'us_stocks', label: 'US stocks', short: 'US', color: 'var(--chart-cat-1)' },
  { value: 'intl_stocks', label: 'International stocks', short: 'Intl', color: 'var(--chart-cat-5)' },
  { value: 'bonds', label: 'Bonds', short: 'Bonds', color: 'var(--chart-cat-3)' },
  { value: 'cash', label: 'Cash', short: 'Cash', color: 'var(--chart-cat-8)' },
  { value: 'real_estate', label: 'Real estate funds', short: 'REIT', color: 'var(--chart-cat-4)' },
  { value: 'gold', label: 'Gold', short: 'Gold', color: 'var(--chart-cat-7)' },
  { value: 'crypto', label: 'Crypto', short: 'Crypto', color: 'var(--chart-cat-6)' },
  { value: 'single_stock', label: 'Single stock', short: 'Stock', color: 'var(--chart-cat-2)' },
  { value: 'custom', label: 'Custom asset', short: 'Custom', color: 'var(--ink-3)' },
];

/** Annual amount of a budget line now, in today's dollars (same arithmetic as core). */
export function annualAmount(item: BudgetItem): number {
  return item.amount * frequency(item.frequency).perYear;
}

/** Annual amount once retired. */
export function annualInRetirement(item: BudgetItem): number {
  const r = item.in_retirement;
  switch (r.type) {
    case 'same':
      return annualAmount(item);
    case 'stops':
      return 0;
    case 'factor':
      return annualAmount(item) * r.factor;
    case 'amount':
      return r.amount * frequency(item.frequency).perYear;
  }
}

export function budgetTotals(profile: Profile) {
  const items = profile.spending.items;
  const sum = (f: (i: BudgetItem) => number, pred: (i: BudgetItem) => boolean = () => true) =>
    items.filter(pred).reduce((n, i) => n + f(i), 0);
  return {
    now: sum(annualAmount),
    essentialNow: sum(annualAmount, (i) => i.essential),
    retired: sum(annualInRetirement),
    essentialRetired: sum(annualInRetirement, (i) => i.essential),
  };
}

/** Sum of the incomes that have a headline annual figure, for the section summary. */
export function annualIncome(profile: Profile): number {
  return profile.incomes.reduce((n, inc) => {
    const k = inc.kind;
    switch (k.type) {
      case 'job':
        return n + k.salary;
      case 'self_employment':
        return n + k.net_annual;
      default:
        return n + k.annual;
    }
  }, 0);
}
