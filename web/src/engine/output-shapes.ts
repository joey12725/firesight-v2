// Runtime shapes of the engine's JSON documents, mirroring `output-types.ts` field for
// field. `output-shapes.test.ts` validates real wasm output against them; the `Same`
// assertions at the bottom make the compiler fail when a shape and its TypeScript type
// disagree. Change both together.
import * as T from './output-types';
import { anything, arr, bool, nullable, num, obj, oneOf, record, str, type Infer, type Same, type Shape } from './shape';

const n = nullable(num);
const nums = arr(num);
const nnums = arr(n);

const estimate = obj({ value: num, std_error: num, lo: num, hi: num });
const spread = obj({ p10: n, p25: n, p50: n, p75: n, p90: n, p50_lo: n, p50_hi: n, count: num });
const status = oneOf(T.STATUSES);
const histSeries = oneOf(T.HIST_SERIES);
const scalar = oneOf(T.PATH_SCALARS);
const unit = oneOf(T.SCALAR_UNITS);

const ageCurve = obj({ ages: nums, counts: nums, cumulative: nums, lo: nums, hi: nums, never: num, dots: arr(n, 100) });

export const summaryShape = obj({
  version: num,
  paths: num,
  axis: obj({ start: str, start_age: num, first_year_months: num, years: nums, ages: nums, alive: nums }),
  headline: obj({
    success_rate: estimate,
    fi_rate: estimate,
    retired_rate: estimate,
    depletion_rate: estimate,
    median_fi_age: n,
    fi_age: spread,
    median_retirement_age: n,
    retirement_age: spread,
    depletion_age: spread,
    years_depleted: spread,
    median_terminal_net_worth: n,
    p10_terminal_net_worth: n,
    terminal_net_worth: spread,
    investable_at_retirement: spread,
    delivered_spending_ratio: obj({ value: n, std_error: n, lo: n, hi: n }),
    years_cutting_share: n,
    median_lifetime_tax: n,
    lifetime_tax: spread,
    lifetime_spending: spread,
    funded_ratio: nullable(spread),
  }),
  spending: obj({
    retired_paths: num,
    paths_below_90: obj({ value: n, std_error: n, lo: n, hi: n }),
    paths_below_75: obj({ value: n, std_error: n, lo: n, hi: n }),
    years_below_90: obj({ value: n, std_error: n, lo: n, hi: n }),
    years_below_75: obj({ value: n, std_error: n, lo: n, hi: n }),
    years_cut: spread,
    worst_cut_levels: nums,
    worst_cut: nnums,
    by_year: obj({ retired: nnums, below_90: nnums, below_75: nnums }),
  }),
  quantiles: arr(obj({ series: histSeries, levels: arr(num, 9), values: arr(nnums, 9) }), 5),
  density: arr(obj({ series: histSeries, edges: nums, starts: nums, counts: arr(nums), zeros: nums }), 5),
  status: obj({ statuses: arr(status, 6), shares: arr(nums, 6) }),
  survival: obj({ household: nums, people: arr(nums) }),
  composition: arr(obj({ series: oneOf(T.YEAR_SERIES), kind: oneOf(T.YEAR_SERIES_KINDS), values: nnums }), 43),
  fi: ageCurve,
  retirement: ageCurve,
  depletion: ageCurve,
  distributions: arr(
    obj({ scalar, unit, count: num, missing: num, mean: n, std_dev: n, min: n, max: n, quantiles: arr(n, 9), dots: arr(n), edges: nums, counts: nums }),
    27,
  ),
  scatter: obj({ paths: nums, columns: arr(obj({ scalar, values: nnums })) }),
  failure: obj({
    failed: num,
    never_retired: num,
    depleted: num,
    causes: arr(obj({ cause: oneOf(T.FAILURE_CAUSES), count: num, share_of_failed: n, share_of_all: num }), 7),
    factors: arr(obj({ factor: oneOf(T.FAILURE_FACTORS), scalar, above: bool, threshold: n, failed: n, succeeded: n })),
    contrasts: arr(
      obj({ scalar, failed_count: num, succeeded_count: num, failed_median: n, succeeded_median: n, failed_mean: n, succeeded_mean: n }),
    ),
  }),
  picks: arr(obj({ label: str, scalar, path_index: num, value: n })),
  samples: arr(obj({ path_index: num, net_worth: nums, investable: nums, spending: nums })),
});

const nullableEstimate = obj({ value: n, std_error: n, lo: n, hi: n });

export const comparisonShape = obj({
  paths: num,
  unpaired_a: num,
  unpaired_b: num,
  outcomes: arr(
    obj({
      outcome: oneOf(['success', 'fi', 'retired', 'depleted'] as const),
      a: estimate,
      b: estimate,
      both: num,
      a_only: num,
      b_only: num,
      neither: num,
      difference: estimate,
      z: n,
      within_noise: bool,
    }),
    4,
  ),
  scalars: arr(
    obj({
      scalar,
      unit,
      both: num,
      a_only: num,
      b_only: num,
      neither: num,
      mean_a: n,
      mean_b: n,
      mean_delta: nullableEstimate,
      quantiles: arr(n, 9),
      dots: arr(n),
      b_higher: n,
      a_higher: n,
      equal: n,
      within_noise: bool,
    }),
  ),
  headline: arr(obj({ metric: oneOf(T.COMPARISON_METRICS), a: n, b: n, delta: n })),
});

const traceColumn = obj({ key: str, unit: str, values: nnums });

export const pathTraceShape = obj({
  path_index: num,
  seed: num,
  start: str,
  months: num,
  statuses: arr(status, 6),
  status: nums,
  columns: arr(traceColumn),
  annual: obj({ years: nums, months: nums, status: nums, columns: arr(traceColumn) }),
  tax_returns: arr(
    obj({ year: num, price_level: num, result: record(anything), lines: arr(obj({ key: str, label: str, amount: num, note: nullable(str) })) }),
  ),
  events: arr(obj({ month: str, kind: oneOf(T.TRACE_EVENT_KINDS), label: str, amount: n })),
  scalars: arr(obj({ scalar, value: n }), 27),
});

const digestRate = obj({ p: num, lo: num, hi: num });
const digestSpread = obj({ p10: n, p50: n, p90: n });

export const digestShape = obj({
  notes: str,
  paths: num,
  start_year: num,
  end_year: num,
  start_age: num,
  success: digestRate,
  fi: digestRate,
  depletion: digestRate,
  fi_age: digestSpread,
  retirement_age: digestSpread,
  terminal_net_worth: digestSpread,
  lifetime_tax_p50: n,
  fi_by_age: obj({ ages: nums, p: nums }),
  depletion_by_age: obj({ ages: nums, p: nums }),
  by_age: obj({
    ages: nums,
    alive: nums,
    net_worth_p10: nnums,
    net_worth_p50: nnums,
    net_worth_p90: nnums,
    spending_p10: nnums,
    spending_p50: nnums,
    spending_p90: nnums,
  }),
  spending_delivery: obj({ delivered_ratio: n, paths_below_90: n, paths_below_75: n, years_below_90: n, worst_cut_p90: n }),
  failure_causes: arr(obj({ cause: oneOf(T.FAILURE_CAUSES), share: num })),
  trace_paths: obj({ p10: n, median: n, p90: n, failing: n }),
});

export const planPreviewShape = obj({
  annual_spending: num,
  annual_spending_retired: num,
  annual_income: num,
  savings_rate: num,
  annual_tax: num,
  investable: num,
  wealth: num,
  funded_requirement: num,
  funded_ratio: n,
  retirement_need: num,
  retirement_tax: num,
  future_income_value: num,
  horizon_years: num,
  life_expectancy: nums,
  expected_age_at_death: nums,
});

// Compile-time: each shape describes exactly its TypeScript type.
type Check<S extends Shape<unknown>, U> = Same<Infer<S>, U>;
export const SHAPES_MATCH_TYPES: [
  Check<typeof summaryShape, T.Summary>,
  Check<typeof comparisonShape, T.Comparison>,
  Check<typeof pathTraceShape, T.PathTrace>,
  Check<typeof digestShape, T.SummaryDigest>,
  Check<typeof planPreviewShape, T.PlanPreview>,
] = [true, true, true, true, true];
