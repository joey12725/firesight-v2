# stats module: integration report

STATS SUBSYSTEM: implemented and committed. Gates pass: cargo fmt --check, clippy -p firesight-core --all-targets -D warnings, cargo test -p firesight-core (124 passed, 44 of them stats); firesight-wasm and firesight-cli tests still pass. STUB allow line removed.

FILES (all under /Users/joeyhersh/.t3/projects/firsight-v2/.claude/worktrees/wf_9cc6fa29-24b-6)
- crates/firesight-core/src/stats.rs became crates/firesight-core/src/stats/: mod.rs (enums, records, Recorder), hist.rs (grid + codec), accumulator.rs, num.rs (Estimate, Spread, sort, JSON number encoding), summary.rs (Summary + digest), compare.rs, trace.rs, testkit.rs (cfg(test) synthetic lives).
- docs/engine-output.md: field-by-field reference for Summary / SummaryDigest / Comparison / PathTrace, the enum lists and the recorder contract. Web UI engineers should code against it.
- Not touched: lib.rs, model/, other modules, Cargo.toml.

=== SIGNATURE CHANGES VERSUS THE STUB (every one) ===
1. YearRecord: field `hist: [f64; 5]` REMOVED; fields `retired: bool` and `months: u8` ADDED. Now `{ status, retired, months, values: [f64; YearSeries::COUNT] }`. The five histogram quantities are derived from `values`, so they cannot disagree with composition.
2. MonthRecord fully defined: `{ month, price_level, status, retired, values: [f64; 43], portfolio_return, asset_returns: [f64; N_ASSETS] }`; `net_worth` field removed (method `net_worth()` / `investable()` instead). `MonthRecord::new(month)` gives zeros with price level 1.
3. PathEnd keeps `scalars: [f32; PathScalar::COUNT]` (default all NaN); added `set(scalar, f64)`, `get`, `set_outcome(Option<FailureCause>)`. Entries for derived scalars are overwritten by the recorder.
4. TraceEvent: `kind` is now the enum `TraceEventKind` (was String); `message` renamed `label`.
5. SummaryContext gained `second_birth: Option<YearMonth>` (per-person survival).
6. Accumulator: no longer derives Serialize/Deserialize (use to_bytes/from_bytes); VERSION = 2; new pub fields `months: Vec<u64>` ([year]) and `cuts: Vec<u32>` ([year][3]); HIST_BINS is 2945 (was 512); hand-written Debug (one line); ScalarChunk equality is bitwise so NaN equals NaN. New methods: `rows()`, `path_scalars(path)`, `histogram(series, year)`, `alive(year)`.
7. SamplePath series are `Vec<i64>` whole dollars (were Vec<f32>).
8. Summary restructured (below). The seven fields scenario.rs reads are kept with the same names and types: headline.success_rate.value, median_fi_age, median_retirement_age, median_terminal_net_worth, p10_terminal_net_worth, delivered_spending_ratio.value, median_lifetime_tax. Removed: start_year, ages (now in `axis`), fi_by_year, means (now `composition`), failure_attribution (now `failure`), headline.fi_age_range. Estimate gained `lo`, `hi`.
9. PathTrace restructured (below); TraceTaxReturn.lines are structs, PathTrace.scalars are `{scalar, value}`. TraceRecorder::new(seed, start) and finish() unchanged.
10. hist_bin_value returns the bin midpoint (not geometric centre); added hist_bin_edges(bin) -> (lo, hi).
11. Recorder trait methods unchanged.

=== FINAL ENUMS (key, meaning; money = today's dollars) ===
HistSeries (5, all derived by the recorder from YearRecord.values): net_worth = 5 account balances + property_value - mortgage - debt (year end); investable = cash+taxable+traditional+roth+hsa; spending = sum of the 8 spend_* groups; gross_income = sum of the 6 income_* series; total_tax = sum of the 4 tax_* series. The three flows are annualised (x 12/months) in the histogram only when months < 12.

Status (6): working; retired_surplus (retired, on plan, investable >= 2x its level at retirement); retired_on_plan (delivered >= 95% of planned); retired_cutting (delivered < 95%); depleted (takes precedence, working or retired); dead (NEVER recorded; derived as paths with no record that year). Use `Status::classify(retired, depleted, delivered, planned, investable, investable_at_retirement)`. Constants: ON_PLAN_TOLERANCE 0.95, MILD_CUT_RATIO 0.90, DEEP_CUT_RATIO 0.75, SURPLUS_WEALTH_MULTIPLE 2.0.

YearSeries (43, in index order; balances/liabilities period-end, everything else period total; non-negative except investment_return). `kind()` returns SeriesKind {balance, liability, income, withdrawal, tax, spending, saving, transfer, memo}.
 balance: balance_cash, balance_taxable, balance_traditional, balance_roth, balance_hsa, property_value (market value of ALL properties; replaces home_equity)
 liability: mortgage (principal on all properties), debt (non-mortgage)
 income: income_wages (wages, bonus, commission, vested equity, gross), income_self_employment, income_social_security (gross), income_pensions (incl. annuities), income_rental (net operating income before the rental's mortgage), income_other (incl. windfalls)
 withdrawal: withdraw_cash, withdraw_taxable, withdraw_traditional (incl. RMDs, excl. conversions), withdraw_roth, withdraw_hsa
 tax: tax_federal (income + capital gains + NIIT), tax_state (or foreign income tax), tax_payroll (FICA + SE), tax_penalties. Cash basis: withholding, estimates and the April settlement in the year paid.
 spending: spend_essential (incl. rent), spend_discretionary, spend_housing (owned property: P&I, property tax, insurance, upkeep, HOA, PMI), spend_healthcare, spend_long_term_care, spend_children, spend_debt (non-mortgage payments), spend_one_off (shocks, event expenses, closing costs)
 saving: save_cash, save_taxable, save_traditional, save_roth, save_hsa (employee money), save_employer (401k match + employer HSA; renamed from save_employer_match)
 transfer: roth_conversion, property_purchase (cash into property at purchase), property_sale (net cash from a sale)
 memo: spend_planned, investment_income (interest/dividends inside accounts; renamed from income_investment and excluded from gross income), investment_return (balance change not explained by flows; may be negative)
 spend_planned = what total spending would have been with no spending-rule cut and no depletion shortfall, same scope as `spending`; MUST equal actual spending while working. It is the denominator of every delivered-vs-planned metric and of the status test.
 Intended identity per period (`YearRecord::cash_flow_residual()` == 0, for the sim's conservation test): income + withdrawals + property_sale + save_employer = taxes + spending + saving + property_purchase.

PathScalar (26; f32; NaN = did not happen). `is_derived()`, `unit()` (dollars/age/years/months/ratio/rate/index/flag).
 Sim supplies (indices 0-13): fi_age, retirement_age, depletion_age (ages of person 0), death_age_0, death_age_1 (own age; NaN if single; age at horizon if alive), investable_at_retirement, early_retirement_return (annualised real portfolio return over first 10 retirement years), worst_drawdown (positive fraction), months_unemployed, long_term_care_cost, inflation (annualised CPI over the path), funded_ratio (at retirement; NaN if not computed), failure_cause (FailureCause index), success (1.0/0.0).
 Recorder derives (indices 14-25; whatever the sim puts there is overwritten): years_simulated, terminal_net_worth, peak_net_worth, min_investable (lowest year-end balance while retired), lifetime_tax, lifetime_spending, delivered_spending_ratio (sum of retired-year spending / sum of retired-year spend_planned), worst_spending_ratio (lowest single retired year), years_retired, years_below_90, years_below_75, years_depleted.

FailureCause (7): market_sequence, job_loss, health, longevity, overspending, inflation, other (last two added). A failed path with a NaN cause is counted as other.
TraceEventKind (28): see docs/engine-output.md.

=== RECORDER CONTRACT ===
Per path: begin_path(i); record_year(y, &YearRecord) once per calendar year in order from y = 0 (calendar year of profile.start), no gaps, last call = year of the household's last death or the horizon; end_path(&PathEnd). Never record a year after everyone is dead and never pass Status::Dead (ignored in release, debug_assert). `months` = months of that calendar year covered (1..=12): under 12 in year 0 of a mid-year start and in the final year. `retired` = retirement date passed by period end. Sums are stored as recorded (not annualised), rounded to whole dollars per path-year. Year index >= acc.years is dropped (debug_assert). Recording a path index twice panics. The first SAMPLE_PATHS = 96 path indices keep annual net worth / investable / spending.
Detail (only when wants_detail()): record_month once per simulated month in order starting at `start` (debug_assert), record_event(TraceEvent { month, kind, label, amount: Option<f64> }), record_tax_return(year, &TaxDetail) after that year's months are recorded (it stamps the December price level). TraceRecorder::begin_path resets it.
Paths may be recorded in any order; merge is exact. Tested: one chunk of 1,000 equals ten merged chunks of 100 in four orders plus a merge tree with uneven chunks, byte for byte.

=== HISTOGRAM GRID ===
2,945 bins, symmetric: one exact-zero bin (zero-width; a mass of zeros reads back as exactly 0); $16 linear bins for |x| < $1,024; from $1,024 to 2^32 (~$4.29B) each octave split into 64 equal bins (0.78% to 1.5625% wide; worst relative width between $1k and $1B is 1.61%, at the $992 bin); beyond 2^32 clamps to the outer bin; NaN goes to the zero bin. Mapping uses the exponent and top 6 mantissa bits (no ln, bit-identical on wasm). Quantiles interpolate linearly inside a bin: against exact quantiles of 100k lognormal samples (sigma 1.2) the error is under 0.4% from the 0.1st to the 99.9th percentile. In memory dense u32 (3.5 MB for 60 years); on the wire run-length packed varints (1 byte per empty block, about 1-2 bytes per occupied bin).

=== SUMMARY JSON (Summary::from_accumulator(&acc, &ctx)) ===
Conventions: whole dollars; missing numbers are null (serde `with` modules map NaN to null and write whole numbers without a fraction; JSON only); per-year arrays indexed by year; distributions by year cover households alive that year.
- version (1), paths
- axis { start "YYYY-MM", start_age, first_year_months, years[], ages[] (person 0 in December), alive[] }
- headline { success_rate, fi_rate, retired_rate, depletion_rate: Estimate{value,std_error,lo,hi} (Wilson); median_fi_age + fi_age: Spread and median_retirement_age + retirement_age: Spread (quantiles over ALL paths, never = later than any age, null if not reached); depletion_age, years_depleted (among depleting paths); median_terminal_net_worth, p10_terminal_net_worth, terminal_net_worth; investable_at_retirement; delivered_spending_ratio (mean, Estimate); years_cutting_share; median_lifetime_tax, lifetime_tax, lifetime_spending; funded_ratio: Spread|null }. Spread = {p10,p25,p50,p75,p90,p50_lo,p50_hi,count}.
- spending { retired_paths, paths_below_90, paths_below_75 (Estimate, share of retired paths with such a year), years_below_90, years_below_75 (Estimate, share of retired path-years, SE clustered by path), years_cut: Spread, worst_cut_levels [0.5,0.75,0.9,0.95,0.99], worst_cut[], by_year { retired[], below_90[], below_75[] } }
- quantiles[5] { series, levels [0.01,0.05,0.10,0.25,0.50,0.75,0.90,0.95,0.99], values[level][year] }
- density[5] { series, edges[], starts[year], counts[year][], zeros[year] }: 8 cells per doubling; cell k of year y spans edges[starts[y]+k .. +1]; exact zeros separate.
- status { statuses[6], shares[status][year] } (share of ALL paths, rows sum to 1, dead derived)
- survival { household[year], people[person][year] } (alive at some point in the year)
- composition[43] { series, kind, values[year] }: mean per living household; flows annualised
- fi, retirement, depletion: AgeCurve { ages[], counts[], cumulative[], lo[], hi[], never, dots[100] } over all paths
- distributions[26] { scalar, unit, count, missing, mean, std_dev, min, max, quantiles[9], dots[100], edges[41], counts[40] }
- scatter { paths[], columns[8] { scalar, values[] } }: up to 5,000 evenly spaced paths; early_retirement_return, terminal_net_worth, fi_age, retirement_age, depletion_age, death_age_0, investable_at_retirement, success
- failure { failed, never_retired, depleted, causes[7] {cause,count,share_of_failed,share_of_all}, factors[6] {factor,scalar,above,threshold,failed,succeeded} (overlapping tags: poor_early_returns, deep_drawdown, job_loss, long_term_care, long_life, high_inflation; present = beyond the 75th / below the 25th percentile of all paths), contrasts[24] {scalar, failed_count, succeeded_count, failed_median, succeeded_median, failed_mean, succeeded_mean} }
- picks[] { label: p10|median|p90 (terminal net worth), median_fi, failing; scalar, path_index, value }
- samples[<=96] { path_index, net_worth[], investable[], spending[] }
`Summary::path_at_quantile(&acc, scalar, q) -> Option<u64>` (nearest rank among paths with a value, ties by path index).
`Summary::digest() -> SummaryDigest`: notes, paths, start/end year, start_age, success/fi/depletion {p,lo,hi}, fi_age/retirement_age/terminal_net_worth {p10,p50,p90}, lifetime_tax_p50, fi_by_age and depletion_by_age {ages,p}, by_age {ages[6], alive, net_worth_p10/p50/p90, spending_p10/p50/p90}, spending_delivery, failure_causes (top 3), trace_paths {p10, median, p90, failing}.
`compare(&a, &b) -> Comparison` (B minus A, paths matched by index, unmatched counted in unpaired_a/b): outcomes[4] (success, fi, retired, depleted) with a, b, both, a_only, b_only, neither, difference (Estimate with SE sqrt((d - delta^2)/n)), z (McNemar), within_noise; scalars[24] with both/a_only/b_only/neither, mean_a, mean_b, mean_delta (Estimate), quantiles[9], dots[100], b_higher/a_higher/equal, within_noise; headline[10] {metric, a, b, delta}.
PathTrace: path_index, seed, start, months, statuses, status[month] (u8 index), columns[] {key, unit, values} (net_worth, investable, every non-zero YearSeries key, price_level, portfolio_return, return_<asset>; whole dollars, rates to 6 dp), annual {years, months, status, columns incl. spending/gross_income/total_tax}, tax_returns[] {year, price_level, result (TaxResult as JSON), lines[]} in nominal dollars, events[], scalars[26] (identical to the aggregate row; tested).

=== MEASURED (native release, one M-series core, 60-year horizon, synthetic lives) ===
- record_year: 21 ns.
- 500-path chunk: 111 KB (161 KB for the first chunk, which carries the 96 sample paths); 52 KB of that is scalars. Encode 0.8 ms, decode 0.4 ms, merge 0.2 ms.
- 100k-path accumulator: 10.6 MB serialised = histograms 0.16 MB + scalars 10.4 MB.
- Summary for 100k paths: 51 ms; JSON 530 KB. Digest 1.66 KB. Comparison not timed.
- PathTrace JSON, 720 months with 30 active series: 180 KB.
These come from the testkit generator, not the real sim; real spreads will move the histogram and JSON sizes somewhat.

=== THINGS THE INTEGRATOR SHOULD KNOW ===
- The scalar table dominates the 100k accumulator (26 x 4 bytes x paths). The wasm `mergeChunks(a, b) -> bytes` re-encodes the running total on every merge, about 1 GB of copying over 200 chunks. A stateful merger object in firesight-wasm would avoid it. I did not compress scalars; a per-column varint mode for integer and age columns would save roughly a third if wanted.
- wasm `summarize` still has its TODO; it now needs SummaryContext { start, first_birth, second_birth }.
- Survival per person is computed from death_age_0/1 rounded to whole months; the sim must report ages in twelfths.
- The `job_loss` / `long_term_care` / `long_life` factors use a strict "beyond the 75th percentile" test, so a factor reads 0 when more than a quarter of paths tie at the maximum (for example everyone reaching the horizon).
- Composition means for flows divide by household-years recorded (months/12), so partial first and last years do not dilute them.
- FI/retirement/depletion ages in the testkit only move in whole years; nothing in stats depends on that.
- lib.rs module table still says stats is pending and architecture.md section 5 still says "512 bins, 1%" and lists the old scalar set; I did not edit either.

## Shared type requests

- crates/firesight-wasm/src/lib.rs api::summarize: replace the TODO with Summary::from_accumulator(&acc, &SummaryContext { start, first_birth, second_birth }) (SummaryContext gained second_birth: Option<YearMonth>).
- crates/firesight-wasm: mergeChunks(bytes, bytes) -> bytes re-encodes the whole running accumulator on each merge (10.6 MB at 100k paths); add a stateful merger (hold an Accumulator, merge chunk bytes into it, serialise once) and expose compare(a, b) and Summary::digest().
- crates/firesight-core/src/lib.rs: update the module table row for stats (now implemented) - doc comment only.
- docs/architecture.md section 5: replace '512 bins, about 1% resolution from $100 to $1B' with the implemented grid (2,945 bins: exact zero, $16 linear below $1,024, 64 bins per octave to 2^32), and update the per-path scalar list, the status list (retired_surplus added) and the failure causes (inflation, other added).
- crates/firesight-core/src/sim.rs (sim owner): YearRecord no longer has `hist`; fill `values`, `status` (via Status::classify), `retired`, `months`. PathEnd: fill only the 14 supplied scalars (set / set_outcome). MonthRecord has new fields (status, retired, portfolio_return, asset_returns). TraceEvent.kind is TraceEventKind and `message` is now `label`.
- crates/firesight-core/src/scenario.rs Metric::read still compiles unchanged; if more metrics are wanted, headline now also offers fi_rate, retired_rate, depletion_rate (Estimates) and spending.paths_below_90 / years_below_90.
