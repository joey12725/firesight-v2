//! The parameters of the opt-in housing decisions are listed, with their sources and
//! a confidence (judgements say so), in `data/actuarial/params.json` under
//! `housing_decisions`. This test keeps the code and that section equal.

use firesight_core::data::Data;
use firesight_core::home_equity;
use firesight_core::model::Heloc;

fn section() -> serde_json::Value {
    Data::embedded().actuarial["housing_decisions"].clone()
}

fn value(v: &serde_json::Value) -> f64 {
    v["value"].as_f64().unwrap()
}

/// Every entry carries a source, and a confidence that is one of the known levels.
fn check_sourced(v: &serde_json::Value, path: &str) {
    if let Some(obj) = v.as_object() {
        if obj.contains_key("value") {
            let source = obj["source"].as_str().unwrap_or("");
            assert!(source.len() > 20, "{path}: no source");
            let confidence = obj["confidence"].as_str().unwrap_or("");
            assert!(
                ["high", "medium", "low", "judgement"].contains(&confidence),
                "{path}: confidence {confidence:?}"
            );
            if confidence == "judgement" {
                assert!(source.starts_with("Judgement"), "{path}");
            }
            return;
        }
        for (k, child) in obj {
            if k != "summary" {
                check_sourced(child, &format!("{path}/{k}"));
            }
        }
    }
}

#[test]
fn every_housing_parameter_has_a_source() {
    let s = section();
    assert!(s.is_object());
    check_sourced(&s, "housing_decisions");
}

#[test]
fn heloc_defaults_match_the_data_file() {
    let h = &section()["heloc"];
    let d = Heloc::default();
    assert_eq!(d.max_combined_ltv, value(&h["default_max_combined_ltv"]));
    assert_eq!(d.spread, value(&h["default_spread_over_bills"]));
    assert_eq!(
        d.freeze_probability,
        value(&h["default_freeze_probability"])
    );
    let years = h["draw_and_repay_years"]["value"].as_array().unwrap();
    assert_eq!(d.draw_years, years[0].as_f64().unwrap());
    assert_eq!(d.repay_years, years[1].as_f64().unwrap());
    assert_eq!(
        home_equity::HOUSING_SLUMP_DRAWDOWN,
        value(&h["housing_slump_drawdown"])
    );
}

#[test]
fn hecm_constants_match_the_data_file() {
    use firesight_core::model::ReverseMortgage;
    let h = &section()["hecm"];
    assert_eq!(home_equity::HECM_UPFRONT_MIP, value(&h["upfront_mip"]));
    assert_eq!(home_equity::HECM_ANNUAL_MIP, value(&h["annual_mip"]));
    assert_eq!(
        home_equity::HECM_MAX_CLAIM_2026,
        value(&h["max_claim_2026"])
    );
    assert_eq!(home_equity::HECM_MIN_AGE, value(&h["min_age"]));
    assert_eq!(
        home_equity::HECM_EXPECTED_RATE_FLOOR,
        value(&h["expected_rate_floor"])
    );
    assert_eq!(
        home_equity::HECM_FIRST_YEAR_SHARE,
        value(&h["first_year_share"])
    );
    let fee: Vec<f64> = h["origination_fee"]["value"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    assert_eq!(
        fee,
        vec![
            home_equity::HECM_ORIGINATION_MIN,
            home_equity::HECM_ORIGINATION_MAX,
            home_equity::HECM_ORIGINATION_TIER
        ]
    );
    let plf = &h["principal_limit_factors"];
    for (i, row) in plf["value"].as_array().unwrap().iter().enumerate() {
        for (j, v) in row.as_array().unwrap().iter().enumerate() {
            assert_eq!(home_equity::PLF_TABLE[i][j], v.as_f64().unwrap(), "{i} {j}");
        }
    }
    for (i, a) in plf["ages"].as_array().unwrap().iter().enumerate() {
        assert_eq!(home_equity::PLF_AGES[i], a.as_f64().unwrap());
    }
    for (j, r) in plf["expected_rates"].as_array().unwrap().iter().enumerate() {
        assert_eq!(home_equity::PLF_RATES[j], r.as_f64().unwrap());
    }
    let d = ReverseMortgage::default();
    assert_eq!(d.lender_margin, value(&h["default_lender_margin"]));
    assert_eq!(
        d.other_closing_costs,
        value(&h["default_other_closing_costs"])
    );
}

#[test]
fn downsize_constants_match_the_data_file() {
    use firesight_core::model::Downsize;
    use firesight_core::sim::downsize_constants as c;
    let h = &section()["downsize"];
    let d = Downsize::default();
    assert_eq!(d.to_value_fraction, value(&h["default_to_value_fraction"]));
    assert_eq!(
        d.selling_cost_fraction,
        value(&h["default_selling_cost_fraction"])
    );
    assert_eq!(
        d.buying_cost_fraction,
        value(&h["default_buying_cost_fraction"])
    );
    assert_eq!(c::DOWNSIZE_MIN_DOWN, value(&h["min_down_payment"]));
    assert_eq!(f64::from(c::DOWNSIZE_LOAN_MONTHS), value(&h["loan_months"]));
    assert_eq!(
        f64::from(c::WIDOWED_DELAY_MONTHS),
        value(&h["widowed_delay_months"])
    );
    let ages = h["children_leave_ages"]["value"].as_array().unwrap();
    assert_eq!(f64::from(c::LEAVE_AGE), ages[0].as_f64().unwrap());
    assert_eq!(f64::from(c::LEAVE_AGE_COLLEGE), ages[1].as_f64().unwrap());
}

#[test]
fn cash_out_constants_match_the_data_file() {
    use firesight_core::model::CashOutRefinance;
    use firesight_core::sim::cash_out_constants as c;
    let h = &section()["cash_out_refinance"];
    let d = CashOutRefinance::default();
    assert_eq!(d.max_ltv, value(&h["default_max_ltv"]));
    assert_eq!(d.rate_premium, value(&h["default_rate_premium"]));
    assert_eq!(
        f64::from(c::CASH_OUT_MIN_MONTHS),
        value(&h["min_months_between"])
    );
    assert_eq!(
        c::CASH_OUT_RAISE_MONTHS,
        value(&h["raise_months_of_spending"])
    );
    assert_eq!(c::CASH_OUT_MIN_CASH, value(&h["min_cash"]));
}

#[test]
fn rental_dynamics_constants_match_the_data_file() {
    use firesight_core::model::RentalDynamics;
    use firesight_core::rental_market as r;
    let h = &section()["rental_dynamics"];
    let d: RentalDynamics = serde_json::from_str(r#"{"type":"realistic"}"#).unwrap();
    assert_eq!(
        d,
        RentalDynamics::Realistic {
            annual_turnover: value(&h["default_annual_turnover"])
        }
    );
    assert_eq!(
        r::RECESSION_SPELL_FACTOR,
        value(&h["recession_spell_factor"])
    );
    assert_eq!(r::RECESSION_SHARE, value(&h["recession_share"]));
    assert_eq!(
        r::RENT_INDEX_ANNUAL_PERSISTENCE,
        value(&h["rent_index_annual_persistence"])
    );
    assert_eq!(r::RENT_INDEX_SD, value(&h["rent_index_sd"]));
    assert_eq!(r::EVICTION_ANNUAL, value(&h["eviction_annual"]));
    assert_eq!(r::EVICTION_COST_MONTHS, value(&h["eviction_cost_months"]));
    assert_eq!(r::CAPEX_SHARE, value(&h["capex_share"]));
    assert_eq!(
        r::CAPEX_MONTHLY_PROBABILITY,
        value(&h["capex_monthly_probability"])
    );
}
