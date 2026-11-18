# money module: integration report

ACCOUNTS + DECUMULATION: implemented, gates green (cargo fmt --check; clippy -p firesight-core --all-targets -D warnings; cargo test -p firesight-core: 140 passed; cargo check --workspace ok).

Files (all under /Users/joeyhersh/.t3/projects/firsight-v2/.claude/worktrees/wf_9cc6fa29-24b-5/crates/firesight-core/src/): accounts.rs, accounts/tests.rs, decumulation.rs, decumulation/tests.rs. No other file touched (lib.rs module table still says "stub" for these two; update it when integrating). STUB allow lines removed.

IMPORTANT CAVEAT: tax::compute is todo!() in this worktree, so plan_year (which calls it) has never run against the real tax module. Every plan test goes through plan_year_with(..., &mini_tax), a small test-only tax function driven by a hand-built TaxYearParams. First thing to do after merging tax: run plan_year on a real household.

================ ACCOUNTS (accounts.rs) ================
Units: nominal dollars. Rates/yields are decimals. All types Copy, fixed arrays, no allocation. size_of::<Accounts>() < 1 KiB.

Types (stub types kept; additions marked +):
- Holdings = Weights = [f64; N_ASSETS]; Bucket {Cash, Taxable, Traditional(p), Roth(p), Hsa(p)}
- TaxableBucket {value, basis}; PersonAccounts {traditional, roth, hsa}
- RothBucket {value, contribution_basis, seasoned_conversions, unseasoned:[f64;5], +opened: bool, +years_open: u8}
- Accounts {cash, taxable, person:[PersonAccounts;2], +ytd: ContributionYtd}
- +ContributionYtd {k401_employee, k401_additions, ira, roth_ira, hsa: each [f64;2]}
- CashTerms {tracking_share, spread, fixed_rate} with +annual_rate(tbill_annual) and +monthly_rate(tbill_annual)
- InvestmentIncome {interest, ordinary_dividends, qualified_dividends, fees, +net_change}
- Withdrawal {amount, ordinary, long_term_gain, penalty_base}
- WithdrawContext {owner_age, qualified_medical: f64, penalty_exempt, prefer_overweight: Option<Weights>}
- Realized {long_term_gain, turnover}
- +PersonSavings {present, age, hsa_eligible, traditional_deductible, after_tax_401k}
- +SavingsContext {limits: ContributionLimits, filing_status, magi, earned_income, marginal_rate, retirement_rate, hsa_family, person:[PersonSavings;2]}
- +SavingsFlows {hsa, traditional_ira, roth_ira, after_tax_401k: [f64;2], taxable} with total()
- +PayPeriod {gross_pay, month: u32 (1..=12), age (at year end)}; +PayrollDeferral {employee_pretax, employee_roth, employer}
- pub consts: CONVERSION_SEASONING_YEARS=5, ROTH_ACCOUNT_SEASONING_YEARS=5, EARLY_WITHDRAWAL_AGE=59.5, HSA_PENALTY_FREE_AGE=65, CATCH_UP_AGE=50, HSA_CATCH_UP_AGE=55 (a test checks them against data/tax/us-federal-2026.json).

SIGNATURE CHANGES versus the stub:
1. WithdrawContext.medical: bool -> qualified_medical: f64 (dollars of this HSA withdrawal that are qualified medical; f64::INFINITY = all).
2. WithdrawContext.prefer_overweight keeps its type but now means "the household target mix": the withdrawal sells first what is overweight household-wide (as far as this bucket holds it), then pro rata. None = pro rata to the bucket's holdings.
3. Added fields listed above (RothBucket.opened/years_open, Accounts.ytd, InvestmentIncome.net_change). Anything building these with struct literals needs ..Default::default().
All other stub method signatures are unchanged.

Methods on Accounts:
- from_profile(&Profile) -> Accounts. Pools accounts; no holdings => policy.target; default basis fractions; existing Roth assumed past the 5-year clock (years_open=5). If location==TaxEfficient and no account states holdings, opening holdings are placed tax-efficiently (same household mix, total basis preserved, nothing realised). Does not apply the glide path; the first rebalance does.
- total(Bucket), by_kind() -> [f64;5], investable(), asset_totals() (cash bucket excluded), unrealized_gain(), roth_accessible(p) (basis + seasoned, capped at value)
- apply_returns(&returns, cash_rate_monthly, fee_monthly, &yields_annual) -> InvestmentIncome. value *= max(1+r,0)*(1-fee). Taxable distribution = value*yield/12, reinvested, added to basis. Character: bonds+cash sleeves => interest; RealEstate/Gold/Crypto/Custom => ordinary_dividends; Us/Intl/SingleStock => qualified. Cash bucket interest goes in interest. net_change = exact change in investable() (the dividends are inside it, not on top).
- contribute(bucket, amount, &mix). Raw deposit, no limits. Roth raises contribution_basis; taxable raises basis; unusable mix buys the cash sleeve; bad amounts ignored.
- +buy_mix(amount, &target) -> Weights: underweight-first mix for new money; pass to contribute.
- withdraw(bucket, amount, &ctx) -> Withdrawal. Never overdraws. Taxable: average cost per asset class, gain may be negative, all gains long-term. Traditional: ordinary=amount, penalty_base=amount if age<59.5 and !penalty_exempt. Roth: contributions -> seasoned conversions -> unseasoned oldest first (penalty base, not income, if <59.5) -> earnings (ordinary + penalty if <59.5; ordinary only if >=59.5 and years_open<5; free otherwise). HSA: free up to qualified_medical, rest ordinary, penalty_base if <65.
- convert_to_roth(p, amount) -> converted (ordinary income, never penalised); mix preserved; recorded in unseasoned[4].
- roll_year(): ages tranches (a conversion becomes seasoned at the 5th roll), years_open++, resets ytd.
- +harvest_gains(gain) -> realised gain: raises basis without changing holdings.
- rebalance_to(&target, AssetLocation) -> Realized. Mirror: every bucket to target. TaxEfficient: household mix to target; taxable sells only what exceeds the household target for that asset class plus anything sellable without a gain; sheltered buckets absorb the rest (traditional takes Bonds, RealEstate, Cash, Gold, Custom first; Roth/HSA get the high-growth end; broad stock funds land in taxable). Bucket totals never change.
- max_drift(&target) -> f64
- inherit(deceased, survivor: Option<usize>): spousal rollover incl. Roth basis/tranches and HSA; taxable basis fully stepped up in both cases.
- +hsa_room(person, age, other_age, family, &limits) -> f64; +contribute_hsa(person, amount, room, &mix) -> deposited (use for payroll and employer HSA money so it counts against the limit).
- +save_surplus(surplus, &SavingsContext, &target) -> SavingsFlows. Order: HSA -> IRA -> after-tax 401(k) with in-plan Roth conversion (if after_tax_401k) -> taxable. IRA rule: traditional when traditional_deductible && marginal_rate > retirement_rate; otherwise Roth up to limit*(MAGI phase-out share), remaining deductible room to traditional, else falls through to taxable. IRA total capped at earned_income. surplus must be NEW money (not already in accounts.cash).
- +payroll_deferral(person, &EmployerPlan, &PayPeriod, &limits, &target) -> PayrollDeferral. employee = employee_fraction*pay, or with employee_max the remaining limit / months left; capped by elective limit (+catch-up at 50), by pay and by the 415(c) additions limit. Employer = match_rate * min(employee, match_up_to*pay), per period, no true-up, deposited pre-tax. Deposits are made inside the call.
Free functions: glide_target(&PortfolioPolicy, Option<f64>) -> Weights (TargetDate, BondTent, Custom; None => plain target); cash_terms(&Profile) -> CashTerms.

================ DECUMULATION (decumulation.rs) ================
Spending rules:
- RuleParams {guardrail_sunset_years 15, vanguard_default_rate 0.04, floor_ceiling_default_rate 0.045, vpw_max_rate 0.10, vpw_growth_real_return 0.05, vpw_defensive_real_return 0.019}; Default = named consts; from_data(&Data) reads data/actuarial/params.json (test asserts equality); vpw_real_return(&Weights) -> f64.
- SpendingState unchanged. SpendingInputs gains prior_return_negative: bool and prior_inflation: f64 (Guyton-Klinger withdrawal rule); years_remaining counts the current year. SpendingDecision.adjustment is the relative change of the multiple (-0.1 = cut 10%).
- SpendingPlanner {rule, +params}; new(rule); +from_data(rule, &Data); start(&inputs) -> SpendingState; decide(&mut state, &inputs) -> SpendingDecision. Call start once at retirement, then decide for the first retirement year and every January.
- pub fn vpw_rate(r, years) -> f64 (uncapped; decide caps at 10%).
- Semantics: rules carry last year's spending as a multiple of the budget, so planned budget changes pass through. ConstantReal = budget. PercentOfPortfolio = rate*B + other. Guardrails act on the withdrawal W = m*budget - other with all three GK rules. VanguardDynamic and FloorCeiling target IWR*B + other (the SpendingRule variants carry no percentage, so the household's own initial rate is used; data default if unknown). IWR is fixed in the first year with a positive withdrawal. Spending is never set below min(essential_real, budget_real).

Plan:
- PlanInputs: net_need, ages, rmd, base_tax_inputs, on_aca, sepp_available + penalty_exempt:[bool;2], medical_need, cash_target, cash_floor, in_drawdown.
  net_need = spending + tax due on base_tax_inputs - other income (may be negative). plan_year adds only the EXTRA tax its own withdrawals cause.
- WithdrawalPlan: from_cash, from_taxable, from_traditional, from_roth, from_hsa (medical), roth_conversion, penalized, estimated_tax (whole year), shortfall + from_hsa_other, sepp:[f64;2], cash_refill, excess, gain_harvest, estimated_magi; withdrawals().
- plan_year(&order, early_access, &accounts, &inputs, &tax_params) -> WithdrawalPlan (stub signature unchanged); +plan_year_with(..., compute: &dyn Fn(&TaxYearParams,&TaxInputs)->TaxResult).
- +execute_plan(&plan, fraction, &mut accounts, &inputs, &mut tax_inputs, sell_target: Option<&Weights>) -> Executed {to_cash, from_taxable, from_traditional, from_roth, from_hsa, converted, long_term_gain, penalty_base, shortfall}. Sells, MOVES PROCEEDS INTO accounts.cash, converts, harvests, and adds tax character to tax_inputs. investable() unchanged. from_cash is not moved.
- +cover_shortfall(amount, early_access, &mut accounts, &inputs, &mut tax_inputs, sell_target) -> Executed: unplanned deficit; conventional order, no gross-up, cash bucket untouched, proceeds into accounts.cash.
- +record_withdrawal(&mut TaxInputs, Bucket, &Withdrawal) for any direct withdraw the sim does.
- sepp_payment(balance, age, rate) -> f64 (stub signature): fixed amortisation over single life expectancy. $1M at 50 at 5% = $60,312/yr.
- pub consts: SEPP_MAX_RATE 0.05, ACA_CLIFF_FPL_MULTIPLE 4.0 (both tested against data), ACA_PLANNING_MARGIN 0.02 and HARVEST_MAX_COST 0.001 (planning cushions, not sourced).

Plan logic: (1) RMDs and running SEPP forced; HSA pays medical_need. (2) Order: Conventional = spendable cash, taxable, traditional, Roth. Proportional = spendable cash, then taxable/traditional/Roth pro rata. TaxAware = spendable cash; traditional up to min(bracket room, ACA room); taxable while gains fit under the ACA cliff; Roth; more taxable; more traditional. If funded without costly sources and roth_conversions: convert the remaining bracket room, capped by ACA room and by (spendable cash + taxable)/fill_rate, pro rata across spouses, skipping anyone on SEPP. Then harvest 0% gains if total tax net of ACA credit rises by under 0.1% of the gain. (3) EarlyAccess::Auto, under 59.5 and not exempt: traditional and the non-accessible Roth are held back; then new SEPP up to sepp_payment(balance, age, 5%), then penalised traditional, then Roth earnings/unseasoned. PayPenalty: no restriction. (4) Last: HSA non-medical, cash below the floor, then shortfall. (5) Cash bucket: in_drawdown => all cash above cash_floor spendable, no refill; otherwise only cash above max(cash_target, cash_floor) is spendable and cash_refill = target - cash is added (dropped before any costly source). (6) Gross-up: fixed-point step then secant steps to within one cent.
TaxAware under TaxKind::Flat/None or foreign_tax_rate.is_some() behaves exactly like Conventional.

================ CALL SEQUENCE FOR sim ================
Compile: accounts = Accounts::from_profile(p); terms = cash_terms(p); planner = SpendingPlanner::from_data(rule, data); fee_monthly = (expense_ratio + advisory_fee)/12.
Each month:
 1. Payroll: d = accounts.payroll_deferral(person, &plan, &PayPeriod{gross_pay, month, age}, &limits, &target). Take-home -= employee_pretax + employee_roth; TaxInputs.pretax_deferrals[p] += employee_pretax; employer is not income. HSA payroll/employer: room = hsa_room(..); contribute_hsa(p, amt, room, &buy_mix(amt,&target)); TaxInputs.hsa_contributions[p] += employee payroll part.
 2. Settlement: surplus -> top up accounts.cash to the emergency target (contribute(Bucket::Cash,..)) -> accounts.save_surplus(rest, &ctx, &target). Map to tax: flows.traditional_ira[p] and flows.hsa[p] -> pretax_deferrals[p] (income-tax deductible, not FICA); roth_ira/after_tax_401k/taxable none.
    Deficit: accounts.withdraw(Bucket::Cash,..) first; if still short, cover_shortfall(amount, early_access, &mut accounts, &plan_inputs, &mut year_tax_inputs, Some(&target)), then pay from cash; Executed.shortfall > 0 is depletion.
 3. Retired: execute_plan(&plan, 1.0/12.0, ...) each month, or once with 1.0 in January; spending and tax are paid out of accounts.cash.
 4. inc = accounts.apply_returns(&returns, terms.monthly_rate(tbill_annual), fee_monthly, &yields); add inc.interest / ordinary_dividends / qualified_dividends to the year's TaxInputs; inc.net_change is the conservation-test flow.
 5. Rebalance::Bands: if accounts.max_drift(&target) > width { r = accounts.rebalance_to(&target, location); long_term_gains += r.long_term_gain }.
 6. On death: accounts.inherit(deceased, survivor).
Each January, in this order:
 a. accounts.roll_year().
 b. Index tax params; target = glide_target(&policy, years_to_retirement).
 c. Rebalance::Annual: rebalance_to as above, gain into the new year's TaxInputs.
 d. Retired: inputs.expected_real_return = planner.params.vpw_real_return(&target); decision = planner.decide(&mut state, &inputs) (planner.start first in the retirement year). Scale discretionary lines by decision.budget_multiple.
 e. Build PlanInputs (rmd from tax::rmd_divisor on prior year-end balances; cash_target = retirement_cash_years * spending; sepp_available[p] = last year's plan.sepp[p] while the schedule runs, i.e. until the later of 5 years and 59.5; sim owns that state). plan = plan_year(...). Use plan.estimated_tax for withholding; log plan.roth_conversion / gain_harvest / penalized / shortfall.
 f. plan.excess (RMDs beyond need) is left in cash by execute_plan; sweep it with save_surplus or contribute(Bucket::Taxable).
Year end / April: the year's TaxInputs already hold everything execute_plan, cover_shortfall and record_withdrawal added; call tax::compute once.

================ MEASURED PERFORMANCE (release, one M-series core) ================
apply_returns 52 ns (all 63 slots); withdraw pro rata 10 ns, overweight-first 42 ns; contribute 8 ns; max_drift 6 ns; save_surplus 60 ns; rebalance_to ~145 ns including generating a random target; SpendingPlanner::decide 16 ns. plan_year excluding the cost of the real tax::compute: Conventional 345 ns, Proportional 398 ns, TaxAware 428 ns, with a mean of 4.8 tax calls per TaxAware plan and a maximum of 8 over 1,000 random households. Add (tax calls x cost of the real compute).

================ SIMPLIFICATIONS ================
- One Roth bucket per person for Roth IRA + Roth 401(k); Roth 401(k) deferrals and mega-backdoor money count as contribution basis.
- All taxable gains long-term; average cost per asset class, no lots, no wash-sale or loss-harvest logic.
- Yield/12 simple monthly distribution; fee charged on end-of-month value.
- Full basis step-up at first death.
- Roth IRA phase-out linear, no $10 rounding or $200 minimum. No 60-63 super catch-up, no 401(a)(17) compensation cap, no Roth-catch-up mandate (not in ContributionLimits). Match per period, no true-up.
- Early test uses age at the start of the year (someone turning 59.5 mid-year is restricted all year).
- SEPP life expectancy: IRS Single Life Table at 5-year anchors (ages 20-70) hard-coded FROM MEMORY in decumulation.rs and interpolated; it is not in data/ and must be verified. Rate fixed at 5%.
- ACA: only the 400% FPL cliff is planned around (2% cushion), using federal.fpl_base/fpl_per_person and base_tax_inputs.aca_household_size. Assumes MAGI rises 1:1 with traditional withdrawals and conversions.
- Bracket room found locally by a two-step secant on (TaxResult.taxable_income - qualified dividends - positive LTCG), because room_to_bracket_top was not available; swap it in if its semantics are "traditional dollars to the top of the bracket".
- Guyton-Klinger portfolio-management rule not implemented; use prefer_overweight / sell_target for the same effect.
- PercentOfPortfolio and Vpw add other_income_real on top of the portfolio share, so with a large still-working income the multiple can exceed 1.

================ NEEDED FROM tax THAT THE INTERFACE DOES NOT STATE ================
1. hsa_nonqualified must be both taxed as ordinary income and penalised 20% inside compute; early_distributions must be a penalty base only (its income is already in traditional_distributions / other_ordinary). record_withdrawal relies on both.
2. TaxResult.taxable_income must include preferential income, and magi must be AGI + untaxed Social Security.
3. FederalYear.fpl_base / fpl_per_person must be the guideline for the coverage year; capital_gains[status] must have its 0% bracket first.
4. compute must be cheap and pure: plan_year calls it up to 8 times.

## Shared type requests

- tax::ContributionLimits: add k401_super_catch_up (ages 60-63, 11,250 in 2026, replaces the age-50 catch-up) and compensation_limit_401a17 (360,000); accounts::payroll_deferral currently applies only the age-50 catch-up and no compensation cap.
- tax::ContributionLimits: add the traditional IRA deduction phase-outs (covered single 81k-91k, MFJ 129k-149k, spouse-covered 242k-252k) or a helper ira_deductible_fraction(magi, status, covered) so sim can set PersonSavings.traditional_deductible without hard-coding law.
- tax::TaxInputs: add an above-the-line deduction field (ira_deduction, or a generic adjustments) for deductible traditional IRA and non-payroll HSA contributions. Workaround in use: sim adds SavingsFlows.traditional_ira and SavingsFlows.hsa to pretax_deferrals (income-tax deductible, not FICA).
- tax::TaxInputs: document or split hsa_nonqualified (must be taxed as ordinary income AND penalised 20% inside compute) and confirm early_distributions is a penalty base only; decumulation::record_withdrawal relies on both. A separate field for taxable Roth earnings would be cleaner than other_ordinary.
- tax: confirm room_to_bracket_top(&TaxYearParams, &TaxInputs, rate) returns the additional traditional-distribution dollars that bring ordinary taxable income to the top of the highest bracket taxed at or below `rate`; decumulation::plan_year_with currently computes this itself with two extra compute calls and can switch to it.
- data/tax/us-federal-2026.json: add the IRS Single Life Table (26 CFR 1.401(a)(9)-9(b)) under early_withdrawal.sepp_72t; decumulation.rs hard-codes 5-year anchors for ages 20-70 (65.0, 60.2, 55.3, 50.5, 45.7, 41.0, 36.2, 31.6, 27.1, 22.9, 18.8) from memory and they need verifying against the regulation.
- model::SpendingRule::VanguardDynamic and FloorCeiling carry no target percentage; decumulation uses the household's initial withdrawal rate (data default_p only when it is unknown). Add an optional `rate` field if users should be able to set it.
- model::SpendingRule::Guardrails: optional inflation_cap (Guyton 2004, 6%) is described in the research data but has no field; not implemented.
- accounts::WithdrawContext.medical: bool was replaced by qualified_medical: f64, and accounts::Accounts gained `ytd: ContributionYtd`, RothBucket gained `opened`/`years_open`, InvestmentIncome gained `net_change`; any other module constructing these with struct literals must add ..Default::default().
- lib.rs module table still lists accounts and decumulation as interface stubs; update the doc comment when integrating (not edited here per the ownership rule).
