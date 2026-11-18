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
// Status in this checkout: `summarize` and `Engine.trace` still throw "not implemented"
// in the wasm wrapper and there is no `compare` export yet, so nothing has been checked
// against real output. Treat a mismatch as a bug in this file.

/** A probability or mean with its 95% interval (sampling error only). */
export interface Estimate {
  value: number;
  std_error: number;
  lo: number;
  hi: number;
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

export type Status = 'working' | 'retired_surplus' | 'retired_on_plan' | 'retired_cutting' | 'depleted' | 'dead';

export type HistSeries = 'net_worth' | 'investable' | 'spending' | 'gross_income' | 'total_tax';

export type YearSeriesKind = 'balance' | 'liability' | 'income' | 'withdrawal' | 'tax' | 'spending' | 'saving' | 'transfer' | 'memo';

export type YearSeries =
  | 'balance_cash' | 'balance_taxable' | 'balance_traditional' | 'balance_roth' | 'balance_hsa' | 'property_value'
  | 'mortgage' | 'debt'
  | 'income_wages' | 'income_self_employment' | 'income_social_security' | 'income_pensions' | 'income_rental' | 'income_other'
  | 'withdraw_cash' | 'withdraw_taxable' | 'withdraw_traditional' | 'withdraw_roth' | 'withdraw_hsa'
  | 'tax_federal' | 'tax_state' | 'tax_payroll' | 'tax_penalties'
  | 'spend_essential' | 'spend_discretionary' | 'spend_housing' | 'spend_healthcare' | 'spend_long_term_care' | 'spend_children' | 'spend_debt' | 'spend_one_off'
  | 'save_cash' | 'save_taxable' | 'save_traditional' | 'save_roth' | 'save_hsa' | 'save_employer'
  | 'roth_conversion' | 'property_purchase' | 'property_sale'
  | 'spend_planned' | 'investment_income' | 'investment_return';

export type PathScalar =
  | 'fi_age' | 'retirement_age' | 'depletion_age' | 'death_age_0' | 'death_age_1' | 'investable_at_retirement'
  | 'early_retirement_return' | 'worst_drawdown' | 'months_unemployed' | 'long_term_care_cost' | 'inflation'
  | 'funded_ratio' | 'failure_cause' | 'success'
  | 'years_simulated' | 'terminal_net_worth' | 'peak_net_worth' | 'min_investable' | 'lifetime_tax' | 'lifetime_spending'
  | 'delivered_spending_ratio' | 'worst_spending_ratio' | 'years_retired' | 'years_below_90' | 'years_below_75' | 'years_depleted';

export type ScalarUnit = 'age' | 'dollars' | 'rate' | 'ratio' | 'months' | 'years' | 'index' | 'flag';

export type FailureCause = 'market_sequence' | 'job_loss' | 'health' | 'longevity' | 'overspending' | 'inflation' | 'other';

export type FailureFactor = 'poor_early_returns' | 'deep_drawdown' | 'job_loss' | 'long_term_care' | 'long_life' | 'high_inflation';

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
  median_terminal_net_worth: number;
  p10_terminal_net_worth: number;
  terminal_net_worth: Spread;
  investable_at_retirement: Spread;
  /** Mean over retired paths. */
  delivered_spending_ratio: Estimate;
  /** Share of retired path-years spent cutting or depleted. */
  years_cutting_share: number;
  median_lifetime_tax: number;
  lifetime_tax: Spread;
  lifetime_spending: Spread;
  funded_ratio: Spread | null;
}

export interface SpendingDelivery {
  retired_paths: number;
  paths_below_90: Estimate;
  paths_below_75: Estimate;
  years_below_90: Estimate;
  years_below_75: Estimate;
  years_cut: Spread;
  /** [0.5, 0.75, 0.9, 0.95, 0.99] */
  worst_cut_levels: number[];
  worst_cut: (number | null)[];
  by_year: { retired: (number | null)[]; below_90: (number | null)[]; below_75: (number | null)[] };
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
  label: 'p10' | 'median' | 'p90' | 'median_fi' | 'failing' | (string & {});
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
  mean_delta: Estimate;
  /** Of the per-path difference, over paths with a value in both runs. */
  quantiles: (number | null)[];
  dots: (number | null)[];
  b_higher: number;
  a_higher: number;
  equal: number;
  within_noise: boolean;
}

/**
 * Paired comparison of a scenario (B) against the baseline (A), path by path.
 * TODO(wasm): the wasm wrapper has no `compare` export yet, so `EnginePool.compare`
 * and `ViewProps.comparison[].paired` are null until it does.
 */
export interface Comparison {
  paths: number;
  unpaired_a: number;
  unpaired_b: number;
  outcomes: OutcomeComparison[];
  scalars: ScalarComparison[];
  headline: { metric: string; a: number | null; b: number | null; delta: number | null }[];
}

// ---- PathTrace (one life in full) ---------------------------------------------------

export type TraceEventKind =
  | 'financially_independent' | 'retired' | 'laid_off' | 'rehired' | 'career_break' | 'equity_event' | 'bought_home' | 'sold_home'
  | 'relocated' | 'windfall' | 'major_expense' | 'expense_shock' | 'roth_conversion' | 'spending_cut' | 'spending_raise' | 'depleted'
  | 'recovered' | 'claimed_social_security' | 'health_coverage' | 'long_term_care' | 'rmd_started' | 'debt_paid_off' | 'child'
  | 'partner_joined' | 'died' | 'market' | 'life_event' | 'other';

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
