use super::*;
use crate::model::{Mortgage, RentalTerms};
use crate::rng::Stream;

fn data() -> &'static Data {
    Data::embedded()
}

fn rng(path: u64) -> Rng {
    Rng::stream(11, path, Stream::Housing(0))
}

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "expected {expected}, got {actual} (tolerance {tolerance})"
    );
}

/// A home with no price noise, so that every figure is deterministic.
fn home(value: f64, mortgage: Option<Mortgage>) -> Property {
    Property {
        id: "home".into(),
        value,
        mortgage,
        property_tax_rate: Some(0.012),
        idiosyncratic_volatility: 0.0,
        ..Property::default()
    }
}

fn loan(balance: f64, rate: f64, remaining_months: u32) -> Option<Mortgage> {
    Some(Mortgage {
        balance,
        rate,
        remaining_months,
        payment: None,
        ..Mortgage::default()
    })
}

fn purchase(price: f64, down: f64) -> BuyHome {
    BuyHome {
        price,
        down_payment_fraction: down,
        rate: Some(0.065),
        idiosyncratic_volatility: 0.0,
        property_tax_rate: Some(0.012),
        ..BuyHome::default()
    }
}

#[test]
fn amortized_payment_matches_published_tables() {
    // 300,000 at 6% over 30 years: 1,798.65 a month.
    assert!((amortized_payment(300_000.0, 0.06, 360) - 1_798.65).abs() < 0.01);
    // 20,000 at 0% over 40 months.
    assert_eq!(amortized_payment(20_000.0, 0.0, 40), 500.0);
    assert_eq!(amortized_payment(0.0, 0.05, 360), 0.0);
    assert_eq!(amortized_payment(-5.0, 0.05, 360), 0.0);
    assert_eq!(amortized_payment(1_000.0, 0.05, 0), 0.0);
}

#[test]
fn parameters_match_the_research_file() {
    let p = HousingParams::from_data(data());
    // The 740-759 column of the PMI rate card, in percent: 0.20, 0.38, 0.53, 0.70.
    let expected = [(0.85, 0.002), (0.90, 0.0038), (0.95, 0.0053), (0.97, 0.007)];
    for (band, (upper, rate)) in p.pmi_bands.iter().zip(expected) {
        assert_eq!(band.0, upper);
        close(band.1, rate, 1e-15);
    }
    assert_eq!(p.sigma_local_annual, 0.04);
    assert_eq!(p.sigma_sale, 0.1);
    assert_eq!(p.pmi_rate(0.5), 0.0);
    assert_eq!(p.pmi_rate(0.80), 0.0);
    assert_eq!(p.pmi_rate(0.81), 0.002);
    assert_eq!(p.pmi_rate(0.90), 0.0038);
    assert_eq!(p.pmi_rate(0.95), 0.0053);
    close(p.pmi_rate(0.965), 0.007, 1e-15);
    close(p.pmi_rate(1.0), 0.007, 1e-15);
}

#[test]
fn amortisation_schedule_is_exact() {
    // 300,000 at 6% for 360 months. Reference values from the closed-form schedule:
    // month 1 interest 1,500.00, principal 298.65, balance 299,701.35; balance after
    // 12 months 296,315.96, after 120 months 251,057.17; total interest 347,514.57;
    // the last payment is 1,789.70 principal plus 8.95 interest.
    let m =
        PropertyModel::from_property(&home(500_000.0, loan(300_000.0, 0.06, 360)), 0.01, data());
    let mut s = m.initial_state();
    let mut r = rng(0);
    let payment = s.mortgage.unwrap().payment;
    close(payment, 1_798.651_6, 1e-4);
    let (mut interest, mut principal) = (0.0, 0.0);
    for month in 1..=360 {
        let f = m.step(&mut s, 0.0, 1.0, &mut r);
        interest += f.interest;
        principal += f.principal;
        assert!(f.interest + f.principal <= payment + 1e-9);
        assert_eq!(f.deductible_mortgage_interest, f.interest);
        assert_eq!(f.pmi, 0.0);
        match month {
            1 => {
                close(f.interest, 1_500.0, 1e-9);
                close(f.principal, 298.6516, 1e-4);
                close(s.mortgage_balance(), 299_701.348_4, 1e-4);
            }
            12 => close(s.mortgage_balance(), 296_315.964_9, 1e-4),
            120 => close(s.mortgage_balance(), 251_057.174_9, 1e-4),
            359 => close(s.mortgage_balance(), 1_789.703_1, 1e-4),
            360 => {
                close(f.principal, 1_789.703_1, 1e-4);
                close(f.interest, 8.9485, 1e-4);
                close(f.principal + f.interest, payment, 1e-6);
            }
            _ => {}
        }
        assert_eq!(s.mortgage.is_some(), month < 360);
    }
    close(interest, 347_514.567_2, 1e-4);
    close(principal, 300_000.0, 1e-6);
    assert_eq!(s.equity(), 500_000.0);
    // After payoff the loan costs nothing, but tax, insurance and upkeep go on.
    let f = m.step(&mut s, 0.0, 1.0, &mut r);
    assert_eq!((f.interest, f.principal), (0.0, 0.0));
    close(f.property_tax, 500_000.0 * 0.012 / 12.0, 1e-9);
    close(f.insurance, 500_000.0 * 0.005 / 12.0, 1e-9);
    close(f.maintenance, 500_000.0 * 0.01 / 12.0, 1e-9);
    close(
        f.cash_out(),
        500.0 + 208.333_333_333 + 416.666_666_667,
        1e-6,
    );
}

#[test]
fn odd_loans_end_cleanly() {
    let d = data();
    // A remaining term that is not a whole number of level payments: the entered
    // payment is larger than needed, so the last payment is a partial one.
    let mut p = home(100_000.0, loan(10_000.0, 0.06, 12));
    p.mortgage.as_mut().unwrap().payment = Some(3_000.0);
    let m = PropertyModel::from_property(&p, 0.01, d);
    let mut s = m.initial_state();
    let mut r = rng(1);
    let mut paid = Vec::new();
    while s.mortgage.is_some() {
        let f = m.step(&mut s, 0.0, 1.0, &mut r);
        paid.push(f.principal + f.interest);
    }
    // 10,000 -> 7,050 -> 4,085.25 -> 1,105.68 -> 0: three full payments and a stub.
    assert_eq!(paid.len(), 4);
    close(paid[0], 3_000.0, 1e-9);
    close(paid[3], 1_105.676_25 * 1.005, 1e-6);

    // Zero-rate loan: 40 payments of 500, no interest.
    let m = PropertyModel::from_property(&home(100_000.0, loan(20_000.0, 0.0, 40)), 0.01, d);
    let mut s = m.initial_state();
    for _ in 0..40 {
        let f = m.step(&mut s, 0.0, 1.0, &mut r);
        assert_eq!((f.interest, f.principal), (0.0, 500.0));
    }
    assert!(s.mortgage.is_none());

    // A payment below the interest due pays the interest and no principal.
    let mut p = home(400_000.0, loan(300_000.0, 0.06, 360));
    p.mortgage.as_mut().unwrap().payment = Some(1_000.0);
    let m = PropertyModel::from_property(&p, 0.01, d);
    let mut s = m.initial_state();
    let f = m.step(&mut s, 0.0, 1.0, &mut r);
    assert_eq!((f.interest, f.principal), (1_500.0, 0.0));
    assert_eq!(s.mortgage_balance(), 300_000.0);

    // A mortgage with no balance is no mortgage.
    let m = PropertyModel::from_property(&home(400_000.0, loan(0.0, 0.06, 360)), 0.01, d);
    assert!(m.initial_state().mortgage.is_none());
}

#[test]
fn extra_principal_shortens_the_loan() {
    // 300,000 at 6% with 200 a month extra: repaid in 279 months, interest 256,341.13.
    let m =
        PropertyModel::from_property(&home(500_000.0, loan(300_000.0, 0.06, 360)), 0.01, data());
    let mut s = m.initial_state();
    let mut r = rng(2);
    let (mut months, mut interest, mut principal) = (0, 0.0, 0.0);
    while s.mortgage.is_some() {
        let f = m.step(&mut s, 0.0, 1.0, &mut r);
        months += 1;
        interest += f.interest;
        principal += f.principal + s.prepay(200.0);
    }
    assert_eq!(months, 279);
    close(interest, 256_341.132_8, 1e-3);
    close(principal, 300_000.0, 1e-6);
    // Nothing left to prepay; negative amounts are ignored; overpayment is capped.
    assert_eq!(s.prepay(1_000.0), 0.0);
    let mut s = m.initial_state();
    assert_eq!(s.prepay(-50.0), 0.0);
    assert_eq!(s.prepay(1e12), 300_000.0);
    assert!(s.mortgage.is_none());
}

#[test]
fn purchase_cash_arithmetic() {
    let d = data();
    // 500,000 in today's dollars after 20% national appreciation: 600,000. 20% down
    // (120,000), 3% closing costs (18,000), loan 480,000 at the market rate of 7%.
    let mut event = purchase(500_000.0, 0.2);
    event.rate = None;
    let m = PropertyModel::from_purchase(&event, 0.01, d);
    let mut s = m.initial_state();
    assert!(!s.owned);
    assert_eq!(s.equity(), 0.0);
    let mut r = rng(3);
    // An empty slot produces no flows.
    assert_eq!(m.step(&mut s, 0.01, 1.0, &mut r), HousingMonth::default());
    assert_eq!(m.sell(&mut s, 5, 0.06, 500_000.0), Sale::default());

    let p = m.buy(&mut s, 40, 1.2, 0.07);
    close(p.price, 600_000.0, 1e-9);
    close(p.down_payment, 120_000.0, 1e-9);
    close(p.closing_costs, 18_000.0, 1e-9);
    close(p.cash_needed, 138_000.0, 1e-9);
    close(p.loan, 480_000.0, 1e-9);
    assert_eq!(p.rate, 0.07);
    close(p.payment, amortized_payment(480_000.0, 0.07, 360), 1e-12);
    close(p.payment, 3_193.45, 0.01);
    assert_eq!(p.pmi_monthly, 0.0);
    assert!(s.owned);
    close(s.value, 600_000.0, 1e-9);
    close(s.basis, 618_000.0, 1e-9);
    close(s.equity(), 120_000.0, 1e-9);
    assert_eq!(s.owned_since_month, 40);
    // Buying again does nothing.
    assert_eq!(m.buy(&mut s, 41, 2.0, 0.03), Purchase::default());
    close(s.value, 600_000.0, 1e-9);

    // A rate fixed by the event overrides the market; 15-year term.
    let mut event = purchase(300_000.0, 0.25);
    event.rate = Some(0.05);
    event.term_years = 15;
    event.closing_cost_fraction = 0.0;
    let m = PropertyModel::from_purchase(&event, 0.01, d);
    let mut s = m.initial_state();
    let p = m.buy(&mut s, 0, 1.0, 0.09);
    assert_eq!(p.rate, 0.05);
    close(p.payment, amortized_payment(225_000.0, 0.05, 180), 1e-12);
    close(p.cash_needed, 75_000.0, 1e-9);

    // All cash.
    let m = PropertyModel::from_purchase(&purchase(200_000.0, 1.0), 0.01, d);
    let mut s = m.initial_state();
    let p = m.buy(&mut s, 0, 1.0, 0.07);
    close(p.cash_needed, 206_000.0, 1e-9);
    assert_eq!(p.loan, 0.0);
    assert!(s.mortgage.is_none());

    // An existing property has no purchase to make.
    let m = PropertyModel::from_property(&home(1.0, None), 0.01, d);
    let mut s = PropertyState::default();
    assert_eq!(m.buy(&mut s, 0, 1.0, 0.07), Purchase::default());
}

#[test]
fn pmi_is_charged_until_enough_equity() {
    let d = data();
    // 400,000 home, 10% down: loan 360,000 at 6.5%, loan-to-value 90%, premium
    // 0.38% of the loan a year = 114.00 a month.
    let m = PropertyModel::from_purchase(&purchase(400_000.0, 0.10), 0.01, d);
    let mut s = m.initial_state();
    let p = m.buy(&mut s, 0, 1.0, 0.07);
    close(p.pmi_monthly, 114.0, 1e-9);
    close(p.payment, 2_275.444_9, 1e-4);

    // Flat prices: the balance reaches 80% of the value (320,000) after 95 payments,
    // so PMI is charged in months 1-95 and never again.
    let mut flat = s;
    let mut r = rng(4);
    let mut charged = Vec::new();
    for month in 1..=140 {
        let f = m.step(&mut flat, 0.0, 1.0, &mut r);
        if f.pmi > 0.0 {
            close(f.pmi, 114.0, 1e-9);
            charged.push(month);
        }
    }
    assert_eq!(charged, (1..=95).collect::<Vec<_>>());

    // Prices 20% lower: only the automatic rule applies, at 78% of the original
    // value (312,000), reached after 109 payments.
    let mut fallen = s;
    fallen.value = 320_000.0;
    let months = (0..200)
        .filter(|_| m.step(&mut fallen, 0.0, 1.0, &mut r).pmi > 0.0)
        .count();
    assert_eq!(months, 109);

    // Prices rising 1% a month: after 11 payments the balance (about 356,350) is
    // below 80% of the value (320,000 * 1.01^11 = 357,015), so PMI is paid 11 times.
    let mut rising = s;
    let months = (0..200)
        .filter(|_| m.step(&mut rising, 0.01, 1.0, &mut r).pmi > 0.0)
        .count();
    assert_eq!(months, 11);

    // 5% down is the 90-95% band (0.53%); 3% down the top band (0.70%).
    let m5 = PropertyModel::from_purchase(&purchase(400_000.0, 0.05), 0.01, d);
    let mut s5 = m5.initial_state();
    close(
        m5.buy(&mut s5, 0, 1.0, 0.07).pmi_monthly,
        380_000.0 * 0.0053 / 12.0,
        1e-9,
    );
    let m3 = PropertyModel::from_purchase(&purchase(400_000.0, 0.03), 0.01, d);
    let mut s3 = m3.initial_state();
    close(
        m3.buy(&mut s3, 0, 1.0, 0.07).pmi_monthly,
        388_000.0 * 0.007 / 12.0,
        1e-9,
    );

    // An existing loan above 80% of the home's basis carries PMI on its balance.
    let m = PropertyModel::from_property(&home(400_000.0, loan(350_000.0, 0.065, 350)), 0.01, d);
    close(
        m.initial_state().mortgage.unwrap().pmi_monthly,
        350_000.0 * 0.0038 / 12.0,
        1e-9,
    );
    let m = PropertyModel::from_property(&home(400_000.0, loan(320_000.0, 0.065, 350)), 0.01, d);
    assert_eq!(m.initial_state().mortgage.unwrap().pmi_monthly, 0.0);
}

#[test]
fn sale_of_a_home_applies_the_exclusion() {
    let d = data();
    // Bought for 400,000 (basis), now worth 900,000, 200,000 still owed, 6% costs.
    let mut p = home(900_000.0, loan(200_000.0, 0.05, 240));
    p.basis = Some(400_000.0);
    let m = PropertyModel::from_property(&p, 0.01, d);

    let mut s = m.initial_state();
    let sale = m.sell(&mut s, 0, 0.06, 500_000.0);
    close(sale.price, 900_000.0, 1e-9);
    close(sale.selling_costs, 54_000.0, 1e-9);
    close(sale.mortgage_payoff, 200_000.0, 1e-9);
    close(sale.net_proceeds, 646_000.0, 1e-9);
    // Gain 900,000 - 54,000 - 400,000 = 446,000, all inside a couple's exclusion.
    close(sale.excluded_gain, 446_000.0, 1e-9);
    assert_eq!(sale.taxable_gain, 0.0);
    assert_eq!(sale.depreciation_recapture, 0.0);
    assert!(!s.owned && s.mortgage.is_none());
    assert_eq!(s.equity(), 0.0);
    // Selling again does nothing.
    assert_eq!(m.sell(&mut s, 1, 0.06, 500_000.0), Sale::default());

    // A single owner excludes 250,000 and is taxed on 196,000.
    let mut s = m.initial_state();
    let sale = m.sell(&mut s, 0, 0.06, 250_000.0);
    close(sale.excluded_gain, 250_000.0, 1e-9);
    close(sale.taxable_gain, 196_000.0, 1e-9);

    // A vacation home has no exclusion.
    let mut v = p.clone();
    v.usage = PropertyUse::Vacation;
    let mv = PropertyModel::from_property(&v, 0.01, d);
    let mut s = mv.initial_state();
    let sale = mv.sell(&mut s, 0, 0.06, 500_000.0);
    assert_eq!(sale.excluded_gain, 0.0);
    close(sale.taxable_gain, 446_000.0, 1e-9);

    // A loss on a home is not deductible; under water the seller brings cash.
    let mut l = home(300_000.0, loan(310_000.0, 0.05, 300));
    l.basis = Some(400_000.0);
    let ml = PropertyModel::from_property(&l, 0.01, d);
    let mut s = ml.initial_state();
    let sale = ml.sell(&mut s, 0, 0.06, 500_000.0);
    assert_eq!(sale.taxable_gain, 0.0);
    assert_eq!(sale.excluded_gain, 0.0);
    close(sale.net_proceeds, 300_000.0 - 18_000.0 - 310_000.0, 1e-9);
}

#[test]
fn exclusion_needs_two_years_of_ownership() {
    // Bought at month 10 for 300,000 with no closing costs; prices double at once.
    let mut event = purchase(300_000.0, 1.0);
    event.closing_cost_fraction = 0.0;
    let m = PropertyModel::from_purchase(&event, 0.01, data());
    for (sold_at, qualifies) in [(33, false), (34, true), (200, true)] {
        let mut s = m.initial_state();
        m.buy(&mut s, 10, 1.0, 0.07);
        s.value = 600_000.0;
        let sale = m.sell(&mut s, sold_at, 0.0, 250_000.0);
        if qualifies {
            close(sale.excluded_gain, 250_000.0, 1e-9);
            close(sale.taxable_gain, 50_000.0, 1e-9);
        } else {
            assert_eq!(sale.excluded_gain, 0.0);
            close(sale.taxable_gain, 300_000.0, 1e-9);
        }
    }
}

fn rental(terms: RentalTerms) -> Property {
    Property {
        id: "rental".into(),
        usage: PropertyUse::Rental(terms),
        value: 300_000.0,
        basis: Some(300_000.0),
        mortgage: loan(200_000.0, 0.06, 360),
        property_tax_rate: Some(0.012),
        insurance_rate: 0.005,
        maintenance_rate: 0.01,
        hoa_monthly: 50.0,
        idiosyncratic_volatility: 0.0,
        ..Property::default()
    }
}

fn terms() -> RentalTerms {
    RentalTerms {
        monthly_rent: 2_500.0,
        vacancy: 0.0,
        management_fee: 0.08,
        rent_real_growth: 0.0,
        land_fraction: 0.2,
        years_in_service: 0.0,
    }
}

#[test]
fn rental_operating_and_taxable_income() {
    let m = PropertyModel::from_property(&rental(terms()), 0.03, data());
    let mut s = m.initial_state();
    let mut r = rng(5);
    let f = m.step(&mut s, 0.0, 1.0, &mut r);
    // Rent 2,500; management 8% = 200; tax 1.2% = 300; insurance 0.5% = 125;
    // maintenance 1% = 250; HOA 50: operating costs 925, NOI 1,575.
    close(f.gross_rent, 2_500.0, 1e-9);
    assert_eq!(f.vacancy_loss, 0.0);
    close(f.rent, 2_500.0, 1e-9);
    close(f.management, 200.0, 1e-9);
    close(f.property_tax, 300.0, 1e-9);
    close(f.insurance, 125.0, 1e-9);
    close(f.maintenance, 250.0, 1e-9);
    close(f.hoa, 50.0, 1e-9);
    close(f.operating_costs(), 925.0, 1e-9);
    close(f.net_operating_income(), 1_575.0, 1e-9);
    // Mortgage: 200,000 at 6%, payment 1,199.10, of which 1,000 is interest.
    close(f.interest, 1_000.0, 1e-9);
    close(f.interest + f.principal, 1_199.10, 0.005);
    // Depreciation: 80% of 300,000 over 330 months = 727.27.
    close(f.depreciation, 240_000.0 / 330.0, 1e-9);
    // Taxable: 1,575 - 1,000 - 727.27 = -152.27, although cash flow is +375.90.
    close(
        f.rental_taxable_income,
        1_575.0 - 1_000.0 - 727.272_727_27,
        1e-6,
    );
    assert_eq!(f.rental_taxable(), f.rental_taxable_income);
    close(f.rent - f.cash_out(), 1_575.0 - 1_199.10, 0.005);
    // Rental interest is a rental expense, not home mortgage interest.
    assert_eq!(f.deductible_mortgage_interest, 0.0);

    // Rent follows the price level and its own real growth; HOA follows prices.
    let mut growing = terms();
    growing.rent_real_growth = 0.02;
    let m = PropertyModel::from_property(&rental(growing), 0.03, data());
    let mut s = m.initial_state();
    let mut f = HousingMonth::default();
    for _ in 0..=24 {
        f = m.step(&mut s, 0.0, 1.10, &mut r);
    }
    close(f.gross_rent, 2_500.0 * 1.02 * 1.02 * 1.10, 1e-6);
    close(f.hoa, 55.0, 1e-9);
}

#[test]
fn passive_losses_carry_forward_and_release_on_sale() {
    let m = PropertyModel::from_property(&rental(terms()), 0.03, data());
    let mut s = m.initial_state();
    let mut r = rng(6);
    let mut year_one = 0.0;
    for _ in 0..12 {
        year_one += m.step(&mut s, 0.0, 1.0, &mut r).rental_taxable_income;
    }
    assert!(year_one < 0.0);
    close(s.rental_year_to_date, year_one, 1e-9);
    // A loss year reports zero and carries the loss forward.
    assert_eq!(s.close_tax_year(), 0.0);
    close(s.passive_loss_carryforward, -year_one, 1e-9);
    assert_eq!(s.rental_year_to_date, 0.0);
    // A profitable year first uses the carryforward.
    s.rental_year_to_date = 3_000.0;
    close(s.close_tax_year(), 3_000.0 + year_one, 1e-9);
    assert_eq!(s.passive_loss_carryforward, 0.0);
    s.rental_year_to_date = 500.0;
    close(s.close_tax_year(), 500.0, 1e-9);
    // A carryforward larger than the income is used only in part.
    s.passive_loss_carryforward = 800.0;
    s.rental_year_to_date = 300.0;
    assert_eq!(s.close_tax_year(), 0.0);
    close(s.passive_loss_carryforward, 500.0, 1e-9);
    // On sale, what is left (and this year's loss so far) is released.
    s.rental_year_to_date = -200.0;
    let sale = m.sell(&mut s, 30, 0.06, 0.0);
    close(sale.passive_loss_released, 700.0, 1e-9);
    assert_eq!(s.close_tax_year(), 0.0);
    assert_eq!(s.passive_loss_carryforward, 0.0);
}

#[test]
fn rental_sale_recaptures_depreciation() {
    let d = data();
    let mut p = rental(terms());
    p.mortgage = None;
    let m = PropertyModel::from_property(&p, 0.03, d);
    // Ten years of depreciation: 120 * 240,000 / 330 = 87,272.73.
    let mut s = m.initial_state();
    let mut r = rng(7);
    for _ in 0..120 {
        m.step(&mut s, 0.0, 1.0, &mut r);
    }
    close(s.depreciation_taken, 87_272.727_273, 1e-5);
    let held = s;

    // Sold for 400,000 with 6% costs: gain 400,000 - 24,000 - (300,000 - 87,272.73)
    // = 163,272.73, of which 87,272.73 is recapture and 76,000 capital gain. The
    // exclusion offered is ignored for a rental.
    s.value = 400_000.0;
    let sale = m.sell(&mut s, 120, 0.06, 500_000.0);
    close(sale.depreciation_recapture, 87_272.727_273, 1e-5);
    close(sale.taxable_gain, 76_000.0, 1e-5);
    assert_eq!(sale.excluded_gain, 0.0);
    close(sale.net_proceeds, 376_000.0, 1e-9);

    // Sold for 250,000: gain 250,000 - 15,000 - 212,727.27 = 22,272.73, all recapture.
    let mut s = held;
    s.value = 250_000.0;
    let sale = m.sell(&mut s, 120, 0.06, 0.0);
    close(sale.depreciation_recapture, 22_272.727_273, 1e-5);
    close(sale.taxable_gain, 0.0, 1e-5);

    // Sold for 200,000: a loss of 24,727.27, deductible for a rental.
    let mut s = held;
    s.value = 200_000.0;
    let sale = m.sell(&mut s, 120, 0.06, 0.0);
    assert_eq!(sale.depreciation_recapture, 0.0);
    close(sale.taxable_gain, -24_727.272_727, 1e-5);
}

#[test]
fn depreciation_stops_after_27_5_years() {
    // Already 20 years in service: 240/330 of the building is written off and 90
    // months remain.
    let mut t = terms();
    t.years_in_service = 20.0;
    let m = PropertyModel::from_property(&rental(t), 0.03, data());
    let mut s = m.initial_state();
    close(s.depreciation_taken, 240_000.0 * 240.0 / 330.0, 1e-6);
    let mut r = rng(8);
    let mut months = 0;
    for _ in 0..200 {
        if m.step(&mut s, 0.0, 1.0, &mut r).depreciation > 1e-9 {
            months += 1;
        }
    }
    assert_eq!(months, 90);
    close(s.depreciation_taken, 240_000.0, 1e-6);
    // Fully depreciated on arrival.
    t.years_in_service = 40.0;
    let m = PropertyModel::from_property(&rental(t), 0.03, data());
    let mut s = m.initial_state();
    close(s.depreciation_taken, 240_000.0, 1e-9);
    assert_eq!(m.step(&mut s, 0.0, 1.0, &mut r).depreciation, 0.0);
}

#[test]
fn vacancy_comes_in_whole_months_at_the_configured_rate() {
    for vacancy in [0.0, 0.05, 0.25, 0.6, 1.0] {
        let mut t = terms();
        t.vacancy = vacancy;
        let m = PropertyModel::from_property(&rental(t), 0.03, data());
        let mut s = m.initial_state();
        let mut r = rng(9);
        let months = 300_000;
        let mut vacant = 0u32;
        let (mut gross, mut lost) = (0.0, 0.0);
        for _ in 0..months {
            let f = m.step(&mut s, 0.0, 1.0, &mut r);
            s.mortgage = None;
            s.value = 300_000.0;
            gross += f.gross_rent;
            lost += f.vacancy_loss;
            if f.vacancy_loss > 0.0 {
                // A vacant month loses all the rent and pays no management fee.
                assert_eq!(f.vacancy_loss, f.gross_rent);
                assert_eq!((f.rent, f.management), (0.0, 0.0));
                vacant += 1;
            } else {
                close(f.management, 200.0, 1e-9);
            }
        }
        let share = f64::from(vacant) / f64::from(months);
        // Standard error at most 0.1 points.
        close(share, vacancy, 0.004);
        close(lost / gross, vacancy, 0.004);
    }
}

#[test]
fn property_tax_is_assessed_once_a_year() {
    let m = PropertyModel::from_property(&home(600_000.0, None), 0.01, data());
    let mut s = m.initial_state();
    let mut r = rng(10);
    for month in 0..36 {
        let value = s.value;
        let f = m.step(&mut s, 0.01, 1.0, &mut r);
        // Tax is 1.2% of the value at the last anniversary; insurance and
        // maintenance track the current value.
        let assessed = 600_000.0 * math::powi(1.01, month / 12 * 12);
        close(f.property_tax, assessed * 0.012 / 12.0, 1e-6);
        close(f.insurance, value * 0.005 / 12.0, 1e-9);
        close(f.maintenance, value * 0.01 / 12.0, 1e-9);
    }
    // The state default applies when the property has no rate of its own.
    let mut p = home(600_000.0, None);
    p.property_tax_rate = None;
    let m = PropertyModel::from_property(&p, 0.02, data());
    let mut s = m.initial_state();
    close(m.step(&mut s, 0.0, 1.0, &mut r).property_tax, 1_000.0, 1e-9);
}

#[test]
fn value_follows_the_national_index_and_drift_without_noise() {
    let mut p = home(400_000.0, None);
    p.appreciation_vs_national = 0.01;
    let m = PropertyModel::from_property(&p, 0.01, data());
    let mut s = m.initial_state();
    let mut r = rng(11);
    for _ in 0..120 {
        m.step(&mut s, 0.004, 1.0, &mut r);
    }
    close(
        s.value,
        400_000.0 * math::powi(1.004, 120) * math::powi(1.01, 10),
        1e-4,
    );
    // A total collapse of the index cannot make the value negative.
    m.step(&mut s, -1.5, 1.0, &mut r);
    assert_eq!(s.value, 0.0);
    let f = m.step(&mut s, 0.0, 1.0, &mut r);
    assert!(f.cash_out().is_finite());
}

/// Mean-preserving noise: over 30 years the average simulated value equals the
/// national index path times the property's drift, and the dispersion is the
/// research's local random walk (4% a year at the default setting).
#[test]
fn mean_home_value_after_30_years_equals_index_times_drift() {
    let mut p = home(400_000.0, None);
    p.appreciation_vs_national = -0.005;
    p.idiosyncratic_volatility = Property::DEFAULT_IDIOSYNCRATIC_VOLATILITY;
    let m = PropertyModel::from_property(&p, 0.01, data());
    // A national path with a boom, a bust and a recovery.
    let national = |t: usize| match t % 120 {
        0..=59 => 0.006,
        60..=83 => -0.008,
        _ => 0.003,
    };
    let mut expected = 400_000.0 * math::powi(0.995, 30);
    for t in 0..360 {
        expected *= 1.0 + national(t);
    }
    let paths = 50_000u64;
    let (mut sum, mut sum_l, mut sum_l2) = (0.0, 0.0, 0.0);
    let (mut sale_sum, mut sale_l, mut sale_l2) = (0.0, 0.0, 0.0);
    for path in 0..paths {
        let mut r = rng(path);
        let mut s = m.initial_state();
        for t in 0..360 {
            m.step(&mut s, national(t), 1.0, &mut r);
        }
        sum += s.value;
        let l = math::ln(s.value / expected);
        sum_l += l;
        sum_l2 += l * l;
        // Selling adds the one-off transaction shock (10% at the default setting).
        let value = s.value;
        let ratio = m.sell(&mut s, 360, 0.0, 0.0).price / value;
        sale_sum += ratio;
        sale_l += math::ln(ratio);
        sale_l2 += math::ln(ratio) * math::ln(ratio);
    }
    let n = paths as f64;
    // Standard error of the mean is about 0.1%.
    close(sum / n / expected, 1.0, 0.004);
    let sd = (sum_l2 / n - (sum_l / n) * (sum_l / n)).sqrt();
    close(sd, 0.04 * 30f64.sqrt(), 0.004);
    close(sale_sum / n, 1.0, 0.002);
    let sale_sd = (sale_l2 / n - (sale_l / n) * (sale_l / n)).sqrt();
    close(sale_sd, 0.10, 0.002);

    // Twice the default setting doubles both shocks.
    p.idiosyncratic_volatility = 0.16;
    let m2 = PropertyModel::from_property(&p, 0.01, data());
    close(m2.sigma_local_monthly, 0.08 / 12f64.sqrt(), 1e-15);
    close(m2.sigma_sale, 0.20, 1e-15);
}

#[test]
fn draws_are_fixed_and_paths_repeat() {
    let d = data();
    let owned = PropertyModel::from_property(&rental(terms()), 0.01, d);
    let empty = PropertyModel::from_purchase(&purchase(300_000.0, 0.2), 0.01, d);
    let mut a = rng(12);
    let mut b = rng(12);
    let mut c = rng(12);
    let mut sa = owned.initial_state();
    let mut sb = empty.initial_state();
    let mut sc = owned.initial_state();
    let mut flows_a = Vec::new();
    let mut flows_c = Vec::new();
    for t in 0..240 {
        flows_a.push(owned.step(&mut sa, 0.002, 1.0, &mut a));
        if t == 100 {
            empty.buy(&mut sb, t, 1.3, 0.06);
        }
        if t == 180 {
            empty.sell(&mut sb, t, 0.06, 250_000.0);
        }
        empty.step(&mut sb, 0.002, 1.0, &mut b);
        flows_c.push(owned.step(&mut sc, 0.002, 1.0, &mut c));
    }
    assert_eq!(flows_a, flows_c);
    assert_eq!(sa, sc);
    let next = (a.next_u64(), a.normal());
    assert_eq!((b.next_u64(), b.normal()), next);
    assert_eq!((c.next_u64(), c.normal()), next);
}

#[test]
fn equity_is_value_less_debt() {
    let mut p = PropertyState {
        owned: true,
        value: 400_000.0,
        ..PropertyState::default()
    };
    p.mortgage = Some(MortgageState {
        balance: 250_000.0,
        ..MortgageState::default()
    });
    assert_eq!(p.equity(), 150_000.0);
    assert_eq!(p.mortgage_balance(), 250_000.0);
    p.owned = false;
    assert_eq!(p.equity(), 0.0);
}
