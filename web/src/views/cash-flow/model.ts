// From the Summary's composition series (means per living household, by year) to what
// the Cash flow view draws. Everything here is a mean over households with someone
// alive that year: at any age it mixes lives still working with lives already retired,
// and late in life it is pulled up by the luckiest. The view says so wherever it matters.
import type { Summary, YearSeries } from '@/engine/output-types';
import { compositionSeries, statusShares, summaryTime, type TimeAxis } from '@/ui/charts';

/** A stack member: one colour, one label, possibly the sum of several engine series. */
export interface Group {
  key: string;
  label: string;
  /** For labels inside narrow bands and in the flow diagram. */
  short: string;
  /** Index into `theme.categorical`. Fixed per entity, never by rank. */
  color: number;
  /** By year index. */
  values: number[];
  /** The engine series folded into this group, for tooltips, tables and the flow diagram. */
  parts: Part[];
}

export interface Part {
  key: string;
  label: string;
  values: number[];
}

export interface CashFlowModel {
  time: TimeAxis;
  /** First year index whose flows are a full calendar year (the plan's first year is usually partial). */
  flowStart: number;
  /** Last year index drawn: at least 1 household in 4 still alive, and never the plan's final (part) year. */
  end: number;
  /** Money in, bottom of the stack first. Only groups that ever matter. */
  moneyIn: Group[];
  /** Money out, nearest the zero line first. */
  moneyOut: Group[];
  totalIn: number[];
  totalOut: number[];
  /** Balance sheet: assets (bottom first) and liabilities (nearest zero first), year-end means. */
  assets: Group[];
  liabilities: Group[];
  netWorth: number[];
  investable: number[];
  /** Saving by destination and withdrawals by account, in account colours. */
  savingBy: Group[];
  withdrawBy: Group[];
  taxes: Group[];
  /** Total tax over all money coming in. Null where nothing comes in. */
  taxShare: (number | null)[];
  /** Saving over gross income and employer money, while a meaningful share of lives still work. */
  savingRate: (number | null)[];
  /** Net withdrawals over investable assets a year earlier, once a meaningful share of lives are retired. */
  withdrawalRate: (number | null)[];
  /** Share of living households still working / retired, by year. */
  workingShare: number[];
}

/** Averages are drawn while at least this share of households still has someone alive. */
export const MIN_ALIVE = 0.25;

/** Account colours, shared by the balance sheet and the account-flow chart (indices into theme.categorical). */
export const ACCOUNT_COLOR = { cash: 6, taxable: 0, traditional: 5, roth: 1, hsa: 2, property: 4, mortgage: 3, debt: 7 } as const;

const add = (...rows: number[][]) => (rows[0] ?? []).map((_, i) => rows.reduce((t, r) => t + (r[i] ?? 0), 0));

/** A group is kept when it reaches a visible share of the largest yearly total at least once. */
function present(groups: Group[], from: number, to: number, floor = 0.004): Group[] {
  let peak = 0;
  for (let i = from; i <= to; i++) peak = Math.max(peak, groups.reduce((t, g) => t + (g.values[i] ?? 0), 0));
  return groups.filter((g) => Math.max(0, ...g.values.slice(from, to + 1)) >= peak * floor && peak > 0);
}

export function cashFlowModel(summary: Summary): CashFlowModel {
  const time = summaryTime(summary);
  const n = time.length;
  const s = (key: YearSeries) => {
    const row = compositionSeries(summary, key);
    return Array.from({ length: n }, (_, i) => Math.max(0, row[i] ?? 0));
  };
  const part = (key: string, label: string, values: number[]): Part => ({ key, label, values });
  const group = (key: string, label: string, short: string, color: number, parts: Part[]): Group => ({ key, label, short, color, parts, values: add(...parts.map((p) => p.values)) });

  // Averages over a handful of survivors are noise, and the plan's final year is a part-year
  // blown up to a full one: stop while at least 1 household in 4 is alive, short of the last year.
  let end = Math.max(1, n - 2);
  while (end > 1 && (summary.axis.alive[end] ?? 0) < summary.paths * MIN_ALIVE) end--;
  const flowStart = summary.axis.first_year_months < 12 && n > 1 ? 1 : 0;

  // Money often passes through the cash account on its way somewhere else: only the net move counts.
  const cashOut = s('withdraw_cash');
  const cashIn = s('save_cash');
  const fromCash = cashOut.map((v, i) => Math.max(0, v - (cashIn[i] ?? 0)));
  const toCash = cashIn.map((v, i) => Math.max(0, v - (cashOut[i] ?? 0)));

  const withdrawParts = [
    part('withdraw_taxable', 'From taxable accounts', s('withdraw_taxable')),
    part('withdraw_traditional', 'From pre-tax accounts', s('withdraw_traditional')),
    part('withdraw_roth', 'From Roth accounts', s('withdraw_roth')),
    part('withdraw_hsa', 'From the HSA', s('withdraw_hsa')),
    part('withdraw_cash', 'From cash', fromCash),
  ];
  const savingParts = [
    part('save_taxable', 'To taxable accounts', s('save_taxable')),
    part('save_traditional', 'To pre-tax accounts', s('save_traditional')),
    part('save_roth', 'To Roth accounts', s('save_roth')),
    part('save_hsa', 'To the HSA', s('save_hsa')),
    part('save_employer', 'Employer contributions', s('save_employer')),
    part('save_cash', 'To cash', toCash),
  ];
  const taxParts = [
    part('tax_federal', 'Federal income tax', s('tax_federal')),
    part('tax_state', 'State income tax', s('tax_state')),
    part('tax_payroll', 'Payroll tax', s('tax_payroll')),
    part('tax_penalties', 'Penalties', s('tax_penalties')),
  ];

  const allIn = [
    group('wages', 'Wages', 'Wages', 0, [part('income_wages', 'Wages', s('income_wages'))]),
    group('self_employment', 'Self-employment', 'Self-employed', 6, [part('income_self_employment', 'Self-employment', s('income_self_employment'))]),
    group('rental', 'Rental income', 'Rental', 5, [part('income_rental', 'Rental income', s('income_rental'))]),
    group('pensions', 'Pensions and annuities', 'Pensions', 2, [part('income_pensions', 'Pensions and annuities', s('income_pensions'))]),
    group('social_security', 'Social Security', 'Social Security', 4, [part('income_social_security', 'Social Security', s('income_social_security'))]),
    group('withdrawals', 'Withdrawals from savings', 'Withdrawals', 3, withdrawParts),
    group('other_in', 'Other money in', 'Other', 7, [
      part('income_other', 'Other income', s('income_other')),
      part('save_employer', 'Employer contributions', s('save_employer')),
      part('property_sale', 'Property sale proceeds', s('property_sale')),
    ]),
  ];
  const allOut = [
    group('essentials', 'Essentials', 'Essentials', 1, [part('spend_essential', 'Essentials', s('spend_essential'))]),
    group('housing', 'Housing', 'Housing', 2, [part('spend_housing', 'Housing (owned homes)', s('spend_housing'))]),
    group('children', 'Children', 'Children', 5, [part('spend_children', 'Children', s('spend_children'))]),
    group('health', 'Healthcare and care', 'Health', 4, [part('spend_healthcare', 'Healthcare', s('spend_healthcare')), part('spend_long_term_care', 'Long-term care', s('spend_long_term_care'))]),
    group('discretionary', 'Discretionary', 'Discretionary', 3, [part('spend_discretionary', 'Discretionary', s('spend_discretionary'))]),
    group('other_out', 'Other spending', 'Other', 7, [
      part('spend_debt', 'Debt payments', s('spend_debt')),
      part('spend_one_off', 'One-off costs', s('spend_one_off')),
      part('property_purchase', 'Property purchases', s('property_purchase')),
    ]),
    group('taxes', 'Taxes', 'Taxes', 6, taxParts),
    group('saving', 'Saving', 'Saving', 0, savingParts),
  ];

  const moneyIn = present(allIn, flowStart, end);
  const moneyOut = present(allOut, flowStart, end);
  const totalIn = add(...allIn.map((g) => g.values));
  const totalOut = add(...allOut.map((g) => g.values));

  const single = (key: string, label: string, short: string, color: number, series: YearSeries) => group(key, label, short, color, [part(series, label, s(series))]);
  const assets = present(
    [
      single('cash', 'Cash', 'Cash', ACCOUNT_COLOR.cash, 'balance_cash'),
      single('taxable', 'Taxable accounts', 'Taxable', ACCOUNT_COLOR.taxable, 'balance_taxable'),
      single('traditional', 'Pre-tax accounts', 'Pre-tax', ACCOUNT_COLOR.traditional, 'balance_traditional'),
      single('roth', 'Roth accounts', 'Roth', ACCOUNT_COLOR.roth, 'balance_roth'),
      single('hsa', 'HSA', 'HSA', ACCOUNT_COLOR.hsa, 'balance_hsa'),
      single('property', 'Property', 'Property', ACCOUNT_COLOR.property, 'property_value'),
    ],
    0,
    end,
  );
  const liabilities = present([single('mortgage', 'Mortgages', 'Mortgage', ACCOUNT_COLOR.mortgage, 'mortgage'), single('debt', 'Other debt', 'Debt', ACCOUNT_COLOR.debt, 'debt')], 0, end, 0.02);
  const investable = add(s('balance_cash'), s('balance_taxable'), s('balance_traditional'), s('balance_roth'), s('balance_hsa'));
  const netWorth = investable.map((v, i) => v + (s('property_value')[i] ?? 0) - (s('mortgage')[i] ?? 0) - (s('debt')[i] ?? 0));

  const account = (key: string, label: string, short: string, color: number, values: number[]) => group(key, label, short, color, [part(key, label, values)]);
  const savingBy = present(
    [
      account('to_cash', 'Cash', 'Cash', ACCOUNT_COLOR.cash, toCash),
      account('to_taxable', 'Taxable accounts', 'Taxable', ACCOUNT_COLOR.taxable, s('save_taxable')),
      account('to_traditional', 'Pre-tax accounts', 'Pre-tax', ACCOUNT_COLOR.traditional, s('save_traditional')),
      account('to_roth', 'Roth accounts', 'Roth', ACCOUNT_COLOR.roth, s('save_roth')),
      account('to_hsa', 'HSA', 'HSA', ACCOUNT_COLOR.hsa, s('save_hsa')),
      account('to_employer', 'Employer contributions', 'Employer', ACCOUNT_COLOR.debt, s('save_employer')),
    ],
    flowStart,
    end,
    0.01,
  );
  const withdrawBy = present(
    [
      account('from_cash', 'Cash', 'Cash', ACCOUNT_COLOR.cash, fromCash),
      account('from_taxable', 'Taxable accounts', 'Taxable', ACCOUNT_COLOR.taxable, s('withdraw_taxable')),
      account('from_traditional', 'Pre-tax accounts', 'Pre-tax', ACCOUNT_COLOR.traditional, s('withdraw_traditional')),
      account('from_roth', 'Roth accounts', 'Roth', ACCOUNT_COLOR.roth, s('withdraw_roth')),
      account('from_hsa', 'HSA', 'HSA', ACCOUNT_COLOR.hsa, s('withdraw_hsa')),
    ],
    flowStart,
    end,
    0.01,
  );
  const taxes = present(
    [
      account('tax_federal', 'Federal income tax', 'Federal', 4, s('tax_federal')),
      account('tax_state', 'State income tax', 'State', 5, s('tax_state')),
      account('tax_payroll', 'Payroll tax', 'Payroll', 1, s('tax_payroll')),
      account('tax_penalties', 'Penalties', 'Penalties', 2, s('tax_penalties')),
    ],
    flowStart,
    end,
    0.01,
  );

  const alive = summary.survival.household;
  const working = statusShares(summary, 'working');
  const workingShare = working.map((w, i) => ((alive[i] ?? 0) > 0 ? Math.min(1, w / (alive[i] as number)) : 0));
  const taxTotal = add(...taxParts.map((p) => p.values));
  const savingTotal = add(...savingParts.map((p) => p.values));
  const withdrawTotal = add(...withdrawParts.map((p) => p.values));
  const gross = add(s('income_wages'), s('income_self_employment'), s('income_social_security'), s('income_pensions'), s('income_rental'), s('income_other'), s('save_employer'));

  const inRange = (i: number) => i >= flowStart && i <= end;
  const taxShare = totalIn.map((t, i) => (inRange(i) && t > 0 ? (taxTotal[i] ?? 0) / t : null));
  const savingRate = gross.map((g, i) => (inRange(i) && g > 0 && (workingShare[i] ?? 0) >= 0.25 ? (savingTotal[i] ?? 0) / g : null));
  const withdrawalRate = withdrawTotal.map((w, i) => {
    const before = investable[i - 1];
    if (!inRange(i) || before === undefined || before <= 0 || 1 - (workingShare[i] ?? 0) < 0.25) return null;
    return Math.max(0, w - (savingTotal[i] ?? 0)) / before;
  });

  return { time, flowStart, end, moneyIn, moneyOut, totalIn, totalOut, assets, liabilities, netWorth, investable, savingBy, withdrawBy, taxes, taxShare, savingRate, withdrawalRate, workingShare };
}

/** One node of the single-year flow diagram: a part of a group, in the group's colour. */
export interface YearNode {
  key: string;
  label: string;
  group: string;
  color: number;
  /** By year index. */
  values: number[];
}

const NODE_LABEL: Record<string, string> = {
  income_wages: 'Wages',
  income_self_employment: 'Self-employment',
  income_rental: 'Rental income',
  income_pensions: 'Pensions',
  income_social_security: 'Social Security',
  income_other: 'Other income',
  save_employer: 'Employer money',
  property_sale: 'Property sale',
  withdraw_taxable: 'From taxable',
  withdraw_traditional: 'From pre-tax',
  withdraw_roth: 'From Roth',
  withdraw_hsa: 'From HSA',
  withdraw_cash: 'From cash',
  spend_essential: 'Essentials',
  spend_housing: 'Housing',
  spend_children: 'Children',
  spend_healthcare: 'Healthcare',
  spend_long_term_care: 'Long-term care',
  spend_discretionary: 'Discretionary',
  spend_debt: 'Debt payments',
  spend_one_off: 'One-off costs',
  property_purchase: 'Property purchase',
  tax_federal: 'Federal tax',
  tax_state: 'State tax',
  tax_payroll: 'Payroll tax',
  tax_penalties: 'Penalties',
  save_taxable: 'To taxable',
  save_traditional: 'To pre-tax',
  save_roth: 'To Roth',
  save_hsa: 'To HSA',
  save_cash: 'To cash',
};

/**
 * The nodes of the flow diagram: every part that is ever a visible share of a year's
 * total, in stack order. The set is fixed for a summary, so the diagram can glide from
 * year to year; a part that is zero in the chosen year simply has no height.
 */
export function yearNodes(model: CashFlowModel): { sources: YearNode[]; uses: YearNode[] } {
  const side = (groups: Group[], total: number[], prefix: string) =>
    groups.flatMap((g) =>
      g.parts
        .filter((p) => p.values.some((v, i) => i >= model.flowStart && i <= model.end && v >= (total[i] ?? 0) * 0.005 && v > 0))
        .map((p): YearNode => ({ key: `${prefix}:${p.key}`, label: NODE_LABEL[p.key] ?? p.label, group: g.key, color: g.color, values: p.values })),
    );
  return { sources: side(model.moneyIn, model.totalIn, 'in'), uses: side(model.moneyOut, model.totalOut, 'out') };
}

/** Totals of the three broad uses in a year, as shares of money out. */
export function sharesOfUses(model: CashFlowModel, i: number): { spent: number; taxed: number; saved: number; total: number } {
  const total = model.totalOut[i] ?? 0;
  const of = (key: string) => model.moneyOut.find((g) => g.key === key)?.values[i] ?? 0;
  const taxed = of('taxes');
  const saved = of('saving');
  return total > 0 ? { spent: (total - taxed - saved) / total, taxed: taxed / total, saved: saved / total, total } : { spent: 0, taxed: 0, saved: 0, total: 0 };
}

/** The first year (index) from which withdrawals bring in more than work does and keep doing so for three years. */
export function handoverIndex(model: CashFlowModel): number | null {
  const of = (groups: Group[], key: string, i: number) => groups.find((g) => g.key === key)?.values[i] ?? 0;
  const ahead = (i: number) => of(model.moneyIn, 'withdrawals', i) > of(model.moneyIn, 'wages', i) + of(model.moneyIn, 'self_employment', i);
  for (let i = model.flowStart; i <= model.end - 2; i++) if (ahead(i) && ahead(i + 1) && ahead(i + 2)) return i;
  return null;
}
