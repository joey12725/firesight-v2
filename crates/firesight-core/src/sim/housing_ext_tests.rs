//! Unit tests of the rent market and the sale terms, with hand-computed values,
//! and the check that the code's constants equal `params.json`.

use super::*;
use crate::data::Data;
use crate::housing::{arm, insurance, maintenance};

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "expected {expected}, got {actual} (tolerance {tolerance})"
    );
}

#[test]
fn market_stress_blends_recession_and_falling_prices() {
    assert_eq!(market_stress(false, 0.05), 0.0);
    assert_eq!(market_stress(true, 0.05), 0.5);
    assert_eq!(market_stress(false, -0.05), 0.25);
    assert_eq!(market_stress(true, -0.10), 1.0);
    // A fall beyond 10% does not count for more.
    assert_eq!(market_stress(true, -0.30), 1.0);
}

#[test]
fn sale_terms_follow_the_stress() {
    // Calm market, u = 0: the baseline two months and no discount.
    assert_eq!(sale_terms(0.0, 0.0, 0.5), (2, 0.0));
    // Full stress, mean extra 0.5 + 3 = 3.5 months. u = 1 - 1/e gives -ln(1 - u) = 1:
    // extra 3.5 months, floor 3: 5 months. Discount 6% * 1 * (0.5 + 0.5) = 6%.
    let (months, discount) = sale_terms(1.0, 1.0 - crate::math::exp(-1.0), 0.5);
    assert_eq!(months, 5);
    close(discount, 0.06, 1e-12);
    // Capped at two years and a quarter of the price.
    let (months, discount) = sale_terms(1.0, 1.0 - 1e-15, 1.0);
    assert_eq!(months, MAX_CLOSE_MONTHS);
    assert!(discount <= MAX_DISCOUNT);
    // Mean months over many uniforms at stress 0.5 are 2 + about 2 - 0.5 (flooring).
    let n = 20_000;
    let mean: f64 = (0..n)
        .map(|i| f64::from(sale_terms(0.5, (i as f64 + 0.5) / n as f64, 0.5).0))
        .sum::<f64>()
        / n as f64;
    // E[floor(2 X)] for X exponential(1) is 1 / (e^(1/2) - 1) - ... = 1.54: months 3.5.
    close(mean, 2.0 + 1.0 / (crate::math::exp(0.5) - 1.0), 0.05);
}

#[test]
fn rent_renewal_closes_part_of_the_gap_or_resets_to_market() {
    let mut r = RentMarket {
        market: 1.2,
        paid: 1.0,
        ..RentMarket::default()
    };
    // Not forced: paid = 1.0 * 1.2^0.75 = exp(0.75 * 0.182322) = 1.14654.
    assert!(!r.renew(0.5));
    close(r.paid, 1.146_54, 1e-4);
    // Forced move: a new lease at market plus 4%: 1.2 * 1.04.
    assert!(r.renew(0.01));
    close(r.paid, 1.248, 1e-12);
}

#[test]
fn rent_market_trend_with_no_shocks() {
    // Home prices flat in real terms: the real-HPI growth is 0 so the national index
    // grows by (0.005 + 0.15 * (0 - 0.01)) = 0.35% a year; a word of 2^63 is a normal
    // draw of about 0.
    let mut r = RentMarket::default();
    for _ in 0..12 {
        r.step_month(0.0, 0.0, [1 << 63, 0]);
    }
    close(r.log_national, 0.0035, 1e-12);
    close(r.local, 0.0, 1e-9);
    close(r.market, crate::math::exp(0.0035), 1e-9);
    // Prices rising 10% a year in real terms (12 x the monthly rate of 0.79741% is
    // 9.5689%) push rents up: after the smoothed growth settles,
    // 0.005 + 0.15 * (0.095689 - 0.01) = 1.7853% a year.
    let mut r = RentMarket::default();
    let monthly = crate::math::pow(1.10, 1.0 / 12.0) - 1.0;
    for _ in 0..600 {
        r.step_month(monthly, 0.0, [1 << 63, 0]);
    }
    let before = r.log_national;
    for _ in 0..12 {
        r.step_month(monthly, 0.0, [1 << 63, 0]);
    }
    close(r.log_national - before, 0.017_853, 0.0002);
}

#[test]
fn constants_match_the_research_file() {
    let h = &Data::embedded().actuarial["housing_realism"];
    let num = |v: &serde_json::Value| v.as_f64().unwrap();
    let m = &h["maintenance"];
    assert_eq!(
        num(&m["capital_share"]["value"]),
        maintenance::CAPITAL_SHARE
    );
    assert_eq!(
        num(&m["weibull_shape"]["value"]),
        f64::from(maintenance::WEIBULL_SHAPE)
    );
    assert_eq!(
        num(&m["initial_age_span"]["value"]),
        maintenance::INITIAL_AGE_SPAN
    );
    for (name, life, weight) in maintenance::COMPONENTS {
        let c = &m["components"][name];
        assert_eq!(num(&c["life_years"]), life, "{name}");
        assert_eq!(num(&c["budget_share"]), weight, "{name}");
    }
    let i = &h["insurance_drift"];
    assert_eq!(num(&i["mu"]["value"]), insurance::MU);
    assert_eq!(num(&i["rho"]["value"]), insurance::RHO);
    assert_eq!(num(&i["sigma"]["value"]), insurance::SIGMA);
    assert_eq!(num(&i["jump_rate"]["value"]), insurance::JUMP_RATE);
    assert_eq!(
        num(&i["jump_log_median"]["value"]),
        insurance::JUMP_LOG_MEDIAN
    );
    assert_eq!(num(&i["jump_log_sd"]["value"]), insurance::JUMP_LOG_SD);
    let r = &h["rent_market"];
    assert_eq!(num(&r["real_trend"]["value"]), RENT_REAL_TREND);
    assert_eq!(num(&r["hpi_passthrough"]["value"]), HPI_PASSTHROUGH);
    assert_eq!(num(&r["hpi_real_trend"]["value"]), HPI_REAL_TREND);
    assert_eq!(num(&r["local_persistence"]["value"]), LOCAL_PERSISTENCE);
    assert_eq!(num(&r["local_sd"]["value"]), LOCAL_SD);
    assert_eq!(
        num(&r["renewal_pass_through"]["value"]),
        RENEWAL_PASS_THROUGH
    );
    assert_eq!(
        num(&r["forced_move_probability"]["value"]),
        FORCED_MOVE_PROBABILITY
    );
    assert_eq!(num(&r["new_lease_premium"]["value"]), NEW_LEASE_PREMIUM);
    let l = &h["liquidity"];
    assert_eq!(
        num(&l["base_close_months"]["value"]),
        f64::from(BASE_CLOSE_MONTHS)
    );
    assert_eq!(num(&l["base_extra_months"]["value"]), BASE_EXTRA_MONTHS);
    assert_eq!(num(&l["stress_extra_months"]["value"]), STRESS_EXTRA_MONTHS);
    assert_eq!(
        num(&l["full_stress_discount"]["value"]),
        FULL_STRESS_DISCOUNT
    );
    assert_eq!(
        num(&l["full_stress_price_fall"]["value"]),
        FULL_STRESS_PRICE_FALL
    );
    assert_eq!(
        num(&h["arm"]["initial_discount_to_fixed"]["value"]),
        arm::INITIAL_DISCOUNT
    );
    // The figures recalled from memory are also listed where a reviewer looks for
    // them.
    let recalled = &Data::embedded().actuarial["unverified_from_memory"]["housing_realism"];
    for (name, life, _) in maintenance::COMPONENTS {
        assert_eq!(
            num(&recalled["home_component_life_years"][name]),
            life,
            "{name}"
        );
    }
    assert_eq!(
        num(&recalled["arm_initial_discount_to_fixed"]),
        arm::INITIAL_DISCOUNT
    );
    // Every entry names a source or says it is a judgement.
    fn check(v: &serde_json::Value, path: &str) {
        if let Some(map) = v.as_object() {
            if map.contains_key("value") {
                assert!(
                    map.contains_key("source") || map.contains_key("judgement"),
                    "{path}: no source and not marked as a judgement"
                );
                assert!(map.contains_key("confidence"), "{path}: no confidence");
            } else {
                for (k, x) in map {
                    check(x, &format!("{path}/{k}"));
                }
            }
        }
    }
    check(&h["maintenance"]["capital_share"], "capital_share");
    for k in ["insurance_drift", "rent_market", "liquidity", "arm"] {
        check(&h[k], k);
    }
}
