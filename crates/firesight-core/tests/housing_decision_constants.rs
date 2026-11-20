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
