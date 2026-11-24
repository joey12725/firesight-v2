//! Tests of the opt-in housing realism mechanisms, with hand-computed values.

use super::*;
use crate::model::{Assessment, InsuranceGrowth, MaintenanceModel, Mortgage, MortgageKind};
use crate::rng::Stream;

fn data() -> &'static Data {
    Data::embedded()
}

fn rng() -> Rng {
    Rng::stream(11, 0, Stream::Housing(0))
}

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "expected {expected}, got {actual} (tolerance {tolerance})"
    );
}

/// A still, noiseless home that costs only what the test turns on.
fn bare(value: f64) -> Property {
    Property {
        id: "home".into(),
        value,
        property_tax_rate: Some(0.012),
        insurance_rate: 0.0,
        maintenance_rate: 0.0,
        idiosyncratic_volatility: 0.0,
        ..Property::default()
    }
}

// ---- F11 lumpy maintenance -------------------------------------------------------

#[test]
fn component_shares_sum_to_one_and_sizes_follow_the_formula() {
    let total: f64 = maintenance::COMPONENTS.iter().map(|c| c.2).sum();
    close(total, 1.0, 1e-12);
    // Roof: 0.30 * 0.5 * 0.01 * 25 years = 3.75% of the home's value.
    close(maintenance::repair_share(0, 0.01), 0.0375, 1e-12);
    // All five: (7.5 + 4.25 + 0.55 + 2.4 + 2.6) * 0.5 * 0.01 = 8.65% of the value, one
    // of each every 11 to 25 years.
    let sum: f64 = (0..5).map(|i| maintenance::repair_share(i, 0.01)).sum();
    close(sum, 0.0865, 1e-12);
}

#[test]
fn failure_probability_matches_the_weibull_hazard() {
    // Shape 3, mean life 25: scale 25 / 0.8929795 = 27.99. At age = scale, the
    // chance of failing in the next month is 1 - exp(-((1 + x)^3 - 1)) with
    // x = (1/12) / 27.99 = 0.0029774: (1 + x)^3 - 1 = 0.0089583.
    let scale = 25.0 / 0.892_979_511_569_249_2;
    let p = maintenance::monthly_failure_probability(25.0, scale);
    close(p, 0.008_918, 2e-6);
    // A new roof essentially never fails in its first month.
    assert!(maintenance::monthly_failure_probability(25.0, 0.0) < 1e-7);
    // The hazard rises with age.
    assert!(
        maintenance::monthly_failure_probability(25.0, 30.0)
            > maintenance::monthly_failure_probability(25.0, 10.0)
    );
}

#[test]
fn the_weibull_lifetime_has_the_stated_mean() {
    // Integrate the survival function month by month: the mean must be the life.
    for life in [11.0, 25.0] {
        let mut survive = 1.0;
        let mut mean = 0.0;
        for m in 0..(12 * 100) {
            let age = m as f64 / 12.0;
            mean += survive / 12.0;
            survive *= 1.0 - maintenance::monthly_failure_probability(life, age);
        }
        close(mean, life, 0.02 * life);
    }
}

#[test]
fn a_repair_costs_the_formula_and_resets_the_age() {
    // All-zero words: every component draws u = 0 and so fails at once, with initial
    // ages of 0. The bill is 8.65% of the 440,000 value = 38,060.
    let mut st = maintenance::MaintState::default();
    let words = [0u64; maintenance::DRAWS_PER_MONTH];
    let cost = maintenance::advance(&mut st, 0.01, 440_000.0, &words);
    close(cost, 38_060.0, 1e-6);
    assert!(st.age.iter().all(|a| *a == 0.0));
    // With words that never fail (all ones, u close to 1), ages advance a month.
    let words = [u64::MAX; maintenance::DRAWS_PER_MONTH];
    let cost = maintenance::advance(&mut st, 0.01, 440_000.0, &words);
    assert_eq!(cost, 0.0);
    close(st.age[0], 1.0 / 12.0, 1e-12);
}

#[test]
fn lumpy_maintenance_charges_half_smoothly_and_the_repair_on_top() {
    let mut p = bare(400_000.0);
    p.maintenance_rate = 0.01;
    p.maintenance = MaintenanceModel::Lumpy;
    let model = PropertyModel::from_property(&p, 0.012, data());
    assert!(model.has_lumpy_maintenance());
    let mut st = model.initial_state();
    st.ext.repair = 1_000.0;
    let hm = model.step(&mut st, 0.0, 1.0, &mut rng());
    // Routine: 400,000 * 0.01 * 0.5 / 12 = 166.67, plus the 1,000 repair.
    close(
        hm.maintenance,
        400_000.0 * 0.01 * 0.5 / 12.0 + 1_000.0,
        1e-9,
    );
    assert_eq!(st.ext.repair, 0.0);
    // The next month has no repair.
    let hm = model.step(&mut st, 0.0, 1.0, &mut rng());
    close(hm.maintenance, 400_000.0 * 0.01 * 0.5 / 12.0, 1e-9);
}

#[test]
fn smooth_maintenance_is_unchanged() {
    let mut p = bare(400_000.0);
    p.maintenance_rate = 0.01;
    let model = PropertyModel::from_property(&p, 0.012, data());
    assert!(!model.has_ext());
    let mut st = model.initial_state();
    let hm = model.step(&mut st, 0.0, 1.0, &mut rng());
    close(hm.maintenance, 400_000.0 * 0.01 / 12.0, 1e-12);
}

#[test]
fn average_repair_cost_equals_the_capital_share_of_the_rate() {
    // 400 homes for 40 years: mean annual repair cost against 0.5 * 1% * 400,000 =
    // 2,000 a year (no inflation, no change of value).
    let mut total = 0.0;
    let homes = 400;
    for path in 0..homes {
        let mut r = Rng::stream(3, path, Stream::Maintenance(0));
        let mut st = maintenance::MaintState::default();
        for _ in 0..(40 * 12) {
            let mut w = [0u64; maintenance::DRAWS_PER_MONTH];
            for x in &mut w {
                *x = r.next_u64();
            }
            total += maintenance::advance(&mut st, 0.01, 400_000.0, &w);
        }
    }
    let mean_year = total / (homes as f64 * 40.0);
    // The initial ages are only an approximation of the stationary mix, and 400
    // homes leave sampling error of about 4%: accept 10%.
    close(mean_year, 2_000.0, 200.0);
}

// ---- F12 assessment and insurance -------------------------------------------------

#[test]
fn capped_assessment_rises_two_percent_a_year_from_the_basis() {
    let mut p = bare(1_000_000.0);
    p.basis = Some(400_000.0);
    p.assessment = Assessment::Capped {
        annual_cap: 0.02,
        assessed_value: None,
    };
    let model = PropertyModel::from_property(&p, 0.012, data());
    let mut st = model.initial_state();
    assert_eq!(st.assessed_value, 400_000.0);
    // The first year is taxed on the assessed value of today: 400,000 * 1.2% / 12.
    for _ in 0..12 {
        let hm = model.step(&mut st, 0.0, 1.0, &mut rng());
        close(hm.property_tax, 400.0, 1e-9);
    }
    // The next assessment: 400,000 * 1.02 = 408,000; the one after, 416,160.
    for _ in 0..12 {
        let hm = model.step(&mut st, 0.0, 1.0, &mut rng());
        close(hm.property_tax, 408.0, 1e-9);
    }
    let hm = model.step(&mut st, 0.0, 1.0, &mut rng());
    close(hm.property_tax, 416.16, 1e-9);
}

#[test]
fn capped_assessment_returns_to_the_capped_base_when_prices_recover() {
    let mut p = bare(1_000_000.0);
    p.assessment = Assessment::Capped {
        annual_cap: 0.02,
        assessed_value: Some(900_000.0),
    };
    let model = PropertyModel::from_property(&p, 0.012, data());
    let mut st = model.initial_state();
    // Year 1: the market is below the base of 900,000, so tax is on the market value.
    st.value = 700_000.0;
    let hm = model.step(&mut st, 0.0, 1.0, &mut rng());
    close(hm.property_tax, 700.0, 1e-9);
    for _ in 0..11 {
        model.step(&mut st, 0.0, 1.0, &mut rng());
    }
    // Year 2: prices are back. The assessment is the base grown by the cap,
    // 900,000 * 1.02 = 918,000, not 2% above last year's lowered 700,000.
    st.value = 1_000_000.0;
    let hm = model.step(&mut st, 0.0, 1.0, &mut rng());
    close(hm.property_tax, 918.0, 1e-9);
}

#[test]
fn capped_assessment_follows_the_market_down() {
    let mut p = bare(1_000_000.0);
    p.assessment = Assessment::Capped {
        annual_cap: 0.02,
        assessed_value: Some(900_000.0),
    };
    let model = PropertyModel::from_property(&p, 0.012, data());
    let mut st = model.initial_state();
    assert_eq!(st.assessed_value, 900_000.0);
    // The market drops below the assessed value before the next assessment: the
    // assessment is the lesser.
    st.value = 700_000.0;
    let hm = model.step(&mut st, 0.0, 1.0, &mut rng());
    close(hm.property_tax, 700_000.0 * 0.012 / 12.0, 1e-9);
}

#[test]
fn market_assessment_is_unchanged_and_a_purchase_is_assessed_at_its_price() {
    let model = PropertyModel::from_property(&bare(500_000.0), 0.012, data());
    let mut st = model.initial_state();
    let hm = model.step(&mut st, 0.0, 1.0, &mut rng());
    close(hm.property_tax, 500.0, 1e-9);

    let event = BuyHome {
        price: 800_000.0,
        rate: Some(0.06),
        idiosyncratic_volatility: 0.0,
        property_tax_rate: Some(0.012),
        insurance_rate: 0.0,
        maintenance_rate: 0.0,
        assessment: Assessment::Capped {
            annual_cap: 0.02,
            assessed_value: None,
        },
        ..BuyHome::default()
    };
    let model = PropertyModel::from_purchase(&event, 0.012, data());
    let mut st = model.initial_state();
    // Bought when prices are 1.25 times the start: assessed at 1,000,000 and kept
    // there by a 2% cap (tax 1,000,000 * 1.2% / 12 = 1,000).
    model.buy(&mut st, 0, 1.25, 0.065);
    assert_eq!(st.assessed_value, 1_000_000.0);
    let hm = model.step(&mut st, 0.0, 1.0, &mut rng());
    close(hm.property_tax, 1_000.0, 1e-9);
}

#[test]
fn insurance_index_scales_the_premium() {
    let mut p = bare(400_000.0);
    p.insurance_rate = 0.005;
    p.insurance_real_growth = InsuranceGrowth::Stochastic;
    let model = PropertyModel::from_property(&p, 0.012, data());
    let mut st = model.initial_state();
    // Before the index has run, the premium is the plain one: 400,000 * 0.5% / 12.
    let hm = model.step(&mut st, 0.0, 1.0, &mut rng());
    close(hm.insurance, 166.666_666_666, 1e-6);
    st.ext.ins_factor = 1.5;
    let hm = model.step(&mut st, 0.0, 1.0, &mut rng());
    close(hm.insurance, 250.0, 1e-6);
}

#[test]
fn insurance_index_drifts_up_with_jumps_and_stays_bounded() {
    // Mean log drift per year: MU + JUMP_RATE * E[jump] = 0.005 + 0.08 * exp(-2.2 + 0.125)
    // = 0.005 + 0.08 * 0.1256 = 0.01505. Over 10 years: 0.1505.
    let mut sum = 0.0;
    let paths = 1_500;
    for path in 0..paths {
        let mut r = Rng::stream(5, path, Stream::InsuranceDrift);
        let mut st = insurance::InsState::default();
        for _ in 0..120 {
            let w = [r.next_u64(), r.next_u64(), r.next_u64()];
            insurance::advance(&mut st, &w);
            let (lo, hi) = insurance::FACTOR_RANGE;
            assert!(st.factor >= lo && st.factor <= hi);
        }
        sum += st.log_index;
    }
    close(sum / paths as f64, 0.1505, 0.02);
}

// ---- F15 adjustable-rate mortgages ------------------------------------------------

fn arm(fixed: u32, every: u32) -> MortgageKind {
    MortgageKind::Arm {
        fixed_months: fixed,
        adjust_every_months: every,
        margin: 0.0275,
        periodic_cap: 0.02,
        lifetime_cap: 0.05,
    }
}

#[test]
fn reset_months_follow_the_fixed_period() {
    let terms = ArmTerms::from_kind(arm(60, 12)).unwrap();
    let mut s = ArmState::new(terms, 0.04, 360);
    let due: Vec<u32> = (0..80)
        .filter(|age| {
            s.age = *age;
            s.reset_due()
        })
        .collect();
    assert_eq!(due, vec![60, 72]);
    assert!(ArmTerms::from_kind(MortgageKind::Fixed).is_none());
}

#[test]
fn reset_rate_applies_margin_caps_and_floor() {
    let terms = ArmTerms::from_kind(arm(0, 12)).unwrap();
    let s = ArmState::new(terms, 0.05, 360);
    // Index 4.5% + margin 2.75% = 7.25%, but the periodic cap allows 5% + 2% = 7%.
    close(s.reset_rate(0.05, 0.045), 0.07, 1e-12);
    // Index 1%: fully indexed 3.75% but the cap on falls is 5% - 2% = 3%... which is
    // above the floor of 2.75% and the fully indexed rate, so 3.75% applies.
    close(s.reset_rate(0.05, 0.01), 0.0375, 1e-12);
    // Index 0: fully indexed 2.75%, periodic floor 3%: the rate falls only to 3%.
    close(s.reset_rate(0.05, 0.0), 0.03, 1e-12);
    // Current 3%, index 0: floor is the margin, 2.75%.
    close(s.reset_rate(0.03, 0.0), 0.0275, 1e-12);
    // Lifetime cap: starting 5%, now 9%, index 12%: periodic would allow 11%, the
    // lifetime cap stops it at 10%.
    close(s.reset_rate(0.09, 0.12), 0.10, 1e-12);
}

fn arm_home(kind: MortgageKind) -> Property {
    Property {
        mortgage: Some(Mortgage {
            balance: 300_000.0,
            rate: 0.04,
            remaining_months: 360,
            payment: None,
            kind,
        }),
        basis: Some(600_000.0),
        ..bare(600_000.0)
    }
}

#[test]
fn an_arm_resets_and_reamortises_over_the_remaining_term() {
    let model = PropertyModel::from_property(&arm_home(arm(0, 12)), 0.012, data());
    assert!(model.has_arm());
    let mut st = model.initial_state();
    let first_payment = st.mortgage.unwrap().payment;
    close(first_payment, amortized_payment(300_000.0, 0.04, 360), 1e-9);

    // Month 1 is a reset month. Index 5% + 2.75% = 7.75% but the cap allows 6%; the
    // payment on 300,000 over 360 months at 6% is 1,798.65 (published table).
    st.ext.index = 0.05;
    let hm = model.step(&mut st, 0.0, 1.0, &mut rng());
    let m = st.mortgage.unwrap();
    close(m.rate, 0.06, 1e-12);
    close(m.payment, 1_798.65, 0.01);
    close(hm.interest, 1_500.0, 1e-6);
    close(hm.principal, 1_798.65 - 1_500.0, 0.01);

    // Eleven more months: no reset in between.
    for _ in 0..11 {
        st.ext.index = 0.0;
        model.step(&mut st, 0.0, 1.0, &mut rng());
        assert_eq!(st.mortgage.unwrap().rate, 0.06);
    }
    // Month 13: reset to index 1% + 2.75% = 3.75%, limited to 6% - 2% = 4% from
    // below: 3.75 < 4 so the rate is 4%. The payment repays the balance over the 348
    // months left.
    let balance = st.mortgage.unwrap().balance;
    st.ext.index = 0.01;
    model.step(&mut st, 0.0, 1.0, &mut rng());
    let m = st.mortgage.unwrap();
    close(m.rate, 0.04, 1e-12);
    close(m.payment, amortized_payment(balance, 0.04, 348), 1e-9);
}

#[test]
fn a_fixed_loan_ignores_the_index() {
    let model = PropertyModel::from_property(&arm_home(MortgageKind::Fixed), 0.012, data());
    let mut st = model.initial_state();
    let before = st.mortgage.unwrap();
    st.ext.index = 0.09;
    model.step(&mut st, 0.0, 1.0, &mut rng());
    let after = st.mortgage.unwrap();
    assert_eq!(after.rate, before.rate);
    assert_eq!(after.payment, before.payment);
}

#[test]
fn an_arm_bought_without_a_rate_starts_below_the_market_rate() {
    let event = BuyHome {
        price: 500_000.0,
        idiosyncratic_volatility: 0.0,
        mortgage_kind: arm(60, 12),
        ..BuyHome::default()
    };
    let model = PropertyModel::from_purchase(&event, 0.012, data());
    let mut st = model.initial_state();
    let q = model.buy(&mut st, 0, 1.0, 0.07);
    close(q.rate, 0.07 - 0.0075, 1e-12);
    let a = st.mortgage.unwrap().arm.unwrap();
    assert_eq!(a.remaining, 360);
    close(a.start_rate, 0.0625, 1e-12);
}

#[test]
fn refinancing_an_arm_makes_it_fixed_over_the_remaining_term() {
    let mut p = arm_home(arm(60, 12));
    p.mortgage = Some(Mortgage {
        balance: 300_000.0,
        rate: 0.07,
        remaining_months: 300,
        payment: None,
        kind: arm(60, 12),
    });
    let model = PropertyModel::from_property(&p, 0.012, data());
    let mut st = model.initial_state();
    // The market is two points lower: refinancing pays (2% of 300,000 for 4 years is
    // 24,000, against 6,000 of costs). 300,000 at 5% over 300 months is 1,753.77.
    assert!(st.refinance(0.05, 0.01, 6_000.0, 4.0));
    let m = st.mortgage.unwrap();
    assert!(m.arm.is_none());
    close(m.rate, 0.05, 1e-12);
    close(m.payment, 1_753.77, 0.01);
}

// ---- F14 slow home sales -----------------------------------------------------------

#[test]
fn a_discounted_sale_fetches_the_factor_times_the_price() {
    let model = PropertyModel::from_property(&arm_home(MortgageKind::Fixed), 0.012, data());
    let mut a = model.initial_state();
    let mut b = a;
    // 600,000 home with a 300,000 loan and 6% costs: 600,000 * 0.94 - 300,000 =
    // 264,000. At 5% below market: 570,000 * 0.94 - 300,000 = 235,800.
    let full = model.sell(&mut a, 0, 0.06, 0.0);
    close(full.price, 600_000.0, 1e-6);
    close(full.net_proceeds, 264_000.0, 1e-6);
    let cut = model.sell_discounted(&mut b, 0, 0.06, 0.0, 0.95);
    close(cut.price, 570_000.0, 1e-6);
    close(cut.selling_costs, 34_200.0, 1e-6);
    close(cut.mortgage_payoff, 300_000.0, 1e-6);
    close(cut.net_proceeds, 235_800.0, 1e-6);
    assert!(!b.owned);
    // A factor of one is the plain sale.
    let mut c = model.initial_state();
    assert_eq!(model.sell_discounted(&mut c, 0, 0.06, 0.0, 1.0), full);
}
