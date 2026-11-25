// The JSON the engine produces: `Summary` (one run), `Comparison` (two runs on the same
// paths) and `PathTrace` (one life). Transcribed from docs/engine-output.md, which is the
// field-by-field guide; the Rust types in crates/firesight-core/src/stats are the source
// of truth. If they change, this is the only file to update.
//
// Conventions (see the doc): money is today's dollars, whole numbers; ages are the first
// person's; rates and shares are fractions of 1; a number that does not exist is null.
// Per-year arrays are indexed by year index y (axis.years[y], axis.ages[y]) and cover
// households with someone alive that year (axis.alive[y] is the sample size).
//
// `output-shapes.ts` holds a runtime shape for every document here and
// `output-shapes.test.ts` checks real engine output against them (and the shapes against
// these types at compile time), so drift in either direction fails the tests.

/** A probability or mean with its 95% interval (sampling error only). */
export interface Estimate {
  value: number;
  std_error: number;
  lo: number;
  hi: number;
}

/**
 * An estimate over a subset of paths (the retired, the paired): every field is null
 * when the subset is empty. Shares of all paths use `Estimate` and are never null.
 */
export interface NullableEstimate {
  value: number | null;
  std_error: number | null;
  lo: number | null;
  hi: number | null;
}

/**
 * Quantiles of a per-path quantity. For FI age and retirement age, "never" counts as
 * later than any age, so `p50` is null when fewer than half the paths get there.
 */
export interface Spread {
  p10: number | null;
  p25: number | null;
  p50: number | null;
  p75: number | null;
  p90: number | null;
  /** Distribution-free 95% interval on the median. */
  p50_lo: number | null;
  p50_hi: number | null;
  /** Paths with a value. */
  count: number;
}

/** Year-end state of a household, in the engine's (stable) order. */
export const STATUSES = ['working', 'retired_surplus', 'retired_on_plan', 'retired_cutting', 'depleted', 'dead'] as const;
export type Status = (typeof STATUSES)[number];

export const HIST_SERIES = ['net_worth', 'investable', 'spending', 'gross_income', 'total_tax', 'spend_planned'] as const;
export type HistSeries = (typeof HIST_SERIES)[number];

export const YEAR_SERIES_KINDS = ['balance', 'liability', 'income', 'withdrawal', 'tax', 'spending', 'saving', 'transfer', 'memo'] as const;
export type YearSeriesKind = (typeof YEAR_SERIES_KINDS)[number];

// prettier-ignore
export const YEAR_SERIES = [
  'balance_cash', 'balance_taxable', 'balance_traditional', 'balance_roth', 'balance_hsa', 'property_value',
  'mortgage', 'debt', 'income_wages', 'income_self_employment', 'income_social_security', 'income_pensions',
  'income_rental', 'income_other', 'withdraw_cash', 'withdraw_taxable', 'withdraw_traditional', 'withdraw_roth',
  'withdraw_hsa', 'tax_federal', 'tax_state', 'tax_payroll', 'tax_penalties', 'spend_essential',
  'spend_discretionary', 'spend_housing', 'spend_healthcare', 'spend_long_term_care', 'spend_children', 'spend_debt',
  'spend_one_off', 'save_cash', 'save_taxable', 'save_traditional', 'save_roth', 'save_hsa',
  'save_employer', 'roth_conversion', 'property_purchase', 'property_sale', 'spend_planned', 'investment_income',
  'investment_return', 'income_equity', 'equity_exercise', 'equity_paper_value',
] as const;
export type YearSeries = (typeof YEAR_SERIES)[number];

/**
 * One number per path, in the engine's order. `coast_fi_age` was appended in accumulator
 * version 3, the nine from `extra_spending_ratio` in version 5, `medicaid_age` and the six
 * private-company scalars after it in version 7.
 */
// prettier-ignore
export const PATH_SCALARS = [
  'fi_age', 'retirement_age', 'depletion_age', 'death_age_0', 'death_age_1', 'investable_at_retirement',
  'early_retirement_return', 'worst_drawdown', 'months_unemployed', 'long_term_care_cost', 'inflation', 'funded_ratio',
  'failure_cause', 'success', 'years_simulated', 'terminal_net_worth', 'peak_net_worth', 'min_investable',
  'lifetime_tax', 'lifetime_spending', 'delivered_spending_ratio', 'worst_spending_ratio', 'years_retired', 'years_below_90',
  'years_below_75', 'years_depleted', 'coast_fi_age', 'extra_spending_ratio', 'initial_withdrawal_rate', 'home_sale_age',
  'plan_delivered_share', 'social_security_claim_age_0', 'social_security_claim_age_1', 'person_retirement_age_0',
  'person_retirement_age_1', 'mortgage_payoff_age', 'medicaid_age', 'equity_outcome', 'equity_event_age',
  'equity_gross_proceeds', 'equity_exercise_cost', 'equity_tender_proceeds', 'equity_net_proceeds',
] as const;
export type PathScalar = (typeof PATH_SCALARS)[number];

export const SCALAR_UNITS = ['age', 'dollars', 'rate', 'ratio', 'months', 'years', 'index', 'flag'] as const;
export type ScalarUnit = (typeof SCALAR_UNITS)[number];

export const FAILURE_CAUSES = ['market_sequence', 'job_loss', 'health', 'longevity', 'overspending', 'inflation', 'other'] as const;
export type FailureCause = (typeof FAILURE_CAUSES)[number];

/**
 * What happened to a private company on one path, in the engine's order (the value of
 * the `equity_outcome` scalar is an index into this list). `still_private` is "no sale,
 * listing or failure within the life", which includes a write-off after thirty years.
 */
export const EQUITY_OUTCOMES = ['failed', 'still_private', 'acquired', 'ipo'] as const;
export type EquityOutcome = (typeof EQUITY_OUTCOMES)[number];

export const FAILURE_FACTORS = ['poor_early_returns', 'deep_drawdown', 'job_loss', 'long_term_care', 'long_life', 'high_inflation'] as const;
export type FailureFactor = (typeof FAILURE_FACTORS)[number];

export interface Axis {
  /** "YYYY-MM". */
  start: string;
  start_age: number;
  first_year_months: number;
  years: number[];
  ages: number[];
  /** Households with someone alive in each year. Fade or cut charts where it gets small. */
  alive: number[];
}

export interface Headline {
  success_rate: Estimate;
  fi_rate: Estimate;
  retired_rate: Estimate;
  depletion_rate: Estimate;
  median_fi_age: number | null;
  fi_age: Spread;
  median_retirement_age: number | null;
  retirement_age: Spread;
  /** Among paths that deplete. */
  depletion_age: Spread;
  years_depleted: Spread;
  median_terminal_net_worth: number | null;
  p10_terminal_net_worth: number | null;
  terminal_net_worth: Spread;
  investable_at_retirement: Spread;
  /** Mean over retired paths of sum(min(spending, planned)) / sum(planned) over retired years. Never above 1. */
  delivered_spending_ratio: NullableEstimate;
  /** Mean over retired paths of the per-year capped share min(spending / planned, 1): every year weighs the same. */
  plan_delivered_share: NullableEstimate;
  /** Mean over retired paths of retirement spending above the plan, as a fraction of plan. */
  extra_spending_ratio: NullableEstimate;
  /** First retired year's draw over investable assets at retirement, among paths that retire. */
  initial_withdrawal_rate: Spread;
  /** Share of all paths on which a home was sold as a last resort. These paths are not counted as depleted. */
  home_sale_rate: Estimate;
  /** Share of all paths on which Medicaid paid for long-term care after savings were spent down. Zero unless `spending.healthcare.medicaid` is on. */
  medicaid_rate: Estimate;
  /** Share of retired path-years spent cutting or depleted. */
  years_cutting_share: number | null;
  median_lifetime_tax: number | null;
  lifetime_tax: Spread;
  lifetime_spending: Spread;
  funded_ratio: Spread | null;
}

export interface SpendingDelivery {
  retired_paths: number;
  paths_below_90: NullableEstimate;
  paths_below_75: NullableEstimate;
  years_below_90: NullableEstimate;
  years_below_75: NullableEstimate;
  years_cut: Spread;
  /** [0.5, 0.75, 0.9, 0.95, 0.99] */
  worst_cut_levels: number[];
  worst_cut: (number | null)[];
  by_year: {
    /** Share of the living that are retired. */
    retired: (number | null)[];
    /** Share of the retired below each line. */
    below_90: (number | null)[];
    below_75: (number | null)[];
    /** Share of living households whose year ended depleted. */
    depleted: (number | null)[];
    /** Among those, mean planned less delivered spending for the year (annualised); null when nobody is depleted. */
    depleted_shortfall: (number | null)[];
  };
}

/** `values[i][y]` is quantile `levels[i]` in year y. Levels: 1, 5, 10, 25, 50, 75, 90, 95, 99%. */
export interface QuantileCurves {
  series: HistSeries;
  levels: number[];
  values: (number | null)[][];
}

/** Yearly histogram on a coarse grid. Cell k of year y spans edges[starts[y]+k] .. edges[starts[y]+k+1]. */
export interface DensityFan {
  series: HistSeries;
  edges: number[];
  starts: number[];
  counts: number[][];
  /** Paths at exactly zero, which are in no cell. */
  zeros: number[];
}

/** `shares[s][y]`: share of all paths in `statuses[s]`; the rows sum to 1 each year. */
export interface StatusShares {
  statuses: Status[];
  shares: number[][];
}

export interface Survival {
  household: number[];
  people: number[][];
}

/** Mean per living household: year-end level for balances, annualised for flows. */
export interface CompositionSeries {
  series: YearSeries;
  kind: YearSeriesKind;
  values: (number | null)[];
}

/** When something happens, by the first person's age, over all paths. */
export interface AgeCurve {
  /** Whole ages; entry i covers [ages[i], ages[i] + 1). */
  ages: number[];
  counts: number[];
  /** Share of all paths by the end of that age, with its Wilson interval. */
  cumulative: number[];
  lo: number[];
  hi: number[];
  /** Share of paths on which it never happens. */
  never: number;
  /** 100 ages at quantiles (i + 0.5) / 100; null is "never". The quantile dot plot. */
  dots: (number | null)[];
}

export interface ScalarDistribution {
  scalar: PathScalar;
  unit: ScalarUnit;
  count: number;
  missing: number;
  mean: number | null;
  std_dev: number | null;
  min: number | null;
  max: number | null;
  quantiles: (number | null)[];
  dots: (number | null)[];
  /** 40 bins spanning the 0.5th to 99.5th percentile; empty when there are no values. */
  edges: number[];
  counts: number[];
}

export interface Scatter {
  /** Path indices, usable with `pool.trace`. */
  paths: number[];
  columns: { scalar: PathScalar; values: (number | null)[] }[];
}

export interface FailureAttribution {
  failed: number;
  never_retired: number;
  depleted: number;
  causes: { cause: FailureCause; count: number; share_of_failed: number | null; share_of_all: number }[];
  /** Overlapping tags, not an additive decomposition. */
  factors: { factor: FailureFactor; scalar: PathScalar; above: boolean; threshold: number | null; failed: number | null; succeeded: number | null }[];
  contrasts: {
    scalar: PathScalar;
    failed_count: number;
    succeeded_count: number;
    failed_median: number | null;
    succeeded_median: number | null;
    failed_mean: number | null;
    succeeded_mean: number | null;
  }[];
}

/** A representative life chosen by a stated rule. Trace it with `pool.trace(profile, path_index)`. */
export interface PathPick {
  /**
   * 'p10' | 'median' | 'p90' (rank of terminal net worth), 'median_fi', 'failing', and for a
   * plan with private-company equity 'equity_failed' (median terminal net worth among lives
   * where the company fails), 'equity_paid_median' and 'equity_paid_p90' (those ranks of net
   * proceeds among lives where the equity paid something).
   */
  label: string;
  scalar: PathScalar;
  path_index: number;
  value: number | null;
}

export interface SamplePath {
  path_index: number;
  net_worth: number[];
  investable: number[];
  spending: number[];
}

/** `summarize(acc, profileJson)` output. */
export interface Summary {
  version: number;
  paths: number;
  axis: Axis;
  headline: Headline;
  spending: SpendingDelivery;
  quantiles: QuantileCurves[];
  density: DensityFan[];
  status: StatusShares;
  survival: Survival;
  composition: CompositionSeries[];
  fi: AgeCurve;
  retirement: AgeCurve;
  depletion: AgeCurve;
  distributions: ScalarDistribution[];
  scatter: Scatter;
  failure: FailureAttribution;
  picks: PathPick[];
  /** The first 96 paths, annual. */
  samples: SamplePath[];
  /** The same annual series for each distinct `picks[].path_index`, in the order of `picks`. May be empty. */
  pick_paths: SamplePath[];
  plan_moments: PlanMoments;
  /** Outcomes of the household's private-company equity. Absent when the plan has no private company. */
  equity?: EquitySummary;
}

/** One outcome's share of paths. */
export interface EquityOutcomeShare {
  outcome: EquityOutcome;
  count: number;
  /** Wilson 95% interval, like the headline rates. */
  share: Estimate;
}

/** Headline results among the paths with one equity outcome. */
export interface EquityConditional {
  outcome: EquityOutcome;
  /** Paths with this outcome. */
  paths: number;
  /** False when fewer than 30 paths have it: the spreads are then empty and the rates have very wide intervals. */
  enough: boolean;
  /** Null only when the class is empty. */
  success_rate: NullableEstimate;
  fi_rate: NullableEstimate;
  /** FI age within the class, counting "never" as later than any age. */
  fi_age: Spread;
  retirement_age: Spread;
  terminal_net_worth: Spread;
  net_proceeds: Spread;
}

/**
 * `Summary.equity`: what happens to the household's private-company equity. Money in
 * today's dollars. With several companies the outcome shares describe the one with the
 * largest position at the start (`company`); the dollar figures total every counted company.
 */
export interface EquitySummary {
  /** Name of the company the outcome shares describe. */
  company: string;
  /** Paths with an outcome (all paths of a plan with private equity). */
  paths: number;
  /** Share of paths with each outcome, in `EQUITY_OUTCOMES` order. */
  outcomes: EquityOutcomeShare[];
  /** Share of paths on which the equity paid nothing at all (gross proceeds 0). */
  nothing_rate: Estimate;
  /** Share of paths on which more went in (exercise cost and tax) than came back (net proceeds below 0). */
  loss_rate: Estimate;
  /** Share of paths on which options were exercised for cash. */
  exercised_rate: Estimate;
  /** Share of paths with proceeds from a tender offer. */
  tender_rate: Estimate;
  /** Gross proceeds over all paths, zeros included. */
  gross_proceeds: Spread;
  /** Net proceeds (after exercise cost and tax) over all paths. */
  net_proceeds: Spread;
  /** Mean net proceeds over all paths. Carried by the few best outcomes, so the interval is wide. */
  net_proceeds_mean: Estimate;
  /**
   * Fewest paths whose net proceeds add up to half of all the positive net proceeds: how
   * few lives carry the mean. When small (under about 100) the mean's interval is not a
   * 95% bound. 0 when no path has positive proceeds.
   */
  net_proceeds_mean_half_from: number;
  /** Net proceeds among paths that received something. */
  net_proceeds_if_paid: Spread;
  /** Exercise cost among paths that paid any. */
  exercise_cost: Spread;
  /** Tender proceeds among paths that had any. */
  tender_proceeds: Spread;
  /** Share of paths on which the company had been acquired or gone public by each age (first person's). */
  liquidity_by_age: AgeCurve;
  /** Share of paths on which the company had failed by each age. */
  failed_by_age: AgeCurve;
  /** The plan's headline results among the paths with each outcome, in `EQUITY_OUTCOMES` order. */
  by_outcome: EquityConditional[];
}

/**
 * When the plan's moments fall. Ages are medians over the paths on which the thing happens
 * (each person's own age; the first person's for the mortgage); shares are of all paths.
 */
export interface PlanMoments {
  social_security_claim_age: (number | null)[];
  social_security_claim_share: (number | null)[];
  person_retirement_age: (number | null)[];
  person_retirement_share: (number | null)[];
  mortgage_payoff_age: number | null;
  mortgage_payoff_year: number | null;
  mortgage_payoff_share: number | null;
}

// ---- Comparison (B minus A; both runs on the same seed) -----------------------------

export interface OutcomeComparison {
  outcome: 'success' | 'fi' | 'retired' | 'depleted';
  a: Estimate;
  b: Estimate;
  both: number;
  /** For `success`: harmed by the change. */
  a_only: number;
  /** For `success`: rescued by the change. */
  b_only: number;
  neither: number;
  difference: Estimate;
  z: number | null;
  /** True when the 95% interval of the difference contains zero. */
  within_noise: boolean;
}

export interface ScalarComparison {
  scalar: PathScalar;
  unit: ScalarUnit;
  both: number;
  a_only: number;
  b_only: number;
  neither: number;
  mean_a: number | null;
  mean_b: number | null;
  /** All four fields are null when no path has a value in both runs. */
  mean_delta: NullableEstimate;
  /** Of the per-path difference, over paths with a value in both runs. */
  quantiles: (number | null)[];
  /** 100 values, or empty when no path has a value in both runs. */
  dots: (number | null)[];
  /** Shares of the paired paths; null when there are none. */
  b_higher: number | null;
  a_higher: number | null;
  equal: number | null;
  within_noise: boolean;
}

export const COMPARISON_METRICS = [
  'success_rate', 'fi_rate', 'depletion_rate', 'median_fi_age', 'median_retirement_age', 'median_terminal_net_worth',
  'p10_terminal_net_worth', 'median_lifetime_tax', 'median_lifetime_spending', 'delivered_spending_ratio',
] as const;
export type ComparisonMetric = (typeof COMPARISON_METRICS)[number];

/** Paired comparison of a scenario (B) against the baseline (A), path by path: wasm `compare(a, b, profile)`. */
export interface Comparison {
  paths: number;
  unpaired_a: number;
  unpaired_b: number;
  outcomes: OutcomeComparison[];
  scalars: ScalarComparison[];
  /** The levels of `scalars[].quantiles`. */
  quantile_levels: number[];
  headline: { metric: ComparisonMetric; a: number | null; b: number | null; delta: number | null }[];
  /** The comparison within each equity outcome of run A. Absent when A has no private company. */
  by_equity_outcome?: PairedByOutcome[];
}

/** A paired comparison among the lives with one equity outcome in run A. */
export interface PairedByOutcome {
  outcome: EquityOutcome;
  /** Pairs whose A row has this outcome. */
  paths: number;
  /** Success in A and in B, with flips. */
  success: OutcomeComparison;
  /** B minus A terminal net worth. */
  terminal_net_worth: ScalarComparison;
}

// ---- PathTrace (one life in full) ---------------------------------------------------

// prettier-ignore
export const TRACE_EVENT_KINDS = [
  'financially_independent', 'retired', 'laid_off', 'rehired', 'career_break', 'equity_event',
  'bought_home', 'sold_home', 'relocated', 'windfall', 'major_expense', 'expense_shock',
  'roth_conversion', 'spending_cut', 'spending_raise', 'depleted', 'recovered', 'claimed_social_security',
  'health_coverage', 'long_term_care', 'rmd_started', 'debt_paid_off', 'child', 'partner_joined',
  'died', 'tax_loss_harvest', 'qualified_charitable_distribution', 'policy_change', 'market', 'life_event', 'other',
  'health_state', 'bereavement', 'medicaid', 'ltc_insurance', 'options_exercised', 'tender_offer',
  'company_acquired', 'company_ipo', 'company_failed', 'equity_forfeited', 'lockup_sale',
] as const;
export type TraceEventKind = (typeof TRACE_EVENT_KINDS)[number];

/**
 * A series over time. Keys: `net_worth`, `investable`, every YearSeries that is not zero
 * throughout (a missing series is zero), and in the monthly table `price_level`,
 * `portfolio_return` and `return_<asset class>`.
 */
export interface TraceColumn {
  key: string;
  unit: string;
  values: (number | null)[];
}

export interface TraceTaxReturn {
  year: number;
  /** Divide the nominal amounts by this for today's dollars. */
  price_level: number;
  /** The engine's `TaxResult` for the year (federal_ordinary, state, fica[], agi, magi, marginal_ordinary_rate ...). */
  result: Record<string, unknown>;
  /** Nominal dollars of the tax year. */
  lines: { key: string; label: string; amount: number; note: string | null }[];
}

export interface TraceEvent {
  /** "YYYY-MM". */
  month: string;
  kind: TraceEventKind;
  label: string;
  amount: number | null;
}

/** One path's full ledger from `Engine.trace(path)`. */
export interface PathTrace {
  path_index: number;
  seed: number;
  start: string;
  months: number;
  statuses: Status[];
  /** Per month: index into `statuses`. */
  status: number[];
  columns: TraceColumn[];
  annual: { years: number[]; months: number[]; status: number[]; columns: TraceColumn[] };
  tax_returns: TraceTaxReturn[];
  events: TraceEvent[];
  scalars: { scalar: PathScalar; value: number | null }[];
}

// ---- SummaryDigest (the small form for an LLM tool result) ---------------------------

export interface DigestRate {
  p: number;
  lo: number;
  hi: number;
}

export interface DigestSpread {
  p10: number | null;
  p50: number | null;
  p90: number | null;
}

/** `Merger.digest(profileJson)` / `digest(acc, profileJson)`: about 1.7 KB. Dollars to $1,000, probabilities to 3 dp. */
export interface SummaryDigest {
  /** States the units and the definition of success, so the numbers cannot be misread out of context. */
  notes: string;
  paths: number;
  start_year: number;
  end_year: number;
  start_age: number;
  success: DigestRate;
  fi: DigestRate;
  depletion: DigestRate;
  fi_age: DigestSpread;
  retirement_age: DigestSpread;
  terminal_net_worth: DigestSpread;
  lifetime_tax_p50: number | null;
  /** Cumulative share of all lives by each (decade) age. */
  fi_by_age: { ages: number[]; p: number[] };
  depletion_by_age: { ages: number[]; p: number[] };
  /** Six evenly spaced ages; `alive` is the share of households with someone alive. */
  by_age: {
    ages: number[];
    alive: number[];
    net_worth_p10: (number | null)[];
    net_worth_p50: (number | null)[];
    net_worth_p90: (number | null)[];
    spending_p10: (number | null)[];
    spending_p50: (number | null)[];
    spending_p90: (number | null)[];
  };
  spending_delivery: {
    delivered_ratio: number | null;
    extra_ratio: number | null;
    paths_below_90: number | null;
    paths_below_75: number | null;
    years_below_90: number | null;
    worst_cut_p90: number | null;
  };
  /** Top three first causes, as a share of failed paths. */
  failure_causes: { cause: FailureCause; share: number }[];
  trace_paths: { p10: number | null; median: number | null; p90: number | null; failing: number | null };
  /** Private-company equity outcomes. Absent when the plan has no private company. */
  equity?: DigestEquity;
}

/** Private-company equity outcomes, for the digest. Shares to 3 dp, dollars to $1,000. */
export interface DigestEquity {
  /** Company the outcome shares describe. */
  company: string;
  /** Share of lives in which it fails / has no outcome within the life / is acquired / goes public. */
  failed: number;
  still_private: number;
  acquired: number;
  ipo: number;
  /** Share of lives in which the equity pays nothing at all. */
  nothing: DigestRate;
  /** Share of lives in which more went in than came back. */
  loss: number;
  /** Proceeds after exercise cost and tax over all lives. */
  net_proceeds: DigestSpread;
  /** Their mean, carried by the best few lives, and its standard error. */
  net_proceeds_mean: number | null;
  net_proceeds_mean_se: number | null;
  /** Probability of success among lives with each outcome, in `EQUITY_OUTCOMES` order; null where fewer than 30 lives. */
  success_by_outcome: (number | null)[];
}

// ---- PlanPreview (deterministic, no simulation) --------------------------------------

/** `planPreview(profileJson)`: what the plan implies before any simulation. Today's dollars, not rounded. */
export interface PlanPreview {
  /** Budget + home costs + modelled healthcare + children + debt payments, this year. */
  annual_spending: number;
  annual_spending_retired: number;
  /** Gross earned income. */
  annual_income: number;
  /** Share of income neither spent nor taxed. */
  savings_rate: number;
  annual_tax: number;
  investable: number;
  /** What the funded test counts. */
  wealth: number;
  /** The "number": wealth at which the funded test is met today. */
  funded_requirement: number;
  /** Null when the requirement is zero. */
  funded_ratio: number | null;
  retirement_need: number;
  retirement_tax: number;
  future_income_value: number;
  horizon_years: number;
  /** Remaining years, per person. */
  life_expectancy: number[];
  expected_age_at_death: number[];
}
