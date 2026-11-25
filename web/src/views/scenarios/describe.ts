// What a scenario changes, in plain language: one line per change, with the field named
// in words, the thing it belongs to ("Priya", "Rent") and the value before and after.
// Labels come from a short list for the fields what-ifs usually touch and fall back to
// the profile schema's own description of the field.
import { annualAmount, annualInRetirement, ASSETS } from '@/editor/labels';
import { describe as schemaDescription } from '@/editor/schema';
import type { PatchOp, Profile } from '@/engine/types';
import { canonicalPath, getAt, parsePointer } from '@/state/pointer';
import { moneyCompact, moneyPrice, yearMonth } from '@/ui';

export interface ChangeLine {
  key: string;
  /** What changed: "Retirement", "Priya and Daniel: Social Security claim age". */
  label: string;
  /** Null when there was nothing there before (something added). */
  before: string | null;
  after: string;
  /** Indices into the patch of the operations this line stands for. */
  ops: number[];
}

type Obj = Record<string, unknown>;
const isObj = (v: unknown): v is Obj => v !== null && typeof v === 'object' && !Array.isArray(v);

const pct = (v: number, digits = 2) => `${Math.round(v * 100 * 10 ** digits) / 10 ** digits}%`;
const words = (s: string) => s.replace(/_/g, ' ');
const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** Field labels by path pattern (list positions written as `*`). */
const LABELS: Record<string, string> = {
  '/retirement/trigger': 'Retirement',
  '/retirement/trigger/age': 'Retirement age',
  '/retirement/trigger/at': 'Retirement month',
  '/retirement/trigger/withdrawal_rate': 'Funded test: withdrawal rate',
  '/retirement/trigger/min_age': 'Earliest retirement age',
  '/retirement/trigger/max_age': 'Latest retirement age',
  '/retirement/trigger/amount': 'Savings target',
  '/retirement/partner_offset_years': 'Second person keeps working',
  '/retirement/spending_rule': 'Spending rule in retirement',
  '/retirement/withdrawal_order': 'Withdrawal order',
  '/retirement/bequest_target': 'Bequest goal',
  '/household/people/*/social_security/claim_age': 'Social Security claim age',
  '/incomes': 'New income',
  '/incomes/*': 'Income',
  '/incomes/*/kind/salary': 'salary',
  '/incomes/*/kind/net_annual': 'net income',
  '/incomes/*/kind/annual': 'amount a year',
  '/incomes/*/kind/plan/employee_fraction': 'pay into the workplace plan',
  '/incomes/*/kind/plan/employee_max': 'max out the workplace plan',
  '/incomes/*/start': 'starts',
  '/incomes/*/end': 'ends',
  '/spending/items': 'New budget line',
  '/spending/items/*': 'Budget line',
  '/spending/items/*/amount': 'amount',
  '/spending/items/*/in_retirement': 'in retirement',
  '/spending/lifestyle_creep': 'Lifestyle creep',
  '/spending/retirement_smile': 'Spending drifts down in later retirement',
  '/portfolio/target': 'Investment mix',
  '/portfolio/glide': 'Shift toward bonds near retirement',
  '/portfolio/expense_ratio': 'Fund fees',
  '/portfolio/advisory_fee': 'Adviser fee',
  '/portfolio/retirement_cash_years': 'Cash bucket in retirement',
  '/events': 'New event',
  '/events/*': 'Event',
  '/events/*/when': 'when',
  '/events/*/probability': 'chance it happens',
  '/events/*/kind/price': 'price',
  '/events/*/kind/down_payment_fraction': 'down payment',
  '/assumptions/market/kind': 'Market model',
  '/assumptions/market/equity_premium_adjustment': 'Stock returns',
  '/assumptions/market/inflation_adjustment': 'Inflation',
  '/assumptions/market/stress': 'Stress test',
  '/assumptions/market/stress/*': 'Stress test',
  '/assumptions/taxes/social_security_haircut': 'Social Security benefits',
  '/assumptions/longevity': 'Lifespans',
  '/assumptions/career_risk': 'Job loss and pay shocks',
  '/assumptions/health_risk': 'Medical cost and care shocks',
  '/household/location': 'Where you live',
  '/private_equity': 'New company',
  '/private_equity/*': 'Company',
  '/private_equity/*/included': 'counted in the plan',
  '/private_equity/*/stage': 'stage',
  '/private_equity/*/outlook': 'outlook',
  '/private_equity/*/preferred_price': 'latest price per share',
  '/private_equity/*/fmv_409a': '409A value per share',
  '/private_equity/*/preference_overhang': 'preference overhang',
  '/private_equity/*/outcomes': 'what might happen',
  '/private_equity/*/outcomes/*/probability': 'chance',
  '/private_equity/*/tenders/annual_probability': 'chance of a tender offer each year',
  '/private_equity/*/tenders/max_fraction': 'most that can be sold in one tender',
  '/private_equity/*/tenders/next_in_months': 'planned tender in',
  '/private_equity/*/policy/exercise': 'when options are exercised',
  '/private_equity/*/policy/at_deadline': 'when options would lapse',
  '/private_equity/*/policy/tender_sell_fraction': 'share sold in tenders',
  '/private_equity/*/policy/sell_down': 'after the lockup',
  '/private_equity/*/policy/leave_after_months': 'leaves the company after',
  '/private_equity/*/holdings': 'New holding',
  '/private_equity/*/holdings/*': 'Holding',
  '/private_equity/*/holdings/*/shares': 'number of shares or options',
  '/private_equity/*/holdings/*/vested': 'vested today',
  '/simulation/end_age': 'Simulation ends at age',
};

/** The lists whose elements have names, with the word for one element. */
const LISTS: Record<string, string> = {
  '/household/people': 'Person',
  '/household/children': 'Child',
  '/incomes': 'Income',
  '/spending/items': 'Budget line',
  '/accounts': 'Account',
  '/properties': 'Property',
  '/debts': 'Debt',
  '/events': 'Event',
  '/assumptions/market/stress': 'Stress test',
  '/private_equity': 'Company',
  '/private_equity/*/holdings': 'Holding',
  '/private_equity/*/outcomes': 'Outcome',
};

/** Enumerated equity settings in words, by the field's name. */
const EQUITY_WORDS: Record<string, Record<string, string>> = {
  exercise: { at_liquidity: 'only at a sale or IPO', vested_now: 'vested options now; later ones wait for a sale or IPO', as_they_vest: 'vested options now, the rest as they vest' },
  at_deadline: { exercise: 'exercise them if affordable', forfeit: 'let them lapse' },
  outlook: { pessimistic: 'failure 10 points more likely', typical: 'typical', optimistic: 'failure up to 10 points less likely' },
  stage: { seed: 'seed', series_a: 'Series A', series_b: 'Series B', series_c: 'Series C', late: 'late stage', unicorn: 'unicorn' },
};

const isIndex = (segment: string) => /^(\d+|@.*)$/.test(segment);

/** `/incomes/@priya_job/kind/salary` -> `/incomes/* /kind/salary` (without the space). */
export function pathPattern(path: string): string {
  return (
    '/' +
    parsePointer(path)
      .map((s) => (isIndex(s) ? '*' : s))
      .join('/')
  );
}

/** The named thing a path sits inside, if any: its list, its own path and its name. */
function entityOf(profile: unknown, path: string): { list: string; path: string; name: string | null; rest: string } | null {
  const segments = parsePointer(path);
  for (let i = segments.length - 1; i >= 1; i--) {
    if (!isIndex(segments[i] as string)) continue;
    // Lists inside a list element (a company's holdings) are known by their pattern.
    const list = pathPattern('/' + segments.slice(0, i).join('/'));
    if (!(list in LISTS)) continue;
    const own = '/' + segments.slice(0, i + 1).join('/');
    const element = getAt(profile, own);
    const name = isObj(element) && typeof element.name === 'string' && element.name ? element.name : null;
    return { list, path: own, name, rest: segments.slice(i + 1).join('/') };
  }
  return null;
}

function when(v: Obj, profile: Profile): string {
  switch (v.type) {
    case 'date':
      return yearMonth(String(v.at));
    case 'age': {
      const who = profile.household.people[Number(v.person ?? 0)]?.name;
      return `when ${who ?? 'the first person'} is ${String(v.age)}`;
    }
    case 'retirement': {
      const offset = Number(v.offset_years ?? 0);
      return offset === 0 ? 'at retirement' : `${plural(Math.abs(offset), 'year')} ${offset < 0 ? 'before' : 'after'} retirement`;
    }
    default:
      return 'from the start';
  }
}

function allocation(v: Obj): string {
  const entries = Object.entries(v).filter((e): e is [string, number] => typeof e[1] === 'number' && e[1] > 0);
  const total = entries.reduce((n, [, w]) => n + w, 0) || 1;
  const order = (k: string) => {
    const i = ASSETS.findIndex((a) => a.value === k);
    return i < 0 ? ASSETS.length : i;
  };
  return entries
    .sort((a, b) => order(a[0]) - order(b[0]))
    .map(([k, w]) => `${Math.round((w / total) * 100)}% ${ASSETS.find((a) => a.value === k)?.label.toLowerCase().replace('us ', 'US ') ?? words(k)}`)
    .join(', ');
}

const EPISODE: Record<string, string> = {
  great_depression1929: 'the Great Depression (1929 to 1945)',
  stagflation1966: 'the stagflation of 1966 to 1982',
  dot_com2000: 'the dot-com bust and its aftermath (2000 to 2013)',
  financial2008: 'the financial crisis (2007 to 2013)',
};

/** Tagged objects the what-ifs write, in words. */
function tagged(v: Obj, pattern: string, profile: Profile): string | null {
  const type = typeof v.type === 'string' ? v.type : null;
  if (pattern === '/retirement/trigger') {
    if (type === 'at_age') return `at age ${String(v.age)}`;
    if (type === 'at_date') return `in ${yearMonth(String(v.at))}`;
    if (type === 'when_funded') return `when savings cover spending at a ${pct(Number(v.withdrawal_rate ?? 0.04))} withdrawal rate`;
    if (type === 'fixed_number') return `when savings reach ${moneyCompact(Number(v.amount))}`;
    if (type === 'already_retired') return 'already retired';
  }
  if (pattern.endsWith('/in_retirement')) {
    if (type === 'same') return 'same as while working';
    if (type === 'stops') return 'stops';
    if (type === 'factor') return `${pct(Number(v.factor), 0)} of the working amount`;
    if (type === 'amount') return moneyCompact(Number(v.amount));
  }
  if (pattern === '/portfolio/glide') {
    if (type === 'none') return 'none';
    if (type === 'target_date') return `down to ${pct(Number(v.equity_at_retirement ?? 0), 0)} stocks at retirement`;
    if (type === 'bond_tent') return `a bond tent, ${pct(Number(v.equity_at_retirement ?? 0), 0)} stocks at retirement`;
    if (type === 'custom') return 'a custom schedule';
  }
  if (pattern === '/assumptions/taxes/social_security_haircut') return `cut ${pct(1 - Number(v.factor ?? 1), 0)} from ${String(v.from_year)}`;
  if (pattern.startsWith('/assumptions/market/stress')) {
    if (type === 'crash_at_retirement') return `stocks fall ${pct(Number(v.drawdown ?? 0.4), 0)} at retirement and take ${plural(Number(v.recovery_years ?? 5), 'year')} to recover`;
    if (type === 'lost_decade') return isObj(v.start) ? `ten years of flat stocks, ${when(v.start, profile)}` : 'ten years of flat stocks';
    if (type === 'historical_episode') return `a replay of ${EPISODE[String(v.episode)] ?? 'a historical episode'}${isObj(v.start) ? `, ${when(v.start, profile)}` : ''}`;
    if (type === 'lost_generation') return `${plural(Number(v.years ?? 20), 'year')} of stagnant stocks and falling prices${isObj(v.start) ? `, ${when(v.start, profile)}` : ''}`;
    if (type === 'stagflation') return `${plural(Number(v.years ?? 0), 'year')} of ${pct(Number(v.inflation ?? 0), 0)} inflation${isObj(v.start) ? `, ${when(v.start, profile)}` : ''}`;
  }
  if (pattern === '/private_equity/*/policy/sell_down') {
    if (type === 'all_at_lockup_end') return 'sell when the lockup ends';
    if (type === 'over_years') return `sell over ${plural(Number(v.years ?? 1), 'year')}`;
    if (type === 'hold') return 'keep the shares';
  }
  if (pattern === '/private_equity/*/holdings' || pattern === '/private_equity/*/holdings/*') {
    const kind = isObj(v.kind) ? String(v.kind.type ?? '') : '';
    const what = kind === 'iso' ? 'incentive options' : kind === 'nso' ? 'non-qualified options' : kind === 'rsu' ? 'RSUs' : 'shares';
    return `${typeof v.name === 'string' && v.name ? `${v.name}, ` : ''}${Math.round(Number(v.shares ?? 0)).toLocaleString('en-US')} ${what}`;
  }
  if (pattern === '/portfolio/target') return allocation(v);
  if (pattern === '/incomes' || pattern === '/incomes/*') {
    const kind = isObj(v.kind) ? v.kind : {};
    const amount = Number(kind.salary ?? kind.net_annual ?? kind.annual ?? 0);
    const span = [isObj(v.start) ? `from ${when(v.start, profile).replace(/^at /, '')}` : '', isObj(v.end) ? `until ${when(v.end, profile).replace(/^at /, '')}` : ''].filter(Boolean).join(' ');
    return `${typeof v.name === 'string' && v.name ? v.name : words(String(kind.type ?? 'income'))}, ${moneyCompact(amount)} a year${span ? `, ${span}` : ''}`;
  }
  if (pattern.endsWith('/start') || pattern.endsWith('/end') || pattern.endsWith('/when')) return when(v, profile);
  if (typeof v.name === 'string' && v.name) return v.name;
  return type ? words(type) : null;
}

const RATE = /(rate|fraction|adjustment|ratio|fee|probability|drawdown|inflation|raise|growth|creep|volatility|match_up_to|width|overhang)$/;
// A price per share keeps its cents: `moneyCompact` would print a $1.20 strike as "$1".
const PRICE = /(preferred_price|fmv_409a|strike|basis_per_share|fmv_at_exercise)$/;
const MONEY = /(salary|amount|price|annual|target|balance|cost|contribution|employer|monthly|value|principal|payment)$/;

/** A value at a path, in words. */
export function describeValue(value: unknown, pattern: string, profile: Profile): string {
  const key = pattern.split('/').pop() ?? '';
  const equity = pattern.startsWith('/private_equity/');
  if (value === null || value === undefined) {
    if (pattern === '/assumptions/taxes/social_security_haircut') return 'paid in full';
    if (equity && key === 'leave_after_months') return 'stays';
    if (equity && (key === 'annual_probability' || key === 'preference_overhang' || key === 'fmv_409a')) return 'typical for the stage';
    return 'none';
  }
  if (typeof value === 'boolean') return value ? 'yes' : 'no';
  if (typeof value === 'number') {
    if (PRICE.test(key)) return moneyPrice(value);
    if (key === 'factor') return pct(value, 0);
    if (key === 'equity_premium_adjustment' || key === 'inflation_adjustment') return value === 0 ? 'as in history' : `${Math.abs(Math.round(value * 1000) / 10)} points a year ${value < 0 ? 'lower' : 'higher'}`;
    if (key === 'partner_offset_years') return value === 0 ? 'no longer than the first' : `${plural(Math.abs(value), 'year')} ${value < 0 ? 'less' : 'longer'}`;
    if (RATE.test(key)) return pct(value);
    if (key.endsWith('age')) return String(value);
    if (key.endsWith('years')) return plural(value, 'year');
    if (key.endsWith('months')) return plural(value, 'month');
    if (MONEY.test(key)) return moneyCompact(value);
    return String(value);
  }
  if (typeof value === 'string') return /^\d{4}-\d{2}$/.test(value) ? yearMonth(value) : ((equity ? EQUITY_WORDS[key]?.[value] : undefined) ?? words(value));
  if (Array.isArray(value)) {
    if (pattern === '/private_equity/*/outcomes') return value.length === 0 ? 'typical odds for the stage' : `your own odds (${plural(value.length, 'outcome')})`;
    return value.length === 0 ? 'none' : plural(value.length, 'item');
  }
  if (isObj(value)) return tagged(value, pattern, profile) ?? 'changed';
  return String(value as string);
}

/** A label for the field at `path`: the short list, else the schema's description of it, else its name. */
export function fieldLabel(profile: unknown, path: string): string {
  const pattern = pathPattern(path);
  const known = LABELS[pattern];
  if (known) return known;
  let text: string | undefined;
  try {
    text = schemaDescription(profile, path);
  } catch {
    text = undefined;
  }
  if (text) {
    // The description's opening clause, kept short enough to sit on one line.
    const clause = (text.split(/[.:;(]|, /)[0] ?? '').trim();
    if (clause.length >= 4 && clause.length <= 56) return clause;
  }
  const segments = parsePointer(path).filter((s) => !isIndex(s) && s !== 'kind');
  const last = words(segments[segments.length - 1] ?? 'plan');
  return last.charAt(0).toUpperCase() + last.slice(1);
}

function listNames(names: string[]): string {
  if (names.length <= 1) return names[0] ?? '';
  if (names.length === 2) return `${names[0]} and ${names[1]}`;
  return `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`;
}

const upperFirst = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);

/**
 * The changes a patch makes to `base`, as lines to read. `after` is the baseline with
 * the patch applied (null when it no longer applies): it gives totals for a patch that
 * rescales the whole budget, which reads better as one line than as fifteen.
 */
export function describePatch(base: Profile, patch: PatchOp[], after: Profile | null): ChangeLine[] {
  interface Raw {
    index: number;
    pattern: string;
    entity: string | null;
    label: string;
    before: string | null;
    after: string;
  }
  const raws: Raw[] = [];
  const budget: number[] = [];

  patch.forEach((op, index) => {
    const path = canonicalPath(base, op.path);
    const pattern = pathPattern(path);
    if (op.op !== 'remove' && /^\/spending\/items\/\*\/(amount|in_retirement|frequency)$/.test(pattern)) budget.push(index);
    const entity = entityOf(base, path);
    if (op.op === 'remove') {
      const kind = entity && entity.rest === '' ? (LISTS[entity.list] ?? 'Item') : null;
      const existing = getAt(base, path);
      // Removing a company's own odds puts the stage's typical ones back: say that, and for which company.
      const odds = pattern === '/private_equity/*/outcomes';
      raws.push({
        index,
        pattern: `remove:${pattern}`,
        entity: odds ? (entity?.name ?? null) : null,
        label: kind ? `${kind}${entity?.name ? ` “${entity.name}”` : ''}` : fieldLabel(base, path),
        before: kind ? null : describeValue(existing, pattern, base),
        after: odds ? describeValue([], pattern, base) : 'removed',
      });
      return;
    }
    if (op.op === 'append') {
      raws.push({ index, pattern: `append:${pattern}`, entity: null, label: LABELS[pattern] ?? `New ${words(LISTS[pattern] ?? 'item').toLowerCase()}`, before: null, after: describeValue(op.value, pattern, base) });
      return;
    }
    const existing = getAt(base, path);
    raws.push({
      index,
      pattern,
      entity: entity && entity.rest !== '' ? (entity.name ?? LISTS[entity.list] ?? null) : null,
      label: fieldLabel(base, path),
      before: existing === undefined ? null : describeValue(existing, pattern, base),
      after: describeValue(op.value, pattern, base),
    });
  });

  const lines: ChangeLine[] = [];
  const used = new Set<number>();

  // A patch that touches three or more budget lines is a change to the budget: say what it comes to.
  if (budget.length >= 3) {
    budget.forEach((i) => used.add(i));
    const total = (p: Profile, f: (i: Profile['spending']['items'][number]) => number) => p.spending.items.reduce((n, i) => n + f(i), 0);
    const now = [total(base, annualAmount), after ? total(after, annualAmount) : null] as const;
    const retired = [total(base, annualInRetirement), after ? total(after, annualInRetirement) : null] as const;
    const count = plural(budget.length, 'budget line');
    if (!after) {
      lines.push({ key: 'budget', label: 'Budget', before: null, after: `${count} changed`, ops: budget });
    } else {
      const change = (a: number, b: number) => (a > 0 ? ` (${b >= a ? '+' : '−'}${Math.round(Math.abs(b / a - 1) * 100)}%)` : '');
      if (Math.round(now[0]) !== Math.round(now[1] as number)) lines.push({ key: 'budget-now', label: `Budget while working, across ${count}`, before: `${moneyCompact(now[0])} a year`, after: `${moneyCompact(now[1] as number)}${change(now[0], now[1] as number)}`, ops: budget });
      // The plan already says the same (a scenario left over after its twin became the plan).
      if (Math.round(retired[0]) === Math.round(retired[1] as number) && lines.length === 0) lines.push({ key: 'budget-retired', label: `Budget, across ${count}`, before: null, after: `${moneyCompact(retired[0])} a year in retirement, which the plan already has`, ops: budget });
      else if (Math.round(retired[0]) !== Math.round(retired[1] as number))
        lines.push({ key: 'budget-retired', label: `Budget in retirement, across ${count}`, before: `${moneyCompact(retired[0])} a year`, after: `${moneyCompact(retired[1] as number)}${change(retired[0], retired[1] as number)}`, ops: budget });
    }
  }

  // The same change made to several things (both people's claim age) is one line naming them.
  for (const raw of raws) {
    if (used.has(raw.index)) continue;
    const same = raws.filter((r) => !used.has(r.index) && r.entity !== null && raw.entity !== null && r.pattern === raw.pattern && r.before === raw.before && r.after === raw.after);
    const group = same.length > 1 ? same : [raw];
    group.forEach((r) => used.add(r.index));
    const names = group.map((r) => r.entity).filter((n): n is string => n !== null);
    const label = names.length ? `${listNames(names)}: ${raw.label}` : upperFirst(raw.label);
    const noop = raw.before !== null && raw.before === raw.after;
    lines.push({ key: `${raw.pattern}:${raw.index}`, label, before: noop ? null : raw.before, after: noop ? `${raw.after}, which the plan already has` : raw.after, ops: group.map((r) => r.index) });
  }
  return lines;
}

/** One line of text for a change: "Retirement: when funded → at age 50". */
export function lineText(line: ChangeLine): string {
  return line.before === null ? `${line.label}: ${line.after}` : `${line.label}: ${line.before} → ${line.after}`;
}
