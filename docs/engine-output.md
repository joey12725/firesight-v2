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

Keys are stable and never renamed. The enums the accumulator stores by position
(`Status`, `YearSeries`, `PathScalar`, the histogram series) are only appended to, with
a bump of `Accumulator::VERSION`. `TraceEventKind` is written by key, not by position,
so a new kind may sit next to its relatives (`tax_loss_harvest`,
`qualified_charitable_distribution` and `policy_change` come before `market`).

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
| `gross_income` | sum of the seven `income_*` series (`income_equity` included); annualised |
| `total_tax` | sum of the four `tax_*` series; annualised |
| `spend_planned` | the `spend_planned` series (planned spending, same scope as `spending`); annualised. Sixth entry, appended |

### `YearSeries` (46; summed per year, recorded monthly in a trace)

Balances and liabilities are period-end, everything else is the period's total. All
non-negative except `investment_return`. `kind` is in the output next to each series.

| Kind | Keys |
|---|---|
| `balance` | `balance_cash`, `balance_taxable`, `balance_traditional`, `balance_roth`, `balance_hsa`, `property_value` (market value of all properties) |
| `liability` | `mortgage` (principal on all properties), `debt` (non-mortgage) |
| `income` | `income_wages` (incl. bonus, commission, vested equity from a job's own grants), `income_self_employment`, `income_social_security`, `income_pensions` (incl. annuities), `income_rental` (net operating income, before the rental's mortgage), `income_other` (incl. windfalls), `income_equity` (cash from `private_equity` before tax: sales of shares in tender offers, acquisitions and after an IPO, cash-outs of options net of the strike and of RSUs, and the value of shares moved to the taxable account when a lockup ends). Index 43, appended |
| `withdrawal` | `withdraw_cash`, `withdraw_taxable`, `withdraw_traditional` (incl. RMDs, excl. conversions), `withdraw_roth`, `withdraw_hsa` |
| `tax` | `tax_federal` (income, capital gains, NIIT), `tax_state` (or foreign income tax), `tax_payroll` (FICA and SE tax), `tax_penalties`. Cash basis: withholding, estimates and the April settlement in the year paid |
| `spending` | `spend_essential` (incl. rent), `spend_discretionary`, `spend_housing` (owned property: P&I, property tax, insurance, upkeep, HOA, PMI), `spend_healthcare`, `spend_long_term_care`, `spend_children`, `spend_debt` (non-mortgage payments), `spend_one_off` (shocks, event expenses, closing costs) |
| `saving` | `save_cash`, `save_taxable`, `save_traditional`, `save_roth`, `save_hsa` (employee money), `save_employer` (401(k) match and employer HSA) |
| `transfer` | `roth_conversion`, `property_purchase` (cash into property at purchase), `property_sale` (net cash out of a sale), `equity_exercise` (cash paid to exercise options: the strike. The shares bought are outside net worth until sold). Index 44, appended |
| `memo` | `spend_planned`, `investment_income` (interest and dividends inside accounts), `investment_return` (balance change not explained by flows; may be negative), `equity_paper_value` (period-end value of vested private-company holdings at the company's current share price, before tax, options at price less strike. **Not part of net worth** or of any balance; a stock, so its yearly mean is not annualised). Index 45, appended |

`spend_planned` is what total spending would have been with no spending-rule cut and
no depletion shortfall, in the same scope as `spending`; it equals actual spending
while working. It is the denominator of every delivered-versus-planned number.

Cash-flow identity per period (`YearRecord::cash_flow_residual()` is zero):

```
income + withdrawals + property_sale + save_employer
    = taxes + spending + saving + property_purchase + equity_exercise
```

`income` here includes `income_equity`. Shares kept as single stock when a lockup ends
are booked as `income_equity` and `save_taxable` of the same amount in that month, so
they enter net worth through the taxable balance like any other saving.

### `PathScalar` (43; one `f32` per path, `null`/NaN = did not happen)

Supplied by the simulation in `PathEnd`:

| Key | Unit | Meaning |
|---|---|---|
| `fi_age` | age | First month the funded test is met (the trigger's own rule for `when_funded` and `fixed_number`; the default rule, 4% of investable assets counting future income, for `at_age`, `at_date` and `already_retired`, evaluated until the retirement date) |
| `coast_fi_age` | age | First month today's wealth, with no further saving, grown at the plan's expected real return to the planned (or latest) retirement age, would meet the funded test then. Index 26, after the derived scalars |
| `retirement_age` | age | Month retirement begins |
| `depletion_age` | age | First month liquid assets cannot cover essentials |
| `death_age_0` | age | First person's age at death (or at the end of the path if alive then) |
| `death_age_1` | age | Second person's own age at death; null if single |
| `investable_at_retirement` | dollars | Investable assets on the retirement date |
| `early_retirement_return` | rate | Annualised real portfolio return, first 10 retirement years |
| `worst_drawdown` | ratio | Worst peak-to-trough fall of the real portfolio return index (returns only, not contributions or withdrawals) |
| `months_unemployed` | months | Involuntary unemployment, all people |
| `long_term_care_cost` | dollars | Total long-term care cost |
| `inflation` | rate | Annualised CPI inflation over the path |
| `funded_ratio` | ratio | Wealth counted by the funded test divided by its requirement, on the retirement date; null if never retired or the requirement is zero |
| `failure_cause` | index | `FailureCause` index; null if the path succeeded |
| `success` | flag | 1 if never depleted, the bequest target (if any) is met at the end, and the household retired, or died before the plan's latest retirement age (`max_age`; the trigger age; 70 for `fixed_number`) |
| `initial_withdrawal_rate` | rate | The first retired year's draw on the portfolio (budget less after-tax income) over investable assets on the retirement date: the rate the household actually starts on. Index 28 |
| `home_sale_age` | age | First person's age when property was first sold because savings had run out (`retirement.home_equity: last_resort`). Index 29 |
| `social_security_claim_age_0`, `_1` | age | Each person's **own** age when their benefit is first paid (the profile's claim age if it was already in payment at the start). Indices 31, 32 |
| `person_retirement_age_0`, `_1` | age | Each person's **own** age when they stop working (person 1: the retirement date plus `partner_offset_years`). Indices 33, 34 |
| `mortgage_payoff_age` | age | First person's age in the month the household's last mortgage is paid off (by schedule or sale) with none taken out afterwards; null if it never had one or still owes at the end. Index 35 |
| `medicaid_age` | age | First person's age in the first month Medicaid paid for someone's long-term care after savings were spent down (`spending.healthcare.medicaid`); null if it never did, and always null with the option off. Index 36 |
| `equity_outcome` | index | What happened to the household's main private company, as an `EquityOutcome` index; null if the plan has no `private_equity`. With several companies it is the one with the largest position at the start (at the latest preferred price, vested and unvested, counted or not). Index 37 |
| `equity_event_age` | age | First person's age when that company failed, was acquired or went public; null if none of those happened within the life (a write-off after thirty years is not one) or there is no company. Index 38 |
| `equity_gross_proceeds` | dollars | Cash from private-company equity over the life, before tax: the sum of `income_equity`, every counted company. Shares kept as single stock after a lockup are in it at their value on the day they move. 0 when nothing was received; null if the plan has no private company. Index 39 |
| `equity_exercise_cost` | dollars | Cash paid to exercise options over the life: the sum of `equity_exercise`. Index 40 |
| `equity_tender_proceeds` | dollars | The part of the gross proceeds that came from tender offers. Index 41 |
| `equity_net_proceeds` | dollars | Gross proceeds less exercise cost less the tax the equity caused: each year's return with the equity amounts less the same return without them (AMT and the later use of its credit included), in today's dollars of the month the year is closed. For shares kept as single stock, which are not taxed when they move, the tax counted is an estimate: what their gain would add to that year's return if sold as a long-term gain. The tax actually paid when the single stock is sold later is not attributed to the equity. Negative when more was paid in than came back; slightly positive with nothing received when shares bought earlier become a deductible loss. Index 42 |

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
| `delivered_spending_ratio` | ratio | Share of planned retirement spending delivered: sum over retired years of `min(spending, spend_planned)` / sum of `spend_planned`. **Never above 1** (changed: it used to be the plain ratio of sums and could read 128%) |
| `extra_spending_ratio` | ratio | Retirement spending above the plan: sum of `max(spending - spend_planned, 0)` / sum of `spend_planned`. Index 27 |
| `plan_delivered_share` | ratio | Mean over retired years of `min(spending / spend_planned, 1)`: every year weighs the same. Index 30 |
| `worst_spending_ratio` | ratio | Lowest single retired year of spending / `spend_planned` (not capped) |
| `years_retired` | years | Records with `retired = true` |
| `years_below_90` | years | Retired years delivering under 90% of plan |
| `years_below_75` | years | Retired years delivering under 75% of plan |
| `years_depleted` | years | Years ending in `depleted` |

### `FailureCause`

`market_sequence`, `job_loss`, `health`, `longevity`, `overspending`, `inflation`,
`other`. One first cause per failed path, chosen by the simulation
(`PathEnd::set_outcome`) as the first rule that holds:

1. `health`: long-term care paid before the money ran out was at least a quarter of
   the savings held at retirement (two years of the budget if never retired);
2. the money ran out **before retirement**: `job_loss` if at least 6 months of
   involuntary unemployment came before that, otherwise `overspending` (never a
   market or longevity cause); otherwise `job_loss` if at least 18 months of
   unemployment came before the money ran out;
3. `inflation`: inflation over the path averaged 4.5% a year or more;
4. `market_sequence`: the real portfolio return over the first ten retirement years
   was more than two points a year below the plan's expected real return;
5. `longevity`: the money ran out at 90 or later;
6. `overspending`: the first-year withdrawal rate was above 5%, or the spending rule
   had raised spending above the plan;
7. `other`.

### `EquityOutcome`

`failed` (shares and options became worthless), `still_private` (no sale, listing or
failure within the life: the event was dated after the life ended, or the company was
written off after thirty years), `acquired` (for cash, including a sale that pays
common stock nothing), `ipo`. In this order everywhere an array is "by outcome". The
outcome is the company's fate, drawn at path start on its own random stream: it is the
same on a path whatever the household decides, and the same when the company is
counted at zero (`included: false`).

### `TraceEventKind`

`financially_independent`, `retired`, `laid_off`, `rehired`, `career_break`,
`equity_event`, `bought_home`, `sold_home`, `relocated`, `windfall`, `major_expense`,
`expense_shock`, `roth_conversion`, `spending_cut`, `spending_raise`, `depleted`,
`recovered`, `claimed_social_security`, `health_coverage`, `long_term_care`,
`rmd_started`, `debt_paid_off`, `child`, `partner_joined`, `died`,
`tax_loss_harvest`, `qualified_charitable_distribution`, `policy_change`, `market`,
`life_event`, `other`, `health_state`, `bereavement`, `medicaid`, `ltc_insurance`
(in index order; the last four were appended after `other`), and seven for
`private_equity` (appended after those): `options_exercised`
("Exercised 50,000 Northwind Robotics options for $75,000"; amount: the cost),
`tender_offer` ("... tender offer at $17.30 a share: sold 6,000 for $98,000 in today's
dollars", or "sold nothing"; amount: the cash), `company_acquired` ("... acquired at
$21.40 a share: $1,200,000 paid out in today's dollars", or "common stock gets
nothing"), `company_ipo` ("... goes public
at $30.10 a share; shares locked up for 6 months"; no amount), `company_failed` ("...
shuts down: its shares and options are worth nothing", or "... is written off after 30
years without a sale or listing", with "($15,000 paid for its shares is lost)" when
shares were held; amount: the cost lost), `equity_forfeited` (amount: the paper value
given up; four wordings: "Left ...: 30,000 unvested forfeited", with ", and 10,000
vested RSUs that required staying until the IPO or sale" when the plan says vested
RSUs are lost on leaving a company that is still private, or "Left ...: 10,000 vested
RSUs forfeited (they required staying until the IPO or sale)" when nothing was
unvested; the lead is the company's name alone, without "Left", when the owner died
or the job never started; "8,000 vested ... options lapsed unexercised when the
exercise window closed"; and "8,000 vested and 2,000 unvested ... options lapsed
unexercised at their ten-year term", either part alone when the other is nil) and
`lockup_sale` ("... lockup ends: sold 2,000 shares at $50.00 for $80,000 in today's
dollars"; shares sold or kept when a lockup ends, and each scheduled sale after it).
Share prices in labels are nominal dollars; amounts are today's dollars like every
other event, and a label that shows both says so. A re-hire after leaving a job that
grants private-company equity reads "Back at work at a new employer (was <job name>)":
the link to the company is cut for good and the income stands for the next job. Monthly vest payouts after an acquisition or IPO are
not logged. `equity_event` remains the kind for a job's own grants (`Job.equity`),
now also logged when a private grant pays out or its company fails.

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

`YearRecord { status, retired, months, values: [f64; 46] }`: `values` indexed by
`YearSeries::index()` in today's dollars; `months` is how many months of the calendar
year the record covers (1..=12); `retired` is true once the retirement date has
passed; `status` from `Status::classify`. The histogram quantities are computed from
`values`, so there is nothing else to fill.

`PathEnd { scalars: [f32; 43] }`: defaults to all NaN; set the supplied
scalars with `set(scalar, value)`, `set_outcome(None | Some(cause))` and, for a plan
with private-company equity, `set_equity_outcome(None | Some(outcome))` (`None` is
`still_private`; leave it unset when there is no company).

`MonthRecord { month, price_level, status, retired, values: [f64; 46],
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
  quantiles:  [ { series, levels[9], values[level][year] } ]         // one per HistSeries (6)
  density:    [ { series, edges[], starts[year], counts[year][], zeros[year] } ]
  status:     { statuses[6], shares[status][year] },
  survival:   { household[year], people[person][year] },
  composition:[ { series, kind, values[year] } ]                     // all 46 YearSeries
  fi, retirement, depletion: AgeCurve,
  distributions: [ ScalarDistribution ]                              // all 43 PathScalars
  scatter:    { paths[], columns: [ { scalar, values[] } ] },
  failure:    FailureAttribution,
  picks:      [ { label, scalar, path_index, value } ],
  samples:    [ { path_index, net_worth[], investable[], spending[] } ],
  pick_paths: [ { path_index, net_worth[], investable[], spending[] } ],   // added
  plan_moments: PlanMoments,                                               // added
  equity:     EquitySummary,                  // only for a plan with private_equity; the key is absent otherwise
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
| `delivered_spending_ratio` | Estimate | Mean over retired paths of the capped ratio of sums (never above 1) |
| `plan_delivered_share` | Estimate | Mean over retired paths of the per-year capped share (`plan_delivered_share` scalar). Added |
| `extra_spending_ratio` | Estimate | Mean over retired paths of spending above plan as a fraction of plan. Added |
| `initial_withdrawal_rate` | Spread | First-year draw / investable at retirement, among paths that retire. Added |
| `home_sale_rate` | Estimate | Share of all paths on which a home was sold as a last resort. These paths are **not** counted as depleted. Added |
| `medicaid_rate` | Estimate | Share of all paths on which Medicaid paid for long-term care after savings were spent down (`spending.healthcare.medicaid`; 0 with the option off). A month in which Medicaid pays for a nursing-home resident with no partner at home is not counted as depleted. Added with the optional features |
| `years_cutting_share` | number | Retired path-years in `retired_cutting` or `depleted` |
| `median_lifetime_tax`, `lifetime_tax`, `lifetime_spending` | number, Spread, Spread | |
| `funded_ratio` | Spread or null | Present only if the simulation records it |

**`spending`** (planned versus delivered; "retired paths" have at least one retired
year): `retired_paths`; `paths_below_90`, `paths_below_75` (Estimate: share of retired
paths with at least one such year); `years_below_90`, `years_below_75` (Estimate:
share of retired path-years); `years_cut` (Spread: years below 90% among paths with a
cut); `worst_cut_levels` = `[0.5, 0.75, 0.9, 0.95, 0.99]` and `worst_cut[]` (the
single-year cut, as a fraction of plan, that this share of retired paths never
exceed); `by_year { retired[], below_90[], below_75[], depleted[], depleted_shortfall[] }`
(share of the living that are retired; share of the retired below each line;
`depleted[y]`: share of **living** households whose year ended `depleted`;
`depleted_shortfall[y]`: among those, the mean of planned less delivered spending
for the year in whole today's dollars, annualised, `null` when nobody is depleted
that year. The last two are added).

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

With private-company equity there are three more picks: `equity_failed` (the path at
the median terminal net worth among paths on which the company failed) and
`equity_paid_median`, `equity_paid_p90` (the paths at those quantiles of
`equity_net_proceeds` among paths with gross proceeds above 0). A pick is left out when
its class is empty. `failure.contrasts` covers every scalar except `success` and the two
index scalars (`failure_cause`, `equity_outcome`).

**`samples`**: the first 96 paths' annual net worth, investable assets and spending
(whole dollars; shorter than the horizon when the household dies sooner).

**`pick_paths`** (added): the same three annual series for each distinct
`picks[].path_index`, in the order of `picks` (so the named unlucky, middling and
lucky lives can be drawn without tracing them). Filled by `Plan::summarize` /
`Plan::summarize_seeded(acc, seed)` and the wasm `summarize`, which simulate those
few paths again with the run's seed; a path that does not reproduce the
accumulator's record (wrong seed) is left out, and `Summary::from_accumulator` alone
leaves the list empty.

**`plan_moments`** (added): `{ social_security_claim_age[person],
social_security_claim_share[person], person_retirement_age[person],
person_retirement_share[person], mortgage_payoff_age, mortgage_payoff_year,
mortgage_payoff_share }`. Ages are medians over the paths on which the thing
happens (each person's own age for the first four; the first person's for the
mortgage), the `*_share` fields are the share of all paths on which it happens,
`mortgage_payoff_year` is the same median as a calendar year. `null` where it never
happens.

**`equity`** (`EquitySummary`; present only when some path has an `equity_outcome`).
Money in today's dollars. Everything is computed from the six equity scalars and the
existing ones, so it is exact under any chunking and merge order.

```
equity: {
  company,                                  // name of the company the outcomes describe (its id if unnamed)
  paths,                                    // paths with an outcome: all of them
  outcomes: [ { outcome, count, share: Estimate } ],    // 4, in EquityOutcome order; Wilson interval
  nothing_rate:   Estimate,                 // gross proceeds exactly 0
  loss_rate:      Estimate,                 // net proceeds below 0: more went in (exercise cost and tax) than came back
  exercised_rate: Estimate,                 // options were exercised for cash
  tender_rate:    Estimate,                 // proceeds from a tender offer
  gross_proceeds: Spread,                   // all paths, zeros included
  net_proceeds:   Spread,                   // all paths
  net_proceeds_mean: Estimate,              // carried by the few best outcomes: read it with its interval
  net_proceeds_mean_half_from,              // fewest paths that carry half of all positive net proceeds; under about 100 the interval is too narrow and sits low
  net_proceeds_if_paid: Spread,             // among paths with gross proceeds above 0
  exercise_cost:   Spread,                  // among paths that paid any
  tender_proceeds: Spread,                  // among paths that had any
  liquidity_by_age: AgeCurve,               // acquired or public by each age, over all paths
  failed_by_age:    AgeCurve,               // failed by each age, over all paths
  by_outcome: [ { outcome, paths, enough,   // 4, in EquityOutcome order
                  success_rate: Estimate, fi_rate: Estimate,
                  fi_age: Spread, retirement_age: Spread,       // censored within the class
                  terminal_net_worth: Spread, net_proceeds: Spread } ] }
```

`by_outcome[i]` is the plan's headline among the paths with that outcome. `enough` is
false when the class has fewer than 30 paths (`MIN_PATHS_PER_OUTCOME`): its four
spreads are then all null with `count` 0, and its two rates are still given but have
very wide intervals (null when the class is empty). The two age curves end at the
class's share of paths, not at 1: their `never` is everyone else. The outcome shares
are a property of the odds entered, not a result: they differ from the ten-year table
because a company still private after ten years faces the same odds twice more, and
because some lives end first.

## `SummaryDigest` (`Summary::digest()`, about 1.7 KB, about 2.2 KB with equity)

```
{ notes, paths, start_year, end_year, start_age,
  success: {p, lo, hi}, fi: {p, lo, hi}, depletion: {p, lo, hi},
  fi_age: {p10, p50, p90}, retirement_age: {p10, p50, p90},
  terminal_net_worth: {p10, p50, p90}, lifetime_tax_p50,
  fi_by_age: {ages[], p[]}, depletion_by_age: {ages[], p[]},
  by_age: { ages[6], alive[], net_worth_p10[], net_worth_p50[], net_worth_p90[],
            spending_p10[], spending_p50[], spending_p90[] },
  spending_delivery: { delivered_ratio, extra_ratio, paths_below_90, paths_below_75,
                       years_below_90, worst_cut_p90 },   // extra_ratio added
  failure_causes: [ {cause, share} ]   // top 3, share of failed paths
  trace_paths: { p10, median, p90, failing },
  equity: { company,                    // only for a plan with private_equity; absent otherwise
            failed, still_private, acquired, ipo,       // shares of lives
            nothing: {p, lo, hi},       // the equity pays nothing at all
            loss,                       // more went in than came back
            net_proceeds: {p10, p50, p90},              // after exercise cost and tax, all lives
            net_proceeds_mean, net_proceeds_mean_se,
            success_by_outcome[4] } }   // failed, still_private, acquired, ipo; null under 30 lives
```

Dollars are rounded to `$1,000`, probabilities to 3 decimals. `notes` states the
units and the definition of success so the numbers cannot be misread out of context.
`notes` is the same text with and without `equity`, and a digest without equity is
byte for byte what it was before the block existed.

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
  quantile_levels: [0.01, 0.05, 0.10, 0.25, 0.50, 0.75, 0.90, 0.95, 0.99],   // added: the levels of scalars[].quantiles
  headline: [ { metric, a, b, delta } ],
  by_equity_outcome: [ { outcome, paths,          // 4, in EquityOutcome order; absent when run A has no private_equity
                         success: PairedOutcome,              // the `outcomes[]` shape, among these pairs
                         terminal_net_worth: PairedScalar } ] }   // the `scalars[]` shape, B minus A
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

`by_equity_outcome` splits the pairs by the equity outcome of the path **in run A** and
repeats the success and terminal-net-worth comparison within each class; the class
sizes sum to `paths`. The outcome is a fate, identical in A and B unless the scenario
edits the outcome table, so this answers "what does this decision do in the lives where
the company fails, and in those where it lists". A class with few pairs has a wide
interval; check `paths` before quoting it. The five equity dollar and age scalars are
in `scalars[]` like any other; `equity_outcome`, an index, is not.

## `PathTrace` (one life)

```
{ path_index, seed, start: "YYYY-MM", months,
  statuses[6], status[month],                 // index into statuses
  columns: [ { key, unit, values[month] } ],
  annual:  { years[], months[], status[], columns: [ { key, unit, values[year] } ] },
  tax_returns: [ { year, price_level, result: {..TaxResult}, lines: [ {key, label, amount, note} ] } ],
  events:  [ { month: "YYYY-MM", kind, label, amount } ],
  scalars: [ { scalar, value } ] }            // all 43, same as the aggregate row
```

Monthly column keys: `net_worth`, `investable`, every `YearSeries` key that is not
zero throughout, `price_level` (unit `index`), `portfolio_return` and
`return_<asset class>` (unit `rate`, nominal, monthly). Dollar columns are whole
today's dollars; multiply by `price_level` for nominal. A missing series is zero.
`annual` holds exactly the records the aggregate run saw, plus `spending`,
`gross_income` and `total_tax`. Tax-return amounts are nominal dollars of the tax
year; divide by that return's `price_level` for today's dollars.

## What the simulation records, precisely

* **Deflators**: flows of a month are divided by the price level at the start of the
  month, balances by the level at its end. `MonthRecord.price_level` (and the trace
  column) is the end-of-month level.
* **Tax series are cash basis**: FICA withheld from each pay cheque, income tax
  prepaid through the year (a withholding rate on wage-like income plus a level
  instalment, both set from the January estimate), and the balance of the actual
  return in April of the next year. A refund first nets against tax paid that month
  in the same category; any remainder is `income_other`. The last return of a life is
  settled in its last month.
* **The first calendar year** is usually partial. Its return is computed on the
  simulated months scaled to a full year and the tax scaled back; the traced return
  for that year shows the annualised figures.
* **Overdraft**: when nothing is left to sell and costs that cannot be cut (housing,
  debt service, tax) exceed income, the unpaid amount is carried as an overdraft and
  reported under `debt`; `balance_cash` is never negative.
* **Advance premium credit**: `spend_healthcare` is net of the ACA credit advanced
  during the year (estimated each January); the difference from the credit actually
  earned is settled with the return, in `tax_federal`.
* **A partner joining** brings their accounts: the amount is `income_other` and the
  matching `save_*` series that month.
* **Depletion status** of a year is `depleted` if any month of it cut essential
  spending or grew the overdraft.
* **Trace events** also include `life_event` entries for refinancing, postponed or
  abandoned purchases, and income, spending and contribution changes. A last-resort
  property sale is a `sold_home` event labelled "Sold <name> because savings ran out".
* **Traced tax returns** have two more optional lines: `adjustments` (deductible IRA
  and HSA contributions made outside payroll, negative) and `passive_loss_released`;
  `other_income` no longer nets them. `local_income_tax` notes name the base used
  (state adjusted income, wages and business profit, or the city's schedule).
* **Private-company equity** (`private_equity`). Paper value, vested or not, locked up
  or scheduled for sale, is in no balance: not in `net_worth`, `investable`,
  `terminal_net_worth`, the bequest test, the funded test or the retirement trigger. It
  is reported as the `equity_paper_value` memo series (vested holdings only) and
  nowhere else. Cash received counts everywhere from the month it arrives, as
  `income_equity` and in the `gross_income` histogram of that year; the funded test and
  the trigger count it less the tax still owed on it (the event-month hold, the
  instalments still to pay, and the April balance of a year with an equity event).
  Shares kept as single stock after a lockup count from the month they move to the
  taxable account. Pay-type equity income has FICA (with the 0.9% Additional Medicare
  tax on the part over $200,000 of the person's wages for the year) and federal
  supplemental withholding taken in the event month (`tax_payroll`, `tax_federal`); the rest of its
  tax follows in level instalments through the year, which is why `tax_federal` and
  `tax_state` stay raised for the months after an event. The cost of an exercise is
  `equity_exercise` (never negative income), and the taxable savings sold to pay for
  it show as `withdraw_taxable` in the same month. A company counted at zero
  (`included: false`) records no money and no paper value; its fate is still simulated,
  the `equity_outcome` and `equity_event_age` scalars are filled, the four dollar
  scalars are 0, and its failure can still end the linked job. `failure_cause` has no
  equity cause: a life that fails because the equity paid nothing is attributed as
  before, and `equity.by_outcome` is where the difference shows.
* **The alternative minimum tax** is computed only for plans with `private_equity`;
  traced returns then carry the lines `amt` and `amt_credit` (negative) when either is
  non-zero, and `qsbs_excluded_gain` (informational). For a first year that starts
  part-way through, the one-off equity amounts are added to the annualised return
  unscaled (see `docs/architecture.md` section 3).
* **Home equity**: with `retirement.home_equity: last_resort` (the default) property
  is sold before essentials are cut; the household is then not `depleted`. The year
  is recorded with the sale in `property_sale`.

## `PlanPreview` (`Plan::preview()`, wasm `planPreview(profileJson)`)

Deterministic, no simulation, today's dollars:

```
{ annual_spending, annual_spending_retired,   // budget + home costs + modelled healthcare + children + debt payments
  annual_income, savings_rate, annual_tax,    // gross earned income; share not spent or taxed; estimated tax this year
  investable, wealth,                         // wealth = what the funded test counts
  funded_requirement, funded_ratio,           // null ratio when the requirement is zero
  retirement_need, retirement_tax, future_income_value,
  horizon_years, life_expectancy[], expected_age_at_death[] }
```

## `EquityPreview` (`equity::preview`, wasm `equityPreview(companyJson, draws)`)

What one company's holdings could pay, before tax, with no household simulated: the
holder never exercises early and sells everything at the outcome (after the lockup for
an IPO), and stays in the linked job until then unless `policy.leave_after_months` says
otherwise; tender offers before the outcome are left out of the amounts. Option
deadlines are applied as the simulation applies them: the exercise window (from today
for a company with no `job`, from the planned departure otherwise) and each grant's
ten-year term. Options whose deadline comes before the outcome pay nothing under
`at_deadline: forfeit`; under `exercise` they pay the outcome price less the strike on
what had vested by the deadline, the exercise being taken as affordable, and a draw is
never counted below zero (strike lost when the company then fails is not a payout).
Vested RSUs that require employment are lost by a planned departure in or before the
outcome's month. Today's dollars at a fixed 2.9% inflation, the rate that reproduces the
simulation's median price level after ten years. The simulation's own results are lower
where the job ends before the outcome or savings cannot pay for a deadline exercise. `companyJson` is one element of
`profile.private_equity`; `draws` 0 means the default (4,000), anything else is held
between 100 and 50,000. The draws use a fixed seed, so the same company gives the same
bytes, and the fate of each draw is drawn by the function the simulation uses. A
company that does not parse throws a message with the path of the offending field
(`holdings[0].kind: unknown variant ...`), and so does one the simulation would refuse
(`validate::validate_company`: every company check that needs nothing else from the
profile), as `<path>: <message>` with the path relative to the company
(`holdings/@grant/kind/strike: Strike is -4; ...`). A bare company (a stage and a
price, no holdings) is valid. `included` is ignored. CLI:
`firesight equity-preview <profile> [--company ID] [--draws N] [--json]`.

```
{ draws,
  table:   [ OutcomeRow ],                  // the odds in force for the next ten years: preset with outlook, or the
                                            // custom table normalised; exact probabilities; for a preset the years
                                            // are the 10th and 90th percentiles of its timing
  typical: [ OutcomeRow ],                  // the stage preset at a typical outlook, for comparison
  fmv_409a, preference_overhang, tender_annual_probability,   // values in force after defaults
  paper_value_vested, paper_value_total,    // today, at the preferred price; options at price less strike
  exercise_cost_vested, exercise_cost_total,                  // strike only
  spread_iso_vested, spread_nso_vested,     // vested options at the 409A value: counted for AMT / taxed as pay if exercised now
  nothing_share,                            // share of draws that pay nothing at the outcome
  outcome_shares[4],                        // what finally happens over every round: fails, written off after
                                            // thirty years, acquired, ipo (note: not the EquityOutcome order)
  acquisition_zero_share,                   // among acquisitions: common stock gets nothing
  ipo_below_last_round_share,               // among IPOs: priced below today's preferred price
  tender_before_share,                      // at least one tender offer expected before the outcome
  option_deadline_months,                   // months to the first option deadline (window end or ten-year term);
                                            // null with no options; 0 = this month
  option_deadline_cost,                     // strike (before tax) to exercise the vested options due then; 0 when
                                            // the policy lets them lapse or their strike is at or above the price
  deadline_before_outcome_share,            // share of draws in which an option deadline comes before the outcome
  dots[100],                                // equally likely payoffs, ascending, zeros included
  quantiles[5],                             // p10, p25, p50, p75, p90 over all draws
  if_paid[3],                               // p10, p50, p90 among draws that pay; null when none does
  years_to_payout_p50,                      // median wait among draws that pay; null when none does
  implied_multiple, typical_implied_multiple }   // mean common price at the outcome over the preferred price
                                                 // (0 where nothing is paid), by quadrature: it does not move with draws
```

No mean payoff is reported: it would rest on a handful of draws.

## wasm API

As in `docs/architecture.md` section 6. Beyond the original list: `Engine.warnings()`,
`class Merger { add(bytes), paths(), summarize(profileJson), digest(profileJson),
bytes() }`, `compare(aBytes, bBytes, baselineProfileJson)` (the `Comparison` JSON, B
minus A), `digest(accBytes, profileJson)`, `planPreview(profileJson)` and
`exampleProfiles()` (five demo households), and `equityPreview(companyJson, draws)`
(the `EquityPreview` above). `levers(profileJson)` returns up to four more levers for a
profile with counted, non-empty `private_equity`: `equity_outlook` (pessimistic against
optimistic odds), `equity_exercise` (only at a sale or IPO against vested options
now), `equity_tender` (sell nothing against the maximum) and `equity_sell_down` (keep
the shares against sell when the lockup ends), each present only when it would change
something. Every function returning a document returns a JSON string.
`Accumulator::VERSION` is 7 (7 appends the 37th scalar, `medicaid_age`, and after it
six scalars and three series for private-company equity; 5 added nine scalars, a
sixth histogram series and the per-year shortfall sums); bytes from an older build do
not decode: `Merger.add`, `mergeChunks`, `summarize`, `digest` and `compare` throw
"version 6 is not supported (expected 7)", so chunks cached by an older build must be
discarded, not merged.

## Sizes and timings

Measured natively on one Apple M-series core, 60-year horizon, synthetic lives.

| Item | Result |
|---|---|
| `record_year` | about 21 ns |
| 500-path chunk, serialised | 152 KB (201 KB for the first chunk, which carries the 96 sample paths); 84 KB of it is the scalar table (168 bytes per path) |
| 500-path chunk: encode / decode / merge | 0.8 ms / 0.4 ms / 0.2 ms |
| 100,000-path accumulator, serialised | about 17.4 MB: histograms 0.2 MB, scalars 17.2 MB (43 x 4 bytes x paths, 172 bytes per path) |
| `Summary::from_accumulator`, 100,000 paths | about 51 ms |
| `Summary` JSON, 100,000 paths | 554 KB |
| `SummaryDigest` JSON | about 1.7 KB (about 2.2 KB with private-company equity) |
| `PathTrace` JSON, 60 years, 30 active series | about 180 KB |

## Additions made by the engine audit (for consumers with a strict shape check)

Every change to the JSON documents, in one place. Nothing was removed or renamed.

`Summary`
* `headline.plan_delivered_share` (Estimate), `headline.extra_spending_ratio`
  (Estimate), `headline.initial_withdrawal_rate` (Spread), `headline.home_sale_rate`
  (Estimate).
* `headline.delivered_spending_ratio`: same field, now capped at 1 per year (see the
  scalar's definition).
* `spending.by_year.depleted[]`, `spending.by_year.depleted_shortfall[]` (numbers or
  null, one per year).
* `quantiles[]` and `density[]`: a sixth entry each, `series: "spend_planned"`.
* `distributions[]`, `failure.contrasts[]`: nine more entries (36 scalars), appended
  in this order: `extra_spending_ratio`, `initial_withdrawal_rate`, `home_sale_age`,
  `plan_delivered_share`, `social_security_claim_age_0`,
  `social_security_claim_age_1`, `person_retirement_age_0`,
  `person_retirement_age_1`, `mortgage_payoff_age`.
* `pick_paths[]`: `{ path_index, net_worth[], investable[], spending[] }`.
* `plan_moments`: `{ social_security_claim_age[], social_security_claim_share[],
  person_retirement_age[], person_retirement_share[], mortgage_payoff_age,
  mortgage_payoff_year, mortgage_payoff_share }`.

`SummaryDigest`: `spending_delivery.extra_ratio`.

`Comparison`: `quantile_levels[9]`; `scalars[]` has the nine new scalars too.

`PathTrace`: `scalars[]` has 36 entries; tax-return `lines[]` may contain the keys
`adjustments` and `passive_loss_released`.

## Additions made for private-company equity

Again nothing was removed or renamed. For a profile without `private_equity` the
`SummaryDigest` and the `Comparison` are byte for byte what they were, and the
`Summary` differs only by the longer per-scalar and per-series lists below.

`Summary`
* `equity` (`EquitySummary`), absent without private equity.
* `composition[]`: three more entries (46 series), appended: `income_equity`,
  `equity_exercise`, `equity_paper_value`. All zero without private equity.
* `distributions[]`: six more entries (42 scalars), appended: `equity_outcome`,
  `equity_event_age`, `equity_gross_proceeds`, `equity_exercise_cost`,
  `equity_tender_proceeds`, `equity_net_proceeds`. Empty (`count` 0) without private
  equity.
* `failure.contrasts[]`: five more entries (the new scalars except `equity_outcome`).
* `picks[]`: up to three more labels, `equity_failed`, `equity_paid_median`,
  `equity_paid_p90`, with `pick_paths[]` following.
* `quantiles[]` and `density[]` for `gross_income` now include `income_equity`.

`SummaryDigest`: `equity`, absent without private equity.

`Comparison`: `by_equity_outcome[]`, absent when run A has no private equity;
`scalars[]` has five more entries.

`PathTrace`: `scalars[]` has 42 entries; `columns[]` may contain `income_equity`,
`equity_exercise` and `equity_paper_value`; `events[].kind` may be one of the seven new
kinds; tax-return `result` has `tentative_minimum_tax`, `amt`, `amt_credit_used` and
`amt_credit_carryforward`, and `lines[]` may contain `amt`, `amt_credit` and
`qsbs_excluded_gain`.

New document: `EquityPreview`. New wasm function: `equityPreview`. `exampleProfiles()`
returns four households; `levers()` may return four more levers.

Profile schema (inputs): `private_equity` (a list of `PrivateCompany`, default empty),
described in `docs/architecture.md` section 2. New validation codes all start with
`equity_` or name the field; the fourth demo carries one warning, `equity_dominant`.

Profile schema (inputs): `retirement.home_equity` (`"never"` | `"last_resort"`,
default `last_resort`); `debt_policy.refinance_mortgages` (boolean, default true);
`assumptions.market.equity_premium_adjustment` now defaults to `-0.015`. New
validation warnings: `short_history` (a bootstrap `from_year` leaving under 50 years)
and `adjusted_history` (the historical replay with a non-zero equity adjustment).

## Additions made by the optional realism features

The optional features of `docs/methodology.md` section 18 changed the JSON documents
in these places only. Nothing was removed or renamed, and with every feature off
the values of all existing fields are unchanged.

`Summary`
* `headline.medicaid_rate` (Estimate).
* `distributions[]`, `failure.contrasts[]`: one more entry (37 scalars),
  `medicaid_age`, appended last.

`Comparison`: `scalars[]` has `medicaid_age` too.

`PathTrace`
* `scalars[]` has 37 entries.
* `events[].kind` has seven more values: `tax_loss_harvest`,
  `qualified_charitable_distribution`, `policy_change`, `health_state`,
  `bereavement`, `medicaid`, `ltc_insurance`.
* The features write their other events under existing kinds. `life_event`: job
  changes, pay freezes and cuts, disability onset, recovery and the start of
  disability insurance, involuntary retirement, business failure, major home
  repairs, insurance repricing, rent renewals, adjustable-rate resets, listing a
  home for sale, opening and drawing on a home-equity line or reverse mortgage,
  cash-out refinancing, a divorce, inherited-IRA distributions, life-insurance
  lapses, a search for work after retirement, an adaptive Social Security claim,
  a postponed retirement. `child`: births (late, twins, or not arriving), college
  start with years and aid, adult support, joining and leaving the health plan.
  `career_break`: parental leave. `long_term_care`: a parent's care spell and a
  partner caring at home. `windfall`: inheritances and life-insurance payouts.
  `expense_shock`: the four typed shocks. `major_expense`: vehicle replacements.
  `market`: valuation, dividend cuts, stress episodes, panic sales and re-entry,
  lender freezes. `rehired`: back at work after retirement. `spending_cut` and
  `spending_raise`: belt-tightening when unemployed. `other`: saving paused and
  resumed. Labels are sentences for people; consumers should key on `kind`.
* The year and month series are unchanged in number and meaning. Draws on a
  home-equity line or reverse mortgage are in `property_sale` (cash from
  property) and their balances in `mortgage`; a household's payments for a
  parent's care are in `spend_long_term_care`; premiums for long-term-care
  insurance are in `spend_healthcare` and life-insurance premiums in essential
  spending; disability insurance from Social Security is in
  `income_social_security` and private disability insurance in `income_other`.

Profile schema (inputs): the switches are listed in `docs/methodology.md` section 18
and in the schema block of `docs/architecture.md`.

## Additions made by the richer-distributions work

Profile schema (inputs), all optional and off by default, so existing profiles
give the same results:

* `assumptions.market.kind.joins` on the bootstrap (`"uniform"` | `"matched"`,
  default `uniform`): how the month a new block starts from is chosen.
* `assumptions.market.equity_premium_uncertainty` and
  `assumptions.market.inflation_uncertainty` (an uncertain number or null, default
  null): a per-path shift of the long-run stock return and of inflation, mean zero.
  The adjustment plus the furthest shift must stay within the adjustment's own
  limits (-0.1..0.1 and -0.05..0.15), or the range gets an `out_of_range` error at
  the field that sets the offending end (`low`, `high`, `sd`, `p10` or `p90`).
* `assumptions.market.disasters`: `{ annual_probability (0 = off), depth, years,
  inflation_probability, inflation }`, where `depth`, `years` and `inflation` are
  uncertain numbers.

* `portfolio.single_stock.tails` (`"normal"` | `"fat"`, default `normal`) and
  `portfolio.single_stock.ruin_hazard` (a yearly probability from 0 to 0.5, or null,
  default null: set from the volatility).
* `portfolio.crypto`: `{ outlook ("skeptic" | "neutral" | "believer", default
  neutral), token ("bitcoin_or_ether" | "other_tokens", default bitcoin_or_ether),
  dynamics ({"type": "steady"} | {"type": "cycles", calm_months, wild_months,
  volatility_ratio}, default steady) }`.
* `portfolio.custom_asset.tails` (`"normal"` | `"fat"`, default `normal`) and
  `portfolio.custom_asset.loss_event` (`{ annual_probability, loss,
  mean_includes_losses }` or null, default null), where `loss` is an uncertain
  number between 0 and 1.

* `incomes[].kind.risk.wage_shocks` (`"normal"` | `"fat_tailed"`, default
  `normal`), `incomes[].kind.risk.layoff_pay_cut` (`"normal"` | `"two_part"`,
  default `normal`) and `incomes[].kind.risk.job_search` (`"drawn_at_layoff"` |
  `"monthly"`, default `drawn_at_layoff`), on jobs only. They are plain choices, so
  they add no validation codes: a misspelt value is refused when the profile is
  read. They add three schema types: `WageShockShape`, `PayCutShape` and
  `JobSearchModel`.

* `incomes[].kind.failure` on self-employment (`{"type": "never"}` | `{"type":
  "hazard", young_annual_probability, mature_annual_probability, young_years,
  years_in_business (number or null), recession_multiplier, then}`, default
  `never`, where `then` is `{"type": "stops"}` | `{"type": "restart", gap_months,
  income_fraction}`) and `incomes[].kind.monthly_lumpiness` (0 to 2, default 0).
* `incomes[].kind.equity[].company` gains, for `public`: `tails` (`"normal"` |
  `"fat"`, default `normal`), `ruin` (`{"type": "never"}` | `{"type":
  "by_volatility"}` | `{"type": "stated", annual_probability}`, default `never`)
  and `layoff_link` (true | false, default false); for `private`: `exit_timing`
  (`{"type": "constant_hazard"}` | `{"type": "lognormal", median_years, sigma}`,
  default `constant_hazard`) and `exit_size` (`"classes"` | `"pareto_tail"`,
  default `classes`).
  These add five schema types (`BusinessFailure`, `AfterFailure`, `StockRuin`,
  `ExitTiming`, `ExitSize`; `tails` reuses `TailShape`) and no validation codes:
  out-of-range numbers are `out_of_range` errors at the field's path. In a traced
  path a business closure is logged as a `laid_off` event labelled "Business
  closed" and a restart as a `rehired` event labelled "Self-employment
  restarted"; no output document changes shape.

The `adjusted_history` warning is also raised, at the option's own path, when the
historical replay is run with a long-run uncertainty or disasters switched on. The
sleeve options add no warning codes; out-of-range values are `out_of_range` or
`dist_order` errors at the field's path. No output document changes shape.
