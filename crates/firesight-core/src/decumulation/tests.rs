use super::*;
use crate::model::{Allocation, N_ASSETS};
use crate::rng::Rng;
use crate::tax::FilingStatus;

#[track_caller]
fn assert_close(a: f64, b: f64) {
    assert!(
        (a - b).abs() <= 1e-9 * (1.0 + a.abs().max(b.abs())),
        "{a} != {b}"
    );
}

#[track_caller]
fn assert_near(a: f64, b: f64, tolerance: f64) {
    assert!((a - b).abs() <= tolerance, "{a} != {b} (+-{tolerance})");
}

fn mix(pairs: &[(AssetClass, f64)]) -> Weights {
    Allocation::from_pairs(pairs).normalized().unwrap()
}

fn sixty_forty() -> Weights {
    mix(&[(AssetClass::UsStocks, 60.0), (AssetClass::Bonds, 40.0)])
}

// ---------------------------------------------------------------- spending rules

#[test]
fn rule_parameters_match_the_research_data() {
    let data = Data::embedded();
    assert_eq!(RuleParams::from_data(data), RuleParams::default());
    let rules = &data.actuarial["withdrawal_rules"];
    // The profile defaults are the published band values.
    let t = &rules["guyton_klinger"]["thresholds"];
    assert_eq!(t["upper_guardrail"].as_f64(), Some(1.2));
    assert_eq!(t["lower_guardrail"].as_f64(), Some(0.8));
    assert_eq!(t["cut"].as_f64(), Some(0.1));
    assert_eq!(
        rules["vanguard_dynamic_spending"]["ceiling"].as_f64(),
        Some(0.05)
    );
    assert_eq!(
        rules["vanguard_dynamic_spending"]["floor"].as_f64(),
        Some(0.025)
    );
    let fed = &data.tax_federal;
    assert_eq!(
        fed["early_withdrawal"]["sepp_72t"]["interest_rate_floor_cap"].as_f64(),
        Some(SEPP_MAX_RATE)
    );
    assert_eq!(
        fed["aca"]["premium_tax_credit"]["eligibility_max_pct_fpl"].as_f64(),
        Some(ACA_CLIFF_FPL_MULTIPLE)
    );
}

fn spend(budget: f64, portfolio: f64) -> SpendingInputs {
    SpendingInputs {
        budget_real: budget,
        essential_real: 0.0,
        portfolio_real: portfolio,
        other_income_real: 0.0,
        years_remaining: 30.0,
        expected_real_return: 0.0376,
        prior_return_negative: false,
        prior_inflation: 0.0,
    }
}

/// Starts a rule at a 4% initial rate (40k from 1M) and takes the first year.
fn started(rule: SpendingRule) -> (SpendingPlanner, SpendingState) {
    let planner = SpendingPlanner::new(rule);
    let first = spend(40_000.0, 1_000_000.0);
    let mut state = planner.start(&first);
    assert_close(state.initial_withdrawal_rate, 0.04);
    assert_eq!(state.years_retired, 0);
    let d = planner.decide(&mut state, &first);
    assert_eq!(d.adjustment, 0.0);
    assert_eq!(state.years_retired, 1);
    (planner, state)
}

#[test]
fn constant_real_spends_the_budget_whatever_happens() {
    let (planner, mut state) = started(SpendingRule::ConstantReal);
    for portfolio in [2_000_000.0, 300_000.0, 0.0] {
        let d = planner.decide(&mut state, &spend(40_000.0, portfolio));
        assert_eq!(d.spending_real, 40_000.0);
        assert_eq!(d.budget_multiple, 1.0);
        assert_eq!(d.adjustment, 0.0);
    }
    assert_eq!(state.years_retired, 4);
    // A planned budget change is simply followed.
    let d = planner.decide(&mut state, &spend(30_000.0, 1e6));
    assert_eq!(
        (d.spending_real, d.budget_multiple, d.adjustment),
        (30_000.0, 1.0, 0.0)
    );
}

#[test]
fn percent_of_portfolio_follows_the_market() {
    let planner = SpendingPlanner::new(SpendingRule::PercentOfPortfolio { rate: 0.04 });
    let mut state = planner.start(&spend(40_000.0, 1_000_000.0));
    let d = planner.decide(&mut state, &spend(40_000.0, 1_000_000.0));
    assert_close(d.spending_real, 40_000.0);
    let d = planner.decide(&mut state, &spend(40_000.0, 750_000.0));
    assert_close(d.spending_real, 30_000.0);
    assert_close(d.budget_multiple, 0.75);
    assert_close(d.adjustment, -0.25);
    // Other income comes on top of the portfolio's 4%.
    let mut with_income = spend(40_000.0, 750_000.0);
    with_income.other_income_real = 12_000.0;
    let d = planner.decide(&mut state, &with_income);
    assert_close(d.spending_real, 42_000.0);
    // Essentials are never cut by the rule.
    let mut floor = spend(40_000.0, 100_000.0);
    floor.essential_real = 25_000.0;
    let d = planner.decide(&mut state, &floor);
    assert_close(d.spending_real, 25_000.0);
    assert_close(d.budget_multiple, 0.625);
}

fn guardrails() -> SpendingRule {
    SpendingRule::Guardrails {
        upper: 0.2,
        lower: 0.2,
        adjust: 0.1,
    }
}

#[test]
fn guyton_klinger_cut_hold_and_raise() {
    // IWR 4%: the guardrails sit at withdrawal rates of 4.8% and 3.2%.
    let (planner, mut state) = started(guardrails());

    // Year 2: portfolio 800k. 40k / 800k = 5.0% > 4.8%: capital preservation, -10%.
    let d = planner.decide(&mut state, &spend(40_000.0, 800_000.0));
    assert_close(d.spending_real, 36_000.0);
    assert_close(d.budget_multiple, 0.9);
    assert_close(d.adjustment, -0.1);

    // Year 3: 760k. 36k / 760k = 4.74%: inside the band, the cut stays in force.
    let d = planner.decide(&mut state, &spend(40_000.0, 760_000.0));
    assert_close(d.spending_real, 36_000.0);
    assert_eq!(d.adjustment, 0.0);

    // Year 4: 700k. 36k / 700k = 5.14%: a second cut, to 32.4k.
    let d = planner.decide(&mut state, &spend(40_000.0, 700_000.0));
    assert_close(d.spending_real, 32_400.0);
    assert_close(d.budget_multiple, 0.81);

    // Year 5: recovery to 1.1M. 32.4k / 1.1M = 2.95% < 3.2%: prosperity, +10%.
    let d = planner.decide(&mut state, &spend(40_000.0, 1_100_000.0));
    assert_close(d.spending_real, 35_640.0);
    assert_close(d.adjustment, 0.1);

    // Year 6: exactly on the lower guardrail is not below it: no change.
    let d = planner.decide(&mut state, &spend(40_000.0, 35_640.0 / 0.032));
    assert_close(d.spending_real, 35_640.0);
    assert_eq!(state.years_retired, 6);
    assert_close(state.initial_withdrawal_rate, 0.04);
}

#[test]
fn guyton_klinger_inflation_rule_and_sunset() {
    // After a losing year with the rate above the initial 4%, the 3% inflation raise
    // is skipped: real spending falls to 40,000 / 1.03 = 38,834.95. That is 4.57% of
    // 850k, inside the band, so no further cut.
    let (planner, mut state) = started(guardrails());
    let mut year = spend(40_000.0, 850_000.0);
    year.prior_return_negative = true;
    year.prior_inflation = 0.03;
    let d = planner.decide(&mut state, &year);
    assert_close(d.spending_real, 40_000.0 / 1.03);

    // Negative return but the rate is not above 4%: the raise is given.
    let (planner, mut state) = started(guardrails());
    let mut year = spend(40_000.0, 1_050_000.0);
    year.prior_return_negative = true;
    year.prior_inflation = 0.03;
    assert_close(planner.decide(&mut state, &year).spending_real, 40_000.0);

    // Skip and cut together: 38,834.95 / 780k = 4.98% > 4.8%.
    let (planner, mut state) = started(guardrails());
    let mut year = spend(40_000.0, 780_000.0);
    year.prior_return_negative = true;
    year.prior_inflation = 0.03;
    assert_close(
        planner.decide(&mut state, &year).spending_real,
        40_000.0 / 1.03 * 0.9,
    );

    // In the last 15 years the capital preservation rule is off; prosperity is not.
    let (planner, mut state) = started(guardrails());
    let mut late = spend(40_000.0, 500_000.0);
    late.years_remaining = 15.0;
    assert_close(planner.decide(&mut state, &late).spending_real, 40_000.0);
    late.years_remaining = 16.0;
    assert_close(planner.decide(&mut state, &late).spending_real, 36_000.0);
    late.years_remaining = 5.0;
    late.portfolio_real = 2_000_000.0;
    assert_close(planner.decide(&mut state, &late).spending_real, 39_600.0);
}

#[test]
fn guardrails_act_on_the_withdrawal_and_pass_budget_changes_through() {
    // Budget 60k, of which 20k is covered by a pension: the withdrawal is 40k (4%).
    let planner = SpendingPlanner::new(guardrails());
    let mut first = spend(60_000.0, 1_000_000.0);
    first.other_income_real = 20_000.0;
    let mut state = planner.start(&first);
    assert_close(state.initial_withdrawal_rate, 0.04);
    planner.decide(&mut state, &first);
    // 40k / 800k = 5%: the 10% cut applies to the withdrawal: 36k + 20k.
    let mut year = first;
    year.portfolio_real = 800_000.0;
    let d = planner.decide(&mut state, &year);
    assert_close(d.spending_real, 56_000.0);
    // The mortgage ends: budget 45k. Planned spending is the same multiple of the new
    // budget (42k), a 22k withdrawal = 2.75% of 800k, under 3.2%: prosperity +10%.
    year.budget_real = 45_000.0;
    let d = planner.decide(&mut state, &year);
    assert_close(d.spending_real, 22_000.0 * 1.1 + 20_000.0);

    // Retiring with a partner still earning: no withdrawal yet, so the reference
    // rate is fixed in the first year the portfolio is actually drawn on.
    let mut covered = spend(50_000.0, 1_000_000.0);
    covered.other_income_real = 80_000.0;
    let mut state = planner.start(&covered);
    assert_eq!(state.initial_withdrawal_rate, 0.0);
    assert_close(planner.decide(&mut state, &covered).spending_real, 50_000.0);
    assert_eq!(state.initial_withdrawal_rate, 0.0);
    covered.other_income_real = 10_000.0;
    covered.portfolio_real = 800_000.0;
    assert_close(planner.decide(&mut state, &covered).spending_real, 50_000.0);
    assert_close(state.initial_withdrawal_rate, 0.05);
}

#[test]
fn vanguard_dynamic_ceiling_and_floor() {
    let (planner, mut state) = started(SpendingRule::VanguardDynamic {
        ceiling: 0.05,
        floor: 0.025,
    });
    // 4% of 1.2M is 48k, but the raise is capped at +5%: 42k.
    let d = planner.decide(&mut state, &spend(40_000.0, 1_200_000.0));
    assert_close(d.spending_real, 42_000.0);
    assert_close(d.adjustment, 0.05);
    // 4% of 900k is 36k, but the cut is limited to -2.5% of 42k: 40,950.
    let d = planner.decide(&mut state, &spend(40_000.0, 900_000.0));
    assert_close(d.spending_real, 40_950.0);
    assert_close(d.adjustment, -0.025);
    // 4% of 1.03M = 41.2k lies between 39,926 and 42,997: taken as is.
    let d = planner.decide(&mut state, &spend(40_000.0, 1_030_000.0));
    assert_close(d.spending_real, 41_200.0);
    assert_close(state.last_real_spending, 41_200.0);
    // Ceiling and floor of zero is the constant-dollar rule.
    let (planner, mut state) = started(SpendingRule::VanguardDynamic {
        ceiling: 0.0,
        floor: 0.0,
    });
    for portfolio in [2e6, 5e5] {
        assert_close(
            planner
                .decide(&mut state, &spend(40_000.0, portfolio))
                .spending_real,
            40_000.0,
        );
    }
}

#[test]
fn floor_and_ceiling_are_relative_to_the_budget() {
    let (planner, mut state) = started(SpendingRule::FloorCeiling {
        floor: 0.85,
        ceiling: 1.2,
    });
    // 4% of 700k = 28k: the floor holds spending at 85% of the budget.
    let d = planner.decide(&mut state, &spend(40_000.0, 700_000.0));
    assert_close(d.spending_real, 34_000.0);
    // 4% of 1.5M = 60k: the ceiling stops at 120%.
    let d = planner.decide(&mut state, &spend(40_000.0, 1_500_000.0));
    assert_close(d.spending_real, 48_000.0);
    // 4% of 1.1M = 44k: between the two.
    let d = planner.decide(&mut state, &spend(40_000.0, 1_100_000.0));
    assert_close(d.spending_real, 44_000.0);
    assert_close(d.budget_multiple, 1.1);
    // Unlike Vanguard's, the limits do not ratchet: straight back to the floor.
    let d = planner.decide(&mut state, &spend(40_000.0, 500_000.0));
    assert_close(d.spending_real, 34_000.0);
}

#[test]
fn vpw_percentages_by_age() {
    let params = RuleParams::default();
    // 60/40 at 5.0% and 1.9% real: r = 3.76%.
    let r = params.vpw_real_return(&sixty_forty());
    assert_close(r, 0.0376);
    // Bogleheads table for a 60/40 portfolio, last withdrawal at 99 (data file).
    let examples =
        &Data::embedded().actuarial["withdrawal_rules"]["vpw"]["examples_60_40_r_0.0376"];
    for (age, years, key) in [
        (60.0, 40.0, "n_40"),
        (70.0, 30.0, "n_30"),
        (80.0, 20.0, "n_20"),
        (90.0, 10.0, "n_10"),
    ] {
        let published = examples[key].as_f64().unwrap();
        assert_near(vpw_rate(r, years), published, 5e-5);
        assert_eq!(100.0 - age, years);
    }
    // Hand-computed: age 60 (40 years) 4.70%, age 75 (25 years) 6.01%, age 90
    // (10 years) 11.74% (1.0376^-10 = 0.69136; 0.0376 / 0.30864 / 1.0376).
    assert_near(vpw_rate(r, 40.0), 0.04697, 1e-5);
    assert_near(vpw_rate(r, 25.0), 0.06014, 1e-5);
    assert_near(vpw_rate(r, 10.0), 0.11741, 1e-5);
    // Zero return: one n-th. Final year: everything.
    assert_close(vpw_rate(0.0, 25.0), 0.04);
    assert_close(vpw_rate(0.0376, 1.0), 1.0);
    assert_close(vpw_rate(0.0376, 0.0), 1.0);
    // Monotone in the horizon.
    let mut last = 0.0;
    for years in (1..=60).rev() {
        let x = vpw_rate(r, f64::from(years));
        assert!(x > last && x <= 1.0);
        last = x;
    }

    let planner = SpendingPlanner::new(SpendingRule::Vpw);
    let at = |years: f64, portfolio: f64| {
        let mut i = spend(40_000.0, portfolio);
        i.years_remaining = years;
        let mut state = planner.start(&i);
        planner.decide(&mut state, &i).spending_real
    };
    assert_near(at(40.0, 1_000_000.0), 46_970.0, 5.0);
    assert_near(at(25.0, 1_000_000.0), 60_138.0, 5.0);
    // At 90 the formula says 11.74% but the percentage is capped at 10%.
    assert_close(at(10.0, 1_000_000.0), 100_000.0);
    // All-bond and all-stock expected returns.
    assert_close(
        params.vpw_real_return(&mix(&[(AssetClass::Bonds, 1.0)])),
        0.019,
    );
    assert_close(
        params.vpw_real_return(&mix(&[(AssetClass::Crypto, 1.0)])),
        0.05,
    );
    assert_close(params.vpw_real_return(&[0.0; N_ASSETS]), 0.019);
}

#[test]
fn spending_rules_survive_degenerate_inputs() {
    let rules = [
        SpendingRule::ConstantReal,
        SpendingRule::PercentOfPortfolio { rate: 0.04 },
        guardrails(),
        SpendingRule::VanguardDynamic {
            ceiling: 0.05,
            floor: 0.025,
        },
        SpendingRule::Vpw,
        SpendingRule::FloorCeiling {
            floor: 0.85,
            ceiling: 1.2,
        },
    ];
    let mut rng = Rng::from_seed(3);
    for rule in rules {
        let planner = SpendingPlanner::new(rule);
        for case in 0..200 {
            let mut i = spend(40_000.0, 1e6);
            let mut state = planner.start(&i);
            for _ in 0..40 {
                i.portfolio_real = match case % 4 {
                    0 => 0.0,
                    1 => rng.uniform_range(0.0, 5e6),
                    2 => 1e12,
                    _ => rng.uniform_range(0.0, 1e5),
                };
                i.budget_real = if case % 7 == 0 {
                    0.0
                } else {
                    rng.uniform_range(1e3, 2e5)
                };
                i.essential_real = i.budget_real * rng.uniform();
                i.other_income_real = rng.uniform_range(0.0, 1e5);
                i.years_remaining = rng.uniform_range(-2.0, 60.0);
                i.expected_real_return = rng.uniform_range(-0.05, 0.08);
                i.prior_return_negative = rng.bernoulli(0.3);
                i.prior_inflation = rng.uniform_range(-0.02, 0.1);
                let d = planner.decide(&mut state, &i);
                assert!(d.spending_real.is_finite() && d.spending_real >= 0.0);
                assert!(d.budget_multiple.is_finite() && d.budget_multiple >= 0.0);
                assert!(d.adjustment.is_finite());
                assert!(d.spending_real >= i.essential_real.min(i.budget_real) - 1e-9);
            }
        }
    }
}

// ---------------------------------------------------------------- 72(t)

#[test]
fn sepp_amortisation_payment() {
    // 1,000,000 at age 50: single life expectancy 36.2 years at 5%.
    // 50,000 / (1 - 1.05^-36.2) = 50,000 / 0.829019 = 60,312.
    assert_near(sepp_payment(1_000_000.0, 50.0, 0.05), 60_312.0, 5.0);
    // Age 55: 31.6 years: 50,000 / (1 - 1.05^-31.6) = 50,000 / 0.78600 = 63,613.
    assert_near(sepp_payment(1_000_000.0, 55.0, 0.05), 63_613.0, 10.0);
    // Between the anchors the expectancy is interpolated (52.5 -> 33.9 years).
    assert_close(sepp_payment(339_000.0, 52.5, 0.0), 10_000.0);
    // Linear in the balance, rising with age, nothing from nothing.
    assert_close(
        sepp_payment(500_000.0, 50.0, 0.05) * 2.0,
        sepp_payment(1_000_000.0, 50.0, 0.05),
    );
    let mut last = 0.0;
    for age in 18..=75 {
        let x = sepp_payment(1e6, f64::from(age), SEPP_MAX_RATE);
        assert!(x > last && x < 1e6);
        last = x;
    }
    assert_eq!(sepp_payment(0.0, 50.0, 0.05), 0.0);
    assert_eq!(sepp_payment(-5.0, 50.0, 0.05), 0.0);
    assert_eq!(sepp_payment(f64::NAN, 50.0, 0.05), 0.0);
}

// ---------------------------------------------------------------- the annual plan

/// A small tax system for hand examples, driven entirely by the `TaxYearParams` it
/// is given: ordinary income less the standard deduction through the ordinary
/// brackets, gains and qualified dividends stacked on top through the capital-gains
/// brackets, 10% on early distributions and 20% on non-qualified HSA money. Flat and
/// no-tax kinds as the real module defines them.
fn mini_tax(p: &TaxYearParams, i: &TaxInputs) -> TaxResult {
    let ordinary = i.wages[0]
        + i.wages[1]
        + i.interest
        + i.ordinary_dividends
        + i.traditional_distributions
        + i.roth_conversions
        + i.pensions
        + i.other_ordinary
        + i.hsa_nonqualified;
    let preferential = (i.qualified_dividends + i.long_term_gains).max(0.0);
    let agi = ordinary + preferential;
    let mut r = TaxResult {
        agi,
        magi: agi,
        ..TaxResult::default()
    };
    match p.kind {
        TaxKind::None => {}
        TaxKind::Flat { rate } => {
            r.federal_ordinary = rate * agi;
            r.taxable_income = agi;
        }
        TaxKind::UsCurrentLaw => {
            let s = i.filing_status.index();
            let deduction = p.federal.standard_deduction[s];
            let taxable_ordinary = (ordinary - deduction).max(0.0);
            let taxable_pref = (preferential - (deduction - ordinary).max(0.0)).max(0.0);
            let gains = &p.federal.capital_gains[s];
            r.federal_ordinary = p.federal.ordinary[s].tax(taxable_ordinary);
            r.federal_capital_gains =
                gains.tax(taxable_ordinary + taxable_pref) - gains.tax(taxable_ordinary);
            r.penalties = p.federal.early_withdrawal_penalty * i.early_distributions
                + 0.2 * i.hsa_nonqualified;
            r.taxable_income = taxable_ordinary + taxable_pref;
            r.deduction = deduction;
        }
    }
    r.total = r.federal_ordinary + r.federal_capital_gains + r.penalties;
    r
}

/// Hand-built law: standard deduction 15,000; ordinary brackets 10% to 10k, 12% to
/// 40k, 22% to 100k, 32% above; gains 0% to 45k of taxable income, then 15%; poverty
/// line 15,650 plus 5,500 a person.
fn law() -> TaxYearParams {
    let mut p = TaxYearParams {
        cpi_factor: 1.0,
        ..TaxYearParams::default()
    };
    let ordinary = Brackets::from_pairs(&[
        (0.0, 0.10),
        (10_000.0, 0.12),
        (40_000.0, 0.22),
        (100_000.0, 0.32),
    ]);
    let gains = Brackets::from_pairs(&[(0.0, 0.0), (45_000.0, 0.15), (500_000.0, 0.20)]);
    p.federal.ordinary = [ordinary; FilingStatus::COUNT];
    p.federal.capital_gains = [gains; FilingStatus::COUNT];
    p.federal.standard_deduction = [15_000.0; FilingStatus::COUNT];
    p.federal.early_withdrawal_penalty = 0.10;
    p.federal.fpl_base = 15_650.0;
    p.federal.fpl_per_person = 5_500.0;
    p
}

fn no_tax() -> TaxYearParams {
    TaxYearParams {
        kind: TaxKind::None,
        cpi_factor: 1.0,
        ..TaxYearParams::default()
    }
}

fn tax_aware(fill: f64, conversions: bool, aca: bool) -> WithdrawalOrder {
    WithdrawalOrder::TaxAware {
        fill_to_bracket: fill,
        roth_conversions: conversions,
        mind_aca: aca,
    }
}

/// Accounts with the given balances for person 0; the taxable bucket has no gain
/// unless a basis is set afterwards; the Roth is all contributions-free earnings
/// unless basis is set afterwards.
fn holdings(cash: f64, taxable: f64, traditional: f64, roth: f64) -> Accounts {
    let mut a = Accounts {
        cash,
        ..Accounts::default()
    };
    let m = sixty_forty();
    a.contribute(Bucket::Taxable, taxable, &m);
    a.contribute(Bucket::Traditional(0), traditional, &m);
    a.contribute(Bucket::Roth(0), roth, &m);
    a.person[0].roth.contribution_basis = 0.0;
    a.person[0].roth.years_open = 10;
    a
}

fn need(amount: f64, age: f64) -> PlanInputs {
    PlanInputs {
        net_need: amount,
        ages: [age, 0.0],
        ..PlanInputs::default()
    }
}

fn plan(
    order: &WithdrawalOrder,
    access: EarlyAccess,
    a: &Accounts,
    i: &PlanInputs,
    t: &TaxYearParams,
) -> WithdrawalPlan {
    plan_year_with(order, access, a, i, t, &mini_tax)
}

/// Runs a plan for the whole year on a copy and returns the result, the return and
/// the accounts afterwards.
fn run(p: &WithdrawalPlan, a: &Accounts, i: &PlanInputs) -> (Executed, TaxInputs, Accounts) {
    let mut copy = *a;
    let mut t = i.base_tax_inputs;
    let done = execute_plan(p, 1.0, &mut copy, i, &mut t, None);
    (done, t, copy)
}

#[test]
fn conventional_order_is_cash_taxable_traditional_roth() {
    let a = holdings(10_000.0, 50_000.0, 100_000.0, 30_000.0);
    let order = WithdrawalOrder::Conventional;
    let t = no_tax();
    let p = plan(&order, EarlyAccess::Auto, &a, &need(8_000.0, 65.0), &t);
    assert_close(p.from_cash, 8_000.0);
    assert_eq!(p.withdrawals(), 0.0);

    let p = plan(&order, EarlyAccess::Auto, &a, &need(45_000.0, 65.0), &t);
    assert_close(p.from_cash, 10_000.0);
    assert_close(p.from_taxable, 35_000.0);
    assert_eq!(p.from_traditional[0] + p.from_roth[0], 0.0);

    let p = plan(&order, EarlyAccess::Auto, &a, &need(175_000.0, 65.0), &t);
    assert_close(p.from_taxable, 50_000.0);
    assert_close(p.from_traditional[0], 100_000.0);
    assert_close(p.from_roth[0], 15_000.0);
    assert_eq!(p.shortfall, 0.0);
    assert_eq!(p.penalized, 0.0);

    // More than there is: everything goes and the rest is the shortfall.
    let p = plan(&order, EarlyAccess::Auto, &a, &need(250_000.0, 65.0), &t);
    assert_close(p.from_cash + p.withdrawals(), 190_000.0);
    assert_close(p.shortfall, 60_000.0);
    let (done, _, after) = run(&p, &a, &need(250_000.0, 65.0));
    assert_close(done.to_cash, 180_000.0);
    assert_close(after.cash, 190_000.0);
    assert_close(after.investable(), a.investable());

    // Nothing needed, nothing done.
    let p = plan(&order, EarlyAccess::Auto, &a, &need(0.0, 65.0), &t);
    assert_eq!(p, WithdrawalPlan::default());
    let p = plan(&order, EarlyAccess::Auto, &a, &need(-5_000.0, 65.0), &t);
    assert_close(p.excess, 5_000.0);
    assert_eq!(p.withdrawals() + p.from_cash, 0.0);
}

#[test]
fn proportional_order_draws_by_size() {
    let mut a = holdings(0.0, 100_000.0, 300_000.0, 100_000.0);
    a.contribute(Bucket::Traditional(1), 500_000.0, &sixty_forty());
    let i = PlanInputs {
        net_need: 50_000.0,
        ages: [65.0, 66.0],
        ..PlanInputs::default()
    };
    let p = plan(
        &WithdrawalOrder::Proportional,
        EarlyAccess::Auto,
        &a,
        &i,
        &no_tax(),
    );
    assert_close(p.from_taxable, 5_000.0);
    assert_close(p.from_traditional[0], 15_000.0);
    assert_close(p.from_traditional[1], 25_000.0);
    assert_close(p.from_roth[0], 5_000.0);
    // Spendable cash still goes first.
    a.cash = 20_000.0;
    let p = plan(
        &WithdrawalOrder::Proportional,
        EarlyAccess::Auto,
        &a,
        &i,
        &no_tax(),
    );
    assert_close(p.from_cash, 20_000.0);
    assert_close(p.from_taxable, 3_000.0);
}

#[test]
fn tax_is_grossed_up_into_the_withdrawals() {
    // Only a traditional account, age 65, 50,000 needed after tax. In the 12%
    // bracket tax(W) = 1,000 + 0.12 (W - 25,000), so W - tax(W) = 50,000 gives
    // W = 48,000 / 0.88 = 54,545.45 and tax 4,545.45.
    let a = holdings(0.0, 0.0, 1_000_000.0, 0.0);
    let i = need(50_000.0, 65.0);
    let p = plan(
        &WithdrawalOrder::Conventional,
        EarlyAccess::Auto,
        &a,
        &i,
        &law(),
    );
    assert_near(p.from_traditional[0], 54_545.45, 0.01);
    assert_near(p.estimated_tax, 4_545.45, 0.01);
    assert_near(p.estimated_magi, 54_545.45, 0.01);
    assert_eq!(p.shortfall, 0.0);

    // Across a bracket boundary: 80,000 after tax. Above 55,000 of income the rate
    // is 22%: W - (4,600 + 0.22 (W - 55,000)) = 80,000 -> W = 72,500 / 0.78.
    let p = plan(
        &WithdrawalOrder::Conventional,
        EarlyAccess::Auto,
        &a,
        &need(80_000.0, 65.0),
        &law(),
    );
    assert_near(p.from_traditional[0], 72_500.0 / 0.78, 0.01);
    assert_near(p.estimated_tax, 72_500.0 / 0.78 - 80_000.0, 0.01);

    // With tax already due on other income, only the extra tax is added: a 30,000
    // pension leaves 15,000 taxable (1,600 of tax, in the caller's net need).
    let mut j = need(20_000.0, 65.0);
    j.base_tax_inputs.pensions = 30_000.0;
    let p = plan(
        &WithdrawalOrder::Conventional,
        EarlyAccess::Auto,
        &a,
        &j,
        &law(),
    );
    assert_near(p.from_traditional[0], 20_000.0 / 0.88, 0.01);
    assert_near(p.estimated_tax, 1_600.0 + 0.12 * 20_000.0 / 0.88, 0.01);

    // A flat 20% tax: 50,000 / 0.8.
    let flat = TaxYearParams {
        kind: TaxKind::Flat { rate: 0.2 },
        ..TaxYearParams::default()
    };
    let p = plan(
        &WithdrawalOrder::Conventional,
        EarlyAccess::Auto,
        &a,
        &i,
        &flat,
    );
    assert_near(p.from_traditional[0], 62_500.0, 0.01);
    assert_near(p.estimated_tax, 12_500.0, 0.01);
}

#[test]
fn rmds_are_taken_even_when_not_needed() {
    let a = holdings(0.0, 200_000.0, 1_000_000.0, 100_000.0);
    let mut i = need(10_000.0, 76.0);
    i.rmd = [40_000.0, 0.0];
    let p = plan(
        &WithdrawalOrder::Conventional,
        EarlyAccess::Auto,
        &a,
        &i,
        &law(),
    );
    // Tax on 40,000: (40,000 - 15,000) -> 1,000 + 0.12 * 15,000 = 2,800.
    assert_close(p.from_traditional[0], 40_000.0);
    assert_eq!(p.from_taxable + p.from_roth[0], 0.0);
    assert_near(p.estimated_tax, 2_800.0, 0.01);
    assert_near(p.excess, 40_000.0 - 10_000.0 - 2_800.0, 0.01);
    assert_eq!(p.penalized, 0.0);
    let (done, t, after) = run(&p, &a, &i);
    assert_close(done.from_traditional, 40_000.0);
    assert_close(t.traditional_distributions, 40_000.0);
    assert_eq!(t.early_distributions, 0.0);
    assert_close(after.cash, 40_000.0);

    // The RMD covers part of a larger need; the rest follows the order.
    i.net_need = 100_000.0;
    let p = plan(
        &WithdrawalOrder::Conventional,
        EarlyAccess::Auto,
        &a,
        &i,
        &law(),
    );
    assert_close(p.from_traditional[0], 40_000.0);
    assert_near(p.from_taxable, 60_000.0 + 2_800.0, 0.01);
    assert_eq!(p.excess, 0.0);

    // An RMD larger than the account takes what is there.
    let small = holdings(0.0, 0.0, 5_000.0, 0.0);
    let p = plan(
        &WithdrawalOrder::Conventional,
        EarlyAccess::Auto,
        &small,
        &i,
        &no_tax(),
    );
    assert_close(p.from_traditional[0], 5_000.0);
    assert_close(p.shortfall, 95_000.0);
}

#[test]
fn tax_aware_fills_exactly_to_the_bracket_top() {
    // Single, 65, pension 20,000. Standard deduction 15,000 and the 12% bracket ends
    // at 40,000 of taxable income, so ordinary income may reach 55,000: there is
    // room for exactly 35,000 of traditional money.
    let a = holdings(0.0, 500_000.0, 1_000_000.0, 200_000.0);
    let mut i = need(60_000.0, 65.0);
    i.base_tax_inputs.pensions = 20_000.0;
    let order = tax_aware(0.12, false, false);
    let p = plan(&order, EarlyAccess::Auto, &a, &i, &law());
    assert_near(p.from_traditional[0], 35_000.0, 0.01);
    // Tax rises from 500 (on 5,000) to 4,600 (on 40,000): 4,100 more to raise, and
    // it comes from the taxable account, which has no gain to tax.
    assert_near(p.from_taxable, 60_000.0 + 4_100.0 - 35_000.0, 0.01);
    assert_eq!(p.from_roth[0], 0.0);
    assert_eq!(p.roth_conversion, [0.0, 0.0]);
    assert_near(p.estimated_tax, 4_600.0, 0.01);
    let (_, t, _) = run(&p, &a, &i);
    assert_near(mini_tax(&law(), &t).taxable_income, 40_000.0, 0.01);

    // Once the taxable account is gone, the Roth pays rather than going over the
    // bracket; traditional beyond the bracket is the last of the three.
    let b = holdings(0.0, 10_000.0, 1_000_000.0, 200_000.0);
    let p = plan(&order, EarlyAccess::Auto, &b, &i, &law());
    assert_near(p.from_traditional[0], 35_000.0, 0.01);
    assert_close(p.from_taxable, 10_000.0);
    assert_near(p.from_roth[0], 19_100.0, 0.01);
    let c = holdings(0.0, 10_000.0, 1_000_000.0, 5_000.0);
    let p = plan(&order, EarlyAccess::Auto, &c, &i, &law());
    assert_close(p.from_roth[0], 5_000.0);
    // 45,000 + 4,100 still to find, taxed at 22%: 14,100 / 0.78 above the bracket.
    assert_near(p.from_traditional[0], 35_000.0 + 14_100.0 / 0.78, 0.01);

    // A different bracket choice: filling the 22% bracket (to 100,000 taxable)
    // leaves 95,000 of room, more than is needed, so nothing else is touched.
    let p = plan(
        &tax_aware(0.22, false, false),
        EarlyAccess::Auto,
        &a,
        &i,
        &law(),
    );
    assert_eq!(p.from_taxable + p.from_roth[0], 0.0);
    // 60,000 + extra tax; 35,000 at 12% then the rest at 22%.
    // W - (4,100 + 0.22 (W - 35,000)) = 60,000 -> W = 56,400 / 0.78.
    assert_near(p.from_traditional[0], 56_400.0 / 0.78, 0.01);
}

#[test]
fn tax_aware_converts_into_the_unused_bracket_room() {
    // Same household, needing only 10,000. Whatever is withdrawn or converted, the
    // bracket is filled to the top: withdrawal + conversion = 35,000 exactly, the
    // extra tax is 4,100, and the withdrawal is 10,000 + 4,100.
    let a = holdings(0.0, 500_000.0, 1_000_000.0, 200_000.0);
    let mut i = need(10_000.0, 65.0);
    i.base_tax_inputs.pensions = 20_000.0;
    let p = plan(
        &tax_aware(0.12, true, false),
        EarlyAccess::Auto,
        &a,
        &i,
        &law(),
    );
    assert_near(p.from_traditional[0], 14_100.0, 0.01);
    assert_near(p.roth_conversion[0], 20_900.0, 0.01);
    assert_near(p.estimated_tax, 4_600.0, 0.01);
    assert_eq!(p.from_taxable, 0.0);
    let (done, t, after) = run(&p, &a, &i);
    assert_near(
        t.traditional_distributions + t.roth_conversions,
        35_000.0,
        0.01,
    );
    assert_near(mini_tax(&law(), &t).taxable_income, 40_000.0, 0.01);
    assert_near(done.converted, 20_900.0, 0.01);
    assert_near(after.person[0].roth.unseasoned[4], 20_900.0, 0.01);
    assert_close(after.investable(), a.investable());

    // No need at all (other income covers the year): the whole room is converted
    // and the tax on it is raised from the taxable account.
    let mut covered = need(0.0, 65.0);
    covered.base_tax_inputs.pensions = 20_000.0;
    let p = plan(
        &tax_aware(0.12, true, false),
        EarlyAccess::Auto,
        &a,
        &covered,
        &law(),
    );
    assert_near(p.from_traditional[0] + p.roth_conversion[0], 35_000.0, 0.01);
    assert_near(p.from_traditional[0], 4_100.0, 0.01);

    // Conversions are for both spouses, in proportion to their balances.
    let mut two = a;
    two.contribute(Bucket::Traditional(1), 3_000_000.0, &sixty_forty());
    let mut both = covered;
    both.ages = [65.0, 63.0];
    let p = plan(
        &tax_aware(0.12, true, false),
        EarlyAccess::Auto,
        &two,
        &both,
        &law(),
    );
    assert_near(
        p.roth_conversion[1] / p.roth_conversion[0],
        (3_000_000.0 - p.from_traditional[1]) / (1_000_000.0 - p.from_traditional[0]),
        1e-6,
    );
    assert_near(
        p.from_traditional[0] + p.from_traditional[1] + p.roth_conversion[0] + p.roth_conversion[1],
        35_000.0,
        0.01,
    );

    // No taxable money to pay the tax with: no conversion.
    let bare = holdings(0.0, 0.0, 1_000_000.0, 200_000.0);
    let p = plan(
        &tax_aware(0.12, true, false),
        EarlyAccess::Auto,
        &bare,
        &i,
        &law(),
    );
    assert_eq!(p.roth_conversion, [0.0, 0.0]);
    // The top bracket has no top to fill to: no conversion either.
    let p = plan(
        &tax_aware(0.37, true, false),
        EarlyAccess::Auto,
        &a,
        &i,
        &law(),
    );
    assert_eq!(p.roth_conversion, [0.0, 0.0]);
    // 10,000 after tax from traditional money alone: 5,000 more at 10%, the rest at
    // 12%: W - 500 - 0.12 (W - 5,000) = 10,000 -> W = 11,250.
    assert_near(p.from_traditional[0], 11_250.0, 0.01);
}

#[test]
fn bracket_room_accounts_for_an_unused_standard_deduction() {
    // No other income: the first 15,000 is sheltered by the deduction, so the 12%
    // bracket has room for 55,000 of traditional money, not 40,000.
    let a = holdings(0.0, 500_000.0, 1_000_000.0, 0.0);
    let i = need(0.0, 65.0);
    let p = plan(
        &tax_aware(0.12, true, false),
        EarlyAccess::Auto,
        &a,
        &i,
        &law(),
    );
    assert_near(p.from_traditional[0] + p.roth_conversion[0], 55_000.0, 0.01);
    assert_near(p.estimated_tax, 4_600.0, 0.01);
}

#[test]
fn tax_aware_keeps_magi_under_the_aca_cliff() {
    // One person on an ACA plan: 400% of 15,650 is 62,600, planned with a 2% margin
    // at 61,348. Pension 20,000 leaves 41,348 of MAGI room, less than the 95,000 of
    // room in the 22% bracket.
    let a = holdings(0.0, 500_000.0, 1_000_000.0, 200_000.0);
    let mut i = need(90_000.0, 60.0);
    i.base_tax_inputs.pensions = 20_000.0;
    i.base_tax_inputs.aca_household_size = 1;
    i.on_aca = true;
    let cliff = 62_600.0 * 0.98;
    let p = plan(
        &tax_aware(0.22, true, true),
        EarlyAccess::Auto,
        &a,
        &i,
        &law(),
    );
    assert_near(p.from_traditional[0], cliff - 20_000.0, 0.01);
    assert_near(p.estimated_magi, cliff, 0.01);
    assert!(p.from_taxable > 40_000.0);
    assert_eq!(p.roth_conversion, [0.0, 0.0]);

    // Small need: the conversion stops at the cliff too.
    i.net_need = 5_000.0;
    let p = plan(
        &tax_aware(0.22, true, true),
        EarlyAccess::Auto,
        &a,
        &i,
        &law(),
    );
    assert_near(p.estimated_magi, cliff, 0.01);
    assert!(p.roth_conversion[0] > 30_000.0);

    // Gains count toward MAGI: a taxable account that is half gain can only supply
    // twice the remaining room before the Roth takes over.
    let mut gains = holdings(0.0, 500_000.0, 0.0, 200_000.0);
    for b in &mut gains.taxable.basis {
        *b *= 0.5;
    }
    i.net_need = 150_000.0;
    let p = plan(
        &tax_aware(0.22, false, true),
        EarlyAccess::Auto,
        &gains,
        &i,
        &law(),
    );
    assert_near(p.from_taxable, 2.0 * (cliff - 20_000.0), 0.01);
    assert!(p.from_roth[0] > 60_000.0);
    assert_near(p.estimated_magi, cliff, 0.01);

    // Not minding the cliff, or not on an ACA plan: the bracket is the only limit.
    i.net_need = 90_000.0;
    for (order, on_aca) in [
        (tax_aware(0.22, false, false), true),
        (tax_aware(0.22, false, true), false),
    ] {
        i.on_aca = on_aca;
        let p = plan(&order, EarlyAccess::Auto, &a, &i, &law());
        assert_near(p.from_traditional[0], 95_000.0, 0.01);
    }
    // Already over the cliff on other income: nothing to protect.
    i.on_aca = true;
    i.base_tax_inputs.pensions = 70_000.0;
    let p = plan(
        &tax_aware(0.22, false, true),
        EarlyAccess::Auto,
        &a,
        &i,
        &law(),
    );
    assert_near(p.from_traditional[0], 45_000.0, 0.01);
}

#[test]
fn zero_percent_gains_are_harvested_only_when_free() {
    // 500k taxable, half of it gain; nothing else. Need 20,000: the sale realises
    // 10,000 of gain, all inside the unused standard deduction, so taxable income is
    // 0 and the whole 0% bracket (45,000) is open.
    let mut a = holdings(0.0, 500_000.0, 0.0, 0.0);
    for b in &mut a.taxable.basis {
        *b *= 0.5;
    }
    let i = need(20_000.0, 65.0);
    let order = tax_aware(0.12, true, false);
    let p = plan(&order, EarlyAccess::Auto, &a, &i, &law());
    assert_close(p.from_taxable, 20_000.0);
    assert_near(p.gain_harvest, 45_000.0, 0.01);
    assert_eq!(p.estimated_tax, 0.0);
    let (done, t, after) = run(&p, &a, &i);
    assert_near(t.long_term_gains, 55_000.0, 0.01);
    assert_near(done.long_term_gain, 55_000.0, 0.01);
    assert_eq!(mini_tax(&law(), &t).total, 0.0);
    assert_near(after.unrealized_gain(), 250_000.0 - 55_000.0, 0.01);
    assert_close(after.total(Bucket::Taxable), 480_000.0);

    // Where harvesting would cost something (here: a 5% state tax on gains that the
    // federal brackets do not show), it is not done.
    let with_state = |p: &TaxYearParams, i: &TaxInputs| {
        let mut r = mini_tax(p, i);
        r.state = 0.05 * i.long_term_gains.max(0.0);
        r.total += r.state;
        r
    };
    let p = plan_year_with(&order, EarlyAccess::Auto, &a, &i, &law(), &with_state);
    assert_eq!(p.gain_harvest, 0.0);
    // Other orders never harvest.
    let p = plan(
        &WithdrawalOrder::Conventional,
        EarlyAccess::Auto,
        &a,
        &i,
        &law(),
    );
    assert_eq!(p.gain_harvest, 0.0);
}

#[test]
fn tax_aware_without_brackets_is_the_conventional_order() {
    let a = holdings(5_000.0, 50_000.0, 100_000.0, 30_000.0);
    let i = need(100_000.0, 65.0);
    for t in [
        no_tax(),
        TaxYearParams {
            kind: TaxKind::Flat { rate: 0.1 },
            ..TaxYearParams::default()
        },
        TaxYearParams {
            foreign_tax_rate: Some(0.2),
            ..law()
        },
    ] {
        let aware = plan(&tax_aware(0.12, true, true), EarlyAccess::Auto, &a, &i, &t);
        let plain = plan(
            &WithdrawalOrder::Conventional,
            EarlyAccess::Auto,
            &a,
            &i,
            &t,
        );
        assert_eq!(aware, plain);
        assert_eq!(aware.roth_conversion, [0.0, 0.0]);
    }
}

/// Age 50: 100k taxable, 80k Roth of which 50k is contributions, 500k traditional.
fn early_retiree() -> Accounts {
    let mut a = holdings(0.0, 100_000.0, 500_000.0, 80_000.0);
    a.person[0].roth.contribution_basis = 50_000.0;
    a
}

#[test]
fn auto_early_access_uses_penalty_free_money_first() {
    let a = early_retiree();
    let i = need(120_000.0, 50.0);
    for order in [
        WithdrawalOrder::Conventional,
        WithdrawalOrder::Proportional,
        tax_aware(0.12, true, true),
    ] {
        // Taxable, then Roth contributions: no traditional money, no penalty.
        let p = plan(&order, EarlyAccess::Auto, &a, &i, &no_tax());
        assert_close(p.from_taxable + p.from_roth[0], 120_000.0);
        assert!(p.from_roth[0] <= 50_000.0 + 1e-9);
        assert_eq!(p.from_traditional[0], 0.0);
        assert_eq!(p.penalized, 0.0);
        let (done, t, _) = run(&p, &a, &i);
        assert_eq!(done.penalty_base, 0.0);
        assert_eq!(t.early_distributions, 0.0);
        assert_eq!(t.other_ordinary, 0.0);
    }
    let p = plan(
        &WithdrawalOrder::Conventional,
        EarlyAccess::Auto,
        &a,
        &i,
        &no_tax(),
    );
    assert_close(p.from_taxable, 100_000.0);
    assert_close(p.from_roth[0], 20_000.0);

    // Beyond the penalty-free money: a 72(t) schedule of at most the amortisation
    // payment on 500k at age 50, and only then a penalised withdrawal.
    let sepp = sepp_payment(500_000.0, 50.0, SEPP_MAX_RATE);
    assert_near(sepp, 30_156.0, 3.0);
    let j = need(170_000.0, 50.0);
    let p = plan(
        &WithdrawalOrder::Conventional,
        EarlyAccess::Auto,
        &a,
        &j,
        &no_tax(),
    );
    assert_close(p.from_taxable, 100_000.0);
    assert_close(p.from_roth[0], 50_000.0);
    assert_close(p.from_traditional[0], 20_000.0);
    assert_close(p.sepp[0], 20_000.0);
    assert_eq!(p.penalized, 0.0);
    let (_, t, _) = run(&p, &a, &j);
    assert_close(t.traditional_distributions, 20_000.0);
    assert_eq!(t.early_distributions, 0.0);

    let k = need(200_000.0, 50.0);
    let p = plan(
        &WithdrawalOrder::Conventional,
        EarlyAccess::Auto,
        &a,
        &k,
        &no_tax(),
    );
    assert_close(p.sepp[0], sepp);
    assert_close(p.from_traditional[0], 50_000.0);
    assert_close(p.penalized, 50_000.0 - sepp);
    assert_eq!(p.shortfall, 0.0);
    // The last resort after traditional money is the taxable part of the Roth.
    let l = need(700_000.0, 50.0);
    let p = plan(
        &WithdrawalOrder::Conventional,
        EarlyAccess::Auto,
        &a,
        &l,
        &no_tax(),
    );
    assert_close(p.from_traditional[0], 500_000.0);
    assert_close(p.from_roth[0], 80_000.0);
    assert_close(p.penalized, 500_000.0 - sepp + 30_000.0);
    assert_close(p.shortfall, 20_000.0);

    // The same person at 60 just follows the order.
    let p = plan(
        &WithdrawalOrder::Conventional,
        EarlyAccess::Auto,
        &a,
        &k,
        &no_tax(),
    );
    assert!(p.penalized > 0.0);
    let p = plan(
        &WithdrawalOrder::Conventional,
        EarlyAccess::Auto,
        &a,
        &need(200_000.0, 60.0),
        &no_tax(),
    );
    assert_close(p.from_traditional[0], 100_000.0);
    assert_eq!(p.from_roth[0], 0.0);
    assert_eq!(p.penalized, 0.0);
    // So does someone covered by the rule of 55.
    let mut exempt = need(200_000.0, 56.0);
    exempt.penalty_exempt = [true, false];
    let p = plan(
        &WithdrawalOrder::Conventional,
        EarlyAccess::Auto,
        &a,
        &exempt,
        &no_tax(),
    );
    assert_close(p.from_traditional[0], 100_000.0);
    assert_eq!(p.penalized, 0.0);
    assert_eq!(p.sepp[0], 0.0);
}

#[test]
fn pay_penalty_withdraws_directly() {
    let a = early_retiree();
    let i = need(150_000.0, 50.0);
    let p = plan(
        &WithdrawalOrder::Conventional,
        EarlyAccess::PayPenalty,
        &a,
        &i,
        &no_tax(),
    );
    assert_close(p.from_taxable, 100_000.0);
    assert_close(p.from_traditional[0], 50_000.0);
    assert_eq!(p.from_roth[0], 0.0);
    assert_close(p.penalized, 50_000.0);
    assert_eq!(p.sepp[0], 0.0);

    // With tax: W from traditional, taxed and penalised at 10%.
    // Need 20,000 beyond the taxable account: W - tax(W) - 0.1 W = 20,000 with
    // tax(W) = 1,000 + 0.12 (W - 25,000) for W above 25,000: 0.78 W = 18,000 gives
    // W = 23,077, below 25,000, so the 10% bracket applies:
    // W - 0.10 (W - 15,000) - 0.10 W = 20,000 -> W = 18,500 / 0.8 = 23,125.
    let j = need(120_000.0, 50.0);
    let p = plan(
        &WithdrawalOrder::Conventional,
        EarlyAccess::PayPenalty,
        &a,
        &j,
        &law(),
    );
    assert_near(p.from_traditional[0], 23_125.0, 0.01);
    assert_near(p.penalized, 23_125.0, 0.01);
    assert_near(p.estimated_tax, 812.5 + 2_312.5, 0.01);
}

#[test]
fn running_sepp_schedule_is_always_taken_unpenalised() {
    let a = early_retiree();
    let mut i = need(60_000.0, 52.0);
    i.sepp_available = [25_000.0, 0.0];
    let p = plan(
        &WithdrawalOrder::Conventional,
        EarlyAccess::Auto,
        &a,
        &i,
        &no_tax(),
    );
    assert_close(p.from_traditional[0], 25_000.0);
    assert_close(p.sepp[0], 25_000.0);
    assert_close(p.from_taxable, 35_000.0);
    assert_eq!(p.penalized, 0.0);
    // Even when nothing is needed.
    i.net_need = 0.0;
    let p = plan(
        &tax_aware(0.12, true, false),
        EarlyAccess::Auto,
        &a,
        &i,
        &law(),
    );
    assert_close(p.from_traditional[0], 25_000.0);
    // 25,000 - 15,000 deduction = 10,000 taxable: 1,000 of tax.
    assert_near(p.excess, 24_000.0, 0.01);
    // An account on a 72(t) schedule is not converted (that would modify it).
    assert_eq!(p.roth_conversion, [0.0, 0.0]);
}

#[test]
fn hsa_pays_medical_costs_first_and_other_spending_last() {
    let mut a = holdings(0.0, 20_000.0, 0.0, 0.0);
    a.contribute(Bucket::Hsa(0), 3_000.0, &sixty_forty());
    a.contribute(Bucket::Hsa(1), 10_000.0, &sixty_forty());
    let mut i = PlanInputs {
        net_need: 12_000.0,
        ages: [60.0, 58.0],
        medical_need: 5_000.0,
        ..PlanInputs::default()
    };
    let p = plan(
        &WithdrawalOrder::Conventional,
        EarlyAccess::Auto,
        &a,
        &i,
        &law(),
    );
    assert_eq!(p.from_hsa, [3_000.0, 2_000.0]);
    assert_close(p.from_taxable, 7_000.0);
    assert_eq!(p.from_hsa_other, [0.0, 0.0]);
    assert_eq!(p.estimated_tax, 0.0);
    let (done, t, _) = run(&p, &a, &i);
    assert_close(done.from_hsa, 5_000.0);
    assert_eq!(t.hsa_nonqualified + t.other_ordinary, 0.0);

    // Everything else exhausted: the rest of the HSA is used, taxed and (before 65)
    // penalised at 20%.
    i.net_need = 26_000.0;
    let p = plan(
        &WithdrawalOrder::Conventional,
        EarlyAccess::Auto,
        &a,
        &i,
        &no_tax(),
    );
    assert_close(p.from_taxable, 20_000.0);
    assert_close(p.from_hsa_other[1], 1_000.0);
    let (_, t, _) = run(&p, &a, &i);
    assert_close(t.hsa_nonqualified, 1_000.0);
    // After 65 it is ordinary income without the penalty.
    i.ages = [60.0, 66.0];
    let (_, t, _) = run(&p, &a, &i);
    assert_eq!(t.hsa_nonqualified, 0.0);
    assert_close(t.other_ordinary, 1_000.0);
}

#[test]
fn retirement_cash_bucket_is_spent_in_drawdowns_and_refilled_after() {
    let order = WithdrawalOrder::Conventional;
    let t = no_tax();
    // Bucket at its 80k target, markets fine: the bucket is left alone.
    let a = holdings(80_000.0, 500_000.0, 0.0, 0.0);
    let mut i = need(40_000.0, 65.0);
    i.cash_target = 80_000.0;
    let p = plan(&order, EarlyAccess::Auto, &a, &i, &t);
    assert_eq!(p.from_cash, 0.0);
    assert_close(p.from_taxable, 40_000.0);
    assert_eq!(p.cash_refill, 0.0);

    // Equities in drawdown: spend the bucket, sell nothing.
    i.in_drawdown = true;
    let p = plan(&order, EarlyAccess::Auto, &a, &i, &t);
    assert_close(p.from_cash, 40_000.0);
    assert_eq!(p.from_taxable, 0.0);
    // Still in drawdown with only 15k left: the bucket runs out, then sales.
    let low = holdings(15_000.0, 500_000.0, 0.0, 0.0);
    let p = plan(&order, EarlyAccess::Auto, &low, &i, &t);
    assert_close(p.from_cash, 15_000.0);
    assert_close(p.from_taxable, 25_000.0);
    assert_eq!(p.cash_refill, 0.0);

    // Recovery: withdraw the year's need plus the refill.
    i.in_drawdown = false;
    let p = plan(&order, EarlyAccess::Auto, &low, &i, &t);
    assert_eq!(p.from_cash, 0.0);
    assert_close(p.cash_refill, 65_000.0);
    assert_close(p.from_taxable, 105_000.0);
    let (_, _, after) = run(&p, &low, &i);
    // After the withdrawals cash holds the target plus this year's spending money.
    assert_close(after.cash, 80_000.0 + 40_000.0);

    // Cash above the target is spendable first; the bucket itself is a last resort,
    // used before a shortfall is declared.
    let rich = holdings(100_000.0, 30_000.0, 0.0, 0.0);
    let p = plan(&order, EarlyAccess::Auto, &rich, &i, &t);
    assert_close(p.from_cash, 20_000.0);
    assert_close(p.from_taxable, 20_000.0);
    i.net_need = 60_000.0;
    let p = plan(&order, EarlyAccess::Auto, &rich, &i, &t);
    assert_close(p.from_taxable, 30_000.0);
    assert_close(p.from_cash, 30_000.0);
    assert_eq!(p.shortfall, 0.0);
    i.net_need = 150_000.0;
    let p = plan(&order, EarlyAccess::Auto, &rich, &i, &t);
    assert_close(p.from_cash, 100_000.0);
    assert_close(p.shortfall, 20_000.0);

    // The refill is dropped rather than paid for with a penalty.
    let young = early_retiree();
    let mut j = need(150_000.0, 50.0);
    j.cash_target = 60_000.0;
    let p = plan(&order, EarlyAccess::Auto, &young, &j, &t);
    assert_close(p.from_taxable + p.from_roth[0], 150_000.0);
    assert_eq!(p.cash_refill, 0.0);
    assert_eq!(p.from_traditional[0], 0.0);
    j.net_need = 120_000.0;
    let p = plan(&order, EarlyAccess::Auto, &young, &j, &t);
    assert_close(p.cash_refill, 30_000.0);
    assert_eq!(p.penalized, 0.0);
}

#[test]
fn executing_in_twelve_instalments_equals_the_whole() {
    let a = holdings(0.0, 300_000.0, 800_000.0, 100_000.0);
    let mut i = need(90_000.0, 65.0);
    i.base_tax_inputs.pensions = 20_000.0;
    let p = plan(
        &tax_aware(0.12, true, false),
        EarlyAccess::Auto,
        &a,
        &i,
        &law(),
    );
    let (whole, tw, aw) = run(&p, &a, &i);
    let mut monthly = a;
    let mut tm = i.base_tax_inputs;
    let mut sum = Executed::default();
    let target = sixty_forty();
    for _ in 0..12 {
        let d = execute_plan(&p, 1.0 / 12.0, &mut monthly, &i, &mut tm, Some(&target));
        sum.to_cash += d.to_cash;
        sum.converted += d.converted;
        assert_close(monthly.investable(), a.investable());
    }
    assert_near(sum.to_cash, whole.to_cash, 1e-6);
    assert_near(sum.to_cash, p.withdrawals(), 1e-6);
    assert_near(sum.converted, whole.converted, 1e-6);
    assert_near(
        tm.traditional_distributions,
        tw.traditional_distributions,
        1e-6,
    );
    assert_near(monthly.cash, aw.cash, 1e-6);
    // Zero and nonsense fractions do nothing.
    let before = monthly;
    for f in [0.0, -1.0, f64::NAN] {
        assert_eq!(
            execute_plan(&p, f, &mut monthly, &i, &mut tm, None),
            Executed::default()
        );
    }
    assert_eq!(monthly, before);
}

#[test]
fn cover_shortfall_raises_cash_without_touching_the_cash_bucket() {
    let mut a = early_retiree();
    a.cash = 7_000.0;
    let i = need(0.0, 50.0);
    let mut t = TaxInputs::default();
    let done = cover_shortfall(30_000.0, EarlyAccess::Auto, &mut a, &i, &mut t, None);
    assert_close(done.to_cash, 30_000.0);
    assert_close(done.from_taxable, 30_000.0);
    assert_eq!(done.shortfall, 0.0);
    assert_close(a.cash, 37_000.0);

    // Taxable (70k left), Roth contributions (50k), then penalised traditional.
    let done = cover_shortfall(150_000.0, EarlyAccess::Auto, &mut a, &i, &mut t, None);
    assert_close(done.from_taxable, 70_000.0);
    assert_close(done.from_roth, 50_000.0);
    assert_close(done.from_traditional, 30_000.0);
    assert_close(done.penalty_base, 30_000.0);
    assert_close(t.early_distributions, 30_000.0);
    assert_close(t.traditional_distributions, 30_000.0);

    // More than everything: the rest is reported.
    let left = a.investable() - a.cash;
    let done = cover_shortfall(1e7, EarlyAccess::Auto, &mut a, &i, &mut t, None);
    assert_close(done.to_cash, left);
    assert_close(done.shortfall, 1e7 - left);
    assert_close(a.cash, a.investable());
    assert_eq!(
        cover_shortfall(-1.0, EarlyAccess::Auto, &mut a, &i, &mut t, None),
        Executed::default()
    );
}

fn random_accounts(rng: &mut Rng) -> Accounts {
    let mut a = Accounts::default();
    let m = sixty_forty();
    if rng.bernoulli(0.6) {
        a.cash = rng.uniform_range(0.0, 150_000.0);
    }
    if rng.bernoulli(0.7) {
        a.contribute(Bucket::Taxable, rng.uniform_range(0.0, 800_000.0), &m);
        for b in &mut a.taxable.basis {
            *b *= rng.uniform_range(0.2, 1.2);
        }
    }
    for p in 0..MAX_PEOPLE {
        if rng.bernoulli(0.7) {
            a.contribute(Bucket::Traditional(p), rng.uniform_range(0.0, 1.5e6), &m);
        }
        if rng.bernoulli(0.6) {
            a.contribute(Bucket::Roth(p), rng.uniform_range(0.0, 400_000.0), &m);
            let r = &mut a.person[p].roth;
            r.contribution_basis *= rng.uniform();
            r.seasoned_conversions = rng.uniform_range(0.0, 20_000.0);
            r.unseasoned[rng.below(5)] = rng.uniform_range(0.0, 20_000.0);
            r.years_open = rng.below(10) as u8;
        }
        if rng.bernoulli(0.4) {
            a.contribute(Bucket::Hsa(p), rng.uniform_range(0.0, 60_000.0), &m);
        }
    }
    a
}

fn random_inputs(rng: &mut Rng) -> PlanInputs {
    let mut i = PlanInputs {
        net_need: rng.uniform_range(-20_000.0, 250_000.0),
        ages: [rng.uniform_range(40.0, 95.0), rng.uniform_range(40.0, 95.0)],
        on_aca: rng.bernoulli(0.5),
        medical_need: rng.uniform_range(0.0, 15_000.0),
        cash_target: if rng.bernoulli(0.5) {
            rng.uniform_range(0.0, 120_000.0)
        } else {
            0.0
        },
        cash_floor: if rng.bernoulli(0.3) { 10_000.0 } else { 0.0 },
        in_drawdown: rng.bernoulli(0.4),
        ..PlanInputs::default()
    };
    for p in 0..MAX_PEOPLE {
        if i.ages[p] >= 75.0 {
            i.rmd[p] = rng.uniform_range(0.0, 60_000.0);
        }
        if i.ages[p] < 59.5 && rng.bernoulli(0.2) {
            i.sepp_available[p] = rng.uniform_range(0.0, 30_000.0);
        }
        i.penalty_exempt[p] = rng.bernoulli(0.1);
    }
    i.base_tax_inputs.filing_status = FilingStatus::MarriedJoint;
    i.base_tax_inputs.pensions = rng.uniform_range(0.0, 60_000.0);
    i.base_tax_inputs.qualified_dividends = rng.uniform_range(0.0, 10_000.0);
    i.base_tax_inputs.aca_household_size = 2;
    i
}

fn random_order(rng: &mut Rng) -> WithdrawalOrder {
    match rng.below(4) {
        0 => WithdrawalOrder::Conventional,
        1 => WithdrawalOrder::Proportional,
        2 => tax_aware(0.12, rng.bernoulli(0.5), rng.bernoulli(0.5)),
        _ => tax_aware(0.22, true, true),
    }
}

/// Every plan is feasible, balanced and (when sources suffice) covers need plus tax.
#[test]
fn plans_balance_over_random_households() {
    let mut rng = Rng::from_seed(2_024);
    let mut worst = 0.0_f64;
    for case in 0..3_000 {
        let a = random_accounts(&mut rng);
        let i = random_inputs(&mut rng);
        let order = random_order(&mut rng);
        let access = if rng.bernoulli(0.7) {
            EarlyAccess::Auto
        } else {
            EarlyAccess::PayPenalty
        };
        let t = if case % 5 == 0 { no_tax() } else { law() };
        let p = plan(&order, access, &a, &i, &t);
        let again = plan(&order, access, &a, &i, &t);
        assert_eq!(p, again, "deterministic");

        // Feasible: nothing negative, nothing beyond a balance.
        let amounts = [
            p.from_cash,
            p.from_taxable,
            p.cash_refill,
            p.excess,
            p.gain_harvest,
            p.penalized,
            p.shortfall,
            p.estimated_tax,
        ];
        assert!(amounts.iter().all(|x| x.is_finite() && *x >= 0.0), "{p:?}");
        assert!(p.from_cash <= a.cash + 1e-6);
        assert!(p.from_taxable <= a.total(Bucket::Taxable) + 1e-6);
        for q in 0..MAX_PEOPLE {
            let per = [
                p.from_traditional[q],
                p.from_roth[q],
                p.from_hsa[q],
                p.from_hsa_other[q],
                p.roth_conversion[q],
                p.sepp[q],
            ];
            assert!(per.iter().all(|x| x.is_finite() && *x >= 0.0), "{p:?}");
            assert!(
                p.from_traditional[q] + p.roth_conversion[q]
                    <= a.total(Bucket::Traditional(q)) + 1e-6
            );
            assert!(p.from_roth[q] <= a.total(Bucket::Roth(q)) + 1e-6);
            assert!(p.from_hsa[q] + p.from_hsa_other[q] <= a.total(Bucket::Hsa(q)) + 1e-6);
            assert!(p.sepp[q] <= p.from_traditional[q] + 1e-6);
            // Forced amounts are always in the plan.
            let forced = (i.rmd[q] + i.sepp_available[q]).min(a.total(Bucket::Traditional(q)));
            assert!(p.from_traditional[q] >= forced - 1e-6);
        }
        assert!(p.excess == 0.0 || p.shortfall == 0.0);
        if i.in_drawdown {
            assert_eq!(p.cash_refill, 0.0);
        }

        // Executing the plan conserves money and raises exactly what it says.
        let (done, tx, after) = run(&p, &a, &i);
        assert_near(
            after.investable(),
            a.investable(),
            1e-6 * (1.0 + a.investable()),
        );
        assert_near(
            done.to_cash,
            p.withdrawals(),
            1e-6 * (1.0 + p.withdrawals()),
        );
        assert_near(done.penalty_base, p.penalized, 1e-6 * (1.0 + p.penalized));

        // Balanced: sources = need + extra tax + refill + excess - shortfall.
        let result = mini_tax(&t, &tx);
        assert_near(result.total, p.estimated_tax, 1e-6 * (1.0 + result.total));
        let extra_tax = result.total - mini_tax(&t, &i.base_tax_inputs).total;
        let sources = p.from_cash + p.withdrawals();
        let uses = i.net_need + extra_tax + p.cash_refill + p.excess - p.shortfall;
        let gap = (sources - uses).abs();
        worst = worst.max(gap);
        // The gross-up stops within a cent when tax is smooth; across a bracket or
        // penalty kink the last secant step can leave a little.
        assert!(gap <= 1.0, "case {case}: gap {gap} {p:?}");
    }
    assert!(worst <= 1.0);
}

/// `Auto` never pays a penalty while penalty-free sources can cover the need.
#[test]
fn auto_early_access_never_penalised_when_free_sources_suffice() {
    let mut rng = Rng::from_seed(59);
    let mut checked = 0;
    for _ in 0..4_000 {
        let a = random_accounts(&mut rng);
        let mut i = random_inputs(&mut rng);
        i.ages = [rng.uniform_range(40.0, 59.4), rng.uniform_range(40.0, 70.0)];
        i.rmd = [0.0; MAX_PEOPLE];
        i.sepp_available = [0.0; MAX_PEOPLE];
        i.cash_target = 0.0;
        i.cash_floor = 0.0;
        i.medical_need = 0.0;
        let order = random_order(&mut rng);
        // Penalty-free capacity: cash, taxable, and per person either everything
        // (59 and a half or exempt) or Roth basis plus a 72(t) payment.
        let mut free = a.cash + a.total(Bucket::Taxable);
        for q in 0..MAX_PEOPLE {
            if i.ages[q] >= EARLY_WITHDRAWAL_AGE || i.penalty_exempt[q] {
                free += a.total(Bucket::Traditional(q)) + a.total(Bucket::Roth(q));
            } else {
                free += a.roth_accessible(q)
                    + sepp_payment(a.total(Bucket::Traditional(q)), i.ages[q], SEPP_MAX_RATE);
            }
        }
        let p = plan(&order, EarlyAccess::Auto, &a, &i, &no_tax());
        if i.net_need <= free {
            checked += 1;
            assert_eq!(p.penalized, 0.0, "{p:?}");
            assert_eq!(p.shortfall, 0.0);
            assert_eq!(p.from_hsa_other, [0.0, 0.0]);
            let (done, tx, _) = run(&p, &a, &i);
            assert_eq!(done.penalty_base, 0.0);
            assert_eq!(tx.early_distributions, 0.0);
            assert_eq!(tx.hsa_nonqualified, 0.0);
        } else {
            assert!(p.penalized > 0.0 || p.shortfall > 0.0 || p.from_hsa_other != [0.0, 0.0]);
        }
    }
    assert!(checked > 1_000);
}
