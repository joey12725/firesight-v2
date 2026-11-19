//! Tests of `assumptions.taxes.state_detail: full`: California SDI, New York's
//! recapture of the lower brackets and Utah's Social Security credit. Expected values
//! are computed by hand; the arithmetic is written next to each case.

use super::calc::recapture_extra;
use super::*;
use crate::data::Data;
use crate::model::{StateDetail, TaxModel};

const TX: StateCode = StateCode(*b"TX");
const CA: StateCode = StateCode(*b"CA");
const NY: StateCode = StateCode(*b"NY");
const UT: StateCode = StateCode(*b"UT");

fn law_with(model: TaxModel) -> TaxLaw {
    TaxLaw::from_data(Data::embedded(), &model).expect("embedded tax data loads")
}

fn basic() -> TaxLaw {
    law_with(TaxModel::default())
}

fn full() -> TaxLaw {
    law_with(TaxModel {
        state_detail: StateDetail::Full,
        ..TaxModel::default()
    })
}

fn single(state: StateCode, age: f64) -> TaxInputs {
    TaxInputs {
        filing_status: FilingStatus::Single,
        ages: [age, 0.0],
        adults: 1,
        state,
        ..TaxInputs::default()
    }
}

#[track_caller]
fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 0.01,
        "expected {expected}, got {actual}"
    );
}

/// State tax of the same return under the full and the basic rules.
fn state_tax_pair(state: StateCode, x: &TaxInputs) -> (f64, f64) {
    let idx = Indexation::base(2026);
    let f = compute(&full().for_year(idx, state), x).state;
    let b = compute(&basic().for_year(idx, state), x).state;
    (f, b)
}

#[test]
fn state_detail_is_off_in_the_basic_law() {
    let idx = Indexation::base(2026);
    for state in [CA, NY, UT] {
        let p = basic().for_year(idx, state).state;
        assert_eq!(p.sdi_rate, 0.0);
        assert_eq!(p.recapture_agi_start, 0.0);
        assert_eq!(p.ss_credit_rate, 0.0);
    }
    let ca = full().for_year(idx, CA).state;
    assert_eq!(ca.sdi_rate, 0.013);
    let ny = full().for_year(idx, NY).state;
    assert_eq!(ny.recapture_agi_start, 107_650.0);
    assert_eq!(ny.recapture_first_bracket, [4, 3, 4]);
    let ut = full().for_year(idx, UT).state;
    assert_eq!(ut.ss_credit_phase_out_start, [54_000.0, 90_000.0, 90_000.0]);
}

#[test]
fn states_without_a_detail_rule_are_unchanged_by_full() {
    let idx = Indexation::base(2026);
    for state in [TX, StateCode(*b"CO"), StateCode(*b"FL"), StateCode(*b"OR")] {
        assert_eq!(
            basic().for_year(idx, state).state,
            full().for_year(idx, state).state
        );
    }
}

#[test]
fn california_sdi_is_1_3_percent_of_wages_only() {
    // $100,000 of wages: 0.013 x 100,000 = 1,300 on top of the same income tax.
    let mut x = single(CA, 40.0);
    x.wages = [100_000.0, 0.0];
    let (f, b) = state_tax_pair(CA, &x);
    close(f - b, 1_300.0);
    // No wage cap: $1,000,000 of wages pays 13,000.
    x.wages = [1_000_000.0, 0.0];
    let (f, b) = state_tax_pair(CA, &x);
    close(f - b, 13_000.0);
    // Self-employment profit and pensions are not wages.
    let mut y = single(CA, 40.0);
    y.self_employment = [80_000.0, 0.0];
    y.pensions = 20_000.0;
    let (f, b) = state_tax_pair(CA, &y);
    close(f - b, 0.0);
    // Two earners each pay on their own wages.
    let mut z = x;
    z.filing_status = FilingStatus::MarriedJoint;
    z.ages = [40.0, 40.0];
    z.adults = 2;
    z.wages = [60_000.0, 40_000.0];
    let (f, b) = state_tax_pair(CA, &z);
    close(f - b, 1_300.0);
}

#[test]
fn california_sdi_flows_into_the_total_and_the_explanation() {
    let idx = Indexation::base(2026);
    let mut x = single(CA, 40.0);
    x.wages = [100_000.0, 0.0];
    let p = full().for_year(idx, CA);
    let b = basic().for_year(idx, CA);
    let rf = compute(&p, &x);
    let rb = compute(&b, &x);
    // The federal return is unchanged (the standard deduction beats itemising here),
    // so the whole difference is the SDI.
    assert_eq!(rf.federal_ordinary, rb.federal_ordinary);
    close(rf.total - rb.total, 1_300.0);
    let d = explain(&p, &x);
    close(d.amount("state_payroll_tax").unwrap(), 1_300.0);
    assert!(explain(&b, &x).amount("state_payroll_tax").is_none());
}

/// New York's 2024 schedules (IT-201-I) with the 2024 first tiers, for checking the
/// recapture against the bases printed in the state's own worksheets.
fn ny_2024(pairs: &[(f64, f64)], first: usize) -> StateYear {
    let schedule = Brackets::from_pairs(pairs);
    StateYear {
        code: NY,
        brackets: [schedule; 3],
        recapture_agi_start: 107_650.0,
        recapture_phase_width: 50_000.0,
        recapture_agi_top: 25_000_000.0,
        recapture_first_bracket: [first; 3],
        ..StateYear::default()
    }
}

const NY_2024_JOINT: [(f64, f64); 9] = [
    (0.0, 0.04),
    (17_150.0, 0.045),
    (23_600.0, 0.0525),
    (27_900.0, 0.055),
    (161_550.0, 0.06),
    (323_200.0, 0.0685),
    (2_155_350.0, 0.0965),
    (5_000_000.0, 0.103),
    (25_000_000.0, 0.109),
];

const NY_2024_HOH: [(f64, f64); 9] = [
    (0.0, 0.04),
    (12_800.0, 0.045),
    (17_650.0, 0.0525),
    (20_900.0, 0.055),
    (107_650.0, 0.06),
    (269_300.0, 0.0685),
    (1_616_450.0, 0.0965),
    (5_000_000.0, 0.103),
    (25_000_000.0, 0.109),
];

#[test]
fn new_york_recapture_matches_the_printed_worksheets() {
    let joint = ny_2024(&NY_2024_JOINT, 3);
    // Joint, taxable income 300,000 (6% bracket), AGI far above 161,550 + 50,000:
    // the whole recapture of worksheet 3, base 333 plus increment 807 = 1,140.25
    // (flat 6% x 300,000 = 18,000 less the schedule's 16,859.75).
    close(recapture_extra(&joint, 1, 400_000.0, 300_000.0), 1_140.25);
    // Halfway through the phase-in (AGI 186,550): 332.5 + 807.75 x 0.5.
    close(recapture_extra(&joint, 1, 186_550.0, 300_000.0), 736.375);
    // First tier: taxable income 120,000 pays the 5.5% flat rate once AGI is 157,650:
    // 0.055 x 120,000 = 6,600 less 6,267.5 = 332.5 at full phase-in, half at 132,650.
    close(recapture_extra(&joint, 1, 157_650.0, 120_000.0), 332.5);
    close(recapture_extra(&joint, 1, 132_650.0, 120_000.0), 166.25);
    // Nothing at or below the start.
    assert_eq!(recapture_extra(&joint, 1, 107_650.0, 100_000.0), 0.0);
    assert_eq!(recapture_extra(&joint, 1, 90_000.0, 80_000.0), 0.0);
    // Above 25 million of AGI everything pays 10.9%: 0.109 x 26,000,000 = 2,834,000
    // less the schedule's tax of 2,587,262.75.
    close(
        recapture_extra(&joint, 1, 26_000_000.0, 26_000_000.0),
        246_737.25,
    );

    // Head of household, taxable income 200,000: worksheet 12's 787 (0.06 x 269,300 -
    // 15,371.125 = 786.875, and the same at 200,000 inside the 6% bracket).
    let hoh = ny_2024(&NY_2024_HOH, 4);
    close(recapture_extra(&hoh, 2, 300_000.0, 200_000.0), 786.875);
    // Taxable income 1,000,000: the printed base of 3,076 = 787 + 2,289; exactly
    // 0.0685 x 1,616,450 - 107,650.9 = 3,075.925.
    close(
        recapture_extra(&hoh, 2, 1_400_000.0, 1_000_000.0),
        3_075.925,
    );
}

#[test]
fn new_york_recapture_is_continuous_across_tiers() {
    let joint = ny_2024(&NY_2024_JOINT, 3);
    // At the top of one bracket and the bottom of the next (AGI equal to taxable
    // income, as with no deductions) the recapture is the same: the next tier starts
    // from the base the last one ended with.
    for edge in [161_550.0, 323_200.0, 2_155_350.0] {
        let below = recapture_extra(&joint, 1, edge, edge);
        let above = recapture_extra(&joint, 1, edge + 1.0, edge + 1.0);
        assert!((above - below).abs() < 0.1, "{edge}: {below} {above}");
    }
}

#[test]
fn new_york_recapture_in_the_state_return() {
    // Single in NY on $300,000 of wages, 2026 figures from the data file:
    // taxable income 300,000 - 8,000 = 292,000 in the 6.85% bracket; AGI is 192,350
    // above the 107,650 start, so every tier has phased in. The recapture is
    // flat 5.9% x 215,400 = 12,708.60 less the schedule's 12,140.35 = 568.25, plus
    // 6.85% x 1,077,550 = 73,812.175 less the schedule's 71,197.625 and 568.25 =
    // 2,046.30: 2,614.55.
    let mut x = single(NY, 40.0);
    x.wages = [300_000.0, 0.0];
    let (f, b) = state_tax_pair(NY, &x);
    close(f - b, 2_614.55);
    // $130,000 of wages: taxable income 122,000 in the 5.9% bracket, phase-in
    // (130,000 - 107,650) / 50,000 = 0.447 of the flat-rate benefit of 568.25.
    x.wages = [130_000.0, 0.0];
    let (f, b) = state_tax_pair(NY, &x);
    close(f - b, 568.25 * 0.447);
    // Below the start nothing changes.
    x.wages = [100_000.0, 0.0];
    let (f, b) = state_tax_pair(NY, &x);
    close(f - b, 0.0);
}

#[test]
fn recapture_belongs_to_new_york_only() {
    let nj = StateCode(*b"NJ");
    let mut x = single(nj, 40.0);
    x.wages = [300_000.0, 0.0];
    let (f, b) = state_tax_pair(nj, &x);
    close(f - b, 0.0);
}

#[test]
fn utah_social_security_credit() {
    // Age 70, $20,000 of benefits and a $25,000 pension. Provisional income is
    // 25,000 + 10,000 = 35,000: tier 1 is min(10,000, 0.5 x (34,000 - 25,000) =
    // 4,500, 0.5 x 10,000 = 5,000) = 4,500 and tier 2 is 0.85 x 1,000 = 850, so
    // 5,350 of benefits are taxable. AGI is 30,350, under the 54,000 start, so the
    // credit is the full 4.45% x 5,350 = 238.075.
    let mut x = single(UT, 70.0);
    x.social_security = [20_000.0, 0.0];
    x.pensions = 25_000.0;
    let (f, b) = state_tax_pair(UT, &x);
    close(b - f, 238.075);

    // Benefits of 30,000 and a 40,000 pension: provisional income 55,000; tier 1 =
    // min(15,000, 4,500, 15,000) = 4,500; tier 2 = 0.85 x 21,000 = 17,850: taxable
    // benefits 22,350 (under the 25,500 cap). AGI is 62,350, 8,350 over 54,000, so
    // the credit of 4.45% x 22,350 = 994.575 loses 0.025 x 8,350 = 208.75.
    x.social_security = [30_000.0, 0.0];
    x.pensions = 40_000.0;
    let (f, b) = state_tax_pair(UT, &x);
    close(b - f, 785.825);

    // At $150,000 of income the credit is gone.
    x.pensions = 150_000.0;
    let (f, b) = state_tax_pair(UT, &x);
    close(b - f, 0.0);

    // Nobody on benefits: no credit.
    let mut y = single(UT, 40.0);
    y.wages = [80_000.0, 0.0];
    let (f, b) = state_tax_pair(UT, &y);
    close(b - f, 0.0);
}

#[test]
fn utah_credit_uses_the_joint_threshold_for_couples() {
    // A couple with $40,000 of benefits and a $45,000 pension: provisional income
    // 45,000 + 20,000 = 65,000 against joint thresholds of 32,000 and 44,000: tier 1
    // = min(20,000, 0.5 x 12,000 = 6,000, 0.5 x 33,000) = 6,000; tier 2 = 0.85 x
    // 21,000 = 17,850: 23,850 taxable (cap 34,000). AGI 68,850 is under 90,000, so
    // the full credit 4.45% x 23,850 = 1,061.325 applies (it would be reduced if
    // the single start of 54,000 were used).
    let mut x = single(UT, 70.0);
    x.filing_status = FilingStatus::MarriedJoint;
    x.ages = [70.0, 68.0];
    x.adults = 2;
    x.social_security = [25_000.0, 15_000.0];
    x.pensions = 45_000.0;
    let (f, b) = state_tax_pair(UT, &x);
    close(b - f, 1_061.325);
}
