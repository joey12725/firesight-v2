// The common what-ifs, built as patches against the baseline plan. Each one is an
// ordinary scenario once created: it can be renamed, edited, duplicated or promoted like
// any other. A what-if is only offered when the plan has something for it to change.
import type { Summary } from '@/engine/output-types';
import type { Allocation, PatchOp, Profile } from '@/engine/types';
import { append, remove, set } from '@/state/patches';
import { at, join, uniqueId } from '@/state/pointer';

export type QuickGroup = 'Retirement' | 'Spending and saving' | 'Income' | 'Investing' | 'Stress tests';

export interface QuickScenario {
  key: string;
  group: QuickGroup;
  /** The button: an imperative, a few words. */
  label: string;
  /** The scenario's name once created. */
  name: string;
  /** What it does to the plan, in a sentence, including any modelling choice it makes. */
  hint: string;
  patch: PatchOp[];
}

export const QUICK_GROUPS: QuickGroup[] = ['Retirement', 'Spending and saving', 'Income', 'Investing', 'Stress tests'];

const round = (v: number, digits = 4) => Math.round(v * 10 ** digits) / 10 ** digits;

function shiftMonth(ym: string, months: number): string {
  const [y = 0, m = 1] = ym.split('-').map(Number);
  const total = y * 12 + (m - 1) + months;
  return `${Math.floor(total / 12)}-${String((total % 12) + 1).padStart(2, '0')}`;
}

/** The first person's age at the plan's start, in years. */
export function startAge(profile: Profile): number | null {
  const birth = profile.household.people[0]?.birth;
  const start = profile.start;
  if (!birth || !start) return null;
  const [by = 0, bm = 1] = birth.split('-').map(Number);
  const [sy = 0, sm = 1] = start.split('-').map(Number);
  return sy - by + (sm - bm) / 12;
}

function retireShift(profile: Profile, summary: Summary | null, years: number): QuickScenario | null {
  const trigger = profile.retirement.trigger;
  const word = years < 0 ? 'earlier' : 'later';
  const n = Math.abs(years);
  const base = { key: `retire_${word}`, group: 'Retirement' as const, label: `Retire ${n} years ${word}` };
  const now = startAge(profile);
  switch (trigger.type) {
    case 'at_age': {
      const age = trigger.age + years;
      if (now !== null && age <= now + 0.5) return null;
      return { ...base, name: `Retire at ${age}`, hint: `Moves the planned retirement age from ${trigger.age} to ${age}.`, patch: [set('/retirement/trigger/age', age)] };
    }
    case 'at_date': {
      const at = shiftMonth(trigger.at, years * 12);
      if (profile.start && at <= profile.start) return null;
      return { ...base, name: `Retire ${n} years ${word}`, hint: `Moves the planned retirement month ${n} years ${word}.`, patch: [set('/retirement/trigger/at', at)] };
    }
    case 'when_funded':
    case 'fixed_number': {
      // The plan retires when a savings test is met, so there is no date to move. The
      // honest equivalent is a fixed age two years either side of when the middle life retires.
      const median = summary?.headline.retirement_age.p50 ?? null;
      if (median === null) return null;
      const age = Math.round(median) + years;
      if (now !== null && age <= now + 0.5) return null;
      return {
        ...base,
        name: `Retire at ${age}`,
        hint: `The plan retires once savings meet a test, which is around ${Math.round(median)} in the middle life. This fixes retirement at ${age} in every life instead, ready or not.`,
        patch: [set('/retirement/trigger', { type: 'at_age', person: 0, age })],
      };
    }
    case 'already_retired':
      return null;
  }
}

function retirementSpending(profile: Profile, factor: number): QuickScenario | null {
  const ops: PatchOp[] = [];
  profile.spending.items.forEach((item, i) => {
    const path = join('/spending/items', at(item, i), 'in_retirement');
    const r = item.in_retirement;
    if (r.type === 'same') ops.push(set(path, { type: 'factor', factor }));
    else if (r.type === 'factor') ops.push(set(path, { type: 'factor', factor: round(r.factor * factor) }));
    else if (r.type === 'amount') ops.push(set(path, { type: 'amount', amount: Math.round(r.amount * factor) }));
  });
  if (ops.length === 0) return null;
  const pct = Math.round(Math.abs(1 - factor) * 100);
  const word = factor < 1 ? 'less' : 'more';
  return {
    key: `spend_${word}`,
    group: 'Spending and saving',
    label: `Spend ${pct}% ${word} in retirement`,
    name: `Spend ${pct}% ${word} in retirement`,
    hint: `Scales every budget line by ${factor} from retirement on. Spending while working, housing, healthcare and care costs are untouched.`,
    patch: ops,
  };
}

function saveMore(profile: Profile, points: number): QuickScenario | null {
  const ops: PatchOp[] = [];
  profile.incomes.forEach((income, i) => {
    const k = income.kind;
    if (k.type !== 'job' || !k.plan || k.plan.employee_max || k.plan.employee_fraction >= 1) return;
    ops.push(set(join('/incomes', at(income, i), 'kind', 'plan', 'employee_fraction'), round(Math.min(1, k.plan.employee_fraction + points))));
  });
  if (ops.length === 0) return null;
  const pts = Math.round(points * 100);
  return {
    key: 'save_more',
    group: 'Spending and saving',
    label: `Save ${pts} points more`,
    name: `Save ${pts} points more of pay`,
    hint: `Raises the share of pay going into each workplace retirement plan by ${pts} points (up to the IRS limit). Take-home pay falls by the same amount, less the tax saved.`,
    patch: ops,
  };
}

function partTime(profile: Profile, years: number): QuickScenario | null {
  if (profile.retirement.trigger.type === 'already_retired') return null;
  const earned = profile.incomes
    .filter((i) => i.owner === 0)
    .map((i) => (i.kind.type === 'job' ? i.kind.salary : i.kind.type === 'self_employment' ? i.kind.net_annual : 0));
  const top = Math.max(0, ...earned);
  if (top <= 0) return null;
  const salary = Math.max(12_000, Math.round((top * 0.4) / 1000) * 1000);
  const id = uniqueId('part_time', profile.incomes.map((i) => i.id));
  const who = profile.household.people[0]?.name;
  return {
    key: 'part_time',
    group: 'Income',
    label: `Work part-time for ${years} years after retiring`,
    name: `Part-time for ${years} years after retiring`,
    hint: `Adds a job paying $${Math.round(salary / 1000)}k a year (40% of ${who ? `${who}’s` : 'the'} main pay today) from the retirement date for ${years} years, with no retirement plan and no layoff risk.`,
    patch: [
      append('/incomes', {
        id,
        name: 'Part-time work after retiring',
        owner: 0,
        kind: { type: 'job', salary, growth: { type: 'flat', real_raise: 0 }, risk: { level: 'none' }, plan: null, ends_at_retirement: false },
        start: { type: 'retirement', offset_years: 0 },
        end: { type: 'retirement', offset_years: years },
      }),
    ],
  };
}

function delaySocialSecurity(profile: Profile, age: number): QuickScenario | null {
  const ops: PatchOp[] = [];
  profile.household.people.forEach((p, i) => {
    if (p.social_security.basis.type === 'none' || p.social_security.claim_age >= age) return;
    ops.push(set(join('/household/people', at(p, i), 'social_security', 'claim_age'), age));
  });
  if (ops.length === 0) return null;
  return {
    key: 'delay_social_security',
    group: 'Income',
    label: `Delay Social Security to ${age}`,
    name: `Claim Social Security at ${age}`,
    hint: `Everyone claims at ${age}. The monthly benefit is larger for life, and savings carry the years in between.`,
    patch: ops,
  };
}

function skipHome(profile: Profile): QuickScenario | null {
  const index = profile.events.findIndex((e) => e.kind.type === 'buy_home');
  const event = profile.events[index];
  if (!event) return null;
  const rents = event.kind.type === 'buy_home' && event.kind.replaces_rent;
  return {
    key: 'skip_home',
    group: 'Spending and saving',
    label: 'Do not buy the house',
    name: `Without “${event.name || 'the home purchase'}”`,
    hint: `Removes the planned purchase${rents ? ', so the rent it would have replaced carries on' : ''}. The baseline keeps buying it.`,
    patch: [remove(join('/events', at(event, index)))],
  };
}

const STOCKS = ['us_stocks', 'intl_stocks'] as const;

/** Share of the target mix held in stocks of any kind, 0 to 1. */
export function stockShare(target: Allocation): number {
  const total = Object.values(target).reduce((n: number, v) => n + (v ?? 0), 0);
  if (total <= 0) return 0;
  return ((target.us_stocks ?? 0) + (target.intl_stocks ?? 0) + (target.single_stock ?? 0)) / total;
}

function mix(profile: Profile, stocks: number): QuickScenario | null {
  const target = profile.portfolio.target;
  const current = stockShare(target);
  const plain = Object.entries(target).every(([k, v]) => !v || k === 'bonds' || (STOCKS as readonly string[]).includes(k));
  if (Math.abs(current - stocks) < 0.005 && plain && profile.portfolio.glide.type === 'none') return null;
  const us = target.us_stocks ?? 0;
  const intl = target.intl_stocks ?? 0;
  const usShare = us + intl > 0 ? us / (us + intl) : 0.75;
  const next: Allocation = {};
  const usWeight = Math.round(stocks * 100 * usShare);
  if (usWeight > 0) next.us_stocks = usWeight;
  if (Math.round(stocks * 100) - usWeight > 0) next.intl_stocks = Math.round(stocks * 100) - usWeight;
  if (stocks < 1) next.bonds = Math.round((1 - stocks) * 100);
  const label = stocks >= 1 ? '100% stocks' : `${Math.round(stocks * 100)}/${Math.round((1 - stocks) * 100)} stocks and bonds`;
  return {
    key: stocks >= 1 ? 'all_stocks' : 'sixty_forty',
    group: 'Investing',
    label: stocks >= 1 ? 'Hold 100% stocks' : 'Hold 60/40',
    name: stocks >= 1 ? '100% stocks' : '60/40 stocks and bonds',
    hint: `Holds ${label} for life, with the plan’s split between US and international stocks and no shift toward bonds near retirement.`,
    patch: [set('/portfolio/target', next), set('/portfolio/glide', { type: 'none' })],
  };
}

function crash(profile: Profile): QuickScenario | null {
  if (profile.assumptions.market.stress.some((s) => s.type === 'crash_at_retirement')) return null;
  if (profile.retirement.trigger.type === 'already_retired') return null;
  return {
    key: 'crash_at_retirement',
    group: 'Stress tests',
    label: 'A crash at retirement',
    name: 'A 40% crash at retirement',
    hint: 'Forces a 40% fall in stocks in every life, starting the month of retirement, with five years to climb back. The worst timing a plan can meet.',
    patch: [append('/assumptions/market/stress', { type: 'crash_at_retirement', drawdown: 0.4, recovery_years: 5 })],
  };
}

function haircut(profile: Profile): QuickScenario | null {
  if (profile.assumptions.taxes.social_security_haircut) return null;
  if (profile.household.people.every((p) => p.social_security.basis.type === 'none')) return null;
  return {
    key: 'social_security_haircut',
    group: 'Stress tests',
    label: 'A Social Security cut',
    name: 'Social Security cut 21% from 2033',
    hint: 'Pays 79% of scheduled benefits from 2033, roughly what payroll taxes alone could fund when the trust fund runs short.',
    patch: [set('/assumptions/taxes/social_security_haircut', { from_year: 2033, factor: 0.79 })],
  };
}

/**
 * The what-ifs this plan can try. `summary` is the baseline result: retiring earlier or
 * later in a plan that retires "when funded" needs to know when the middle life retires.
 */
export function quickScenarios(profile: Profile, summary: Summary | null): QuickScenario[] {
  return [
    retireShift(profile, summary, -2),
    retireShift(profile, summary, 2),
    skipHome(profile),
    retirementSpending(profile, 0.9),
    retirementSpending(profile, 1.1),
    saveMore(profile, 0.05),
    partTime(profile, 5),
    delaySocialSecurity(profile, 70),
    mix(profile, 1),
    mix(profile, 0.6),
    crash(profile),
    haircut(profile),
  ].filter((q): q is QuickScenario => q !== null);
}

/** Two patches that make the same scenario (ignoring key order inside values). */
export function samePatch(a: PatchOp[], b: PatchOp[]): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}
