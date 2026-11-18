# tax module: integration report

TAX MODULE: implemented, all gates green (cargo fmt; clippy -p firesight-core --all-targets -D warnings; cargo test -p firesight-core: 142 lib tests + 1 integration test pass, 1 ignored bench; cargo check --workspace passes).

FILES (all under /Users/joeyhersh/.t3/projects/firsight-v2/.claude/worktrees/wf_9cc6fa29-24b-1/crates/firesight-core/)
- src/tax.rs was turned into src/tax/: mod.rs (types, public API), law.rs (from_data, for_year), calc.rs (compute and helpers; the simplification list is in its header), explain.rs, tests.rs.
- NEW file outside the stub: tests/tax_no_alloc.rs. It is a separate test binary because it installs a counting #[global_allocator]; it asserts zero heap allocations across for_year, compute, room_to_bracket_top, room_to_aca_cliff, effective_marginal_rate, irmaa_monthly_surcharge and rmd_divisor.
- Not touched: lib.rs, data.rs, model/, Cargo.toml. The lib.rs and data.rs doc tables still call tax a stub/TODO; that text is now stale.

SIGNATURE CHANGES AGAINST THE STUB
1. TaxLaw::for_year(&self, index: Indexation, state: StateCode) -> TaxYearParams. Was (cpi_factor: f64, state). The calendar year is needed for the SALT schedule, senior-deduction sunset, IRMAA top-tier freeze and ACA cliff flag; wages, the poverty line and Medicare premiums need their own factors.
2. StateYear.personal_exemption is now [f64; 3] (amount for the whole return, by status); it was a per-filer f64. The data gives single/joint amounts that are not always 1:2.
3. TaxInputs now derives Serialize + Deserialize with #[serde(default)], which unblocks wasm taxDetail. StateCode serialises as a two-letter string ("--" for none, case-insensitive on input) and gained PartialOrd/Ord.
4. rmd_start_age now reads the data: 70 (for the historical 70.5) for births up to 1949, 72 for 1950, 73 for 1951-1959, 75 for 1960+. rmd_divisor(age, birth_year) -> Option<f64> is unchanged in signature; ages below 72 but past the start age use the first table row, ages above 120 the last.
Everything else in the stub keeps its name and type. Added fields are listed below.

PUBLIC API (crate::tax)
- pub struct Indexation { year: i32, cpi_factor: f64, wage_factor: f64, fpl_factor: f64, health_factor: f64 } (Copy). Constructors: Indexation::base(year) (all factors 1.0); Indexation::from_cpi(base_year, year, cpi_factor) (wage = cpi * 1.01^(year-base_year), fpl = health = cpi); builders with_wage_factor / with_fpl_factor / with_health_factor. pub const DEFAULT_REAL_WAGE_GROWTH: f64 = 0.01.
- TaxLaw::from_data(&Data, &TaxModel) -> Result<TaxLaw, DataError>. Fails on the first missing or malformed key with file and JSON path, e.g. "fica.oasdi_wage_base: missing". Validates rates in [0,1], bracket order, table contiguity, and states meta.year == federal tax_year.
- TaxLaw pub fields: base_year (2026), kind, index_brackets, published_next_year: Indexation, rmd: RmdTable.
- TaxLaw::index_from_cpi(year, cpi_factor) -> Indexation; knows_state(StateCode) -> bool; property_tax_rate(StateCode) -> f64 (unweighted mean of the 51 jurisdictions, 0.00896, for NONE or unknown); local_rate(StateCode, Option<&str> city) -> f64.
- TaxYearParams gained `year: i32` and `with_foreign_rate(self, Option<f64>) -> TaxYearParams`. for_year always returns foreign_tax_rate None; set it for a household abroad.
- pub fn compute(&TaxYearParams, &TaxInputs) -> TaxResult
- pub fn explain(&TaxYearParams, &TaxInputs) -> TaxDetail; TaxDetail::amount(key) -> Option<f64>
- pub fn room_to_bracket_top(&TaxYearParams, &TaxInputs, rate: f64) -> f64
- pub fn room_to_aca_cliff(&TaxYearParams, &TaxInputs) -> f64
- pub fn effective_marginal_rate(&TaxYearParams, &TaxInputs) -> f64
- pub fn irmaa_tier(&TaxYearParams, FilingStatus, magi) -> usize; pub fn irmaa_monthly_surcharge(&TaxYearParams, FilingStatus, magi) -> f64 (Part B + D, per person per month)
- pub fn rmd_divisor(age: u32, birth_year: i32) -> Option<f64>; pub fn rmd_start_age(birth_year: i32) -> u32. Both read Data::embedded() through a OnceLock; TaxLaw.rmd holds the same table for a custom Data.
- pub fn index_rounded(base, factor, increment) -> f64; Brackets::indexed_rounded; FilingStatus::ALL and ::key(); AcaYear::applicable_percentage(ratio); new types AcaYear, IrmaaTable, RmdTable.

CALL SEQUENCE
1. Once per profile: law = TaxLaw::from_data(data, &profile.assumptions.taxes)?.
2. Each January: p = law.for_year(index, state). If abroad: p = p.with_foreign_rate(location.foreign_tax_rate). Optionally p.state.local_rate = law.local_rate(state, city); the default is the statewide average.
3. Planning: room_to_bracket_top(&p, &base_inputs, fill_rate), room_to_aca_cliff(&p, &base_inputs), then compute(&p, &inputs) for the withholding estimate.
4. Year end / April: compute(&p, &actual_inputs); for a trace, explain(&p, &inputs).
TaxInputs.state must equal p.state.code (debug_assert), unless the params are NONE. People are counted as present where ages[i] > 0; the 65+ tests use ages[i] >= 65.

HOW for_year EXPECTS THE FACTORS (all relative to tax year 2026 = 1.0)
- cpi_factor: price level for brackets, standard and additional deductions, LTCG thresholds, QBI thresholds, CTC amounts, 401k/IRA/HSA limits, Roth phase-outs, IRMAA tiers. Statute uses the 12 months to August of the prior year, so pass simulated CPI at the start of tax year Y (or the prior autumn) over the same point for 2026.
- First simulated January: law.published_next_year holds the factors implied by the official 2027 figures (year 2027, cpi 1.03243, wage 1.04228, fpl 1.01981, health 1.03204). Indexing 2026 by it gives exactly the published 2027 wage base 192,300, single standard deduction 16,600, CTC 2,300, 401k 25,500, and the other brackets within one $50 step. Suggested chain: factor(Y) = published_next_year.cpi_factor * simCPI(Jan Y)/simCPI(Jan 2027). Published 2027 tables themselves are not used.
- wage_factor: average wage level, used only for the OASDI wage base (nearest $300). Bend points belong to benefits.
- fpl_factor: the poverty guideline lags a further year; pass the previous year's cpi_factor. 2026 coverage uses the 2025 guideline 15,650 + 5,500; AK and HI are selected from the state code.
- health_factor: scales the IRMAA surcharge dollars (Medicare premium growth).
- index_brackets == false forces cpi and wage factors to 1.0 (TaxYearParams.cpi_factor reports 1.0). The poverty line and IRMAA surcharges still move, and calendar rules still apply.
- Rounding: index_rounded rounds to the NEAREST statutory increment, not down, because only the already-rounded base is known and rounding down again would bias low. A factor of exactly 1.0 returns the published value untouched (201,775 is not a multiple of 50). Increments: $50 brackets/deductions/LTCG/QBI/HSA, $100 CTC and IRA catch-up, $500 401k and IRA, $1,000 415(c), Roth phase-outs and IRMAA tiers, $300 wage base. Poverty line and state amounts are not rounded.
- Frozen nominal: NIIT and additional Medicare thresholds, Social Security taxation thresholds, CTC phase-out thresholds, $500 other-dependant credit, $2,500 earned floor, $3,000 capital loss limit, senior deduction amount and thresholds, QBI phase-in width, non-itemiser charity amounts, SALT floor, HSA catch-up, penalty rates.
- Calendar: SALT cap 40,400/505,000 (2026), 40,804 (2027), 41,212.04 (2028), 41,624.16 (2029), flat 10,000 with no phase-down from 2030. Senior deduction is 0 outside 2025-2028. IRMAA top tier is frozen through 2027, then indexed from the 2026 base (overstates it by about one year of inflation). ACA cliff flag from the data by year; the last known flag persists.
- States: brackets, exemptions/credits and SS exemption thresholds scale with cpi only when the state's inflation_indexed is true; standard deduction only when its own flag is true; follows_federal states take the indexed federal standard deduction for all three statuses. WA excise deduction is indexed, its $1M break is not. Unknown state code gives NONE params (no state tax).

UNITS
Nominal dollars of the tax year throughout; rates are decimals. aca_benchmark_premium is the ANNUAL benchmark premium for the covered people; the credit is scaled by aca_months/12. mortgage_interest must already be limited to the debt ceiling by the caller. irmaa_surcharge is annual, for everyone flagged on_medicare, using this year's thresholds and this year's MAGI. For a correct two-year lookback call irmaa_monthly_surcharge(&params_of_premium_year, status, magi_from_two_years_ago).

TaxResult ADDED FIELDS
local (already inside state), aca_fpl_ratio, irmaa_tier: u32, magi_irmaa (= AGI; no tax-exempt interest is modelled), capital_loss_carryforward (positive; feed next year as negative short_term_gains), rental_loss_suspended (positive; feed next year as negative rental_net).
- magi is the ACA MAGI (AGI + untaxed Social Security). Do not use it for IRMAA.
- deduction excludes the senior and QBI deductions.
- total = federal ordinary + capital gains - credits + NIIT + sum(fica) + sum(se_tax) + state + penalties + foreign. It can be negative (refundable child credit). The ACA credit and IRMAA are not in total.
- fica[i] and se_tax[i] each include that person's pro-rata share of the 0.9% additional Medicare tax.
- marginal_ordinary_rate is the federal bracket rate, 0 while ordinary income is below the deductions.

FederalYear ADDED FIELDS
senior_age, senior_deduction, senior_phaseout[3], senior_phaseout_rate, salt_phase_down_threshold (infinite when none), salt_phase_down_rate, salt_floor, charitable_floor_rate, charitable_non_itemizer[3], itemized_limit_fraction, capital_loss_limit, se_net_earnings_factor, se_oasdi_rate, se_medicare_rate, se_minimum_earnings, se_deduction_fraction, qbi_rate, qbi_threshold[3], qbi_phase_in_width[3], child_credit_refundable_max, child_credit_earned_floor, child_credit_refundable_rate, other_dependent_credit, child_credit_phaseout_per_1000, ss_max_taxable_fraction, hsa_penalty, aca: AcaYear, irmaa: [IrmaaTable; 3]. ContributionLimits gained k401_catch_up_60_63.

StateYear ADDED FIELDS
standard_deduction_phaseout_start/end[3], standard_credit[3] with phase-out start and rate, allows_itemizing, personal_credit[3], dependent_credit, ss_exempt_below_agi[3], ss_phase_out_end_agi[3], ss_phase_out_step, ss_phase_out_fraction_per_step, ss_max_taxable_fraction_of_benefits, ss_full_exemption_age, ss_exemption_requires_fra, full_retirement_age, capital_gains_exclusion_fraction/amount, capital_gains_surtax_rate and agi threshold, investment_surtax_rate and threshold, capital_gains_excise: Brackets and its deduction, local_rate. retirement_exclusion is f64::INFINITY for states that do not tax retirement income.

PLANNING HELPERS
- room_to_bracket_top returns the amount of extra ordinary INCOME (probed as roth_conversions) before federal ordinary taxable income passes the top of the bracket taxed at `rate`. It accounts for extra taxable Social Security and shrinking deductions. Returns the largest amount that stays at or below the top, to half a cent; 0 if already above; infinity for the top bracket, a rate not in the schedule, and flat/none/foreign modes.
- room_to_aca_cliff = max(0, 4.0 * FPL(household) - ACA MAGI), exact, since every kind of income moves ACA MAGI one for one. Infinity when nobody is on the marketplace, when the law has no cliff, or in flat modes. Exactly 400% keeps the credit (half-cent tolerance); one dollar more loses it.
- effective_marginal_rate = change in (total - ACA credit) per dollar over the next $100 of other_ordinary; includes state tax, the Social Security phase-in and phase-outs; can exceed 1 at the cliff; excludes IRMAA.

explain()
Ordered lines with stable keys: wages, self_employment, pretax_contributions, investment_income, capital_gains, retirement_income, rental_income, other_income, half_se_tax_deduction, social_security_benefits, taxable_social_security, agi, deduction (label "Standard deduction" or "Itemized deductions"), senior_deduction, qbi_deduction, taxable_income, federal_ordinary_tax (note e.g. "12,400 at 10% + 38,000 at 12% + 33,500 at 22%"), federal_capital_gains_tax (stacking note), child_credits, federal_income_tax, niit, fica_0/fica_1, se_tax_0/se_tax_1, additional_medicare_tax (informational, already inside fica/se), early_withdrawal_penalty, hsa_penalty, state_taxable_income, state_income_tax, state_capital_gains_excise, local_income_tax, total, aca_premium_credit, irmaa_surcharge. Zero-amount optional lines are omitted. The income lines sum to AGI and the tax lines sum to total (tested). Flat/none/foreign modes give income, flat_tax | foreign_tax | no_tax, total.

MODELLING DECISIONS AND SIMPLIFICATIONS (please review)
- AMT ignored. No tax-exempt interest, kiddie tax, MFS status, blindness deduction, or child-care, saver's, education, EITC or foreign tax credits. Collectibles and section 1250 gain are taxed as ordinary long-term gains.
- QBI is implemented for a sole proprietor without employees: 20% of (profit less half SE tax less the deferral that wages could not absorb), capped at 20% of (taxable income less preferential income), phased out linearly to zero across the phase-in range (single 201,750 to 276,750).
- Rental losses: IRC 469(i) $25,000 allowance, phased out at 50% above $100,000. These three numbers are named constants in calc.rs because they are not in the data.
- Capital gains: short and long are netted; a net loss offsets ordinary income up to $3,000. NII = interest + all dividends + net gains + allowed rental.
- Senior deduction, CTC phase-out, NIIT and the SALT phase-down all use AGI as MAGI.
- Itemised = min(SALT paid, cap after phase-down) + mortgage interest + gifts above 0.5% of AGI, less 2/37 of min(itemised, AGI - senior deduction - start of the 37% bracket). QBI is left out of that base; under this model nobody in the 37% bracket has QBI. Non-itemisers add up to $1,000/$2,000 of gifts to the standard deduction.
- SALT uses state_tax_paid if > 0, otherwise the state plus local income tax compute() itself calculates, plus property_tax.
- CTC: $50 per $1,000 or part of AGI over 200k/400k. Refundable part = min(remaining credit, 1,700 per child, 15% of (wages + SE profit - half SE tax - 2,500)). The 15% is a named constant in law.rs; the data has it only in words. Because of this phase-in, total is NOT monotone in wages for low earners with children. That is the only such case found; the monotonicity tests run without children.
- ACA: the credit is zero above 400% FPL (data flag). Below the statutory 100% floor the credit is NOT refused; it is priced from the first table band (2.10%) as a stand-in for Medicaid or a small income top-up. aca_fpl_ratio is reported so health/sim can treat ratios below 1.0 (or below 1.38) differently. The applicable-percentage table is held at its 2026 values in every year. The table itself steps from 2.10% to 3.14% at 133% FPL, so the credit drops by up to about $300 there.
- Flat, None and Foreign modes: base = max(0, wages + SE - deferrals + interest + dividends + net gains after the loss limit + distributions + conversions + pensions + other + rental + ALL Social Security); tax = rate * base, reported in federal_ordinary (Flat) or foreign (Foreign). No FICA, penalties, ACA credit or IRMAA. agi = magi = taxable_income = base. TaxKind::None overrides a foreign rate; Flat overrides it too. Contribution limits are still filled.
- States: head of household uses the single schedule. Deduction = max(state standard deduction, mortgage interest + property tax + gifts) wherever the state's standard deduction is of type "deduction"; this is my rule, the data has no itemising flag. Modelled: personal/dependant exemptions or credits; SS rules for CO (age 65), CT (25% cap above the AGI test), MN (stepped), NM (cliff), RI (FRA + AGI), VT (linear), MT (follows federal); UT is treated as follows-federal with no SS credit. Retirement exclusion = per-person amount times people at or above min_age, applied to traditional distributions + pensions (not conversions). States with taxed == "none" exempt them once anyone is 59.5 or older. LTCG: exclusions (AR, AZ, ND, SC, WI, NM, MO), rate cap (HI, MT), MD 2% surtax on all net gains above 350k AGI, MN 1% on NII over $1M, WA excise. SC's deduction phase-out and UT's credit phase-out are modelled. Local tax = statewide average rate times state-adjusted income, counted in `state`.
- States NOT modelled: NY/CT recapture, other exemption phase-outs (AL, WI, MN, RI, MD, CT), income limits on retirement exclusions, AR alternate table. CA's $1M mental-health threshold is scaled with the rest of the indexed schedule although the statute freezes it.

MEASURED PERFORMANCE (release, one M-series core, #[ignore] test tax::tests::bench_compute)
compute(): 76 ns (two-earner NY itemiser) and 72 ns (retired couple). room_to_bracket_top: 157 ns. for_year: 214 ns. Zero heap allocations (tests/tax_no_alloc.rs).

TESTS (62 in tax/, plus the allocation test)
- Rev. Proc. 2025-32 cumulative-tax constants at every bracket boundary for single, joint and HoH.
- Hand-computed whole returns: single $100k (total 20,820), joint two earners with 401k and two children (42,500), HoH.
- FICA wage base and additional Medicare; SE tax (14,129.55 on $100k), with wages above the base, the $400 floor and a loss; QBI cap and phase-out.
- LTCG stacking across the 0% and 15% thresholds; gains absorbing unused deduction; loss netting and carryforward.
- Social Security taxation at six provisional incomes single and one joint; senior deduction phase-out and 2029 sunset.
- NIIT; CTC phase-out and refundable limit; penalties; itemised vs standard, SALT phase-down and schedule, 2/37 limitation; passive-loss allowance.
- ACA at 150/175/250/399/400/401% FPL, partial year, two people, AK/HI, no-cliff regime; IRMAA tier boundaries.
- CA $500k single, NY $600k joint, TX and every no-wage-tax state; PA, IL, CO, VT, MN, CT, UT, SC, WA, WI, MD, HI rules; a sanity sweep over all 51 jurisdictions.
- Indexing: doubling, rounding, frozen items, calendar rules, index_brackets off, bracket-creep invariance.
- Properties: total is non-decreasing and moves by less than the $250 step over wage sweeps to $1M in 9 states x 3 statuses; the ACA credit only shrinks, with exactly one drop above $500, at 400% FPL; total is non-decreasing in withdrawals through the SS phase-in in 7 states.
- Zero, negative and billion-dollar inputs; explain consistency and exact note strings; JSON round trip; RMD table; loud failures on six kinds of broken data.

## Shared type requests

- sim/decumulation: TaxLaw::for_year now takes (index: tax::Indexation, state: StateCode) instead of (cpi_factor: f64, state). Build the Indexation with law.index_from_cpi(year, cpi_factor) or Indexation { year, cpi_factor, wage_factor, fpl_factor, health_factor }; use law.published_next_year for the first simulated January (2027).
- sim: location.foreign_tax_rate is not applied by for_year; call params.with_foreign_rate(location.foreign_tax_rate) when the household is outside the US, and pass StateCode::NONE as the state.
- sim: feed TaxResult.capital_loss_carryforward into next year's TaxInputs.short_term_gains (negative) and TaxResult.rental_loss_suspended into next year's rental_net (negative); otherwise those losses are dropped.
- sim/health: use TaxResult.magi only for ACA. For IRMAA use TaxResult.magi_irmaa from two years earlier with tax::irmaa_monthly_surcharge(&params_of_premium_year, status, magi); TaxResult.irmaa_surcharge is a same-year convenience figure (annual, all on_medicare people).
- health: TaxInputs.aca_benchmark_premium must be the ANNUAL benchmark (second-lowest silver) premium for all covered people; compute() scales the credit by aca_months/12. Incomes below 100% FPL still get a credit priced from the first table band (Medicaid stand-in); check TaxResult.aca_fpl_ratio if you want different behaviour.
- tax::StateYear.personal_exemption changed from f64 (per filer) to [f64; FilingStatus::COUNT] (amount for the whole return). No other module referenced it at the time of the change.
- lib.rs module table and data.rs wiring table still describe tax as a stub / TODO; whoever owns those files should update the text (no code change needed).
- wasm: TaxInputs now implements Serialize + Deserialize (serde default on every field; state is a two-letter string), so tax_detail in crates/firesight-wasm/src/lib.rs can be wired as TaxLaw::from_data -> for_year -> tax::explain.
- data/tax/us-states-2026.json: Utah's taxpayer-credit phase-out (1.3 cents per dollar above $18,213 single / $36,426 joint) exists only in the UT notes text; it is hard-coded as named constants in tax/law.rs. Please add structured fields (e.g. standard_deduction.credit_phase_out {rate, single, married_joint}). An explicit allows_itemizing flag per state and a flag for whether local income tax applies to wages only or to all income would remove two assumptions.
- data/tax/us-federal-2026.json: the refundable child credit phase-in rate (15%) appears only inside the refundable_formula sentence, and the IRMAA top-tier freeze year (2027) and the $1,000 IRMAA rounding only in prose; they are named constants in tax/law.rs. The passive-loss allowance (IRC 469(i): $25,000, 50% phase-out above $100,000 MAGI) is not in the data at all and is three named constants in tax/calc.rs. Structured keys for these would let the constants go.
- model/TaxInputs: there is no field for tax-exempt interest, so IRMAA MAGI equals AGI and ACA MAGI omits it; add one if municipal bonds are ever modelled.
