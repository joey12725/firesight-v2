# Engine output reference

What leaves `firesight-core`: the `Summary` (one run), the `SummaryDigest` (its small
form for an LLM tool result), the `Comparison` (two runs on the same paths) and the
`PathTrace` (one life in full). The Rust types in `crates/firesight-core/src/stats/`
are the source of truth and derive JSON Schema; this file is the field-by-field guide
for people coding against the JSON.

## Conventions

* Money is **today's dollars** (dollars at `profile.start`), rounded to whole dollars.
  The only nominal amounts are the lines of a traced tax return.
* Ages are the **first person's**, in years. Rates and shares are fractions of 1.
* A number that does not exist is `null` (never `NaN`, never a sentinel): a quantile
  nobody reaches, a mean over zero paths, "never happened" for a path.
* Whole numbers are written without a fraction (`1234567`, not `1234567.0`).
* Per-year arrays are indexed by year index `y`; `axis.years[y]` is the calendar year
  and `axis.ages[y]` the first person's age that December.
* Everything "by year" (quantiles, density, composition) covers **households with
  someone alive that year**; `axis.alive[y]` is that sample size. Fade or cut charts
  where it gets small.
* Flows in a partial year (year 0 of a plan that does not start in January, and the
  year of the last death) are **annualised** in the yearly distributions and means.
  Trace columns and sample paths are as recorded, not annualised.
* Every probability is an `Estimate` with a 95% interval. Sampling error only: the
  spread between return models is larger and is reported by running each model.

### Shared shapes

`Estimate { value, std_error, lo, hi }`: `lo`/`hi` is a 95% interval. Wilson score for
proportions, normal for means and paired differences, linearised (clustered by path)
for shares of path-years.

`Spread { p10, p25, p50, p75, p90, p50_lo, p50_hi, count }`: quantiles of a per-path
quantity; `p50_lo`/`p50_hi` is a distribution-free 95% interval on the median; `count`
is the number of paths with a value. For FI age and retirement age the quantiles are
taken over **all** paths with "never" counted as later than any age (right-censored),
so `p50` is `null` when fewer than half the paths get there. v1 dropped those paths
and reported the median of the lucky ones.

## Identities (the enums the simulation fills)

Keys are stable: never renamed, only appended (with a bump of `Accumulator::VERSION`).

### `Status` (year-end state of a living household)

Use `Status::classify(retired, depleted, delivered, planned, investable,
investable_at_retirement)` so every caller applies the same thresholds.

| Key | Meaning |
|---|---|
| `working` | Not retired, not depleted |
| `retired_surplus` | Retired, on plan, real investable assets at least 2x their level at retirement |
| `retired_on_plan` | Retired, spending at least 95% of plan |
| `retired_cutting` | Retired, spending below 95% of plan |
| `depleted` | Liquid assets cannot cover essential spending (working or retired) |
| `dead` | Everyone has died. Never recorded; derived as paths with no record that year |

### `HistSeries` (full distribution kept per year; derived from `YearSeries`)

| Key | Definition |
|---|---|
| `net_worth` | accounts + `property_value` - `mortgage` - `debt`, year end |
| `investable` | cash + taxable + traditional + Roth + HSA, year end |
| `spending` | sum of the eight `spend_*` groups (not `spend_planned`); annualised |
| `gross_income` | sum of the six `income_*` series; annualised |
| `total_tax` | sum of the four `tax_*` series; annualised |

### `YearSeries` (43; summed per year, recorded monthly in a trace)

Balances and liabilities are period-end, everything else is the period's total. All
non-negative except `investment_return`. `kind` is in the output next to each series.

| Kind | Keys |
|---|---|
| `balance` | `balance_cash`, `balance_taxable`, `balance_traditional`, `balance_roth`, `balance_hsa`, `property_value` (market value of all properties) |
| `liability` | `mortgage` (principal on all properties), `debt` (non-mortgage) |
| `income` | `income_wages` (incl. bonus, commission, vested equity), `income_self_employment`, `income_social_security`, `income_pensions` (incl. annuities), `income_rental` (net operating income, before the rental's mortgage), `income_other` (incl. windfalls) |
| `withdrawal` | `withdraw_cash`, `withdraw_taxable`, `withdraw_traditional` (incl. RMDs, excl. conversions), `withdraw_roth`, `withdraw_hsa` |
| `tax` | `tax_federal` (income, capital gains, NIIT), `tax_state` (or foreign income tax), `tax_payroll` (FICA and SE tax), `tax_penalties`. Cash basis: withholding, estimates and the April settlement in the year paid |
| `spending` | `spend_essential` (incl. rent), `spend_discretionary`, `spend_housing` (owned property: P&I, property tax, insurance, upkeep, HOA, PMI), `spend_healthcare`, `spend_long_term_care`, `spend_children`, `spend_debt` (non-mortgage payments), `spend_one_off` (shocks, event expenses, closing costs) |
| `saving` | `save_cash`, `save_taxable`, `save_traditional`, `save_roth`, `save_hsa` (employee money), `save_employer` (401(k) match and employer HSA) |
| `transfer` | `roth_conversion`, `property_purchase` (cash into property at purchase), `property_sale` (net cash out of a sale) |
| `memo` | `spend_planned`, `investment_income` (interest and dividends inside accounts), `investment_return` (balance change not explained by flows; may be negative) |

`spend_planned` is what total spending would have been with no spending-rule cut and
no depletion shortfall, in the same scope as `spending`; it equals actual spending
while working. It is the denominator of every delivered-versus-planned number.

Cash-flow identity per period (`YearRecord::cash_flow_residual()` is zero):

```
income + withdrawals + property_sale + save_employer
    = taxes + spending + saving + property_purchase
```

### `PathScalar` (26; one `f32` per path, `null`/NaN = did not happen)

Supplied by the simulation in `PathEnd`:

| Key | Unit | Meaning |
|---|---|---|
| `fi_age` | age | First month the retirement trigger is satisfied |
| `retirement_age` | age | Month retirement begins |
| `depletion_age` | age | First month liquid assets cannot cover essentials |
| `death_age_0` | age | First person's age at death (or at the horizon) |
| `death_age_1` | age | Second person's own age at death; null if single |
| `investable_at_retirement` | dollars | Investable assets on the retirement date |
| `early_retirement_return` | rate | Annualised real portfolio return, first 10 retirement years |
| `worst_drawdown` | ratio | Worst peak-to-trough fall of the real investable balance from returns |
| `months_unemployed` | months | Involuntary unemployment, all people |
| `long_term_care_cost` | dollars | Total long-term care cost |
| `inflation` | rate | Annualised CPI inflation over the path |
| `funded_ratio` | ratio | Funded ratio at retirement; null if not computed |
| `failure_cause` | index | `FailureCause` index; null if the path succeeded |
| `success` | flag | 1 if retired by the plan's latest age and never depleted |

Derived by the recorder from the year records (anything the simulation puts there is
overwritten):

| Key | Unit | Meaning |
|---|---|---|
| `years_simulated` | years | Calendar years recorded |
| `terminal_net_worth` | dollars | Net worth in the last recorded year |
| `peak_net_worth` | dollars | Highest year-end net worth |
| `min_investable` | dollars | Lowest year-end investable balance while retired |
| `lifetime_tax` | dollars | Sum of `total_tax` |
| `lifetime_spending` | dollars | Sum of `spending` |
| `delivered_spending_ratio` | ratio | Retired-year spending / retired-year `spend_planned` |
| `worst_spending_ratio` | ratio | Lowest single retired year of the same ratio |
| `years_retired` | years | Records with `retired = true` |
| `years_below_90` | years | Retired years delivering under 90% of plan |
| `years_below_75` | years | Retired years delivering under 75% of plan |
| `years_depleted` | years | Years ending in `depleted` |

### `FailureCause`

`market_sequence`, `job_loss`, `health`, `longevity`, `overspending`, `inflation`,
`other`. One first cause per failed path, chosen by the simulation
(`PathEnd::set_outcome`).

### `TraceEventKind`

`financially_independent`, `retired`, `laid_off`, `rehired`, `career_break`,
`equity_event`, `bought_home`, `sold_home`, `relocated`, `windfall`, `major_expense`,
`expense_shock`, `roth_conversion`, `spending_cut`, `spending_raise`, `depleted`,
`recovered`, `claimed_social_security`, `health_coverage`, `long_term_care`,
`rmd_started`, `debt_paid_off`, `child`, `partner_joined`, `died`, `market`,
`life_event`, `other`.

## Recorder contract (what the simulation calls)

```rust
trait Recorder {
    fn begin_path(&mut self, path_index: u64);
    fn record_year(&mut self, year_index: usize, record: &YearRecord);
    fn end_path(&mut self, end: &PathEnd);
    fn wants_detail(&self) -> bool;                       // false for Accumulator
    fn record_month(&mut self, record: &MonthRecord);     // detail only
    fn record_event(&mut self, event: TraceEvent);        // detail only
    fn record_tax_return(&mut self, year: i32, detail: &tax::TaxDetail);  // detail only
}
```

Per path: `begin_path(i)`; one `record_year(y, ..)` per calendar year, in order from
`y = 0` (the calendar year of `profile.start`) with no gaps, the last one being the
year of the household's last death or the horizon; `end_path(..)`. Nothing after
everyone has died.

`YearRecord { status, retired, months, values: [f64; 43] }`: `values` indexed by
`YearSeries::index()` in today's dollars; `months` is how many months of the calendar
year the record covers (1..=12); `retired` is true once the retirement date has
passed; `status` from `Status::classify`. The histogram quantities are computed from
`values`, so there is nothing else to fill.

`PathEnd { scalars: [f32; 26] }`: defaults to all NaN; set the fourteen supplied
scalars with `set(scalar, value)` and `set_outcome(None | Some(cause))`.

`MonthRecord { month, price_level, status, retired, values: [f64; 43],
portfolio_return, asset_returns: [f64; 9] }`: month-end balances and the month's flows
in today's dollars; returns are nominal monthly rates. One call per simulated month,
in order.

Paths may be recorded in any order and any chunking; `Accumulator::merge` is exact, so
one chunk of 1,000 paths and ten merged chunks of 100 give identical bytes. Recording
a path index twice panics.

## Histogram grid

Symmetric about zero, 2,945 bins: an exact-zero bin; `$16` linear bins below `$1,024`
in magnitude; from `$1,024` to `2^32` (about `$4.29B`) each doubling is split into 64
equal bins (0.78% to 1.56% of the value wide); larger magnitudes clamp into the last
bin. Quantiles interpolate linearly inside a bin. Against exact quantiles of 100,000
lognormal samples the error is under 0.4% from the 0.1st to the 99.9th percentile.

## `Summary`

```
Summary {
  version: 1,
  paths,
  axis:       { start: "YYYY-MM", start_age, first_year_months, years[], ages[], alive[] },
  headline:   Headline,
  spending:   SpendingDelivery,
  quantiles:  [ { series, levels[9], values[level][year] } ]         // one per HistSeries
  density:    [ { series, edges[], starts[year], counts[year][], zeros[year] } ]
  status:     { statuses[6], shares[status][year] },
  survival:   { household[year], people[person][year] },
  composition:[ { series, kind, values[year] } ]                     // all 43 YearSeries
  fi, retirement, depletion: AgeCurve,
  distributions: [ ScalarDistribution ]                              // all 26 PathScalars
  scatter:    { paths[], columns: [ { scalar, values[] } ] },
  failure:    FailureAttribution,
  picks:      [ { label, scalar, path_index, value } ],
  samples:    [ { path_index, net_worth[], investable[], spending[] } ],
}
```

**`headline`**

| Field | Type | Meaning |
|---|---|---|
| `success_rate`, `fi_rate`, `retired_rate`, `depletion_rate` | Estimate | Shares of all paths |
| `median_fi_age`, `fi_age` | number/null, Spread | Censored over all paths |
| `median_retirement_age`, `retirement_age` | number/null, Spread | Censored over all paths |
| `depletion_age` | Spread | Among paths that deplete |
| `years_depleted` | Spread | Duration of shortfall among paths that deplete |
| `median_terminal_net_worth`, `p10_terminal_net_worth`, `terminal_net_worth` | number, number, Spread | The bequest |
| `investable_at_retirement` | Spread | Among paths that retire |
| `delivered_spending_ratio` | Estimate | Mean over retired paths |
| `years_cutting_share` | number | Retired path-years in `retired_cutting` or `depleted` |
| `median_lifetime_tax`, `lifetime_tax`, `lifetime_spending` | number, Spread, Spread | |
| `funded_ratio` | Spread or null | Present only if the simulation records it |

**`spending`** (planned versus delivered; "retired paths" have at least one retired
year): `retired_paths`; `paths_below_90`, `paths_below_75` (Estimate: share of retired
paths with at least one such year); `years_below_90`, `years_below_75` (Estimate:
share of retired path-years); `years_cut` (Spread: years below 90% among paths with a
cut); `worst_cut_levels` = `[0.5, 0.75, 0.9, 0.95, 0.99]` and `worst_cut[]` (the
single-year cut, as a fraction of plan, that this share of retired paths never
exceed); `by_year { retired[], below_90[], below_75[] }` (share of the living that are
retired; share of the retired below each line).

**`quantiles`**: levels are `[0.01, 0.05, 0.10, 0.25, 0.50, 0.75, 0.90, 0.95, 0.99]`.
`values[i][y]` is quantile `levels[i]` in year `y`, `null` when nobody is alive.

**`density`** (density fan): the yearly histogram on a coarse grid, 8 cells per
doubling of value (`$128` cells inside `$1,024`). Cell `k` of year `y` spans
`edges[starts[y] + k] .. edges[starts[y] + k + 1]` and holds `counts[y][k]` paths.
`edges` is shared by all years of the series and trimmed to the occupied range.
Paths at exactly zero are in `zeros[y]`, not in a cell. Divide by `axis.alive[y]` for
a share.

**`status`**: `shares[s][y]` is the share of **all** paths in `statuses[s]`; the six
rows sum to 1 in every year.

**`survival`**: share of paths with the household (someone) or each person alive at
some point in the year.

**`composition`**: mean per living household. Balances and liabilities: year-end
level. Flows: annualised (sum divided by household-years recorded).

**`AgeCurve`** (`fi`, `retirement`, `depletion`): `ages[]` (whole ages; entry `i`
covers `[ages[i], ages[i] + 1)`), `counts[]` (the histogram), `cumulative[]` with
Wilson `lo[]`/`hi[]` (share of **all** paths by the end of that age), `never` (share
on which it never happens), `dots[100]` (ages at quantiles `(i + 0.5) / 100` over all
paths, `null` for "never": the quantile dot plot).

**`ScalarDistribution`**: `scalar`, `unit`, `count`, `missing`, `mean`, `std_dev`,
`min`, `max`, `quantiles[9]`, `dots[100]`, and a 40-bin histogram `edges[41]` /
`counts[40]` spanning the 0.5th to 99.5th percentile (outliers in the end bins; a
single bin when the values do not spread; empty arrays when there are no values).

**`scatter`**: up to 5,000 evenly spaced paths. Columns: `early_retirement_return`,
`terminal_net_worth`, `fi_age`, `retirement_age`, `depletion_age`, `death_age_0`,
`investable_at_retirement`, `success`. `paths[i]` is the path index to trace.

**`failure`**: `failed`, `never_retired`, `depleted` (counts); `causes[]` (every
`FailureCause`: `count`, `share_of_failed`, `share_of_all`); `factors[]` (overlapping
tags: `factor`, `scalar`, `above`, `threshold`, `failed`, `succeeded`: the share of
failed and of successful paths whose scalar lies beyond the 75th, or below the 25th,
percentile of all paths; factors are `poor_early_returns`, `deep_drawdown`, `job_loss`,
`long_term_care`, `long_life`, `high_inflation`); `contrasts[]` (for every scalar:
`failed_count`, `succeeded_count`, `failed_median`, `succeeded_median`, `failed_mean`,
`succeeded_mean`).

**`picks`**: `p10`, `median`, `p90` (path at that rank of terminal net worth),
`median_fi` (median FI age among paths that reach it), `failing` (median depletion age
among failed paths, else the first failed path). `Summary::path_at_quantile(acc,
scalar, q)` finds the path at any quantile of any scalar.

**`samples`**: the first 96 paths' annual net worth, investable assets and spending
(whole dollars; shorter than the horizon when the household dies sooner).

## `SummaryDigest` (`Summary::digest()`, about 1.7 KB)

```
{ notes, paths, start_year, end_year, start_age,
  success: {p, lo, hi}, fi: {p, lo, hi}, depletion: {p, lo, hi},
  fi_age: {p10, p50, p90}, retirement_age: {p10, p50, p90},
  terminal_net_worth: {p10, p50, p90}, lifetime_tax_p50,
  fi_by_age: {ages[], p[]}, depletion_by_age: {ages[], p[]},
  by_age: { ages[6], alive[], net_worth_p10[], net_worth_p50[], net_worth_p90[],
            spending_p10[], spending_p50[], spending_p90[] },
  spending_delivery: { delivered_ratio, paths_below_90, paths_below_75,
                       years_below_90, worst_cut_p90 },
  failure_causes: [ {cause, share} ]   // top 3, share of failed paths
  trace_paths: { p10, median, p90, failing } }
```

Dollars are rounded to `$1,000`, probabilities to 3 decimals. `notes` states the
units and the definition of success so the numbers cannot be misread out of context.

## `Comparison` (`compare(a, b)`; B minus A)

Both runs must use the same seed; paths are matched by index and the rest ignored.

```
{ paths, unpaired_a, unpaired_b,
  outcomes: [ { outcome: "success" | "fi" | "retired" | "depleted",
                a: Estimate, b: Estimate,
                both, a_only, b_only, neither,
                difference: Estimate, z, within_noise } ],
  scalars:  [ { scalar, unit, both, a_only, b_only, neither,
                mean_a, mean_b, mean_delta: Estimate,
                quantiles[9], dots[100], b_higher, a_higher, equal, within_noise } ],
  headline: [ { metric, a, b, delta } ] }
```

For `success`, `b_only` is "rescued by the change", `a_only` "harmed", the rest
unchanged. `difference` has the paired standard error `sqrt((d - delta^2) / n)` (`d` =
share of paths that flip, `delta` = net change); `z` is the McNemar statistic.
`scalars` covers every scalar except the index and the flag; statistics are over paths
with a value in both runs, with the one-sided counts alongside. `within_noise` is true
when the 95% interval contains zero. `headline` metrics: `success_rate`, `fi_rate`,
`depletion_rate`, `median_fi_age`, `median_retirement_age`,
`median_terminal_net_worth`, `p10_terminal_net_worth`, `median_lifetime_tax`,
`median_lifetime_spending`, `delivered_spending_ratio`.

## `PathTrace` (one life)

```
{ path_index, seed, start: "YYYY-MM", months,
  statuses[6], status[month],                 // index into statuses
  columns: [ { key, unit, values[month] } ],
  annual:  { years[], months[], status[], columns: [ { key, unit, values[year] } ] },
  tax_returns: [ { year, price_level, result: {..TaxResult}, lines: [ {key, label, amount, note} ] } ],
  events:  [ { month: "YYYY-MM", kind, label, amount } ],
  scalars: [ { scalar, value } ] }            // all 26, same as the aggregate row
```

Monthly column keys: `net_worth`, `investable`, every `YearSeries` key that is not
zero throughout, `price_level` (unit `index`), `portfolio_return` and
`return_<asset class>` (unit `rate`, nominal, monthly). Dollar columns are whole
today's dollars; multiply by `price_level` for nominal. A missing series is zero.
`annual` holds exactly the records the aggregate run saw, plus `spending`,
`gross_income` and `total_tax`. Tax-return amounts are nominal dollars of the tax
year; divide by that return's `price_level` for today's dollars.

## Sizes and timings

Measured natively on one Apple M-series core, 60-year horizon, synthetic lives.

| Item | Result |
|---|---|
| `record_year` | about 21 ns |
| 500-path chunk, serialised | 111 KB (161 KB for the first chunk, which carries the 96 sample paths); 52 KB of it is the scalar table |
| 500-path chunk: encode / decode / merge | 0.8 ms / 0.4 ms / 0.2 ms |
| 100,000-path accumulator, serialised | 10.6 MB: histograms 0.16 MB, scalars 10.4 MB (26 x 4 bytes x paths) |
| `Summary::from_accumulator`, 100,000 paths | about 51 ms |
| `Summary` JSON, 100,000 paths | 530 KB |
| `SummaryDigest` JSON | about 1.7 KB |
| `PathTrace` JSON, 60 years, 30 active series | about 180 KB |
