//! The judgement constants of the career-realism features are listed in
//! `data/actuarial/params.json` under `unverified_from_memory.career_realism`, so a
//! reviewer can find them in one place (same convention as `unverified_constants.rs`).
//! This test keeps the code and that list equal.

use firesight_core::career_realism::{disability, late_career, mobility, recession_pay};
use firesight_core::data::Data;

fn numbers(v: &serde_json::Value) -> Vec<f64> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_f64().unwrap())
        .collect()
}

fn section() -> &'static serde_json::Value {
    &Data::embedded().actuarial["unverified_from_memory"]["career_realism"]
}

#[test]
fn section_is_labelled_as_judgement() {
    assert!(section()["note"].as_str().unwrap().contains("JUDGEMENT"));
}

#[test]
fn job_mobility_constants_match_the_data_file() {
    let m = &section()["job_mobility"];
    assert_eq!(
        mobility::ANNUAL_MOVE_PROBABILITY.to_vec(),
        numbers(&m["annual_move_probability_by_age_band"])
    );
    assert_eq!(
        mobility::MEAN_LOG_JUMP.to_vec(),
        numbers(&m["mean_log_jump_by_age_band"])
    );
    let num = |k: &str| m[k].as_f64().unwrap();
    assert_eq!(
        mobility::HIGH_LEVEL_MULTIPLIER,
        num("high_level_multiplier")
    );
    assert_eq!(
        mobility::MAX_ANNUAL_PROBABILITY,
        num("max_annual_probability")
    );
    assert_eq!(
        mobility::RECESSION_RATE_MULTIPLIER,
        num("recession_rate_multiplier")
    );
    assert_eq!(
        mobility::RECESSION_JUMP_PENALTY,
        num("recession_jump_penalty")
    );
    assert_eq!(mobility::JUMP_SD, num("jump_sd"));
}

#[test]
fn recession_pay_constants_match_the_data_file() {
    let m = &section()["recession_pay"];
    let num = |k: &str| m[k].as_f64().unwrap();
    assert_eq!(
        recession_pay::ANNUAL_FREEZE_PROBABILITY,
        num("annual_freeze_probability")
    );
    assert_eq!(
        recession_pay::ANNUAL_CUT_PROBABILITY,
        num("annual_cut_probability")
    );
    assert_eq!(recession_pay::CUT_MIN, num("cut_min"));
    assert_eq!(recession_pay::CUT_MAX, num("cut_max"));
    assert_eq!(f64::from(recession_pay::CUT_MONTHS), num("cut_months"));
    assert_eq!(
        f64::from(recession_pay::FREEZE_COOLDOWN_MONTHS),
        num("freeze_cooldown_months")
    );
    assert_eq!(recession_pay::BONUS_FACTOR, num("bonus_factor"));
    assert_eq!(
        f64::from(recession_pay::BONUS_SLACK_MONTHS),
        num("bonus_slack_months")
    );
}

#[test]
fn disability_constants_match_the_data_file() {
    let m = &section()["disability"];
    let num = |k: &str| m[k].as_f64().unwrap();
    assert_eq!(
        disability::BAND_FROM_AGE.to_vec(),
        numbers(&m["band_from_age"])
    );
    assert_eq!(
        disability::INCIDENCE_PER_1000.to_vec(),
        numbers(&m["incidence_per_1000_by_age_band"])
    );
    assert_eq!(disability::ANNUAL_RECOVERY, num("annual_recovery"));
    assert_eq!(disability::RETURN_SCAR, num("return_scar"));
    assert_eq!(
        f64::from(disability::SSDI_WAIT_MONTHS),
        num("ssdi_wait_months")
    );
    assert_eq!(
        f64::from(disability::LTD_WAIT_MONTHS),
        num("ltd_wait_months")
    );
    assert_eq!(
        disability::MIN_INSURED_YEARS as f64,
        num("min_insured_years")
    );
    assert_eq!(disability::MIN_ONSET_AGE, num("min_onset_age"));
}

#[test]
fn late_career_constants_match_the_data_file() {
    let m = &section()["late_career"];
    let num = |k: &str| m[k].as_f64().unwrap();
    assert_eq!(late_career::FROM_AGE, num("from_age"));
    assert_eq!(
        late_career::SPELL_MULTIPLIER.to_vec(),
        numbers(&m["spell_multiplier_by_age_band"])
    );
    assert_eq!(late_career::SCAR_MULTIPLIER, num("scar_multiplier"));
    assert_eq!(late_career::NEVER_AT_55, num("never_probability_at_55"));
    assert_eq!(
        late_career::NEVER_PER_YEAR,
        num("never_probability_per_year")
    );
    assert_eq!(late_career::NEVER_MAX, num("never_probability_max"));
    assert_eq!(late_career::SPELL_CAP_MONTHS, num("spell_cap_months"));
}
