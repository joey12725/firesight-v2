//! Tests of the tax realism options: the chained-CPI wedge (`assumptions.taxes.chained_cpi`)
//! and the state rules behind `assumptions.taxes.state_detail`. Expected values are
//! computed by hand; the arithmetic is written next to each case.

use super::*;
use crate::data::Data;
use crate::model::TaxModel;

const TX: StateCode = StateCode(*b"TX");

fn law_with(model: TaxModel) -> TaxLaw {
    TaxLaw::from_data(Data::embedded(), &model).expect("embedded tax data loads")
}

// ------------------------------------------------------------ chained CPI

#[test]
fn chained_cpi_off_is_the_plain_cpi_indexation() {
    let plain = law_with(TaxModel::default());
    let explicit = law_with(TaxModel {
        chained_cpi: false,
        ..TaxModel::default()
    });
    let idx = Indexation::from_cpi(2026, 2036, 1.3);
    assert_eq!(
        plain.for_year(idx, TX).federal,
        explicit.for_year(idx, TX).federal
    );
    // 12,400 x 1.3 = 16,120, rounded to $50 is 16,100; 16,100 x 1.3 = 20,930 -> 20,950.
    let p = plain.for_year(idx, TX).federal;
    assert_eq!(p.ordinary[0].lower[1], 16_100.0);
    assert_eq!(p.standard_deduction[0], 20_950.0);
}

#[test]
fn chained_cpi_slows_only_the_chained_thresholds() {
    let plain = law_with(TaxModel::default());
    let chained = law_with(TaxModel {
        chained_cpi: true,
        ..TaxModel::default()
    });
    // 2036 is nine years after the first published year (2027): the factor is
    // 1.3 x 0.9975^9 = 1.27104.
    let idx = Indexation::from_cpi(2026, 2036, 1.3);
    let a = plain.for_year(idx, TX).federal;
    let b = chained.for_year(idx, TX).federal;
    // 12,400 x 1.27104 = 15,761 -> 15,750; standard deduction 16,100 x 1.27104 =
    // 20,463 -> 20,450; top of the 24% bracket 201,775 x 1.27104 = 256,457 -> 256,450;
    // child credit 2,200 x 1.27104 = 2,796 -> 2,800 (plain: 2,860 -> 2,900).
    assert_eq!(b.ordinary[0].lower[1], 15_750.0);
    assert_eq!(b.standard_deduction[0], 20_450.0);
    assert_eq!(b.ordinary[0].lower[4], 256_450.0);
    assert_eq!(b.child_tax_credit, 2_800.0);
    assert_eq!(a.child_tax_credit, 2_900.0);
    assert!(b.capital_gains[0].lower[1] < a.capital_gains[0].lower[1]);
    // Not chained by statute: contribution limits, wage base, IRMAA tiers, frozen
    // thresholds.
    assert_eq!(a.limits, b.limits);
    assert_eq!(a.oasdi_wage_base, b.oasdi_wage_base);
    assert_eq!(a.irmaa, b.irmaa);
    assert_eq!(a.niit_threshold, b.niit_threshold);
    assert_eq!(a.fpl_base, b.fpl_base);
}

#[test]
fn chained_cpi_leaves_the_published_year_alone() {
    let plain = law_with(TaxModel::default());
    let chained = law_with(TaxModel {
        chained_cpi: true,
        ..TaxModel::default()
    });
    for year in [2026, 2027] {
        let idx = Indexation::from_cpi(2026, year, 1.03);
        assert_eq!(
            plain.for_year(idx, TX).federal,
            chained.for_year(idx, TX).federal,
            "{year}"
        );
    }
    // The wedge starts in 2028.
    let idx = Indexation::from_cpi(2026, 2028, 1.0609);
    assert!(
        chained.for_year(idx, TX).federal.ordinary[0].lower[1]
            <= plain.for_year(idx, TX).federal.ordinary[0].lower[1]
    );
}

#[test]
fn chained_cpi_costs_more_tax_on_the_same_income() {
    let plain = law_with(TaxModel::default());
    let chained = law_with(TaxModel {
        chained_cpi: true,
        ..TaxModel::default()
    });
    let idx = Indexation::from_cpi(2026, 2056, 2.0);
    let x = TaxInputs {
        filing_status: FilingStatus::MarriedJoint,
        ages: [50.0, 50.0],
        adults: 2,
        state: TX,
        wages: [250_000.0, 0.0],
        ..TaxInputs::default()
    };
    let tax_plain = compute(&plain.for_year(idx, TX), &x).total;
    let tax_chained = compute(&chained.for_year(idx, TX), &x).total;
    assert!(tax_chained > tax_plain);
    // Thirty years of 0.25 points leave the brackets 7.2% lower: the extra tax is
    // a few thousand dollars on $250k, not tens of thousands.
    let extra = tax_chained - tax_plain;
    assert!((500.0..6_000.0).contains(&extra), "{extra}");
}
