# people module: integration report

Mortality, health costs, long-term care and Social Security are implemented in `crates/firesight-core/src/health.rs` and `crates/firesight-core/src/benefits.rs`; no other file was touched. Gates pass: `cargo fmt --check`, `cargo clippy -p firesight-core --all-targets -- -D warnings`, `cargo test -p firesight-core` (121 tests, 38 of them mine), and `cargo build --workspace`. Both STUB allow lines are removed.

One thing I could not do as asked: ssa.gov and web.archive.org refused fetches, so the "SSA published PIA worked example" is recomputed from the data's AWI, wage-base and bend-point series rather than quoted. For a max earner from 22 retiring at 62 in 2026 it gives AIME $14,358 and PIA $4,216.90. The $2,969 at 62 and 1 month that the test asserts is a figure I recall as SSA's 2026 maximum at 62; it is not in the data. The max-earner-at-FRA test does reproduce the data's published $4,152 (computed 4,152.4).

## Signature changes from the stubs

- **`SocialSecurityModel::new(data, haircut, start: YearMonth)`**: `start` added. Published AWI, wage bases and COLAs are used up to the start year and the simulated price level after it.
- **`Claimant`**:
  - `earnings_this_year` is renamed `annual_earnings`: expected earned income for the calendar year, nominal and annualised. Year-to-date cannot drive a stateless test.
  - `withheld_months: f64` is added.
  - `pia_real` semantics change: `None` means no Social Security at all (no own, spousal or survivor benefit, and none for the widow(er)); `Some(0.0)` means no own record but eligible for spousal and survivor benefits. Before 62 it may hold a projection.
  - `Claimant::new(birth, claim_age, pia_real)` is added.
- **`MonthlyBenefits`**: adds `withheld_months: [f64; MAX_PEOPLE]` and `total()`.
- **`EarningsRecord`**: adds `awi_at_60`, `price_at_62`, `carry_in`.
- **`HealthContext`**: adds `months_since_start: u32` and `price_level: f64`. `Default` is now manual: `price_level` 1.0, `in_us` true, `months_since_coverage_ended` `u32::MAX`.
- **`HealthState`** is replaced: `{ ltc_start_month (u32::MAX = none), ltc_end_month, ltc_monthly_cost, ltc_care: CareType, oop_persistent, oop_shock }`. Create it only with `HealthModel::initial_state`.
- **`HealthMonth`**: amounts are now nominal (the stub said today's dollars); adds `ltc_in_facility` and `total()`.

## health.rs

**MortalityModel**
- `new(birth, sex, health, start, &Longevity, &Data)` (unchanged), about 3 µs.
- `draw_months_to_death(&self, &mut Rng) -> u32`: exactly one `uniform()`. Returns n ≥ 1: alive in sim months 0..n, dead from month n; P(n > k) = `survival(k)`. `FixedAge` consumes the draw and ignores it.
- `survival(months: u32) -> f64`, `survival_table() -> &[f64]` (index = months since start, last entry 0), `life_expectancy() -> f64` (years), `age_months_at_start() -> u32`, `max_months() -> u32`.
- Model: q = min(1, m · q2023(sex, age) · (1−r_near)^(min(year, 2050)−2023) · (1−r_ult)^max(0, year−2050)), with the calendar year of each simulated month and monthly survival (1−q)^(1/12). Everyone is dead at 120.
- Health multipliers come from the data presets: Excellent 0.75, Average 1.0, Poor 1.5. This matches the model's doc comments.
- Verified: e65 and e0 for both sexes within 0.1 of SSA; nine rows of the research table (cohort LE, period LE, P90/P95/P100) within 0.1 year and 0.005; KS test over 200k draws.

**HealthModel**
- `new(&Healthcare, state: Option<&str>, risk_enabled, &Data)` (unchanged). `state` is ignored because the data has no state factors.
- `phase(&HealthContext) -> CoveragePhase`:
  - `in_budget` gives `None`.
  - Employer coverage wins at any age.
  - In the US at 65+ gives `Medicare`.
  - In the US before 65: `Custom` if configured, `Cobra` while `months_since_coverage_ended < COBRA_MONTHS` (18) if chosen, otherwise `Aca`.
  - Abroad without employer coverage: `Custom` if configured, otherwise `None`.
- `initial_state(sex, age_months_at_start: u32, months_to_death: u32, &mut Rng) -> HealthState`: exactly `INITIAL_DRAWS` (5) `next_u64` calls. Schedules the long-term care episode backwards from death and seeds the persistent cost shock.
- `step(&mut HealthState, &HealthContext, &mut Rng) -> HealthMonth`: exactly `STEP_DRAWS` (2) `next_u64` calls, whatever the phase, risk setting or mode; about 9 ns.
- `expected_annual_cost(age, phase) -> f64`: today's dollars, premium plus mean out-of-pocket, no excess inflation, no long-term care.
- `expected_annual_cost_at(age, phase, months_since_start) -> f64`: the same with accumulated excess medical inflation. Use this one for the FI test.
- Helpers: `excess_growth(months) -> f64`, `aca_benchmark_monthly(age) -> f64` (today's dollars), `ltc_probability(sex, age_at_death) -> f64`, `ltc_blended_annual_cost() -> f64`.
- Types and constants: `CareType { None, Home, AssistedLiving, NursingHome, Expected }`, `HealthState::ltc_months()`, `COBRA_MONTHS`, `LTC_FACILITY_SPENDING_OFFSET` (0.5).

**Units of `HealthMonth`**: nominal = today's dollars × `ctx.price_level` × 1.015^(months_since_start/12). The data's 1.5% excess medical inflation applies to premiums, out-of-pocket, long-term care and the Custom cost.

**ACA output for the tax module**
- `HealthMonth.premium` is the unsubsidised premium of the chosen metal tier.
- `HealthMonth.aca_benchmark` is the second-lowest silver premium for that person-month, nominal, and 0 outside the `Aca` phase.
- Sum `aca_benchmark` over covered people and months into `TaxInputs::aca_benchmark_premium`; count `Aca` months for `aca_months`; subtract the credit at tax time.

**Prices** (today's dollars per person per year)

| Phase | Premium | Mean out-of-pocket | Sigma / cap |
|---|---|---|---|
| Employer | $1,765 | $800 (18–44), $1,200 (45–54), $1,700 (55–64) | 1.0 / $4,500 |
| COBRA | $9,804 × 1.02 | same as employer | same |
| ACA | $703/month at 40 × CMS age curve | same means × 1.5 bronze, × 0.7 gold | 1.0 / $12,000 |
| Medicare | $5,135 | $2,183 | 0.5 / none |

- ACA uses the latest benchmark year in the data (2027) and the 2027 out-of-pocket maximum.
- Metal tier premium is the benchmark × AV/0.70, so 0.857 for bronze and 1.143 for gold.
- Every Medicare-phase cost is multiplied by 1.015^(age−65). Total at 65 is $7,300 and the 21-year sum is about $210k, matching the research cross-check.

**Out-of-pocket**
- An annual standardised shock z = p + u is redrawn when `months_since_start % 12 == 0`, with p' = 0.92p + w and a 33% persistent share.
- The year's amount is min(exp(mu + σz), cap)/12 each month, repriced monthly for phase and age.
- mu is solved at compile time so the mean after the cap equals the configured mean (tested).
- With `health_risk` off, the configured mean is charged.

**Long-term care**
- If death is after 65: probability min(0.90, logistic(−1.208 + 0.065(T−70) + shift_sex)); duration from the Johnson bins; care type 45/15/40 at $55,000 / $74,400 / $114,975 a year.
- The episode ends at the death month and cannot start before 65 or before sim month 0.
- 200k lives under the 2023 period table give 38% incidence for men and 55% for women (±1 point) and mean lifetime cost within 8% of $75k and $153k.
- With `health_risk` off: `CareType::Expected`, mean duration at probability × blended cost.
- If `healthcare.long_term_care` is false no episode is scheduled, but the draws are still consumed.
- `in_budget` mode still charges long-term care.

## benefits.rs

**EarningsRecord**
- `backfill(current_earnings, current_age, career_start_age, &model)`: Typical profile.
- `backfill_profile(.., profile: LifecycleProfile, &model)`.
- `backfill_with(current_earnings, current_age, career_start_age, &model, curve: impl Fn(f64) -> f64)`: pass the career module's lifecycle factor here for consistency with simulated wages.
- `from_history(birth, &[(year, nominal)], &model)`.
- `record_year(nominal, wage_index, wage_base)` (low-level); `aime() -> f64` in index-year (2025) wage dollars.
- Backfill rule: past year earnings = current × curve(age)/curve(current_age) × CPI(year)/CPI(today), capped at the published wage base and indexed by published AWI. Typical uses the CGM high-school coefficients, Steep the college ones, Flat is constant.
- `carry_in` holds the pre-start part of the start year, current × (start.month−1)/12, and is added to the first simulated year.

**SocialSecurityModel**
- Wage and limit lookups: `start()`, `index_year()`, `average_wage(year, price_level)`, `wage_index(year, price_level)`, `wage_base(year, price_level)`, `earnings_test_limits(year, price_level) -> [f64; 2]`, `lifecycle_wage(profile, age)`.
- `record_earnings(&mut record, birth, year, nominal_earnings, price_level)`: about 13 ns.
- PIA: `bend_points(eligibility_year, awi_at_60) -> [f64; 2]`, `pia_from_bend_points(aime, bends)`, `pia(aime, wage_index_at_62)` (scales the latest published bend points, 2027), `pia_at_eligibility(&record, birth, awi_at_60) -> (aime, pia)` in nominal age-62 dollars, `pia_real(&record, birth, now, price_level) -> f64` in today's dollars (about 19 ns).
- Claiming: `full_retirement_age_months(birth_year)`, `claim_factor(claim_age_months, fra_months)`, `spousal_factor(age_months, fra_months)`, `payable_fraction(year)`.
- `monthly(now, &[Option<Claimant>; MAX_PEOPLE], price_level) -> MonthlyBenefits`: nominal, stateless, about 28 ns.
- `present_value(&claimant, now, discount_rate, survival: &[f64]) -> f64`: today's dollars; `survival[k]` is P(alive k months after now) up to a constant, so pass `&mortality.survival_table()[t..]`. It walks every month (about 0.7 µs).
- `annuity(birth, claim_age, discount_rate, survival_from_start) -> BenefitAnnuity`, then `BenefitAnnuity::value(months_since_start, pia_real)`: the same number from a table lookup. Use it for the monthly funded test.
- Constants: `REAL_WAGE_GROWTH` 0.011, `PROJECTION_INFLATION` 0.024, `MAX_EARNING_YEARS` 60. `impl From<&SocialSecurityBasis> for BenefitBasis`.

**Rules in `monthly`**
- Own benefit = PIA × claim factor from the claim age (clamped to 62–70, whole months).
- Spousal top-up = max(0, 50% of partner PIA − own PIA) × spousal factor at the age it began (the later of the two claims), paid only when both are alive and both have claimed.
- Survivor, from age 60: max(own, base × age factor). Base is the deceased's PIA × claim factor (floor 82.5%) if they had claimed, PIA with delayed credits to death if they died after FRA unclaimed, otherwise PIA. The age factor runs linearly from 71.5% at 60 to 100% at survivor FRA, fixed at the age the benefit began.
- Earnings test before FRA: withheld = min(benefit, (annual_earnings − limit) × ½ / 12); one third above the higher limit in the calendar year FRA is reached.
- Everything is multiplied by `price_level` × `payable_fraction(year)`.

## What the sim loop calls

**Compile, per person**
- `MortalityModel::new`.
- `SocialSecurityModel::new(data, taxes.social_security_haircut, start)`, once per plan.
- `HealthModel::new`, once per household.
- For a `FromEarnings` basis: `EarningsRecord::backfill_profile` (or `backfill_with` using career's curve), with the sum of the person's wage and self-employment income as `current_earnings`.
- Optionally `ss.annuity(birth, claim_age, discount, mortality.survival_table())`.

**Path start**
- `d = mortality.draw_months_to_death(&mut Rng::stream(seed, path, Longevity(i)))`.
- `health.initial_state(sex, mortality.age_months_at_start(), d, &mut health_rng)`. Pass the uncapped `d`.
- Copy the compiled `EarningsRecord`.
- `Claimant.pia_real`: `KnownPia(x)` gives `Some(x)`; `FromEarnings` gives `Some(ss.pia_real(&rec, birth, start, 1.0))`; `None` gives `None`.

**Monthly, per living person**
- `health.step(&mut state, &ctx, &mut health_rng)`. Call it every month from month 0 with `months_since_start = t`: the annual out-of-pocket redraw keys on `t % 12`, and skipping months shifts the stream.
- `ss.monthly(now, &people, price_level)`, once per household.
- Add `withheld_months[i]` to `Claimant.withheld_months`.
- Keep a dead person in the array with `alive = false` and `died = Some(month)`.

**December, per living `FromEarnings` person**
- `ss.record_earnings(&mut rec, birth, year, covered_earnings_nominal, price_level)`, even in zero-earning years: this call snapshots the age-60 wage and the age-62 price.
- Then refresh `Claimant.pia_real = Some(ss.pia_real(&rec, birth, now, price_level))`.

**January or when pay changes**
- Set `Claimant.annual_earnings`.

**FI test**
- `health.expected_annual_cost_at(age, phase, t)`.
- `annuity.value(t, pia_real)`: own benefit only.

## Simplifications

- COLA is continuous: benefit = real PIA × the price level passed in. Delayed credits apply immediately. No dollar rounding of monthly benefits, family maximum, child or divorced-spouse benefits, or 40-credit check.
- The wage base and earnings-test limits track the same-year average wage, not the statutory two-year lag. Bend points and AIME indexing keep the age-60 rule.
- AWI for the start year is the 2025 value grown at its last rate (4.23%); after that it is price level × 1.011^years.
- With December price levels, the gap between the wage snapshot (60) and the price snapshot (before 62) is 1 year, against roughly 1.25 in reality.
- The earnings test withholds evenly over the year rather than whole months from January. The give-back at FRA moves the claim age later by `withheld_months`.
- Survivors take the survivor benefit at 60 or at the death; the reduction for the survivor's age is the only one applied.
- `present_value` and `annuity` cover the own benefit only, with no spousal or survivor value.
- Data-year dollars (2026 employer and Medicare, 2027 ACA, 2025 care costs) are used as today's dollars without rolling to the start month.
- Each adult on employer coverage pays the single-coverage worker share; there is no family premium. Children's health costs are assumed to sit in the USDA child cost.
- Plan tier affects the ACA phase only.
- One care type and one episode per life; no Medicaid, no long-term care insurance.
- The 50% facility spending offset is exposed as `ltc_in_facility` and a constant, for the spending module to apply.
- IRMAA and ACA credits are left to the tax module.
- I added a private one-uniform inverse-normal (Acklam) in health.rs because `Rng::normal` uses a variable number of draws.

## Shared type requests

- model::HealthStatus default: the research proposes 0.75 as the default mortality multiplier for the FIRE population, but HealthStatus::default() is Average, which the model's doc comments define as population mortality (1.0). I mapped Excellent=0.75, Average=1.0, Poor=1.5 per those comments. To adopt the research default, either make Excellent the default or redefine Average as 0.75; the mapping lives in MortalityModel::cohort_curve.
- docs/architecture.md and the lib.rs module table still describe health/benefits as stubs and say health costs are returned in today's dollars; HealthMonth is now nominal (today's dollars x price_level x excess medical growth).
- rng: Rng::normal uses the polar method with a variable number of uniforms. A one-uniform inverse-CDF normal (currently private in health.rs as inverse_normal_cdf plus open_uniform) would suit any module that needs a fixed draw count; consider moving it into rng.
- model::Healthcare: no field for a state or area premium factor, Medicare style (Medigap vs Advantage), care-risk level, or long-term care insurance; the research lists these as knobs. HealthModel::new accepts `state` but ignores it because the data has no state factors.
- model::SocialSecurityBasis::FromEarnings with no current earned income (already retired at start) yields an empty record and a PIA of 0; validate should warn and suggest KnownBenefit.
- tax::TaxInputs::aca_benchmark_premium is annual for the covered people: the sim must sum HealthMonth.aca_benchmark over people and months in the Aca phase, and count those months for aca_months.
- career: EarningsRecord::backfill uses CGM high_school for Typical and college for Steep from params.json. If career::lifecycle_factor maps profiles differently, call EarningsRecord::backfill_with(.., |age| career::lifecycle_factor(profile, age, data)) so past and simulated wages agree.
