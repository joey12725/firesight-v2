//! Tests of the tax module. Expected values are computed by hand from the 2026 figures
//! in `data/tax/` (the arithmetic is written out next to each case).

use super::*;
use crate::data::Data;
use crate::model::{TaxKind, TaxModel};

const TX: StateCode = StateCode(*b"TX");
const CA: StateCode = StateCode(*b"CA");
const NY: StateCode = StateCode(*b"NY");

fn law() -> TaxLaw {
    TaxLaw::from_data(Data::embedded(), &TaxModel::default()).expect("embedded tax data loads")
}

/// 2026 parameters for a state.
fn params(state: StateCode) -> TaxYearParams {
    law().for_year(Indexation::base(2026), state)
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

fn joint(state: StateCode, ages: [f64; 2]) -> TaxInputs {
    TaxInputs {
        filing_status: FilingStatus::MarriedJoint,
        ages,
        adults: 2,
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

// ---------------------------------------------------------------- loading

#[test]
fn loads_the_embedded_law() {
    let law = law();
    assert_eq!(law.base_year, 2026);
    let p = law.for_year(Indexation::base(2026), TX);
    let f = &p.federal;
    assert_eq!(f.standard_deduction, [16_100.0, 32_200.0, 24_150.0]);
    assert_eq!(f.additional_deduction_65, [2_050.0, 1_650.0, 2_050.0]);
    assert_eq!(f.ordinary[0].len, 7);
    assert_eq!(f.ordinary[0].lower[1], 12_400.0);
    assert_eq!(f.ordinary[0].lower[4], 201_775.0);
    assert_eq!(f.ordinary[1].lower[6], 768_700.0);
    assert_eq!(f.ordinary[2].lower[2], 67_450.0);
    assert_eq!(f.capital_gains[0].lower[..3], [0.0, 49_450.0, 545_500.0]);
    assert_eq!(f.capital_gains[1].rate[..3], [0.0, 0.15, 0.20]);
    assert_eq!(f.oasdi_wage_base, 184_500.0);
    assert_eq!(f.niit_threshold, [200_000.0, 250_000.0, 200_000.0]);
    assert_eq!(f.child_tax_credit, 2_200.0);
    assert_eq!(f.child_credit_refundable_max, 1_700.0);
    assert_eq!(f.salt_cap, 40_400.0);
    assert_eq!(f.salt_phase_down_threshold, 505_000.0);
    assert_eq!(f.senior_deduction, 6_000.0);
    assert_eq!(f.capital_loss_limit, 3_000.0);
    assert!((f.itemized_limit_fraction - 2.0 / 37.0).abs() < 1e-15);
    // 2026 coverage uses the 2025 poverty guideline.
    assert_eq!((f.fpl_base, f.fpl_per_person), (15_650.0, 5_500.0));
    assert!(f.aca.cliff);
    assert_eq!(f.aca.len, 6);
    assert_eq!(f.limits.k401_employee, 24_500.0);
    assert_eq!(f.limits.k401_catch_up, 8_000.0);
    assert_eq!(f.limits.k401_catch_up_60_63, 11_250.0);
    assert_eq!(f.limits.k401_total, 72_000.0);
    assert_eq!(f.limits.ira, 7_500.0);
    assert_eq!(f.limits.hsa_family, 8_750.0);
    assert_eq!(
        f.limits.roth_ira_phaseout_start,
        [153_000.0, 242_000.0, 153_000.0]
    );
    assert_eq!(
        f.limits.roth_ira_phaseout_end,
        [168_000.0, 252_000.0, 168_000.0]
    );
    assert_eq!(p.state.code, TX);
    assert_eq!(p.state.brackets[0].len, 0);
    assert_eq!(p.year, 2026);
}

#[test]
fn knows_every_state_and_dc() {
    let law = law();
    let mut known = 0;
    for a in b'A'..=b'Z' {
        for b in b'A'..=b'Z' {
            if law.knows_state(StateCode([a, b])) {
                known += 1;
            }
        }
    }
    assert_eq!(known, 51);
    assert!(law.knows_state(StateCode(*b"DC")));
    assert!(!law.knows_state(StateCode::NONE));
    // Unknown codes behave as "no state".
    let p = law.for_year(Indexation::base(2026), StateCode(*b"ZZ"));
    assert_eq!(p.state.code, StateCode::NONE);
    assert_eq!(p.state.brackets[0].len, 0);
}

#[test]
fn published_next_year_factors() {
    let law = law();
    let next = law.published_next_year;
    assert_eq!(next.year, 2027);
    // Top single bracket 640,600 -> 661,375; wage base 184,500 -> 192,300;
    // guideline 15,650 -> 15,960; Part B 202.90 -> 209.40.
    assert!((next.cpi_factor - 661_375.0 / 640_600.0).abs() < 1e-12);
    assert!((next.wage_factor - 192_300.0 / 184_500.0).abs() < 1e-12);
    assert!((next.fpl_factor - 15_960.0 / 15_650.0).abs() < 1e-12);
    assert!((next.health_factor - 209.4 / 202.9).abs() < 1e-12);
    // Indexing 2026 by those factors lands on (or within one rounding step of) the
    // published 2027 figures.
    let p = law.for_year(next, TX);
    assert_eq!(p.federal.oasdi_wage_base, 192_300.0);
    assert_eq!(p.federal.standard_deduction[0], 16_600.0);
    assert!((p.federal.standard_deduction[1] - 33_200.0).abs() <= 50.0);
    assert!((p.federal.ordinary[0].lower[2] - 52_025.0).abs() <= 50.0);
    assert!((p.federal.ordinary[1].lower[3] - 218_250.0).abs() <= 50.0);
    assert!((p.federal.ordinary[0].lower[6] - 661_375.0).abs() <= 50.0);
    assert_eq!(p.federal.child_tax_credit, 2_300.0);
    assert_eq!(p.federal.limits.k401_employee, 25_500.0);
    close(p.federal.fpl_base, 15_960.0);
}

#[test]
fn missing_or_malformed_keys_fail_loudly() {
    let model = TaxModel::default();

    let mut data = Data::embedded().clone();
    data.tax_federal["fica"]
        .as_object_mut()
        .unwrap()
        .remove("oasdi_wage_base");
    let err = TaxLaw::from_data(&data, &model).unwrap_err();
    assert_eq!(err.file, "data/tax/us-federal-2026.json");
    assert_eq!(err.message, "fica.oasdi_wage_base: missing");

    let mut data = Data::embedded().clone();
    data.tax_federal["income_tax"]["ordinary_brackets"]["single"][2]["rate"] = "22%".into();
    let err = TaxLaw::from_data(&data, &model).unwrap_err();
    assert_eq!(
        err.message,
        "income_tax.ordinary_brackets.single[2].rate: expected a number"
    );

    let mut data = Data::embedded().clone();
    data.tax_federal["net_investment_income_tax"]["rate"] = 3.8.into();
    let err = TaxLaw::from_data(&data, &model).unwrap_err();
    assert!(err.message.contains("rate between 0 and 1"), "{err}");

    let mut data = Data::embedded().clone();
    data.tax_states["CA"]["long_term_capital_gains"]["treatment"] = "mystery".into();
    let err = TaxLaw::from_data(&data, &model).unwrap_err();
    assert_eq!(err.file, "data/tax/us-states-2026.json");
    assert_eq!(err.message, "CA.long_term_capital_gains: unknown treatment");

    let mut data = Data::embedded().clone();
    data.tax_states["NY"]["brackets"]["single"][3]["over"] = 100.into();
    let err = TaxLaw::from_data(&data, &model).unwrap_err();
    assert!(err.message.contains("NY.brackets.single[3]"), "{err}");

    let mut data = Data::embedded().clone();
    data.tax_states.as_object_mut().unwrap().remove("meta");
    assert!(TaxLaw::from_data(&data, &model).is_err());
}

// ---------------------------------------------------------------- brackets

#[test]
fn brackets_tax_and_marginal_rate() {
    let b = Brackets::from_pairs(&[(0.0, 0.10), (10_000.0, 0.20), (50_000.0, 0.30)]);
    assert_eq!(b.tax(0.0), 0.0);
    assert_eq!(b.tax(-5.0), 0.0);
    assert_eq!(b.tax(10_000.0), 1_000.0);
    assert_eq!(b.tax(60_000.0), 1_000.0 + 8_000.0 + 3_000.0);
    assert_eq!(b.marginal_rate(9_999.0), 0.10);
    assert_eq!(b.marginal_rate(10_000.0), 0.20);
    assert_eq!(b.top_of_rate(0.20), 50_000.0);
    assert_eq!(b.top_of_rate(0.30), f64::INFINITY);
    let i = b.indexed(2.0);
    assert_eq!(i.tax(20_000.0), 2_000.0);
    assert_eq!(Brackets::default().tax(1e9), 0.0);
}

#[test]
fn state_codes() {
    assert_eq!(StateCode::parse("co").unwrap().as_str(), "CO");
    assert!(StateCode::parse("Colorado").is_none());
    assert_eq!(StateCode::NONE.as_str(), "--");
}

/// Rev. Proc. 2025-32 prints the tax at the bottom of each bracket ("$5,800 plus 22%
/// of the excess over $50,400"). The schedule must reproduce those constants.
#[test]
fn federal_schedules_match_the_revenue_procedure_tables() {
    let p = params(TX);
    let cases: [(usize, [(f64, f64); 6]); 3] = [
        (
            0,
            [
                (12_400.0, 1_240.0),
                (50_400.0, 5_800.0),
                (105_700.0, 17_966.0),
                (201_775.0, 41_024.0),
                (256_225.0, 58_448.0),
                (640_600.0, 192_979.25),
            ],
        ),
        (
            1,
            [
                (24_800.0, 2_480.0),
                (100_800.0, 11_600.0),
                (211_400.0, 35_932.0),
                (403_550.0, 82_048.0),
                (512_450.0, 116_896.0),
                (768_700.0, 206_583.50),
            ],
        ),
        (
            2,
            [
                (17_700.0, 1_770.0),
                (67_450.0, 7_740.0),
                (105_700.0, 16_155.0),
                (201_750.0, 39_207.0),
                (256_200.0, 56_631.0),
                (640_600.0, 191_171.0),
            ],
        ),
    ];
    for (status, rows) in cases {
        for (income, tax) in rows {
            close(p.federal.ordinary[status].tax(income), tax);
        }
    }
}

// ---------------------------------------------------------------- whole returns

/// Single, $100,000 of wages, Texas.
/// FICA 100,000 x 7.65% = 7,650. Taxable 100,000 - 16,100 = 83,900.
/// Tax 5,800 + 22% x (83,900 - 50,400) = 5,800 + 7,370 = 13,170.
#[test]
fn single_wage_earner_2026() {
    let mut x = single(TX, 40.0);
    x.wages[0] = 100_000.0;
    let r = compute(&params(TX), &x);
    close(r.fica[0], 7_650.0);
    close(r.agi, 100_000.0);
    close(r.magi, 100_000.0);
    close(r.deduction, 16_100.0);
    assert!(!r.itemized);
    close(r.taxable_income, 83_900.0);
    close(r.federal_ordinary, 13_170.0);
    close(r.federal_capital_gains, 0.0);
    close(r.state, 0.0);
    close(r.total, 20_820.0);
    assert_eq!(r.marginal_ordinary_rate, 0.22);
    assert_eq!(r.fica[1], 0.0);
}

/// Married, wages 150,000 and 90,000, 401(k) deferrals 20,000 and 10,000, two
/// children, Texas.
/// FICA 11,475 and 6,885 (deferrals do not reduce it). AGI 210,000.
/// Taxable 210,000 - 32,200 = 177,800. Tax 11,600 + 22% x 77,000 = 28,540.
/// Child credit 2 x 2,200 = 4,400. Total 28,540 - 4,400 + 18,360 = 42,500.
#[test]
fn married_two_earners_with_children_2026() {
    let mut x = joint(TX, [40.0, 38.0]);
    x.wages = [150_000.0, 90_000.0];
    x.pretax_deferrals = [20_000.0, 10_000.0];
    x.children_under_17 = 2;
    let r = compute(&params(TX), &x);
    close(r.fica[0], 11_475.0);
    close(r.fica[1], 6_885.0);
    close(r.agi, 210_000.0);
    close(r.taxable_income, 177_800.0);
    close(r.federal_ordinary, 28_540.0);
    close(r.credits, 4_400.0);
    close(r.total, 42_500.0);
    assert_eq!(r.marginal_ordinary_rate, 0.22);
}

/// Head of household, $80,000 wages, one child. Taxable 80,000 - 24,150 = 55,850.
/// Tax 1,770 + 12% x (55,850 - 17,700) = 1,770 + 4,578 = 6,348; credit 2,200.
#[test]
fn head_of_household_2026() {
    let mut x = single(TX, 35.0);
    x.filing_status = FilingStatus::HeadOfHousehold;
    x.wages[0] = 80_000.0;
    x.children_under_17 = 1;
    let r = compute(&params(TX), &x);
    close(r.taxable_income, 55_850.0);
    close(r.federal_ordinary, 6_348.0);
    close(r.credits, 2_200.0);
    close(r.total, 6_348.0 - 2_200.0 + 6_120.0);
}

/// Wages above the wage base and the additional Medicare threshold.
/// OASDI 184,500 x 6.2% = 11,439. Medicare 300,000 x 1.45% = 4,350.
/// Additional Medicare 0.9% x (300,000 - 200,000) = 900.
#[test]
fn fica_wage_base_and_additional_medicare() {
    let mut x = single(TX, 45.0);
    x.wages[0] = 300_000.0;
    let r = compute(&params(TX), &x);
    close(r.fica[0], 11_439.0 + 4_350.0 + 900.0);

    // The wage base is per person: two earners at 200,000 each pay OASDI on
    // 184,500 each. Joint additional Medicare: 0.9% x (400,000 - 250,000) = 1,350.
    let mut x = joint(TX, [45.0, 45.0]);
    x.wages = [200_000.0, 200_000.0];
    let r = compute(&params(TX), &x);
    close(r.fica[0], 11_439.0 + 2_900.0 + 675.0);
    close(r.fica[1], 11_439.0 + 2_900.0 + 675.0);

    // Payroll HSA contributions reduce FICA wages and income.
    let mut x = single(TX, 45.0);
    x.wages[0] = 100_000.0;
    x.hsa_contributions[0] = 4_000.0;
    let r = compute(&params(TX), &x);
    close(r.fica[0], 96_000.0 * 0.0765);
    close(r.agi, 96_000.0);
}

// ---------------------------------------------------------------- capital gains

/// Single, wages 40,000, long-term gain 30,000. Taxable 53,900: ordinary 23,900,
/// gains 30,000. The 0% band ends at 49,450, so 25,550 of gain is untaxed and
/// 4,450 pays 15% = 667.50. Ordinary tax 1,240 + 12% x 11,500 = 2,620.
#[test]
fn gains_straddle_the_zero_percent_threshold() {
    let mut x = single(TX, 40.0);
    x.wages[0] = 40_000.0;
    x.long_term_gains = 30_000.0;
    let r = compute(&params(TX), &x);
    close(r.taxable_income, 53_900.0);
    close(r.federal_ordinary, 2_620.0);
    close(r.federal_capital_gains, 667.50);
    assert_eq!(r.marginal_ordinary_rate, 0.12);
}

/// Married retirees living on gains: 100,000 of long-term gain and 20,000 of
/// qualified dividends. Taxable 120,000 - 32,200 = 87,800, below the 98,900 ceiling
/// of the 0% band: no federal tax at all.
#[test]
fn gains_entirely_in_the_zero_percent_band() {
    let mut x = joint(TX, [60.0, 60.0]);
    x.long_term_gains = 100_000.0;
    x.qualified_dividends = 20_000.0;
    let r = compute(&params(TX), &x);
    close(r.taxable_income, 87_800.0);
    close(r.federal_ordinary, 0.0);
    close(r.federal_capital_gains, 0.0);
    close(r.total, 0.0);
    assert_eq!(r.marginal_ordinary_rate, 0.0);
}

/// Gains absorb the unused standard deduction. Single, 10,000 of interest and a
/// 60,000 gain: taxable 53,900, all of it gain (ordinary taxable income is zero).
/// 49,450 at 0%, 4,450 at 15% = 667.50.
#[test]
fn gains_use_the_deduction_ordinary_income_leaves() {
    let mut x = single(TX, 60.0);
    x.interest = 10_000.0;
    x.long_term_gains = 60_000.0;
    let r = compute(&params(TX), &x);
    close(r.taxable_income, 53_900.0);
    close(r.federal_ordinary, 0.0);
    close(r.federal_capital_gains, 667.50);
}

/// Single, 500,000 of ordinary income and a 200,000 long-term gain. Taxable
/// 683,900: ordinary 483,900, gain from 483,900 to 683,900. The 15% band ends at
/// 545,500: 61,600 x 15% = 9,240 and 138,400 x 20% = 27,680, together 36,920.
/// Ordinary tax 58,448 + 35% x (483,900 - 256,225) = 138,134.25.
/// NIIT 3.8% x min(200,000, 700,000 - 200,000) = 7,600.
#[test]
fn gains_straddle_the_fifteen_percent_threshold() {
    let mut x = single(TX, 50.0);
    x.other_ordinary = 500_000.0;
    x.long_term_gains = 200_000.0;
    let r = compute(&params(TX), &x);
    close(r.taxable_income, 683_900.0);
    close(r.federal_ordinary, 138_134.25);
    close(r.federal_capital_gains, 36_920.0);
    close(r.niit, 7_600.0);
    close(r.total, 138_134.25 + 36_920.0 + 7_600.0);
}

/// With ordinary taxable income already past the 0% ceiling every dollar of gain
/// pays 15%, and qualified dividends are taxed like gains.
#[test]
fn gains_above_the_zero_band_pay_fifteen_percent() {
    let mut x = single(TX, 50.0);
    x.other_ordinary = 100_000.0; // ordinary taxable 83,900 > 49,450
    x.long_term_gains = 10_000.0;
    x.qualified_dividends = 5_000.0;
    let r = compute(&params(TX), &x);
    close(r.federal_ordinary, 13_170.0);
    close(r.federal_capital_gains, 2_250.0);
}

#[test]
fn capital_loss_netting_and_limit() {
    let p = params(TX);
    // Net loss 12,000: 3,000 offsets ordinary income, 9,000 carries forward.
    let mut x = single(TX, 50.0);
    x.other_ordinary = 50_000.0;
    x.short_term_gains = -2_000.0;
    x.long_term_gains = -10_000.0;
    let r = compute(&p, &x);
    close(r.agi, 47_000.0);
    close(r.capital_loss_carryforward, 9_000.0);
    close(r.federal_capital_gains, 0.0);

    // A short-term loss reduces the long-term gain: 20,000 - 5,000 = 15,000 at
    // long-term rates. Ordinary taxable 33,900, so all of it sits in the 0% band.
    x.short_term_gains = -5_000.0;
    x.long_term_gains = 20_000.0;
    let r = compute(&p, &x);
    close(r.agi, 65_000.0);
    close(r.taxable_income, 48_900.0);
    close(r.federal_ordinary, 1_240.0 + 0.12 * (33_900.0 - 12_400.0));
    close(r.federal_capital_gains, 0.0);
    close(r.capital_loss_carryforward, 0.0);

    // A long-term loss reduces the short-term gain, and what is left is ordinary.
    x.short_term_gains = 8_000.0;
    x.long_term_gains = -3_000.0;
    let r = compute(&p, &x);
    close(r.agi, 55_000.0);
    close(r.federal_ordinary, 1_240.0 + 0.12 * (38_900.0 - 12_400.0));
    close(r.federal_capital_gains, 0.0);
}

// ---------------------------------------------------------------- Social Security

/// The IRS worksheet (Pub. 915, Form 1040 instructions) for a single filer with
/// 20,000 of benefits: base amount 25,000, adjusted base 34,000.
#[test]
fn social_security_taxation_single() {
    let p = params(TX);
    let mut x = single(TX, 62.0);
    x.social_security[0] = 20_000.0;

    // Provisional income 10,000 + 10,000 = 20,000: nothing is taxable.
    x.pensions = 10_000.0;
    let r = compute(&p, &x);
    close(r.taxable_social_security, 0.0);
    close(r.agi, 10_000.0);
    close(r.magi, 30_000.0);

    // Provisional 30,000: half of the 5,000 above 25,000 = 2,500.
    x.pensions = 20_000.0;
    close(compute(&p, &x).taxable_social_security, 2_500.0);

    // Provisional 34,000, the top of the first tier: 4,500.
    x.pensions = 24_000.0;
    close(compute(&p, &x).taxable_social_security, 4_500.0);

    // Provisional 40,000: 4,500 + 85% x 6,000 = 9,600.
    x.pensions = 30_000.0;
    let r = compute(&p, &x);
    close(r.taxable_social_security, 9_600.0);
    close(r.agi, 39_600.0);
    close(r.magi, 50_000.0);

    // Provisional 90,000: capped at 85% of benefits = 17,000.
    x.pensions = 80_000.0;
    close(compute(&p, &x).taxable_social_security, 17_000.0);

    // No other income: benefits alone are never taxable.
    x.pensions = 0.0;
    x.social_security[0] = 45_000.0;
    close(compute(&p, &x).taxable_social_security, 0.0);
}

/// Married filing jointly, benefits 24,000 + 16,000, pension 30,000. Provisional
/// 50,000; base 32,000, adjusted base 44,000. First tier min(20,000, 6,000, 9,000) =
/// 6,000; second tier 85% x 6,000 = 5,100; taxable 11,100.
#[test]
fn social_security_taxation_joint() {
    let mut x = joint(TX, [63.0, 62.0]);
    x.social_security = [24_000.0, 16_000.0];
    x.pensions = 30_000.0;
    let r = compute(&params(TX), &x);
    close(r.taxable_social_security, 11_100.0);
    close(r.agi, 41_100.0);
    close(r.magi, 70_000.0);
    // 41,100 - 32,200 = 8,900 at 10%.
    close(r.federal_ordinary, 890.0);
}

/// A 67-year-old with 20,000 of benefits and a 30,000 pension. AGI 39,600.
/// Standard deduction 16,100 + 2,050, senior deduction 6,000: taxable 15,450.
/// Tax 1,240 + 12% x 3,050 = 1,606.
#[test]
fn retiree_return_with_age_deductions() {
    let mut x = single(TX, 67.0);
    x.social_security[0] = 20_000.0;
    x.pensions = 30_000.0;
    let r = compute(&params(TX), &x);
    close(r.deduction, 18_150.0);
    close(r.taxable_income, 15_450.0);
    close(r.federal_ordinary, 1_606.0);
    close(r.total, 1_606.0);
}

/// The senior deduction phases out at 6% of MAGI above 150,000 (joint) and expires
/// after 2028. Both 70, 200,000 of pension: each 6,000 - 6% x 50,000 = 3,000.
/// Taxable 200,000 - (32,200 + 2 x 1,650) - 6,000 = 158,500.
#[test]
fn senior_deduction_phase_out_and_sunset() {
    let law = law();
    let mut x = joint(TX, [70.0, 70.0]);
    x.pensions = 200_000.0;
    let r = compute(&law.for_year(Indexation::base(2026), TX), &x);
    close(r.deduction, 35_500.0);
    close(r.taxable_income, 158_500.0);
    let r = compute(&law.for_year(Indexation::base(2028), TX), &x);
    close(r.taxable_income, 158_500.0);
    let r = compute(&law.for_year(Indexation::base(2029), TX), &x);
    close(r.taxable_income, 164_500.0);
    // Fully phased out at 250,000.
    x.pensions = 250_000.0;
    let r = compute(&law.for_year(Indexation::base(2026), TX), &x);
    close(r.taxable_income, 250_000.0 - 35_500.0);
}

// ---------------------------------------------------------------- self-employment

/// IRS Schedule SE arithmetic for 100,000 of net profit: 92,350 x 15.3% =
/// 14,129.55; half (7,064.775) is deducted. AGI 92,935.225.
/// QBI deduction: 20% of (profit less half SE tax) = 18,587.045, limited to 20% of
/// taxable income before it (92,935.225 - 16,100 = 76,835.225) = 15,367.045.
/// Taxable 61,468.18; tax 5,800 + 22% x 11,068.18 = 8,235.00.
#[test]
fn self_employment_tax_and_deductions() {
    let mut x = single(TX, 40.0);
    x.self_employment[0] = 100_000.0;
    let r = compute(&params(TX), &x);
    close(r.se_tax[0], 14_129.55);
    close(r.fica[0], 0.0);
    close(r.agi, 92_935.225);
    close(r.taxable_income, 61_468.18);
    close(r.federal_ordinary, 8_235.00);
    close(r.total, 8_235.00 + 14_129.55);
}

#[test]
fn self_employment_edge_cases() {
    let p = params(TX);
    // Wages already above the wage base: only the 2.9% Medicare part applies to
    // 92.35% x 50,000 = 46,175 -> 1,339.075. Additional Medicare 0.9% x
    // (200,000 + 46,175 - 200,000) = 415.575, shared 200,000 : 46,175.
    let mut x = single(TX, 40.0);
    x.wages[0] = 200_000.0;
    x.self_employment[0] = 50_000.0;
    let r = compute(&p, &x);
    let additional = 0.009 * 46_175.0;
    close(r.se_tax[0], 1_339.075 + additional * 46_175.0 / 246_175.0);
    close(
        r.fica[0],
        11_439.0 + 2_900.0 + additional * 200_000.0 / 246_175.0,
    );
    // Half of 1,339.075 is deductible; the additional Medicare tax is not.
    close(r.agi, 250_000.0 - 669.5375);

    // Wages of 150,000 leave 34,500 of OASDI room for 92,350 of SE earnings.
    let mut x = single(TX, 40.0);
    x.wages[0] = 150_000.0;
    x.self_employment[0] = 100_000.0;
    let r = compute(&p, &x);
    let additional = 0.009 * (150_000.0 + 92_350.0 - 200_000.0);
    close(
        r.se_tax[0],
        0.124 * 34_500.0 + 0.029 * 92_350.0 + additional * 92_350.0 / 242_350.0,
    );

    // Net earnings under $400 owe nothing: 400 x 92.35% = 369.40.
    let mut x = single(TX, 40.0);
    x.self_employment[0] = 400.0;
    close(compute(&p, &x).se_tax[0], 0.0);
    x.self_employment[0] = 434.0; // 400.80 after the factor
    close(compute(&p, &x).se_tax[0], 434.0 * 0.9235 * 0.153);

    // A business loss offsets other income and owes no SE tax.
    let mut x = single(TX, 40.0);
    x.wages[0] = 80_000.0;
    x.self_employment[0] = -10_000.0;
    let r = compute(&p, &x);
    close(r.se_tax[0], 0.0);
    close(r.agi, 70_000.0);
}

/// The QBI deduction phases out between 201,750 and 276,750 of taxable income for a
/// single sole proprietor without employees.
#[test]
fn qbi_deduction_phases_out() {
    let p = params(TX);
    let mut x = single(TX, 40.0);
    x.self_employment[0] = 400_000.0;
    let r = compute(&p, &x);
    // Far above the range: no deduction, taxable = AGI - standard deduction.
    close(r.taxable_income, r.agi - 16_100.0);
    // Halfway through the range the deduction is half of 20%.
    // Use other income to pin taxable income before QBI at 239,250 exactly.
    let mut x = single(TX, 40.0);
    x.self_employment[0] = 300.0; // below the SE tax floor: no half-SE deduction
    x.other_ordinary = 239_250.0 + 16_100.0 - 300.0;
    let r = compute(&p, &x);
    close(r.taxable_income, 239_250.0 - 0.5 * 0.2 * 300.0);
}

// ---------------------------------------------------------------- NIIT, credits

#[test]
fn net_investment_income_tax() {
    let p = params(TX);
    // Single, 180,000 ordinary + 50,000 interest: MAGI 230,000, 30,000 over the
    // threshold, less than the 50,000 of investment income. 3.8% x 30,000 = 1,140.
    let mut x = single(TX, 50.0);
    x.other_ordinary = 180_000.0;
    x.interest = 50_000.0;
    close(compute(&p, &x).niit, 1_140.0);
    // Investment income is the smaller amount: 3.8% x 10,000 = 380.
    x.other_ordinary = 400_000.0;
    x.interest = 10_000.0;
    close(compute(&p, &x).niit, 380.0);
    // Joint threshold is 250,000: the first household owes nothing as a couple.
    let mut x = joint(TX, [50.0, 50.0]);
    x.other_ordinary = 180_000.0;
    x.interest = 50_000.0;
    close(compute(&p, &x).niit, 0.0);
    // Rental income and gains count; wages do not.
    let mut x = single(TX, 50.0);
    x.wages[0] = 300_000.0;
    close(compute(&p, &x).niit, 0.0);
    x.rental_net = 20_000.0;
    x.long_term_gains = 30_000.0;
    close(compute(&p, &x).niit, 0.038 * 50_000.0);
}

#[test]
fn child_tax_credit_phase_out() {
    let p = params(TX);
    // Joint, AGI 410,000, two children: 10 steps of $50 = 500 off 4,400.
    let mut x = joint(TX, [40.0, 40.0]);
    x.other_ordinary = 410_000.0;
    x.children_under_17 = 2;
    close(compute(&p, &x).credits, 3_900.0);
    // One dollar over the threshold already costs a full step.
    x.other_ordinary = 400_001.0;
    close(compute(&p, &x).credits, 4_350.0);
    x.other_ordinary = 400_000.0;
    close(compute(&p, &x).credits, 4_400.0);
    // Gone entirely at 488,000 (88 steps).
    x.other_ordinary = 488_000.0;
    close(compute(&p, &x).credits, 0.0);
    // Single threshold 200,000: three children and one student at 250,000.
    // 3 x 2,200 + 500 = 7,100 less 50 steps x 50 = 2,500 -> 4,600.
    let mut x = single(TX, 40.0);
    x.filing_status = FilingStatus::HeadOfHousehold;
    x.other_ordinary = 250_000.0;
    x.children_under_17 = 3;
    x.other_dependents = 1;
    close(compute(&p, &x).credits, 4_600.0);
}

/// The refundable part: 15% of earned income above 2,500, at most 1,700 a child.
#[test]
fn child_tax_credit_refundable_limit() {
    let p = params(TX);
    // Joint, wages 20,000, two children: no income tax (below the deduction).
    // Refund = min(4,400, 2 x 1,700, 15% x 17,500 = 2,625) = 2,625.
    let mut x = joint(TX, [30.0, 30.0]);
    x.wages[0] = 20_000.0;
    x.children_under_17 = 2;
    let r = compute(&p, &x);
    close(r.federal_ordinary, 0.0);
    close(r.credits, 2_625.0);
    close(r.total, 20_000.0 * 0.0765 - 2_625.0);
    // Wages 40,000: tax 10% x 7,800 = 780 is wiped out, and the refund is capped at
    // 2 x 1,700 = 3,400 (the remaining credit is 3,620).
    x.wages[0] = 40_000.0;
    let r = compute(&p, &x);
    close(r.federal_ordinary, 780.0);
    close(r.credits, 780.0 + 3_400.0);
    // No earned income, no refund; the other-dependant credit is never refundable.
    let mut x = joint(TX, [30.0, 30.0]);
    x.interest = 20_000.0;
    x.children_under_17 = 2;
    x.other_dependents = 1;
    close(compute(&p, &x).credits, 0.0);
}

#[test]
fn penalties() {
    let p = params(TX);
    let mut x = single(TX, 50.0);
    x.traditional_distributions = 30_000.0;
    x.early_distributions = 30_000.0;
    x.hsa_nonqualified = 5_000.0;
    let r = compute(&p, &x);
    close(r.penalties, 3_000.0 + 1_000.0);
    // The HSA withdrawal is also income.
    close(r.agi, 35_000.0);
    close(r.total, r.federal_ordinary + 4_000.0);
}

// ---------------------------------------------------------------- deductions

/// Married, 300,000 of income, mortgage interest 30,000, property tax 15,000, gifts
/// 10,000. SALT 15,000 (under the 40,400 cap); gifts above 0.5% of AGI: 8,500.
/// Itemized 53,500. Taxable 246,500; tax 35,932 + 24% x 35,100 = 44,356.
#[test]
fn itemized_deductions() {
    let mut x = joint(TX, [45.0, 45.0]);
    x.other_ordinary = 300_000.0;
    x.mortgage_interest = 30_000.0;
    x.property_tax = 15_000.0;
    x.charitable = 10_000.0;
    let r = compute(&params(TX), &x);
    assert!(r.itemized);
    close(r.deduction, 53_500.0);
    close(r.federal_ordinary, 44_356.0);
}

/// Non-itemisers deduct up to 2,000 (joint) of cash gifts on top of the standard
/// deduction: 100,000 - 32,200 - 2,000 = 65,800; tax 2,480 + 12% x 41,000 = 7,400.
#[test]
fn non_itemizer_charitable_deduction() {
    let mut x = joint(TX, [45.0, 45.0]);
    x.other_ordinary = 100_000.0;
    x.charitable = 5_000.0;
    let r = compute(&params(TX), &x);
    assert!(!r.itemized);
    close(r.deduction, 34_200.0);
    close(r.federal_ordinary, 7_400.0);
}

#[test]
fn salt_cap_phase_down_and_schedule() {
    let law = law();
    // 600,000 of income: cap 40,400 - 30% x (600,000 - 505,000) = 11,900.
    let mut x = joint(TX, [45.0, 45.0]);
    x.other_ordinary = 600_000.0;
    x.property_tax = 50_000.0;
    x.mortgage_interest = 30_000.0;
    let r = compute(&law.for_year(Indexation::base(2026), TX), &x);
    close(r.deduction, 41_900.0);
    // Never below 10,000.
    x.other_ordinary = 700_000.0;
    let r = compute(&law.for_year(Indexation::base(2026), TX), &x);
    close(r.deduction, 40_000.0);
    // The statutory schedule: 40,804 in 2027, 10,000 flat from 2030.
    x.other_ordinary = 300_000.0;
    let r = compute(&law.for_year(Indexation::base(2027), TX), &x);
    close(r.deduction, 40_804.0 + 30_000.0);
    let p2030 = law.for_year(Indexation::base(2030), TX);
    assert_eq!(p2030.federal.salt_cap, 10_000.0);
    assert!(p2030.federal.salt_phase_down_threshold.is_infinite());
    close(compute(&p2030, &x).deduction, 40_000.0);
    let p2029 = law.for_year(Indexation::base(2029), TX);
    assert_eq!(p2029.federal.salt_cap, 41_624.16);
    assert_eq!(p2029.federal.salt_phase_down_threshold, 520_302.01);
}

/// The 2/37 limitation. Single, 1,000,000 of income, mortgage interest 40,000,
/// property tax 20,000, gifts 100,000. SALT cap is at its 10,000 floor; gifts
/// 100,000 - 5,000 = 95,000; itemized 145,000. Income in the 37% bracket exceeds
/// that, so the reduction is 2/37 x 145,000 = 7,837.84. Taxable 862,837.84; tax
/// 192,979.25 + 37% x 222,237.84 = 275,207.25.
#[test]
fn itemized_deduction_limitation_in_top_bracket() {
    let mut x = single(TX, 50.0);
    x.other_ordinary = 1_000_000.0;
    x.mortgage_interest = 40_000.0;
    x.property_tax = 20_000.0;
    x.charitable = 100_000.0;
    let r = compute(&params(TX), &x);
    assert!(r.itemized);
    close(r.deduction, 145_000.0 * 35.0 / 37.0);
    close(r.federal_ordinary, 275_207.25);
}

#[test]
fn rental_losses_follow_the_passive_loss_allowance() {
    let p = params(TX);
    let mut x = single(TX, 45.0);
    x.other_ordinary = 90_000.0;
    x.rental_net = -30_000.0;
    let r = compute(&p, &x);
    close(r.agi, 65_000.0);
    close(r.rental_loss_suspended, 5_000.0);
    // At 140,000 of other income the allowance is 25,000 - 50% x 40,000 = 5,000.
    x.other_ordinary = 140_000.0;
    let r = compute(&p, &x);
    close(r.agi, 135_000.0);
    close(r.rental_loss_suspended, 25_000.0);
    // Gone at 150,000.
    x.other_ordinary = 160_000.0;
    let r = compute(&p, &x);
    close(r.agi, 160_000.0);
    close(r.rental_loss_suspended, 30_000.0);
}

// ---------------------------------------------------------------- ACA and IRMAA

/// One person on the marketplace all year, benchmark plan 7,500 a year, poverty
/// line 15,650 (the 2025 guideline, used for 2026 coverage).
fn aca_inputs(magi: f64) -> TaxInputs {
    let mut x = single(TX, 60.0);
    x.other_ordinary = magi;
    x.aca_household_size = 1;
    x.aca_months = 12;
    x.aca_benchmark_premium = 7_500.0;
    x
}

#[test]
fn aca_premium_credit_by_poverty_ratio() {
    let p = params(TX);
    // 150%: MAGI 23,475, expected share 4.19% = 983.60; credit 6,516.40.
    let r = compute(&p, &aca_inputs(23_475.0));
    close(r.aca_fpl_ratio, 1.5);
    close(r.aca_premium_credit, 7_500.0 - 0.0419 * 23_475.0);
    close(r.aca_premium_credit, 6_516.40);
    // 175%: halfway between 4.19% and 6.60% = 5.395%.
    let r = compute(&p, &aca_inputs(27_387.5));
    close(r.aca_premium_credit, 7_500.0 - 0.05395 * 27_387.5);
    // 250%: MAGI 39,125, share 8.44% = 3,302.15; credit 4,197.85.
    let r = compute(&p, &aca_inputs(39_125.0));
    close(r.aca_premium_credit, 4_197.85);
    // 399%: MAGI 62,443.50, share 9.96% = 6,219.37; credit 1,280.63.
    let r = compute(&p, &aca_inputs(62_443.5));
    close(r.aca_premium_credit, 1_280.63);
    // Exactly 400% still qualifies: 7,500 - 9.96% x 62,600 = 1,265.04.
    let r = compute(&p, &aca_inputs(62_600.0));
    close(r.aca_premium_credit, 1_265.04);
    // 401%: over the cliff, nothing.
    let r = compute(&p, &aca_inputs(62_756.5));
    close(r.aca_fpl_ratio, 4.01);
    close(r.aca_premium_credit, 0.0);
    // The credit is not part of the tax total.
    let with = compute(&p, &aca_inputs(39_125.0));
    let mut without = aca_inputs(39_125.0);
    without.aca_household_size = 0;
    assert_eq!(with.total, compute(&p, &without).total);
    assert_eq!(compute(&p, &without).aca_premium_credit, 0.0);
}

#[test]
fn aca_credit_details() {
    let law = law();
    let p = law.for_year(Indexation::base(2026), TX);
    // Half a year of coverage gives half the credit.
    let mut x = aca_inputs(39_125.0);
    x.aca_months = 6;
    close(compute(&p, &x).aca_premium_credit, 4_197.85 / 2.0);
    // Two people: poverty line 21,150. MAGI 42,300 is 200%: 6.60%.
    let mut x = joint(TX, [60.0, 60.0]);
    x.other_ordinary = 42_300.0;
    x.aca_household_size = 2;
    x.aca_months = 12;
    x.aca_benchmark_premium = 15_000.0;
    let r = compute(&p, &x);
    close(r.aca_fpl_ratio, 2.0);
    close(r.aca_premium_credit, 15_000.0 - 0.066 * 42_300.0);
    // Untaxed Social Security counts: 30,000 of benefits and 20,000 of pension.
    // Provisional 35,000 (below 32,000 + ... joint base 32,000): taxable
    // min(15,000, 6,000, 1,500) = 1,500. AGI 21,500, ACA MAGI 50,000.
    let mut x = joint(TX, [63.0, 63.0]);
    x.social_security = [15_000.0, 15_000.0];
    x.pensions = 20_000.0;
    x.aca_household_size = 2;
    x.aca_months = 12;
    x.aca_benchmark_premium = 15_000.0;
    let r = compute(&p, &x);
    close(r.agi, 21_500.0);
    close(r.magi, 50_000.0);
    close(r.aca_fpl_ratio, 50_000.0 / 21_150.0);
    // A cheap benchmark fully covered by the expected contribution: no credit.
    let mut x = aca_inputs(60_000.0);
    x.aca_benchmark_premium = 3_000.0;
    close(compute(&p, &x).aca_premium_credit, 0.0);
    // Alaska and Hawaii have their own guidelines.
    let ak = law.for_year(Indexation::base(2026), StateCode(*b"AK"));
    assert_eq!(
        (ak.federal.fpl_base, ak.federal.fpl_per_person),
        (19_550.0, 6_880.0)
    );
    let hi = law.for_year(Indexation::base(2026), StateCode(*b"HI"));
    assert_eq!(
        (hi.federal.fpl_base, hi.federal.fpl_per_person),
        (17_990.0, 6_330.0)
    );
    // Very low income is priced from the first band (2.10%), standing in for Medicaid.
    let r = compute(&p, &aca_inputs(10_000.0));
    close(r.aca_premium_credit, 7_500.0 - 0.021 * 10_000.0);
    assert!(r.aca_fpl_ratio < 1.0);
}

#[test]
fn room_to_the_aca_cliff() {
    let p = params(TX);
    let x = aca_inputs(50_000.0);
    close(room_to_aca_cliff(&p, &x), 12_600.0);
    // Using exactly that room keeps the credit; one more dollar loses it.
    let mut at = x;
    at.roth_conversions = 12_600.0;
    assert!(compute(&p, &at).aca_premium_credit > 0.0);
    at.roth_conversions = 12_601.0;
    assert_eq!(compute(&p, &at).aca_premium_credit, 0.0);
    assert_eq!(room_to_aca_cliff(&p, &at), 0.0);
    // Not on the marketplace: no cliff.
    let mut off = x;
    off.aca_months = 0;
    assert!(room_to_aca_cliff(&p, &off).is_infinite());
    // The enhanced-credit regime has no cliff: the last band's share applies above.
    let mut enhanced = p;
    enhanced.federal.aca.cliff = false;
    assert!(room_to_aca_cliff(&enhanced, &x).is_infinite());
    let r = compute(&enhanced, &aca_inputs(70_000.0));
    close(r.aca_premium_credit, 7_500.0 - 0.0996 * 70_000.0);
}

#[test]
fn irmaa_tiers_and_surcharges() {
    let p = params(TX);
    let s = FilingStatus::Single;
    assert_eq!(irmaa_tier(&p, s, 109_000.0), 0);
    assert_eq!(irmaa_tier(&p, s, 109_000.01), 1);
    assert_eq!(irmaa_tier(&p, s, 137_000.0), 1);
    assert_eq!(irmaa_tier(&p, s, 205_000.01), 4);
    // The top tier starts at its threshold.
    assert_eq!(irmaa_tier(&p, s, 499_999.99), 4);
    assert_eq!(irmaa_tier(&p, s, 500_000.0), 5);
    assert_eq!(irmaa_tier(&p, FilingStatus::HeadOfHousehold, 150_000.0), 2);
    assert_eq!(irmaa_tier(&p, FilingStatus::MarriedJoint, 218_000.0), 0);
    assert_eq!(irmaa_tier(&p, FilingStatus::MarriedJoint, 750_000.0), 5);
    close(irmaa_monthly_surcharge(&p, s, 120_000.0), 81.20 + 14.50);
    close(irmaa_monthly_surcharge(&p, s, 1e7), 487.0 + 91.0);

    // Joint, both on Medicare, AGI 300,000: tier 2, (202.90 + 37.50) x 12 x 2.
    let mut x = joint(TX, [70.0, 68.0]);
    x.pensions = 300_000.0;
    x.on_medicare = [true, true];
    let r = compute(&p, &x);
    assert_eq!(r.irmaa_tier, 2);
    close(r.irmaa_surcharge, 240.40 * 24.0);
    close(r.magi_irmaa, 300_000.0);
    // Only one enrolled: half. Nobody enrolled: nothing, but the tier is reported.
    x.on_medicare = [true, false];
    close(compute(&p, &x).irmaa_surcharge, 240.40 * 12.0);
    x.on_medicare = [false, false];
    let r = compute(&p, &x);
    close(r.irmaa_surcharge, 0.0);
    assert_eq!(r.irmaa_tier, 2);
}

// ---------------------------------------------------------------- states

/// Single, 500,000 of wages in California.
/// FICA 11,439 + 7,250 + 0.9% x 300,000 = 21,389.
/// California: 500,000 - 5,900 = 494,100 through the 2026 schedule X = 43,700.50,
/// less the 158 exemption credit = 43,542.50.
/// Federal: SALT capped at 40,400, which beats the standard deduction. Taxable
/// 459,600; tax 58,448 + 35% x 203,375 = 129,629.25.
#[test]
fn high_earner_in_california() {
    let mut x = single(CA, 45.0);
    x.wages[0] = 500_000.0;
    let r = compute(&params(CA), &x);
    let ca =
        114.56 + 314.02 + 628.16 + 998.22 + 1_255.92 + 28_728.816 + 7_912.254 + 3_748.549 - 158.0;
    close(ca, 43_542.499);
    close(r.state, ca);
    close(r.local, 0.0);
    assert!(r.itemized);
    close(r.deduction, 40_400.0);
    close(r.federal_ordinary, 129_629.25);
    close(r.fica[0], 21_389.0);
    close(r.total, 129_629.25 + 21_389.0 + ca);
    assert_eq!(r.marginal_ordinary_rate, 0.35);
}

/// Married, wages 400,000 and 200,000 in New York.
/// FICA 11,439 + 5,800 and 11,439 + 2,900, plus 0.9% x 350,000 = 3,150 shared 2:1.
/// New York: 600,000 - 16,050 = 583,950 through the joint schedule = 35,789.925.
/// Local income tax at the statewide average 1.6% of 600,000 = 9,600.
/// Federal: SALT cap 40,400 - 30% x 95,000 = 11,900, below the 32,200 standard
/// deduction. Taxable 567,800; tax 116,896 + 35% x 55,350 = 136,268.50.
#[test]
fn high_earners_in_new_york() {
    let mut x = joint(NY, [45.0, 45.0]);
    x.wages = [400_000.0, 200_000.0];
    let r = compute(&params(NY), &x);
    let ny = 668.85 + 283.80 + 221.45 + 7_217.10 + 9_537.35 + 17_861.375;
    close(ny, 35_789.925);
    close(r.local, 9_600.0);
    close(r.state, ny + 9_600.0);
    assert!(!r.itemized);
    close(r.federal_ordinary, 136_268.50);
    close(r.fica[0], 17_239.0 + 2_100.0);
    close(r.fica[1], 14_339.0 + 1_050.0);
    close(r.total, 136_268.50 + 34_728.0 + ny + 9_600.0);

    // A New York City resident pays the city rate instead of the state average.
    let law = law();
    assert_eq!(law.local_rate(NY, Some("New York City")), 0.03876);
    assert_eq!(law.local_rate(NY, Some("Albany")), 0.016);
    assert_eq!(law.local_rate(NY, None), 0.016);
    assert_eq!(
        law.local_rate(StateCode(*b"IN"), Some("indianapolis")),
        0.0202
    );
    assert_eq!(law.local_rate(TX, Some("Austin")), 0.0);
    // The city's own graduated schedule on New York taxable income (600,000 less
    // the 16,050 deduction): 21,600 at 3.078%, 23,400 at 3.762%, 45,000 at 3.819%,
    // the rest at 3.876%.
    let mut p = params(NY);
    law.local_tax(NY, Some("new york city")).apply(&mut p.state);
    let city = 664.848 + 880.308 + 1_718.55 + 0.03876 * (583_950.0 - 90_000.0);
    close(compute(&p, &x).local, city);
}

/// The app's city list calls the place "New York"; people type "NYC". All of them
/// are the city, and every state's major taxing city must resolve from the name
/// the city list (`data/col/cities.csv`) uses for it.
#[test]
fn a_city_resolves_by_any_of_its_names() {
    let law = law();
    let nyc = law.local_tax(NY, Some("New York City"));
    assert!(nyc.brackets[0].len == 4);
    for name in [
        "New York",
        "new york",
        "NYC",
        " Brooklyn ",
        "New-York City",
        "The Bronx",
    ] {
        assert_eq!(law.local_tax(NY, Some(name)), nyc, "{name}");
    }
    assert_ne!(law.local_tax(NY, Some("Yonkers")), nyc);
    // Hand calculation, single with 60,000 of wages: city taxable income 52,000:
    // 369.36 + 489.06 + 954.75 + 2,000 x 3.876% = 1,890.69.
    let mut p = params(NY);
    nyc.apply(&mut p.state);
    let mut x = single(NY, 40.0);
    x.wages[0] = 60_000.0;
    close(compute(&p, &x).local, 1_890.69);

    let csv = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/col/cities.csv"
    ))
    .expect("data/col/cities.csv");
    let cities: std::collections::BTreeSet<String> = csv
        .lines()
        .skip(1)
        .filter_map(|line| line.split(',').next())
        .map(|c| c.trim_matches('"').to_ascii_lowercase())
        .collect();
    let majors = law.major_cities();
    assert!(majors.len() >= 14);
    for (state, _) in majors {
        let average = law.local_tax(state, None);
        let found = cities.iter().any(|c| {
            let t = law.local_tax(state, Some(c));
            t.rate != average.rate || t.brackets != average.brackets
        });
        // A city whose rate equals the statewide figure cannot be told apart; only
        // require a match where the city tax differs.
        let differs = law.major_cities().iter().any(|(s, n)| {
            *s == state && {
                let t = law.local_tax(state, Some(n));
                t.rate != average.rate || t.brackets != average.brackets
            }
        });
        assert!(
            found || !differs,
            "{}: no city in cities.csv matches",
            state.as_str()
        );
    }
}

/// Local taxes have a base. The Portland area's taxes start above 125,000 single /
/// 200,000 joint of taxable income; city wage and earnings taxes do not reach
/// retirement or investment income.
#[test]
fn local_tax_follows_its_base_and_thresholds() {
    let law = law();
    let local = |state: &str, city: &str, x: &TaxInputs| {
        let code = StateCode::parse(state).unwrap();
        let mut p = law.for_year(Indexation::base(2026), code);
        law.local_tax(code, Some(city)).apply(&mut p.state);
        let mut x = *x;
        x.state = code;
        compute(&p, &x)
    };
    // Joint wages of 150,000 in Portland: under the threshold, nothing.
    let mut workers = joint(TX, [40.0, 40.0]);
    workers.wages = [90_000.0, 60_000.0];
    close(local("OR", "Portland", &workers).local, 0.0);
    // Joint wages of 500,000: Oregon taxable income above 200,000 at 2.5%, above
    // 400,000 at 4%.
    workers.wages = [300_000.0, 200_000.0];
    let r = local("OR", "Portland", &workers);
    assert!(r.local > 0.025 * 250_000.0 && r.local < 0.04 * 300_000.0);
    // A retired couple: 50,000 from an IRA, 50,000 of Social Security, 30,000 of
    // qualified dividends and gains. Wage taxes take nothing.
    let mut retired = joint(TX, [67.0, 67.0]);
    retired.traditional_distributions = 50_000.0;
    retired.social_security = [25_000.0, 25_000.0];
    retired.qualified_dividends = 10_000.0;
    retired.long_term_gains = 20_000.0;
    for (state, city) in [
        ("OR", "Portland"),
        ("OH", "Columbus"),
        ("MO", "Kansas City"),
        ("DE", "Wilmington"),
        ("KY", "Louisville"),
        ("PA", "Philadelphia"),
        ("MI", "Detroit"),
        ("OH", "Dayton"),
    ] {
        close(local(state, city, &retired).local, 0.0);
    }
    // Maryland and Indiana tax all income locally.
    assert!(local("MD", "Baltimore", &retired).local > 500.0);
    assert!(local("IN", "Indianapolis", &retired).local > 500.0);
    // And a Columbus worker pays 2.5% of wages.
    workers.wages = [90_000.0, 60_000.0];
    close(local("OH", "Columbus", &workers).local, 3_750.0);
}

/// States that exempt retirement distributions exempt a Roth conversion, which is
/// one; Illinois has no age test, Pennsylvania follows 59 and a half.
#[test]
fn exempt_states_do_not_tax_conversions() {
    let il = StateCode::parse("IL").unwrap();
    let pa = StateCode::parse("PA").unwrap();
    let state_tax = |state: StateCode, age: f64, conversion: bool| {
        let mut x = joint(state, [age, age]);
        if conversion {
            x.roth_conversions = 100_000.0;
        } else {
            x.traditional_distributions = 100_000.0;
        }
        compute(&params(state), &x).state
    };
    for age in [50.0, 62.0] {
        close(state_tax(il, age, true), 0.0);
        close(state_tax(il, age, false), 0.0);
    }
    close(state_tax(pa, 62.0, true), 0.0);
    close(state_tax(pa, 62.0, false), 0.0);
    assert!(state_tax(pa, 50.0, false) > 3_000.0);
    for code in ["MS", "MI", "IA"] {
        let state = StateCode::parse(code).unwrap();
        close(state_tax(state, 62.0, true), state_tax(state, 62.0, false));
    }
    // New York's 20,000 per person applies to a conversion as to a distribution.
    close(state_tax(NY, 62.0, true), state_tax(NY, 62.0, false));
    // Pennsylvania taxes wages before 401(k) deferrals: 150,000 at 3.07%.
    let mut x = joint(pa, [40.0, 40.0]);
    x.wages = [150_000.0, 0.0];
    x.pretax_deferrals = [10_000.0, 0.0];
    let r = compute(&params(pa), &x);
    close(r.state - r.local, 4_605.0);
}

/// California has a head-of-household schedule and deduction of its own.
#[test]
fn california_head_of_household_has_its_own_schedule() {
    let p = params(CA);
    let hoh = FilingStatus::HeadOfHousehold.index();
    assert!(p.state.brackets[hoh].lower[1] > 1.9 * p.state.brackets[0].lower[1]);
    assert_eq!(
        p.state.standard_deduction[hoh],
        p.state.standard_deduction[1]
    );
    let mut x = single(CA, 40.0);
    x.filing_status = FilingStatus::HeadOfHousehold;
    x.wages[0] = 90_000.0;
    x.children_under_17 = 1;
    let head = compute(&p, &x).state;
    x.filing_status = FilingStatus::Single;
    let alone = compute(&p, &x).state;
    assert!(head < alone - 1_500.0, "{head} vs {alone}");
}

/// The same single earner in Texas pays no state tax and takes the standard
/// deduction: taxable 483,900, tax 58,448 + 35% x 227,675 = 138,134.25.
#[test]
fn high_earner_in_a_zero_tax_state() {
    let mut x = single(TX, 45.0);
    x.wages[0] = 500_000.0;
    let r = compute(&params(TX), &x);
    close(r.state, 0.0);
    assert!(!r.itemized);
    close(r.federal_ordinary, 138_134.25);
    close(r.total, 138_134.25 + 21_389.0);
    // Every state without a wage tax.
    for code in ["AK", "FL", "NV", "NH", "SD", "TN", "TX", "WA", "WY"] {
        let state = StateCode::parse(code).unwrap();
        x.state = state;
        close(compute(&params(state), &x).state, 0.0);
    }
    // And no state at all.
    x.state = StateCode::NONE;
    close(
        compute(&params(StateCode::NONE), &x).total,
        138_134.25 + 21_389.0,
    );
}

#[test]
fn state_specific_rules() {
    // Pennsylvania: flat 3.07% with no deductions, plus the 0.99% average local tax.
    let pa = StateCode(*b"PA");
    let mut x = single(pa, 40.0);
    x.wages[0] = 100_000.0;
    let r = compute(&params(pa), &x);
    close(r.local, 990.0);
    close(r.state, 3_070.0 + 990.0);

    // Illinois does not tax retirement income: a 70-year-old with 60,000 of IRA
    // withdrawals owes nothing; a 50-year-old owes 4.95% x (60,000 - 2,925).
    let il = StateCode(*b"IL");
    let mut x = single(il, 70.0);
    x.traditional_distributions = 60_000.0;
    close(compute(&params(il), &x).state, 0.0);
    x.ages[0] = 50.0;
    close(compute(&params(il), &x).state, 0.0495 * 57_075.0);

    // Colorado, age 66: benefits 30,000 and IRA withdrawals 50,000. Federally
    // taxable benefits 25,500 (the 85% cap), AGI 75,500. Colorado exempts benefits
    // at 65 and 20,000 of retirement income: 30,000 - 16,100 = 13,900 at 4.4%.
    let co = StateCode(*b"CO");
    let mut x = single(co, 66.0);
    x.social_security[0] = 30_000.0;
    x.traditional_distributions = 50_000.0;
    let r = compute(&params(co), &x);
    close(r.taxable_social_security, 25_500.0);
    close(r.state, 0.044 * 13_900.0);
    // At 60 the benefits are taxed (follows federal below 65) but the retirement
    // exclusion still applies: 75,500 - 20,000 - 16,100 = 39,400.
    x.ages[0] = 60.0;
    close(compute(&params(co), &x).state, 0.044 * 39_400.0);

    // Vermont: benefits exempt below 50,000 of AGI, phasing out by 60,000.
    // Age 68, benefits 24,000, pension 40,000: federally taxable 19,800, AGI
    // 59,800, so 98% of the 19,800 is taxed. Vermont taxable income
    // 40,000 + 19,404 - 7,850 - 5,400 = 46,154 at 3.35%.
    let vt = StateCode(*b"VT");
    let mut x = single(vt, 68.0);
    x.social_security[0] = 24_000.0;
    x.pensions = 40_000.0;
    let r = compute(&params(vt), &x);
    close(r.taxable_social_security, 19_800.0);
    close(r.state, 0.0335 * 46_154.0);

    // Minnesota: the subtraction falls 10% per 4,000 of AGI above 84,490.
    // Age 68, benefits 30,000, pension 70,000: taxable benefits 25,500, AGI 95,500,
    // 11,010 over: 3 steps, 30% of 25,500 = 7,650 is taxed.
    // Taxable 70,000 + 7,650 - 15,300 = 62,350: 33,310 at 5.35% + 29,040 at 6.8%.
    let mn = StateCode(*b"MN");
    let mut x = single(mn, 68.0);
    x.social_security[0] = 30_000.0;
    x.pensions = 70_000.0;
    close(
        compute(&params(mn), &x).state,
        0.0535 * 33_310.0 + 0.068 * 29_040.0,
    );

    // Connecticut: above 75,000 of AGI at most 25% of benefits are taxed.
    // Same person: min(25,500, 25% x 30,000) = 7,500 is in Connecticut income.
    // 77,500 less the 15,000 exemption = 62,500: 200 + 1,800 + 5.5% x 12,500.
    let ct = StateCode(*b"CT");
    x.state = ct;
    close(
        compute(&params(ct), &x).state,
        0.02 * 10_000.0 + 0.045 * 40_000.0 + 0.055 * 12_500.0,
    );

    // Utah: flat 4.45%, with a 966 credit that loses 1.3 cents per dollar above
    // 18,213. Wages 50,000: 2,225 - (966 - 413.231) = 1,672.231.
    let ut = StateCode(*b"UT");
    let mut x = single(ut, 40.0);
    x.wages[0] = 50_000.0;
    close(compute(&params(ut), &x).state, 1_672.231);

    // South Carolina: the 15,000 deduction phases out between 40,000 and 95,000.
    // Wages 67,500: half of it is left. Taxable 60,000: 597 + 1,563.
    let sc = StateCode(*b"SC");
    let mut x = single(sc, 40.0);
    x.wages[0] = 67_500.0;
    close(compute(&params(sc), &x).state, 2_160.0);

    // Washington: no wage tax, 7% excise on long-term gains above 278,000.
    let wa = StateCode(*b"WA");
    let mut x = single(wa, 50.0);
    x.wages[0] = 200_000.0;
    x.long_term_gains = 500_000.0;
    close(compute(&params(wa), &x).state, 0.07 * 222_000.0);

    // Wisconsin excludes 30% of long-term gains: with gains of 100,000 and other
    // income of 50,000 the state taxes 120,000 less deduction and exemption, the
    // same as someone with 120,000 of ordinary income... whose standard deduction
    // is the same, so compare taxable bases through the tax itself.
    let wi = StateCode(*b"WI");
    let mut gains = single(wi, 50.0);
    gains.other_ordinary = 50_000.0;
    gains.long_term_gains = 100_000.0;
    let mut plain = single(wi, 50.0);
    plain.other_ordinary = 120_000.0;
    close(
        compute(&params(wi), &gains).state,
        compute(&params(wi), &plain).state,
    );

    // Maryland: 2% surtax on gains once federal AGI passes 350,000.
    let md = StateCode(*b"MD");
    let mut x = single(md, 50.0);
    x.other_ordinary = 300_000.0;
    x.long_term_gains = 40_000.0;
    let below = compute(&params(md), &x).state;
    x.other_ordinary = 320_000.0;
    let above = compute(&params(md), &x).state;
    // 20,000 more income at 5.75% state + 2.4% local, plus 2% x 40,000.
    close(above - below, 20_000.0 * (0.0575 + 0.024) + 800.0);

    // Hawaii caps the rate on long-term gains at 7.25%.
    let hi = StateCode(*b"HI");
    let mut x = single(hi, 50.0);
    x.other_ordinary = 400_000.0;
    let base = compute(&params(hi), &x).state;
    x.long_term_gains = 100_000.0;
    close(compute(&params(hi), &x).state - base, 7_250.0);
}

/// State itemising: a California homeowner deducts mortgage interest and property
/// tax instead of the 11,800 standard deduction.
#[test]
fn state_itemized_deductions() {
    let p = params(CA);
    let mut x = joint(CA, [45.0, 45.0]);
    x.wages = [150_000.0, 0.0];
    let standard = compute(&p, &x).state;
    x.mortgage_interest = 25_000.0;
    x.property_tax = 9_000.0;
    let itemized = compute(&p, &x).state;
    // 34,000 instead of 11,800 comes off income taxed at 6% (joint 85,722 to
    // 118,996 covers 116,000 to 138,200 only in part): compute the difference from
    // the schedule itself.
    let schedule = &p.state.brackets[1];
    close(
        standard - itemized,
        schedule.tax(150_000.0 - 11_800.0) - schedule.tax(150_000.0 - 34_000.0),
    );
    // Pennsylvania allows neither.
    let pa = StateCode(*b"PA");
    let mut x = single(pa, 40.0);
    x.wages[0] = 100_000.0;
    let before = compute(&params(pa), &x).state;
    x.mortgage_interest = 25_000.0;
    close(compute(&params(pa), &x).state, before);
}

#[test]
fn every_state_gives_sane_results() {
    let law = law();
    for a in b'A'..=b'Z' {
        for b in b'A'..=b'Z' {
            let state = StateCode([a, b]);
            if !law.knows_state(state) {
                continue;
            }
            let p = law.for_year(Indexation::base(2026), state);
            assert!(
                (0.001..0.03).contains(&p.state.property_tax_rate),
                "{state:?}"
            );
            let mut worker = joint(state, [40.0, 40.0]);
            worker.wages = [120_000.0, 60_000.0];
            worker.children_under_17 = 2;
            worker.long_term_gains = 10_000.0;
            let mut retiree = joint(state, [70.0, 68.0]);
            retiree.social_security = [30_000.0, 20_000.0];
            retiree.traditional_distributions = 60_000.0;
            retiree.pensions = 20_000.0;
            retiree.qualified_dividends = 8_000.0;
            for x in [worker, retiree] {
                let r = compute(&p, &x);
                assert!(r.total.is_finite() && r.state.is_finite(), "{state:?}");
                assert!(r.state >= 0.0, "{state:?}");
                // No state takes more than 12% of this household's income.
                assert!(r.state <= 0.12 * r.magi, "{state:?}: {}", r.state);
                assert!(r.local <= r.state);
            }
        }
    }
    assert!((law.property_tax_rate(TX) - 0.014).abs() < 1e-12);
    assert!((law.property_tax_rate(StateCode::NONE) - 0.008_958_823_529).abs() < 1e-9);
}

// ---------------------------------------------------------------- indexing

#[test]
fn doubling_prices_doubles_indexed_thresholds_and_leaves_frozen_ones() {
    let law = law();
    let base = law.for_year(Indexation::base(2040), CA);
    let index = Indexation {
        year: 2040,
        cpi_factor: 2.0,
        wage_factor: 3.0,
        fpl_factor: 1.5,
        health_factor: 4.0,
    };
    let doubled = law.for_year(index, CA);
    let (b, d) = (&base.federal, &doubled.federal);
    assert_eq!(doubled.cpi_factor, 2.0);
    for s in 0..FilingStatus::COUNT {
        // Indexed to prices.
        for i in 0..b.ordinary[s].len {
            assert_eq!(d.ordinary[s].lower[i], 2.0 * b.ordinary[s].lower[i]);
            assert_eq!(d.ordinary[s].rate[i], b.ordinary[s].rate[i]);
        }
        for i in 0..b.capital_gains[s].len {
            assert_eq!(
                d.capital_gains[s].lower[i],
                2.0 * b.capital_gains[s].lower[i]
            );
        }
        assert_eq!(d.standard_deduction[s], 2.0 * b.standard_deduction[s]);
        assert_eq!(
            d.additional_deduction_65[s],
            2.0 * b.additional_deduction_65[s]
        );
        assert_eq!(d.qbi_threshold[s], 2.0 * b.qbi_threshold[s]);
        assert_eq!(
            d.limits.roth_ira_phaseout_start[s],
            2.0 * b.limits.roth_ira_phaseout_start[s]
        );
        // Frozen by statute.
        assert_eq!(d.niit_threshold[s], b.niit_threshold[s]);
        assert_eq!(
            d.additional_medicare_threshold[s],
            b.additional_medicare_threshold[s]
        );
        assert_eq!(d.ss_taxation_base[s], b.ss_taxation_base[s]);
        assert_eq!(d.ss_taxation_upper[s], b.ss_taxation_upper[s]);
        assert_eq!(d.child_credit_phaseout[s], b.child_credit_phaseout[s]);
        assert_eq!(d.senior_phaseout[s], b.senior_phaseout[s]);
        assert_eq!(d.qbi_phase_in_width[s], b.qbi_phase_in_width[s]);
        assert_eq!(d.charitable_non_itemizer[s], b.charitable_non_itemizer[s]);
        // IRMAA: thresholds follow prices (2040 is past the top-tier freeze),
        // surcharges follow Medicare premiums.
        for t in 1..b.irmaa[s].len {
            assert_eq!(d.irmaa[s].over[t], 2.0 * b.irmaa[s].over[t]);
            assert!((d.irmaa[s].monthly[t] - 4.0 * b.irmaa[s].monthly[t]).abs() < 1e-9);
        }
    }
    assert_eq!(d.child_tax_credit, 2.0 * b.child_tax_credit);
    assert_eq!(
        d.child_credit_refundable_max,
        2.0 * b.child_credit_refundable_max
    );
    assert_eq!(d.limits.k401_employee, 49_000.0);
    assert_eq!(d.limits.k401_total, 144_000.0);
    assert_eq!(d.limits.ira, 15_000.0);
    assert_eq!(d.limits.hsa_self, 8_800.0);
    assert_eq!(d.limits.hsa_catch_up, b.limits.hsa_catch_up);
    assert_eq!(d.other_dependent_credit, b.other_dependent_credit);
    assert_eq!(d.child_credit_earned_floor, b.child_credit_earned_floor);
    assert_eq!(d.capital_loss_limit, b.capital_loss_limit);
    assert_eq!(d.salt_cap, b.salt_cap);
    assert_eq!(d.se_minimum_earnings, b.se_minimum_earnings);
    // Wages, the poverty line and health costs have their own factors.
    assert_eq!(d.oasdi_wage_base, 3.0 * 184_500.0);
    assert_eq!(d.fpl_base, 1.5 * 15_650.0);
    assert_eq!(d.fpl_per_person, 1.5 * 5_500.0);
    assert_eq!(d.aca, b.aca);

    // California indexes its brackets, deduction and exemption credits.
    let (bs, ds) = (&base.state, &doubled.state);
    assert_eq!(ds.brackets[0].lower[5], 2.0 * 75_197.0);
    assert_eq!(ds.standard_deduction[1], 23_600.0);
    assert_eq!(ds.personal_credit[0], 316.0);
    assert_eq!(ds.dependent_credit, 982.0);
    assert_eq!(bs.standard_deduction[1], 11_800.0);

    // New York does not index anything.
    let ny_base = law.for_year(Indexation::base(2040), NY).state;
    let ny_doubled = law.for_year(index, NY).state;
    assert_eq!(ny_base, ny_doubled);

    // States that use the federal standard deduction follow its indexing; Colorado's
    // flat bracket has nothing to index.
    let co = law.for_year(index, StateCode(*b"CO")).state;
    assert_eq!(co.standard_deduction, d.standard_deduction);
    // Washington's excise deduction is indexed, its $1,000,000 rate break is not.
    let wa = law.for_year(index, StateCode(*b"WA")).state;
    assert_eq!(wa.capital_gains_excise_deduction, 556_000.0);
    assert_eq!(wa.capital_gains_excise.lower[1], 1_000_000.0);
}

#[test]
fn indexed_amounts_are_rounded_like_the_statute() {
    assert_eq!(index_rounded(12_400.0, 1.0, 50.0), 12_400.0);
    assert_eq!(index_rounded(201_775.0, 1.0, 50.0), 201_775.0);
    // 12,400 x 1.03 = 12,772 -> 12,750; 16,100 x 1.03 = 16,583 -> 16,600.
    assert_eq!(index_rounded(12_400.0, 1.03, 50.0), 12_750.0);
    assert_eq!(index_rounded(16_100.0, 1.03, 50.0), 16_600.0);
    // 184,500 x 1.04 = 191,880 -> nearest 300 = 192,000.
    assert_eq!(index_rounded(184_500.0, 1.04, 300.0), 192_000.0);
    assert_eq!(index_rounded(f64::INFINITY, 1.5, 50.0), f64::INFINITY);

    let law = law();
    let p = law.for_year(law.index_from_cpi(2027, 1.03), TX);
    assert_eq!(p.federal.ordinary[0].lower[1], 12_750.0);
    assert_eq!(p.federal.standard_deduction[0], 16_600.0);
    assert_eq!(p.federal.limits.k401_employee, 25_000.0); // 25,235 -> 25,000
    assert_eq!(p.federal.limits.ira, 7_500.0); // 7,725 -> 7,500
    assert_eq!(p.federal.child_tax_credit, 2_300.0); // 2,266 -> 2,300
                                                     // Wages: 1.03 x 1.01 = 1.0403 -> 191,935 -> 192,000.
    assert_eq!(p.federal.oasdi_wage_base, 192_000.0);
    // Thresholds stay ordered after rounding at any factor.
    for k in 0..400 {
        let factor = 0.8 + 0.01 * f64::from(k);
        let p = law.for_year(law.index_from_cpi(2035, factor), CA);
        for s in 0..FilingStatus::COUNT {
            for schedule in [&p.federal.ordinary[s], &p.federal.capital_gains[s]] {
                for i in 1..schedule.len {
                    assert!(schedule.lower[i] > schedule.lower[i - 1]);
                }
            }
            for t in 1..p.federal.irmaa[s].len {
                assert!(p.federal.irmaa[s].over[t] >= p.federal.irmaa[s].over[t - 1]);
            }
        }
    }
}

#[test]
fn derived_wage_factor_and_calendar_rules() {
    let law = law();
    let index = law.index_from_cpi(2036, 1.3);
    assert!((index.wage_factor - 1.3 * crate::math::powi(1.01, 10)).abs() < 1e-12);
    assert_eq!(index.fpl_factor, 1.3);
    assert_eq!(index.health_factor, 1.3);
    assert_eq!(index.with_wage_factor(2.0).wage_factor, 2.0);
    assert_eq!(index.with_fpl_factor(1.2).fpl_factor, 1.2);
    assert_eq!(index.with_health_factor(1.9).health_factor, 1.9);

    // The top IRMAA tier is frozen through 2027 and indexed afterwards.
    let i27 = Indexation::from_cpi(2026, 2027, 1.1);
    let p27 = law.for_year(i27, TX);
    assert_eq!(p27.federal.irmaa[0].over[5], 500_000.0);
    assert_eq!(p27.federal.irmaa[0].over[1], 120_000.0); // 119,900 -> nearest 1,000
    let p28 = law.for_year(Indexation::from_cpi(2026, 2028, 1.1), TX);
    assert_eq!(p28.federal.irmaa[0].over[5], 550_000.0);
    assert_eq!(p28.federal.irmaa[1].over[5], 825_000.0);
}

#[test]
fn index_brackets_off_freezes_the_law_in_dollars() {
    let model = TaxModel {
        index_brackets: false,
        ..TaxModel::default()
    };
    let law = TaxLaw::from_data(Data::embedded(), &model).unwrap();
    let index = Indexation {
        year: 2026,
        cpi_factor: 2.0,
        wage_factor: 2.5,
        fpl_factor: 2.0,
        health_factor: 2.0,
    };
    let frozen = law.for_year(index, CA);
    let base = law.for_year(Indexation::base(2026), CA);
    assert_eq!(frozen.cpi_factor, 1.0);
    assert_eq!(frozen.federal.ordinary, base.federal.ordinary);
    assert_eq!(
        frozen.federal.standard_deduction,
        base.federal.standard_deduction
    );
    assert_eq!(frozen.federal.oasdi_wage_base, base.federal.oasdi_wage_base);
    assert_eq!(frozen.federal.limits, base.federal.limits);
    assert_eq!(frozen.state.brackets, base.state.brackets);
    // The poverty line and Medicare surcharges are not tax brackets: they still move.
    assert_eq!(frozen.federal.fpl_base, 31_300.0);
    assert!((frozen.federal.irmaa[0].monthly[1] - 2.0 * 95.7).abs() < 1e-9);
    // Calendar rules still apply.
    let later = law.for_year(Indexation::base(2031), CA);
    assert_eq!(later.federal.salt_cap, 10_000.0);
    assert_eq!(later.federal.senior_deduction, 0.0);
}

/// Bracket creep: with wages and prices both doubled, indexed law takes the same
/// share of income in federal income tax; the frozen thresholds do not double.
#[test]
fn indexing_prevents_bracket_creep() {
    let law = law();
    let base = law.for_year(Indexation::base(2040), TX);
    let doubled = law.for_year(Indexation::from_cpi(2040, 2040, 2.0), TX);
    let mut x = joint(TX, [40.0, 40.0]);
    x.wages = [90_000.0, 60_000.0];
    let r1 = compute(&base, &x);
    x.wages = [180_000.0, 120_000.0];
    let r2 = compute(&doubled, &x);
    close(r2.federal_ordinary, 2.0 * r1.federal_ordinary);
    close(r2.taxable_income, 2.0 * r1.taxable_income);
    // The additional Medicare threshold is frozen at 250,000, so FICA more than
    // doubles: 0.9% x 50,000 = 450 extra.
    close(
        r2.fica[0] + r2.fica[1],
        2.0 * (r1.fica[0] + r1.fica[1]) + 450.0,
    );
}

// ---------------------------------------------------------------- planning helpers

#[test]
fn room_to_bracket_top_simple_cases() {
    let p = params(TX);
    // Joint, wages 100,000: ordinary taxable 67,800; the 12% bracket ends at
    // 100,800, so 33,000 more fits.
    let mut x = joint(TX, [50.0, 50.0]);
    x.wages[0] = 100_000.0;
    close(room_to_bracket_top(&p, &x, 0.12), 33_000.0);
    close(room_to_bracket_top(&p, &x, 0.22), 143_600.0);
    close(room_to_bracket_top(&p, &x, 0.10), 0.0);
    assert!(room_to_bracket_top(&p, &x, 0.37).is_infinite());
    assert!(room_to_bracket_top(&p, &x, 0.5).is_infinite());
    // Income below the standard deduction: the unused deduction is room too.
    let mut x = single(TX, 50.0);
    x.interest = 6_100.0;
    close(room_to_bracket_top(&p, &x, 0.10), 10_000.0 + 12_400.0);
    // Long-term gains do not use ordinary bracket room, and the standard deduction
    // goes to ordinary income first: 16,100 + 50,400.
    let mut x = single(TX, 50.0);
    x.long_term_gains = 200_000.0;
    close(room_to_bracket_top(&p, &x, 0.12), 66_500.0);
}

/// Where an extra dollar of income makes more Social Security taxable, fewer
/// dollars fit than the gap in taxable income suggests.
#[test]
fn room_to_bracket_top_with_social_security() {
    let p = params(TX);
    let mut x = single(TX, 67.0);
    x.social_security[0] = 30_000.0;
    x.pensions = 20_000.0;
    let before = compute(&p, &x);
    let room = room_to_bracket_top(&p, &x, 0.12);
    let naive = 50_400.0 - before.taxable_income;
    assert!(
        room > 1_000.0 && room < naive - 1_000.0,
        "{room} vs {naive}"
    );
    let mut filled = x;
    filled.roth_conversions = room;
    let after = compute(&p, &filled);
    assert!(after.taxable_income <= 50_400.0);
    assert!(after.taxable_income > 50_400.0 - 0.02);
    filled.roth_conversions = room + 1.0;
    assert!(compute(&p, &filled).taxable_income > 50_400.0);

    // A wide sweep: the answer always lands on the bracket top from below.
    for k in 0..60 {
        let mut x = joint(CA, [66.0, 64.0]);
        x.social_security = [2_000.0 * f64::from(k), 14_000.0];
        x.pensions = 1_500.0 * f64::from(k);
        x.long_term_gains = 700.0 * f64::from(k % 7);
        x.mortgage_interest = 900.0 * f64::from(k % 5);
        for rate in [0.10, 0.12, 0.22, 0.24] {
            let p = params(CA);
            let top = p.federal.ordinary[1].top_of_rate(rate);
            let room = room_to_bracket_top(&p, &x, rate);
            let mut filled = x;
            filled.roth_conversions += room;
            let r = compute(&p, &filled);
            let ordinary = r.taxable_income - (x.long_term_gains).min(r.taxable_income);
            if room == 0.0 {
                continue;
            }
            assert!(ordinary <= top + 1e-6, "k={k} rate={rate}");
            assert!(
                ordinary > top - 0.02,
                "k={k} rate={rate}: {ordinary} vs {top}"
            );
        }
    }
}

#[test]
fn effective_marginal_rate_includes_everything() {
    let p = params(TX);
    // A plain wage earner in the 22% bracket with no state tax: other income adds
    // exactly 22%.
    let mut x = single(TX, 40.0);
    x.wages[0] = 100_000.0;
    assert!((effective_marginal_rate(&p, &x) - 0.22).abs() < 1e-9);
    // In the Social Security phase-in each dollar taxes 1.85 dollars at 12%.
    let mut x = single(TX, 67.0);
    x.social_security[0] = 30_000.0;
    x.pensions = 40_000.0;
    let r = compute(&p, &x);
    assert_eq!(r.marginal_ordinary_rate, 0.12);
    assert!((effective_marginal_rate(&p, &x) - 0.12 * 1.85).abs() < 1e-9);
    // Just below the ACA cliff the next $100 costs the whole credit.
    let x = aca_inputs(62_590.0);
    assert!(effective_marginal_rate(&p, &x) > 10.0);
}

// ---------------------------------------------------------------- other modes

#[test]
fn flat_none_and_foreign_modes() {
    let mut x = joint(CA, [40.0, 40.0]);
    x.wages = [100_000.0, 0.0];
    x.pretax_deferrals = [10_000.0, 0.0];
    x.long_term_gains = 20_000.0;
    x.social_security = [0.0, 12_000.0];
    x.early_distributions = 5_000.0;
    x.aca_household_size = 2;
    x.aca_months = 12;
    x.aca_benchmark_premium = 15_000.0;
    x.on_medicare = [true, true];

    let flat = TaxModel {
        kind: TaxKind::Flat { rate: 0.2 },
        ..TaxModel::default()
    };
    let p = TaxLaw::from_data(Data::embedded(), &flat)
        .unwrap()
        .for_year(Indexation::base(2026), CA);
    let r = compute(&p, &x);
    // 100,000 - 10,000 + 20,000 + 12,000 = 122,000 at 20%.
    close(r.total, 24_400.0);
    close(r.federal_ordinary, 24_400.0);
    close(r.agi, 122_000.0);
    assert_eq!(r.marginal_ordinary_rate, 0.2);
    assert_eq!((r.fica, r.se_tax), ([0.0; 2], [0.0; 2]));
    assert_eq!(
        (
            r.state,
            r.penalties,
            r.aca_premium_credit,
            r.irmaa_surcharge
        ),
        (0.0, 0.0, 0.0, 0.0)
    );
    assert!(room_to_bracket_top(&p, &x, 0.12).is_infinite());
    assert!(room_to_aca_cliff(&p, &x).is_infinite());
    // The contribution limits are still there for the accounts module.
    assert_eq!(p.federal.limits.k401_employee, 24_500.0);

    let none = TaxModel {
        kind: TaxKind::None,
        ..TaxModel::default()
    };
    let p = TaxLaw::from_data(Data::embedded(), &none)
        .unwrap()
        .for_year(Indexation::base(2026), CA);
    let r = compute(&p, &x);
    assert_eq!(r.total, 0.0);
    close(r.agi, 122_000.0);
    let detail = explain(&p, &x);
    assert_eq!(detail.amount("total"), Some(0.0));

    // Living abroad: 25% of income, nothing else.
    let p = params(StateCode::NONE).with_foreign_rate(Some(0.25));
    x.state = StateCode::NONE;
    let r = compute(&p, &x);
    close(r.foreign, 30_500.0);
    close(r.total, 30_500.0);
    assert_eq!(r.federal_ordinary, 0.0);
    let detail = explain(&p, &x);
    assert_eq!(detail.amount("foreign_tax"), Some(r.foreign));
    // Back in the US.
    let r = compute(&p.with_foreign_rate(None), &x);
    assert!(r.foreign == 0.0 && r.fica[0] > 0.0);
}

// ---------------------------------------------------------------- properties

fn wage_sweep_inputs(state: StateCode, status: FilingStatus, wages: f64) -> TaxInputs {
    let mut x = TaxInputs {
        filing_status: status,
        ages: [
            45.0,
            if status == FilingStatus::MarriedJoint {
                43.0
            } else {
                0.0
            },
        ],
        adults: if status == FilingStatus::MarriedJoint {
            2
        } else {
            1
        },
        state,
        ..TaxInputs::default()
    };
    x.wages[0] = wages;
    x.interest = 3_000.0;
    x.qualified_dividends = 4_000.0;
    x.long_term_gains = 15_000.0;
    x.mortgage_interest = 12_000.0;
    x.property_tax = 6_000.0;
    x.charitable = 2_000.0;
    x.aca_household_size = x.adults;
    x.aca_months = 12;
    x.aca_benchmark_premium = 9_000.0 * f64::from(x.adults);
    x
}

/// Tax never falls when wages rise, and never jumps: over a $250 step the total
/// moves by less than the step. (Without children: the refundable child credit
/// phases in with earnings, the one place where more pay means less tax.) The
/// premium credit is outside `total`; it is the only cliff, checked below.
#[test]
fn tax_is_monotone_and_continuous_in_wages() {
    let law = law();
    for code in ["TX", "CA", "NY", "CO", "PA", "HI", "UT", "SC", "OR"] {
        let state = StateCode::parse(code).unwrap();
        let p = law.for_year(Indexation::base(2026), state);
        for status in FilingStatus::ALL {
            let mut previous = compute(&p, &wage_sweep_inputs(state, status, 0.0));
            let mut cliff_drops = 0;
            for step in 1..=4_000 {
                let wages = 250.0 * f64::from(step);
                let r = compute(&p, &wage_sweep_inputs(state, status, wages));
                let delta = r.total - previous.total;
                assert!(delta >= -1e-6, "{code} {status:?} wages {wages}: {delta}");
                assert!(delta < 250.0, "{code} {status:?} wages {wages}: {delta}");
                assert!(r.agi >= previous.agi && r.taxable_income >= previous.taxable_income);
                // The premium credit only ever shrinks: gradually, with one small
                // step where the statutory table jumps from 2.10% to 3.14% at 133%
                // of the poverty line, and all at once at 400%.
                let lost = previous.aca_premium_credit - r.aca_premium_credit;
                assert!(lost >= -1e-6, "{code} {status:?} wages {wages}");
                if lost > 500.0 {
                    cliff_drops += 1;
                    assert!(previous.aca_fpl_ratio <= 4.0 && r.aca_fpl_ratio > 4.0);
                }
                previous = r;
            }
            assert_eq!(cliff_drops, 1, "{code} {status:?}");
        }
    }
}

/// The same for a retiree drawing more from a traditional account, through the
/// Social Security phase-in. Here a dollar of income can cost more than 22 + 18.5
/// cents but never a dollar.
#[test]
fn tax_is_monotone_in_withdrawals() {
    let law = law();
    for code in ["TX", "CA", "CO", "VT", "MN", "NM", "CT"] {
        let state = StateCode::parse(code).unwrap();
        let p = law.for_year(Indexation::base(2026), state);
        let mut previous = f64::NEG_INFINITY;
        for step in 0..=3_000 {
            let mut x = joint(state, [68.0, 66.0]);
            x.social_security = [28_000.0, 18_000.0];
            x.qualified_dividends = 5_000.0;
            x.traditional_distributions = 100.0 * f64::from(step);
            x.on_medicare = [true, true];
            let r = compute(&p, &x);
            assert!(r.total >= previous - 1e-6, "{code} step {step}");
            assert!(r.total.is_finite());
            previous = r.total;
        }
    }
}

#[test]
fn zero_negative_and_huge_inputs() {
    let p = params(CA);
    // Nothing in, nothing out.
    let nobody = TaxInputs {
        state: CA,
        ..TaxInputs::default()
    };
    let expected = TaxResult {
        deduction: 16_100.0,
        ..TaxResult::default()
    };
    assert_eq!(compute(&p, &nobody), expected);
    let r = compute(&p, &joint(CA, [40.0, 40.0]));
    assert_eq!(r.total, 0.0);
    assert_eq!(r.taxable_income, 0.0);

    // Losses everywhere: no negative tax, no NaN.
    let mut x = single(CA, 40.0);
    x.self_employment[0] = -50_000.0;
    x.rental_net = -40_000.0;
    x.short_term_gains = -1e6;
    x.other_ordinary = -5_000.0;
    let r = compute(&p, &x);
    assert_eq!(r.total, 0.0);
    assert!(r.agi < 0.0);
    assert_eq!(r.taxable_income, 0.0);
    close(r.capital_loss_carryforward, 997_000.0);

    // Negative amounts where only positive ones make sense are ignored.
    let mut x = single(CA, 40.0);
    x.wages[0] = -10.0;
    x.social_security[0] = -10.0;
    x.early_distributions = -10.0;
    x.charitable = -10.0;
    let r = compute(&p, &x);
    assert_eq!(r.total, 0.0);

    // A billionaire: everything stays finite and close to the top rates.
    let mut x = joint(CA, [50.0, 50.0]);
    x.wages = [1e9, 5e8];
    x.long_term_gains = 2e9;
    x.self_employment[1] = 1e8;
    x.children_under_17 = 3;
    let r = compute(&p, &x);
    assert!(r.total.is_finite());
    let rate = r.total / r.agi;
    assert!((0.40..0.56).contains(&rate), "{rate}");
    assert_eq!(r.credits, 0.0);
    assert_eq!(r.marginal_ordinary_rate, 0.37);

    // Deferrals larger than pay cannot create a loss.
    let mut x = single(TX, 40.0);
    x.wages[0] = 10_000.0;
    x.pretax_deferrals[0] = 50_000.0;
    close(compute(&params(TX), &x).agi, 0.0);
}

#[test]
fn compute_is_deterministic_and_matches_explain() {
    let p = params(NY);
    let mut x = joint(NY, [66.0, 61.0]);
    x.wages = [0.0, 85_000.0];
    x.pretax_deferrals = [0.0, 12_000.0];
    x.self_employment = [22_000.0, 0.0];
    x.social_security = [31_000.0, 0.0];
    x.traditional_distributions = 18_000.0;
    x.roth_conversions = 10_000.0;
    x.interest = 2_500.0;
    x.qualified_dividends = 6_000.0;
    x.long_term_gains = 14_000.0;
    x.short_term_gains = -2_000.0;
    x.rental_net = 4_000.0;
    x.mortgage_interest = 9_000.0;
    x.property_tax = 11_000.0;
    x.charitable = 3_000.0;
    x.children_under_17 = 1;
    x.on_medicare = [true, false];
    x.aca_household_size = 2;
    x.aca_months = 12;
    x.aca_benchmark_premium = 14_000.0;
    let a = compute(&p, &x);
    let b = compute(&p, &x);
    assert_eq!(a, b);
    let detail = explain(&p, &x);
    assert_eq!(detail.result, a);
    // The parts add up to the total.
    let parts = a.federal_ordinary + a.federal_capital_gains - a.credits
        + a.niit
        + a.fica[0]
        + a.fica[1]
        + a.se_tax[0]
        + a.se_tax[1]
        + a.state
        + a.penalties
        + a.foreign;
    close(parts, a.total);
    close(detail.amount("total").unwrap(), a.total);
    close(detail.amount("agi").unwrap(), a.agi);
    close(detail.amount("taxable_income").unwrap(), a.taxable_income);
    // And so do the explained lines.
    let line = |key: &str| detail.amount(key).unwrap_or(0.0);
    let from_lines = line("federal_income_tax")
        + line("niit")
        + line("fica_0")
        + line("fica_1")
        + line("se_tax_0")
        + line("se_tax_1")
        + line("state_income_tax")
        + line("state_capital_gains_excise")
        + line("local_income_tax")
        + line("early_withdrawal_penalty")
        + line("hsa_penalty");
    close(from_lines, a.total);
    // Income lines add up to AGI.
    let income = line("wages")
        + line("self_employment")
        + line("pretax_contributions")
        + line("investment_income")
        + line("capital_gains")
        + line("retirement_income")
        + line("rental_income")
        + line("other_income")
        + line("half_se_tax_deduction")
        + line("taxable_social_security");
    close(income, a.agi);
    // Keys are unique and every line has a label and a note.
    for (i, l) in detail.lines.iter().enumerate() {
        assert!(!l.label.is_empty() && !l.note.is_empty(), "{}", l.key);
        assert!(
            detail.lines[..i].iter().all(|other| other.key != l.key),
            "{}",
            l.key
        );
    }
}

#[test]
fn explain_reads_like_a_return() {
    let mut x = single(TX, 40.0);
    x.wages[0] = 100_000.0;
    let detail = explain(&params(TX), &x);
    let line = |key: &str| {
        detail
            .lines
            .iter()
            .find(|l| l.key == key)
            .unwrap_or_else(|| panic!("no line {key}"))
    };
    assert_eq!(line("wages").amount, 100_000.0);
    assert_eq!(line("deduction").label, "Standard deduction");
    assert_eq!(line("deduction").amount, -16_100.0);
    assert_eq!(line("taxable_income").amount, 83_900.0);
    assert_eq!(
        line("federal_ordinary_tax").note,
        "12,400 at 10% + 38,000 at 12% + 33,500 at 22%"
    );
    assert_eq!(
        line("fica_0").note,
        "6.2% on wages up to 184,500 plus 1.45% on all wages of 100,000"
    );
    assert_eq!(line("state_income_tax").note, "TX does not tax this income");
    assert_eq!(
        line("total").note,
        "20.82% of income of 100,000; the next dollar of ordinary income is in the 22% federal bracket"
    );
    // Lines come in return order.
    let position = |key: &str| detail.lines.iter().position(|l| l.key == key).unwrap();
    assert!(position("wages") < position("agi"));
    assert!(position("agi") < position("deduction"));
    assert!(position("deduction") < position("taxable_income"));
    assert!(position("taxable_income") < position("federal_ordinary_tax"));
    assert!(position("federal_ordinary_tax") < position("total"));

    // A gains-heavy return explains the stacking.
    let mut x = single(TX, 40.0);
    x.wages[0] = 40_000.0;
    x.long_term_gains = 30_000.0;
    let detail = explain(&params(TX), &x);
    let gains = detail
        .lines
        .iter()
        .find(|l| l.key == "federal_capital_gains_tax")
        .unwrap();
    assert_eq!(
        gains.note,
        "Stacked on top of ordinary income: 25,550 at 0% + 4,450 at 15%"
    );
    // Serialises for the UI.
    let json = serde_json::to_value(&detail).unwrap();
    assert_eq!(json["lines"][0]["key"], "wages");
    assert!(json["result"]["total"].is_number());
}

#[test]
fn tax_inputs_round_trip_through_json() {
    let mut x = joint(CA, [41.0, 39.0]);
    x.wages = [120_000.0, 80_000.0];
    x.on_medicare = [false, true];
    let json = serde_json::to_string(&x).unwrap();
    assert!(json.contains("\"state\":\"CA\""), "{json}");
    assert!(
        json.contains("\"filing_status\":\"married_joint\""),
        "{json}"
    );
    let back: TaxInputs = serde_json::from_str(&json).unwrap();
    assert_eq!(back, x);
    // Missing fields default; "--" and lower case are accepted for the state.
    let sparse: TaxInputs =
        serde_json::from_str(r#"{"wages":[50000,0],"state":"ny","ages":[30,0]}"#).unwrap();
    assert_eq!(sparse.state, NY);
    assert_eq!(sparse.filing_status, FilingStatus::Single);
    let none: TaxInputs = serde_json::from_str(r#"{"state":"--"}"#).unwrap();
    assert_eq!(none.state, StateCode::NONE);
    assert!(serde_json::from_str::<TaxInputs>(r#"{"state":"California"}"#).is_err());
}

// ---------------------------------------------------------------- RMD

#[test]
fn rmd_table_and_start_ages() {
    assert_eq!(rmd_start_age(1940), 70);
    assert_eq!(rmd_start_age(1950), 72);
    assert_eq!(rmd_start_age(1951), 73);
    assert_eq!(rmd_start_age(1959), 73);
    assert_eq!(rmd_start_age(1960), 75);
    assert_eq!(rmd_start_age(1995), 75);
    // Uniform Lifetime Table, 26 CFR 1.401(a)(9)-9(c).
    assert_eq!(rmd_divisor(72, 1953), None);
    assert_eq!(rmd_divisor(73, 1953), Some(26.5));
    assert_eq!(rmd_divisor(74, 1960), None);
    assert_eq!(rmd_divisor(75, 1960), Some(24.6));
    assert_eq!(rmd_divisor(80, 1950), Some(20.2));
    assert_eq!(rmd_divisor(90, 1950), Some(12.2));
    assert_eq!(rmd_divisor(100, 1950), Some(6.4));
    assert_eq!(rmd_divisor(120, 1930), Some(2.0));
    assert_eq!(rmd_divisor(130, 1930), Some(2.0));
    assert_eq!(rmd_divisor(0, 1990), None);
    // Ages 70 and 71 (old law) are below the table: its first row is used.
    assert_eq!(rmd_divisor(71, 1940), Some(27.4));
    let law = law();
    assert_eq!(law.rmd.first_age, 72);
    assert_eq!(law.rmd.divisors.len(), 49);
    assert_eq!(law.rmd.divisor(85, 1960), rmd_divisor(85, 1960));
    // A 500,000 balance at 75: 500,000 / 24.6 = 20,325.20.
    close(500_000.0 / rmd_divisor(75, 1960).unwrap(), 20_325.20);
}

// ---------------------------------------------------------------- speed

/// Run with `cargo test -p firesight-core --release tax::tests::bench -- --ignored
/// --nocapture` to print the time per call.
#[test]
#[ignore = "micro-benchmark; prints nanoseconds per call"]
fn bench_compute() {
    use std::hint::black_box;
    use std::time::Instant;

    let law = law();
    let p = law.for_year(law.index_from_cpi(2031, 1.15), NY);
    let mut worker = joint(NY, [41.0, 39.0]);
    worker.wages = [160_000.0, 95_000.0];
    worker.pretax_deferrals = [23_000.0, 12_000.0];
    worker.hsa_contributions = [4_000.0, 0.0];
    worker.interest = 1_800.0;
    worker.qualified_dividends = 5_200.0;
    worker.long_term_gains = 9_000.0;
    worker.mortgage_interest = 17_000.0;
    worker.property_tax = 12_000.0;
    worker.charitable = 2_500.0;
    worker.children_under_17 = 2;
    let mut retiree = joint(NY, [68.0, 66.0]);
    retiree.social_security = [34_000.0, 21_000.0];
    retiree.traditional_distributions = 48_000.0;
    retiree.roth_conversions = 15_000.0;
    retiree.qualified_dividends = 7_000.0;
    retiree.long_term_gains = 12_000.0;
    retiree.on_medicare = [true, true];
    retiree.aca_household_size = 0;

    let n = 2_000_000u32;
    for (name, x) in [("worker", worker), ("retiree", retiree)] {
        let start = Instant::now();
        let mut sum = 0.0;
        for i in 0..n {
            let mut x = x;
            x.interest += f64::from(i & 1023);
            sum += compute(black_box(&p), black_box(&x)).total;
        }
        let ns = start.elapsed().as_nanos() as f64 / f64::from(n);
        println!("compute ({name}): {ns:.1} ns/call (checksum {sum:.0})");
    }
    let start = Instant::now();
    let mut sum = 0.0;
    let m = 200_000u32;
    for i in 0..m {
        let mut x = retiree;
        x.pensions = f64::from(i & 1023) * 10.0;
        sum += room_to_bracket_top(black_box(&p), black_box(&x), 0.22);
    }
    let ns = start.elapsed().as_nanos() as f64 / f64::from(m);
    println!("room_to_bracket_top: {ns:.1} ns/call (checksum {sum:.0})");
    let start = Instant::now();
    let mut sum = 0.0;
    for i in 0..m {
        let index = law.index_from_cpi(2030, 1.0 + f64::from(i & 255) * 0.001);
        sum += law.for_year(black_box(index), NY).federal.oasdi_wage_base;
    }
    let ns = start.elapsed().as_nanos() as f64 / f64::from(m);
    println!("for_year: {ns:.1} ns/call (checksum {sum:.0})");
}
