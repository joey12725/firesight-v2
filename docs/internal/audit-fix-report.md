# Engine audit: final report

Commit `980cbb2` on `main` (local, not pushed; committed directly on `main` because the web engineers share this working tree). Gates are green: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.

## State I found
The previous engineer's work was already in commit `cc4e597` (tree clean), not uncommitted. The tree was inconsistent: 3 clippy errors, 14 failing lib tests and a stale golden. Most critical and major fixes were already implemented but had no regression tests. I repaired the tree, re-measured each fix with rebuilt repro profiles (the auditors' `/tmp` files were gone), added tests, finished the remaining findings, and added the output requests and docs.

One caveat on the tests: I did not run the new tests against the pre-audit code. "Fails before" rests on the auditors' measurements and my before/after runs, not on a checked-out old build.

## Findings

Sim-level tests are in `/Users/joeyhersh/.t3/projects/firsight-v2/crates/firesight-core/tests/sim_audit.rs` unless a module is named.

### Critical
1. **ACA credit lost in 70-88% of pre-Medicare years: fixed.** Plan and credit are solved together, with an 8% cushion, credit-aware bracket filling, sheltered-only rebalancing, and an October re-plan when the cliff is within reach. The repro couple now gets the credit in 119 of 120 household-years, and tax-aware beats conventional (95.5% vs 94.6%). Test: `early_retirees_keep_the_premium_credit`.
2. **Spending rules anchored on the bridge rate: fixed.** The three rate-following rules now measure the rate on the funded test's quantities and adjust total spending. I added a conservative rule for raises above plan. Delivered share is capped at 1, with extra spending reported separately. Family example: guardrails 91.1% vs constant 91.8% (was 83.2% vs 87.8%). Tests: `flexible_spending_rules_do_not_fail_more_than_constant_spending` (tolerance 2.5 points, not strict ≥), `guardrails_do_not_cut_when_bridge_income_ends_as_planned`.
3. **Home equity never tapped: fixed.** New `retirement.home_equity: last_resort` (default) sells property before essentials are cut; reported as `home_sale_rate`. Owning now succeeds on 91.2% vs renting 86.9% (was 82.9% vs 92.3%); `never` gives 76.3%. Test: `home_equity_is_released_before_essentials_are_cut`.

### Major
- **NYC name mismatch: fixed.** Aliases with normalised matching. Test: `tax::tests::a_city_resolves_by_any_of_its_names`.
- **Local tax flat on all AGI: fixed.** Earned-only bases and Portland thresholds. Test: `local_tax_follows_its_base_and_thresholds`.
- **Roth conversions taxed by exempt states: fixed.** Illinois has no age test, Iowa 55. Test: `exempt_states_do_not_tax_conversions`.
- **Plan budgets health gross of the credit: fixed.** Re-saved withdrawals are now 3.4% of withdrawals and HSA excess is about zero. Test: `retirees_do_not_withdraw_in_order_to_save`. The check-mode assertion was not added; the test stands in for it.
- **Windfall withheld twice: fixed.** Test: `a_taxable_windfall_is_taxed_once`.
- **Conversions ignore gain stacking: fixed.** Conversions are capped by real marginal cost. Tax-aware 89.3% vs conventional 88.9%, 0 paths harmed by tax-aware. Test: `tax_aware_is_not_worse_than_conventional_with_appreciated_assets`. The conversion tax is still not in the funded test.
- **Partner-offset cuts: fixed** (same mechanism as critical 2). Guardrails deliver 98% with a 4-year offset vs 92% without. Test: `a_partner_who_works_longer_does_not_trigger_cuts`.
- **72(t) second tranche: fixed.** Test: addition in `decumulation::tests::running_sepp_schedule_is_always_taken_unpenalised`.
- **Funded test counts distant income: fixed.** Liquidity bridge added. At a 4% rule the median first-year rate is 3.88% (was 4.55%). Test: `the_funded_test_does_not_spend_social_security_decades_early`.
- **Bootstrap rate levels / free refinancing (two findings): fixed.** Rate levels are carried from today; refinancing has costs, a 4-year payback test and `debt_policy.refinance_mortgages`. Refinancing in the first January is 1% of paths (was 54%). Tests: `economy::tests::bootstrap_rates_start_from_today_and_move_like_history`, `refinancing_waits_for_rates_to_fall`.
- **Optimistic default: fixed, by a judgement call you should review.** `equity_premium_adjustment` now defaults to −0.015, with a new `adjusted_history` warning when historical replay is run with a non-zero adjustment.
- **Crash recovery: fixed.** Recovery is to the pre-crash nominal level. A 40%/5-year crash now costs the early-career example 26 points of success (was 3.6). Tests: `crash_overlay_*`.
- **Crypto: fixed.** Presets set the expected real return (0% neutral). Test: `crypto_matches_its_stated_assumptions`.
- **Gold and international: fixed.** One long-run assumption in all three resampling models (gold 1% real, international = US − 1 point). I kept the historical replay unscaled. Test: `gold_and_international_follow_one_long_run_assumption_in_every_model`.
- **`from_year`: fixed** with a `short_history` warning; I added no dedicated test for it.
- **Regime home prices: fixed.** Level pull, 12-month starting momentum, and I changed the gap to be measured against expected inflation. Ten-year real change is −18% / +10% / +50% at p5 / p50 / p95. Test: `regime_home_prices_have_the_dispersion_of_the_record`.
- **Future job overpaid: fixed.** I also reset shocks at the job's start. Test: `a_job_that_starts_later_starts_at_its_salary`.
- **`sell_on_vest`: fixed.** Test: `kept_employer_shares_stay_concentrated`.
- **Retired-at-start determinism: fixed.** Test: `sim_invariance::a_household_retired_at_the_start_does_not_depend_on_path_order`.

### Minor, fixed
- NYC graduated schedule.
- Pennsylvania deferrals and California head-of-household schedule.
- Poverty-line chaining (no dedicated test).
- Refundable child credit on taxable earned income.
- `explain()` labels.
- Failure cause for working-age depletion (`running_out_while_working_is_not_blamed_on_the_market`).
- Long-term care in the plan, with raises held during care.
- Funded-test gross-up at the marginal rate.
- Lognormal bills floored at zero; I recentred so the mean is preserved.
- Collapsed assets stay collapsed through a crash overlay.
- Earnings backfill on the career curve.
- Child-cost tier from spending (`a_retired_household_gets_the_child_cost_tier_of_its_spending`).
- January events on the right return (`a_january_sale_is_taxed_in_its_own_year`).
- v1 migration: `real_growth` 0, notes, refinancing off.
- Wage-curve schema text.
- `validate` overflow (saturating add).

### Minor, deferred (all listed in methodology section 17)
- New York recapture, Utah Social Security credit, California SDI.
- Published 2027 figures.
- Refunds booked as `income_other`. I tried negative tax; it breaks the non-negative series invariant, so I reverted.
- First-year contributions.
- Payroll deferrals during a shortfall.
- Post-depletion statistics (partly eased by the home sale).
- Regime return bias and trending.
- Survivor benefits.
- `end_age` 100 truncation.
- Medicare age gradient on Part B/D.
- Children's health insurance.
- wasm speed.

### Rejected
None.

## Additive output fields (for the web shape test)

**Summary**
- `headline.plan_delivered_share` (Estimate)
- `headline.extra_spending_ratio` (Estimate)
- `headline.initial_withdrawal_rate` (Spread)
- `headline.home_sale_rate` (Estimate)
- `headline.delivered_spending_ratio`: same field, now capped at 1.
- `spending.by_year.depleted[]` and `spending.by_year.depleted_shortfall[]` (number or null per year).
- `quantiles[]` and `density[]`: a sixth entry each, `series: "spend_planned"`.
- `distributions[]` and `failure.contrasts[]`: 36 entries (was 27). Appended in order: `extra_spending_ratio`, `initial_withdrawal_rate`, `home_sale_age`, `plan_delivered_share`, `social_security_claim_age_0`, `social_security_claim_age_1`, `person_retirement_age_0`, `person_retirement_age_1`, `mortgage_payoff_age`.
- `pick_paths[]`: `{ path_index, net_worth[], investable[], spending[] }`, one per distinct pick.
- `plan_moments`: `{ social_security_claim_age[], social_security_claim_share[], person_retirement_age[], person_retirement_share[], mortgage_payoff_age, mortgage_payoff_year, mortgage_payoff_share }`.

**Others**
- SummaryDigest: `spending_delivery.extra_ratio`.
- Comparison: `quantile_levels[9]`; `scalars[]` gains the nine scalars.
- PathTrace: `scalars[]` has 36 entries; tax-return lines may include `adjustments` and `passive_loss_released`.
- `Accumulator::VERSION` is 5.
- Profile schema: `retirement.home_equity`, `debt_policy.refinance_mortgages`, and the new default for `assumptions.market.equity_premium_adjustment`.
- Rust API: `Plan::summarize_seeded(acc, seed)`, `stats::SampleRecorder`.

No wasm export was renamed. The wasm `summarize` now fills `pick_paths`. Covered by `the_summary_carries_the_additive_outputs`.

## Performance

`firesight bench`, one thread, paths/s:

| Example | Before (35ea2cc) | After |
|---|---|---|
| dual-income-family | 4,846 | 4,441 |
| early-career-saver | 5,306 | 4,735 |
| side-hustle-landlord | 7,161 | 6,784 |

That is 5-11% slower, and the 5,000/core target is now missed on two of three examples. Of the auditor's prototypes, 1, 3 and 4 are in; prototype 2 is only partly done (generic tax function, single reusable evaluation struct, no measured gain). Beyond those, the October re-plan is skipped when the cliff is out of reach, the second planning pass has a 3% tolerance, and mix normalisation is cheaper. The extra per-year planning work (credit-aware two-pass plans, conversion costing) outweighs them. PGO was not adopted.

Determinism was re-checked after the fixes: native (3 threads) and wasm (2 merged chunks) give identical Summary and trace JSON for the 3 examples × 4 market models. The no-allocation test passes.

## Headline numbers, 5,000 paths, seed 1

| | Before | After |
|---|---|---|
| **Dual-income: success** | 82.9% | 91.4% |
| FI age p50 | 51.8 | 53.4 |
| Delivered | 128.2% | 97.0% (extra 21%) |
| Terminal net worth p50 | $2.50M | $1.72M |
| Lifetime tax p50 | $914k | $969k |
| Home sold | n/a | 23.6% |
| **Early-career: success** | 86.5% | 83.6% |
| FI age p50 | 38.5 | 39.2 |
| Terminal net worth p50 | $7.30M | $3.51M |
| Lifetime tax p50 | $793k | $858k |
| Home sold | n/a | 23.4% |
| **Landlord: success** | 90.1% | 92.4% |
| FI age p50 | 44.5 | 47.5 |
| Delivered | 153.5% | 97.1% (extra 29%) |
| Terminal net worth p50 | $1.73M | $0.99M |
| Lifetime tax p50 | $621k | $595k |
| Home sold | n/a | 19.4% |

With the equity adjustment set back to 0, success is 93.7% / 90.3% / 94.7%. So the −1.5-point default accounts for roughly 2-7 points of the difference.

## Still wrong or needing a decision
1. **Success now counts a last-resort home sale as success.** About 20-24% of paths on all three examples sell the home. The UI must show `home_sale_rate` next to the success rate or the headline flatters.
2. **Three defaults are judgements that move headlines by several points:** the −1.5-point equity default, crypto at 0% expected real return (a 10% sleeve now costs 9.6 points of success), and gold at 1% real.
3. **Flexible rules are not strictly ≥ constant spending.** A retired couple with care risk gets 95.5% vs 98.4%. Guardrails raises are uncapped, so mean extra spending can exceed 100% of plan on lucky paths.
4. **Performance** is below both target and baseline; it needs structural work in `plan_year` and tax evaluation.
5. **Regime model:** the home/CPI annual correlation is 0.73 against 0.55 in history, just inside the test tolerance; the return bias and trending are unfixed.
6. **Bootstrap:** bond returns are inconsistent with the carried yield level, and the 30-year rate range (4.0-10.0% at p5-p95) is narrower than history.
7. **Web-facing changes:** the `delivered_spending_ratio` semantics changed and the accumulator bytes are incompatible, so the web shape test and wasm package need rebuilding.

Files:
- `/Users/joeyhersh/.t3/projects/firsight-v2/docs/methodology.md`
- `/Users/joeyhersh/.t3/projects/firsight-v2/docs/engine-output.md`
- `/Users/joeyhersh/.t3/projects/firsight-v2/docs/architecture.md`
- `/Users/joeyhersh/.t3/projects/firsight-v2/crates/firesight-core/tests/sim_audit.rs`