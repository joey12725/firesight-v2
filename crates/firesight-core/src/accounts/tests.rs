use super::*;
use crate::data::Data;
use crate::model::{Account, Allocation, GlidePoint};
use crate::rng::Rng;

const US: usize = 0;
const INTL: usize = 1;
const BONDS: usize = 2;
const RE: usize = 4;

fn weights(pairs: &[(AssetClass, f64)]) -> Weights {
    Allocation::from_pairs(pairs).normalized().unwrap()
}

fn only(asset: AssetClass) -> Weights {
    weights(&[(asset, 1.0)])
}

fn sixty_forty() -> Weights {
    weights(&[(AssetClass::UsStocks, 60.0), (AssetClass::Bonds, 40.0)])
}

#[track_caller]
fn assert_close(a: f64, b: f64) {
    assert!(
        (a - b).abs() <= 1e-9 * (1.0 + a.abs().max(b.abs())),
        "{a} != {b}"
    );
}

fn ctx(age: f64) -> WithdrawContext {
    WithdrawContext {
        owner_age: age,
        ..WithdrawContext::default()
    }
}

/// 2026 limits from `data/tax/us-federal-2026.json` (`retirement_accounts`).
fn limits_2026() -> ContributionLimits {
    ContributionLimits {
        k401_employee: 24_500.0,
        k401_catch_up: 8_000.0,
        k401_total: 72_000.0,
        ira: 7_500.0,
        ira_catch_up: 1_100.0,
        hsa_self: 4_400.0,
        hsa_family: 8_750.0,
        hsa_catch_up: 1_000.0,
        roth_ira_phaseout_start: [153_000.0, 242_000.0, 153_000.0],
        roth_ira_phaseout_end: [168_000.0, 252_000.0, 168_000.0],
        ..ContributionLimits::default()
    }
}

#[test]
fn constants_match_the_sourced_data() {
    let fed = &Data::embedded().tax_federal;
    let early = &fed["early_withdrawal"];
    assert_eq!(
        early["penalty_free_age"].as_f64(),
        Some(EARLY_WITHDRAWAL_AGE)
    );
    assert_eq!(
        early["hsa_non_medical_penalty"]["penalty_free_age"].as_f64(),
        Some(HSA_PENALTY_FREE_AGE)
    );
    assert_eq!(
        early["roth_ira"]["conversion_five_year_rule_years"].as_u64(),
        Some(CONVERSION_SEASONING_YEARS as u64)
    );
    assert_eq!(
        early["roth_ira"]["five_year_rule_contributions_years"].as_u64(),
        Some(u64::from(ROTH_ACCOUNT_SEASONING_YEARS))
    );
    let ra = &fed["retirement_accounts"];
    assert!(ra["ira"]["catch_up_age_50_plus"].is_number());
    assert!(ra["hsa"]["catch_up_age_55_plus"].is_number());
    assert_eq!(CATCH_UP_AGE, 50.0);
    assert_eq!(HSA_CATCH_UP_AGE, 55.0);
    // The limits used in these tests are the published 2026 figures.
    let l = limits_2026();
    let plans = &ra["employer_plans_401k_403b_457b_tsp"];
    assert_eq!(
        plans["elective_deferral_limit"].as_f64(),
        Some(l.k401_employee)
    );
    assert_eq!(
        plans["catch_up_age_50_plus"].as_f64(),
        Some(l.k401_catch_up)
    );
    assert_eq!(
        plans["total_annual_additions_415c"].as_f64(),
        Some(l.k401_total)
    );
    assert_eq!(ra["ira"]["contribution_limit"].as_f64(), Some(l.ira));
    assert_eq!(
        ra["hsa"]["contribution_limit_family"].as_f64(),
        Some(l.hsa_family)
    );
}

#[test]
fn every_type_is_plain_copy_data() {
    // No heap anywhere: the monthly loop can pass and return these by value.
    fn is_copy<T: Copy>() {}
    is_copy::<Accounts>();
    is_copy::<Withdrawal>();
    is_copy::<WithdrawContext>();
    is_copy::<InvestmentIncome>();
    is_copy::<Realized>();
    is_copy::<SavingsFlows>();
    is_copy::<PayrollDeferral>();
    assert!(std::mem::size_of::<Accounts>() < 1024);
}

#[test]
fn from_profile_pools_accounts_and_applies_default_basis() {
    let mut p = Profile::default();
    let acct = |kind, owner, balance| Account {
        kind,
        owner,
        balance,
        ..Account::default()
    };
    p.accounts = vec![
        acct(AccountKind::Cash, 0, 10_000.0),
        acct(AccountKind::Taxable, 0, 100_000.0),
        acct(AccountKind::Roth, 1, 50_000.0),
        Account {
            holdings: Some(Allocation::single(AssetClass::Bonds)),
            ..acct(AccountKind::Traditional, 0, 200_000.0)
        },
        acct(AccountKind::Hsa, 0, 5_000.0),
        Account {
            cost_basis: Some(15_000.0),
            ..acct(AccountKind::Taxable, 1, 20_000.0)
        },
    ];
    let a = Accounts::from_profile(&p);
    assert_eq!(a.cash, 10_000.0);
    // Default target 60/20/20; default basis 60% of 100k plus the stated 15k.
    assert_close(a.taxable.value[US], 72_000.0);
    assert_close(a.taxable.value[INTL], 24_000.0);
    assert_close(a.taxable.value[BONDS], 24_000.0);
    assert_close(a.taxable.basis[US], 0.6 * 75_000.0);
    assert_close(a.unrealized_gain(), 120_000.0 - 75_000.0);
    assert_close(a.person[0].traditional[BONDS], 200_000.0);
    assert_eq!(a.person[0].traditional[US], 0.0);
    assert_close(a.total(Bucket::Roth(1)), 50_000.0);
    assert_close(a.person[1].roth.contribution_basis, 25_000.0);
    assert_eq!(a.person[1].roth.years_open, ROTH_ACCOUNT_SEASONING_YEARS);
    assert_close(a.roth_accessible(1), 25_000.0);
    assert_close(a.total(Bucket::Hsa(0)), 5_000.0);
    let k = a.by_kind();
    assert_close(k[AccountKind::Cash.index()], 10_000.0);
    assert_close(k[AccountKind::Taxable.index()], 120_000.0);
    assert_close(k[AccountKind::Traditional.index()], 200_000.0);
    assert_close(k[AccountKind::Roth.index()], 50_000.0);
    assert_close(k[AccountKind::Hsa.index()], 5_000.0);
    assert_close(a.investable(), 385_000.0);
    assert_close(sum(&a.asset_totals()), 375_000.0);
    assert_eq!(a.total(Bucket::Roth(7)), 0.0);
}

#[test]
fn from_profile_places_tax_efficiently_without_changing_the_mix() {
    let mut p = Profile::default();
    p.portfolio.location = AssetLocation::TaxEfficient;
    p.portfolio.target =
        Allocation::from_pairs(&[(AssetClass::UsStocks, 60.0), (AssetClass::Bonds, 40.0)]);
    for (kind, balance) in [
        (AccountKind::Taxable, 100_000.0),
        (AccountKind::Traditional, 100_000.0),
        (AccountKind::Roth, 50_000.0),
    ] {
        p.accounts.push(Account {
            kind,
            balance,
            ..Account::default()
        });
    }
    let a = Accounts::from_profile(&p);
    let totals = a.asset_totals();
    assert_close(totals[US], 150_000.0);
    assert_close(totals[BONDS], 100_000.0);
    // All 100k of bonds fit in the traditional account; the rest is stock.
    assert_close(a.person[0].traditional[BONDS], 100_000.0);
    assert_close(a.taxable.value[US], 100_000.0);
    assert_close(a.person[0].roth.value[US], 50_000.0);
    // Basis total is preserved (60% default).
    assert_close(sum(&a.taxable.basis), 60_000.0);
}

#[test]
fn taxable_basis_is_average_cost() {
    // Buy 100, it grows to 150, sell 60: 40 of basis goes with it, gain 20.
    let mut a = Accounts::default();
    a.contribute(Bucket::Taxable, 100.0, &only(AssetClass::UsStocks));
    let mut r = [0.0; N_ASSETS];
    r[US] = 0.5;
    a.apply_returns(&r, 0.0, 0.0, &[0.0; N_ASSETS]);
    assert_close(a.taxable.value[US], 150.0);
    let w = a.withdraw(Bucket::Taxable, 60.0, &ctx(40.0));
    assert_close(w.amount, 60.0);
    assert_close(w.long_term_gain, 20.0);
    assert_eq!(w.ordinary, 0.0);
    assert_eq!(w.penalty_base, 0.0);
    assert_close(a.taxable.value[US], 90.0);
    assert_close(a.taxable.basis[US], 60.0);
    assert_close(a.unrealized_gain(), 30.0);

    // It halves to 45 against a basis of 60: selling 15 realises a loss of 5.
    r[US] = -0.5;
    a.apply_returns(&r, 0.0, 0.0, &[0.0; N_ASSETS]);
    let w = a.withdraw(Bucket::Taxable, 15.0, &ctx(40.0));
    assert_close(w.long_term_gain, 15.0 - 20.0);
    assert_close(a.taxable.basis[US], 40.0);

    // Asking for more than there is takes everything and realises the rest.
    let w = a.withdraw(Bucket::Taxable, 1_000.0, &ctx(40.0));
    assert_close(w.amount, 30.0);
    assert_close(w.long_term_gain, 30.0 - 40.0);
    assert_eq!(a.taxable, TaxableBucket::default());
}

#[test]
fn withdrawals_sell_pro_rata_or_overweight_first() {
    let mut a = Accounts::default();
    a.contribute(
        Bucket::Taxable,
        100.0,
        &weights(&[(AssetClass::UsStocks, 80.0), (AssetClass::Bonds, 20.0)]),
    );
    let half = weights(&[(AssetClass::UsStocks, 1.0), (AssetClass::Bonds, 1.0)]);

    let mut pro_rata = a;
    pro_rata.withdraw(Bucket::Taxable, 30.0, &ctx(40.0));
    assert_close(pro_rata.taxable.value[US], 56.0);
    assert_close(pro_rata.taxable.value[BONDS], 14.0);

    // Target 50/50 of the 70 left is 35 each: stocks are 45 over, bonds are under,
    // so all 30 comes from stocks.
    let prefer = WithdrawContext {
        prefer_overweight: Some(half),
        ..ctx(40.0)
    };
    let mut steered = a;
    steered.withdraw(Bucket::Taxable, 30.0, &prefer);
    assert_close(steered.taxable.value[US], 50.0);
    assert_close(steered.taxable.value[BONDS], 20.0);

    // The overweight asset is mostly held elsewhere: sell what this bucket has of
    // it, then the remainder pro rata.
    let mut b = Accounts::default();
    b.contribute(
        Bucket::Taxable,
        100.0,
        &weights(&[(AssetClass::UsStocks, 10.0), (AssetClass::Bonds, 90.0)]),
    );
    b.contribute(Bucket::Traditional(0), 300.0, &only(AssetClass::UsStocks));
    let w = b.withdraw(Bucket::Taxable, 50.0, &prefer);
    assert_close(w.amount, 50.0);
    assert_close(b.taxable.value[US], 0.0);
    assert_close(b.taxable.value[BONDS], 50.0);
}

#[test]
fn traditional_withdrawals_are_ordinary_and_penalised_before_59_and_a_half() {
    let mut a = Accounts::default();
    a.contribute(Bucket::Traditional(0), 100_000.0, &sixty_forty());

    let w = a.withdraw(Bucket::Traditional(0), 10_000.0, &ctx(45.0));
    assert_close(w.amount, 10_000.0);
    assert_close(w.ordinary, 10_000.0);
    assert_close(w.penalty_base, 10_000.0);
    assert_eq!(w.long_term_gain, 0.0);

    let w = a.withdraw(Bucket::Traditional(0), 10_000.0, &ctx(59.5));
    assert_close(w.ordinary, 10_000.0);
    assert_eq!(w.penalty_base, 0.0);

    // Rule of 55 or a 72(t) schedule.
    let exempt = WithdrawContext {
        penalty_exempt: true,
        ..ctx(56.0)
    };
    let w = a.withdraw(Bucket::Traditional(0), 10_000.0, &exempt);
    assert_close(w.ordinary, 10_000.0);
    assert_eq!(w.penalty_base, 0.0);

    // Mix is preserved by a pro rata sale; overdrawing empties the bucket.
    assert_close(a.person[0].traditional[US], 0.6 * 70_000.0);
    let w = a.withdraw(Bucket::Traditional(0), 1e12, &ctx(70.0));
    assert_close(w.amount, 70_000.0);
    assert_eq!(a.total(Bucket::Traditional(0)), 0.0);
    assert_eq!(
        a.withdraw(Bucket::Traditional(0), 5.0, &ctx(70.0)),
        Withdrawal::default()
    );
}

fn roth_example() -> Accounts {
    // 100k Roth: 30k contributions, 10k seasoned conversions, a 20k conversion from
    // two years ago, a 5k conversion this year, 35k earnings.
    let mut a = Accounts::default();
    a.contribute(Bucket::Roth(0), 100_000.0, &sixty_forty());
    let r = &mut a.person[0].roth;
    r.contribution_basis = 30_000.0;
    r.seasoned_conversions = 10_000.0;
    r.unseasoned[2] = 20_000.0;
    r.unseasoned[4] = 5_000.0;
    r.years_open = 10;
    a
}

#[test]
fn roth_ordering_at_age_45() {
    let mut a = roth_example();
    assert_close(a.roth_accessible(0), 40_000.0);

    // Contributions, then seasoned conversions: free.
    let w = a.withdraw(Bucket::Roth(0), 35_000.0, &ctx(45.0));
    assert_close(w.amount, 35_000.0);
    assert_eq!((w.ordinary, w.penalty_base), (0.0, 0.0));
    assert_close(a.person[0].roth.contribution_basis, 0.0);
    assert_close(a.person[0].roth.seasoned_conversions, 5_000.0);

    // 5k seasoned (free), then 15k of the unseasoned conversion: already taxed, so
    // no income, but the 10% penalty applies.
    let w = a.withdraw(Bucket::Roth(0), 20_000.0, &ctx(45.0));
    assert_eq!(w.ordinary, 0.0);
    assert_close(w.penalty_base, 15_000.0);
    assert_close(a.person[0].roth.unseasoned[2], 5_000.0);

    // Oldest conversion first (5k), then this year's (5k), then 20k earnings: the
    // earnings are income, and all 30k is penalised.
    let w = a.withdraw(Bucket::Roth(0), 30_000.0, &ctx(45.0));
    assert_close(w.ordinary, 20_000.0);
    assert_close(w.penalty_base, 30_000.0);
    assert_eq!(
        a.person[0].roth.unseasoned,
        [0.0; CONVERSION_SEASONING_YEARS]
    );

    // What is left is all earnings; an exception waives the penalty, not the tax.
    let exempt = WithdrawContext {
        penalty_exempt: true,
        ..ctx(45.0)
    };
    let w = a.withdraw(Bucket::Roth(0), 5_000.0, &exempt);
    assert_close(w.ordinary, 5_000.0);
    assert_eq!(w.penalty_base, 0.0);
    assert_close(a.total(Bucket::Roth(0)), 10_000.0);
}

#[test]
fn roth_ordering_at_age_62() {
    // Past 59 and a half with the account over five years old: everything is free.
    let mut a = roth_example();
    let w = a.withdraw(Bucket::Roth(0), 100_000.0, &ctx(62.0));
    assert_close(w.amount, 100_000.0);
    assert_eq!((w.ordinary, w.penalty_base), (0.0, 0.0));
    assert_eq!(a.person[0].roth.contribution_basis, 0.0);

    // Same person, but the account was opened two years ago: basis and conversions
    // are free, earnings are taxed, nothing is penalised.
    let mut a = roth_example();
    a.person[0].roth.years_open = 2;
    let w = a.withdraw(Bucket::Roth(0), 70_000.0, &ctx(62.0));
    assert_close(w.ordinary, 5_000.0);
    assert_eq!(w.penalty_base, 0.0);
}

#[test]
fn conversions_season_after_five_january_rolls() {
    let mut a = Accounts::default();
    a.contribute(Bucket::Traditional(0), 100_000.0, &sixty_forty());
    assert_close(a.convert_to_roth(0, 40_000.0), 40_000.0);
    assert_close(a.total(Bucket::Traditional(0)), 60_000.0);
    assert_close(a.total(Bucket::Roth(0)), 40_000.0);
    assert_close(a.person[0].roth.value[BONDS], 16_000.0);
    assert_close(a.person[0].roth.unseasoned[4], 40_000.0);
    assert_eq!(a.roth_accessible(0), 0.0);
    assert!(a.person[0].roth.opened);

    for year in 1..=4 {
        a.roll_year();
        assert_eq!(a.roth_accessible(0), 0.0, "year {year}");
        assert_eq!(a.person[0].roth.years_open, year);
    }
    assert_close(a.person[0].roth.unseasoned[0], 40_000.0);
    // Still inside the five years at age 50: penalty, no tax.
    let w = a.withdraw(Bucket::Roth(0), 10_000.0, &ctx(50.0));
    assert_eq!(w.ordinary, 0.0);
    assert_close(w.penalty_base, 10_000.0);

    a.roll_year();
    assert_close(a.person[0].roth.seasoned_conversions, 30_000.0);
    assert_close(a.roth_accessible(0), 30_000.0);
    let w = a.withdraw(Bucket::Roth(0), 30_000.0, &ctx(51.0));
    assert_eq!((w.ordinary, w.penalty_base), (0.0, 0.0));

    // Converting more than the balance converts what is there; nothing from nothing.
    assert_close(a.convert_to_roth(0, 1e9), 60_000.0);
    assert_eq!(a.total(Bucket::Traditional(0)), 0.0);
    assert_eq!(a.convert_to_roth(0, 100.0), 0.0);
    assert_eq!(a.convert_to_roth(1, 100.0), 0.0);
    assert_eq!(a.convert_to_roth(9, 100.0), 0.0);
}

#[test]
fn hsa_is_free_for_medical_and_penalised_otherwise_before_65() {
    let mut a = Accounts::default();
    a.contribute(Bucket::Hsa(0), 10_000.0, &sixty_forty());
    let w = a.withdraw(
        Bucket::Hsa(0),
        4_000.0,
        &WithdrawContext {
            qualified_medical: 3_000.0,
            ..ctx(50.0)
        },
    );
    assert_close(w.amount, 4_000.0);
    assert_close(w.ordinary, 1_000.0);
    assert_close(w.penalty_base, 1_000.0);

    let w = a.withdraw(Bucket::Hsa(0), 2_000.0, &ctx(66.0));
    assert_close(w.ordinary, 2_000.0);
    assert_eq!(w.penalty_base, 0.0);

    let w = a.withdraw(
        Bucket::Hsa(0),
        2_000.0,
        &WithdrawContext {
            qualified_medical: f64::INFINITY,
            ..ctx(40.0)
        },
    );
    assert_eq!((w.ordinary, w.penalty_base), (0.0, 0.0));
    assert_close(a.total(Bucket::Hsa(0)), 2_000.0);
}

#[test]
fn cash_withdrawals_and_bad_inputs() {
    let mut a = Accounts::default();
    a.contribute(Bucket::Cash, 500.0, &[0.0; N_ASSETS]);
    assert_eq!(a.withdraw(Bucket::Cash, 200.0, &ctx(30.0)).amount, 200.0);
    assert_eq!(a.withdraw(Bucket::Cash, 1_000.0, &ctx(30.0)).amount, 300.0);
    assert_eq!(a.cash, 0.0);

    let before = a;
    for bad in [0.0, -5.0, f64::NAN] {
        for bucket in [
            Bucket::Cash,
            Bucket::Taxable,
            Bucket::Roth(0),
            Bucket::Hsa(3),
        ] {
            a.contribute(bucket, bad, &sixty_forty());
            assert_eq!(a.withdraw(bucket, bad, &ctx(30.0)), Withdrawal::default());
        }
        assert_eq!(a.convert_to_roth(0, bad), 0.0);
        assert_eq!(a.harvest_gains(bad), 0.0);
    }
    a.contribute(Bucket::Taxable, f64::INFINITY, &sixty_forty());
    a.contribute(Bucket::Traditional(5), 100.0, &sixty_forty());
    assert_eq!(a, before);

    // A mix that cannot be normalised buys the cash sleeve rather than losing money.
    a.contribute(Bucket::Taxable, 100.0, &[0.0; N_ASSETS]);
    assert_eq!(a.taxable.value[AssetClass::Cash.index()], 100.0);
    assert_eq!(a.taxable.basis[AssetClass::Cash.index()], 100.0);
}

#[test]
fn apply_returns_hand_example() {
    let mut a = Accounts {
        cash: 1_000.0,
        ..Accounts::default()
    };
    a.contribute(Bucket::Taxable, 1_000.0, &only(AssetClass::UsStocks));
    a.contribute(Bucket::Taxable, 1_000.0, &only(AssetClass::Bonds));
    a.contribute(Bucket::Taxable, 1_000.0, &only(AssetClass::RealEstate));
    a.taxable.basis[US] = 500.0;
    a.contribute(Bucket::Traditional(0), 1_000.0, &only(AssetClass::UsStocks));

    let mut returns = [0.0; N_ASSETS];
    returns[US] = 0.02;
    returns[BONDS] = 0.01;
    returns[RE] = 0.03;
    let mut yields = [0.0; N_ASSETS];
    yields[US] = 0.024; // 2 a month per 1,000
    yields[BONDS] = 0.048; // 4
    yields[RE] = 0.06; // 5
    let before = a.investable();
    let inc = a.apply_returns(&returns, 0.01, 0.001, &yields);

    assert_close(a.cash, 1_010.0);
    assert_close(a.taxable.value[US], 1_020.0 * 0.999);
    assert_close(a.taxable.value[BONDS], 1_010.0 * 0.999);
    assert_close(a.taxable.value[RE], 1_030.0 * 0.999);
    assert_close(a.person[0].traditional[US], 1_020.0 * 0.999);
    assert_close(inc.interest, 10.0 + 4.0);
    assert_close(inc.qualified_dividends, 2.0);
    assert_close(inc.ordinary_dividends, 5.0);
    assert_close(inc.fees, 1.02 + 1.01 + 1.03 + 1.02);
    // Distributions were reinvested: basis rises by exactly the taxed amount.
    assert_close(a.taxable.basis[US], 502.0);
    assert_close(a.taxable.basis[BONDS], 1_004.0);
    assert_close(a.taxable.basis[RE], 1_005.0);
    assert_close(inc.net_change, 10.0 + 18.98 + 8.99 + 28.97 + 18.98);
    assert_close(a.investable() - before, inc.net_change);

    // A wipe-out (and a nonsensical return below -100%) leaves zero, not a negative.
    returns[US] = -1.5;
    let inc = a.apply_returns(&returns, 0.0, 0.001, &yields);
    assert_eq!(a.taxable.value[US], 0.0);
    assert_eq!(a.person[0].traditional[US], 0.0);
    assert!(inc.net_change < -1_900.0);
    assert_eq!(inc.qualified_dividends, 0.0);
}

#[test]
fn cash_terms_blend_by_balance() {
    let mut p = Profile::default();
    assert_eq!(
        cash_terms(&p),
        CashTerms {
            tracking_share: 1.0,
            spread: -0.005,
            fixed_rate: 0.0
        }
    );
    p.accounts = vec![
        Account {
            kind: AccountKind::Cash,
            balance: 30_000.0,
            cash_yield: CashYield::TracksBills { spread: -0.005 },
            ..Account::default()
        },
        Account {
            kind: AccountKind::Cash,
            balance: 10_000.0,
            cash_yield: CashYield::Fixed { rate: 0.01 },
            ..Account::default()
        },
        Account {
            kind: AccountKind::Taxable,
            balance: 1e6,
            ..Account::default()
        },
    ];
    let t = cash_terms(&p);
    assert_close(t.tracking_share, 0.75);
    assert_close(t.spread, -0.005);
    assert_close(t.fixed_rate, 0.01);
    assert_close(t.annual_rate(0.04), 0.75 * 0.035 + 0.25 * 0.01);
    // The tracking share never pays a negative rate.
    assert_close(t.annual_rate(0.001), 0.0025);
    assert_close(
        math::monthly_to_annual(t.monthly_rate(0.04)),
        t.annual_rate(0.04),
    );
}

#[test]
fn mirror_rebalance_realises_gains_at_average_cost() {
    let mut a = Accounts::default();
    a.taxable.value[US] = 150.0;
    a.taxable.basis[US] = 100.0;
    a.taxable.value[BONDS] = 50.0;
    a.taxable.basis[BONDS] = 50.0;
    a.contribute(Bucket::Roth(1), 80.0, &only(AssetClass::UsStocks));
    let half = weights(&[(AssetClass::UsStocks, 1.0), (AssetClass::Bonds, 1.0)]);
    assert_close(a.max_drift(&half), 230.0 / 280.0 - 0.5);

    let r = a.rebalance_to(&half, AssetLocation::Mirror);
    // Sell 50 of 150 stock with basis 100: cost 33.33, gain 16.67.
    assert_close(r.long_term_gain, 50.0 - 100.0 / 3.0);
    assert_close(r.turnover, 50.0 + 40.0);
    assert_close(a.taxable.value[US], 100.0);
    assert_close(a.taxable.value[BONDS], 100.0);
    assert_close(a.taxable.basis[US], 200.0 / 3.0);
    assert_close(a.taxable.basis[BONDS], 100.0);
    assert_close(a.person[1].roth.value[BONDS], 40.0);
    assert!(a.max_drift(&half) < 1e-12);
    // Already on target: nothing trades.
    assert_eq!(
        a.rebalance_to(&half, AssetLocation::Mirror),
        Realized::default()
    );
    assert_eq!(Accounts::default().max_drift(&half), 0.0);
}

#[test]
fn tax_efficient_rebalance_trades_in_sheltered_accounts_first() {
    // Taxable 100 stock with a gain, traditional 60/40, Roth all stock. Household
    // target 60/40 of 300 = 180 stock, 120 bonds.
    let mut a = Accounts::default();
    a.taxable.value[US] = 100.0;
    a.taxable.basis[US] = 50.0;
    a.contribute(Bucket::Traditional(0), 100.0, &sixty_forty());
    a.contribute(Bucket::Roth(0), 100.0, &only(AssetClass::UsStocks));
    let mut mirror = a;

    let r = a.rebalance_to(&sixty_forty(), AssetLocation::TaxEfficient);
    assert_eq!(r.long_term_gain, 0.0);
    assert_close(a.taxable.value[US], 100.0);
    assert_close(a.taxable.basis[US], 50.0);
    // Bonds fill the traditional account first; the Roth takes the remaining 20.
    assert_close(a.person[0].traditional[BONDS], 100.0);
    assert_close(a.person[0].traditional[US], 0.0);
    assert_close(a.person[0].roth.value[BONDS], 20.0);
    assert_close(a.person[0].roth.value[US], 80.0);
    assert_close(r.turnover, 60.0 + 20.0);
    let totals = a.asset_totals();
    assert_close(totals[US], 180.0);
    assert_close(totals[BONDS], 120.0);

    // Mirror reaches the same household mix but has to sell 40 of taxable stock.
    let r = mirror.rebalance_to(&sixty_forty(), AssetLocation::Mirror);
    assert_close(r.long_term_gain, 20.0);
    let m = mirror.asset_totals();
    assert_close(m[US], totals[US]);
    assert_close(m[BONDS], totals[BONDS]);
}

#[test]
fn tax_efficient_rebalance_sells_taxable_only_when_forced() {
    // 300 taxable stock (basis 150) and 100 traditional bonds; target 50/50 of 400.
    // The sheltered account cannot absorb the excess, so 100 of stock must be sold.
    let mut a = Accounts::default();
    a.taxable.value[US] = 300.0;
    a.taxable.basis[US] = 150.0;
    a.contribute(Bucket::Traditional(1), 100.0, &only(AssetClass::Bonds));
    let half = weights(&[(AssetClass::UsStocks, 1.0), (AssetClass::Bonds, 1.0)]);
    let r = a.rebalance_to(&half, AssetLocation::TaxEfficient);
    assert_close(r.long_term_gain, 50.0);
    assert_close(r.turnover, 100.0);
    assert_close(a.taxable.value[US], 200.0);
    assert_close(a.taxable.value[BONDS], 100.0);
    assert_close(a.taxable.basis[US], 100.0);
    assert_close(a.taxable.basis[BONDS], 100.0);
    assert_close(a.person[1].traditional[BONDS], 100.0);

    // Bonds held at a loss in taxable are moved out for free (and the loss is
    // realised) when a sheltered account can hold them instead.
    let mut b = Accounts::default();
    b.taxable.value[BONDS] = 100.0;
    b.taxable.basis[BONDS] = 110.0;
    b.contribute(Bucket::Traditional(0), 100.0, &only(AssetClass::UsStocks));
    let r = b.rebalance_to(&half, AssetLocation::TaxEfficient);
    assert_close(r.long_term_gain, -10.0);
    assert_close(b.taxable.value[US], 100.0);
    assert_close(b.person[0].traditional[BONDS], 100.0);
}

#[test]
fn tax_efficient_and_mirror_hold_the_same_household_mix() {
    let mut rng = Rng::from_seed(11);
    for _ in 0..200 {
        let mut a = Accounts::default();
        let buckets = [
            Bucket::Taxable,
            Bucket::Traditional(0),
            Bucket::Traditional(1),
            Bucket::Roth(0),
            Bucket::Roth(1),
            Bucket::Hsa(0),
            Bucket::Hsa(1),
        ];
        for b in buckets {
            if rng.bernoulli(0.7) {
                let mix = random_weights(&mut rng);
                a.contribute(b, rng.uniform_range(1.0, 1e6), &mix);
            }
        }
        for b in &mut a.taxable.basis {
            *b *= rng.uniform_range(0.2, 1.3);
        }
        let target = random_weights(&mut rng);
        let total = sum(&a.asset_totals());
        let mut m = a;
        let mut t = a;
        let rm = m.rebalance_to(&target, AssetLocation::Mirror);
        let rt = t.rebalance_to(&target, AssetLocation::TaxEfficient);
        let (mt, tt) = (m.asset_totals(), t.asset_totals());
        for i in 0..N_ASSETS {
            assert!((mt[i] - target[i] * total).abs() < 1e-6 * (1.0 + total));
            assert!((tt[i] - target[i] * total).abs() < 1e-6 * (1.0 + total));
        }
        // Every bucket keeps its own total: rebalancing moves nothing between them.
        for b in buckets {
            assert_close(m.total(b), a.total(b));
            assert!((t.total(b) - a.total(b)).abs() < 1e-6 * (1.0 + a.total(b)));
        }
        assert!(m.max_drift(&target) < 1e-9 && t.max_drift(&target) < 1e-9);
        // Tax-efficient never sells more in the taxable bucket than it must.
        let sold = |after: &Accounts| {
            (0..N_ASSETS)
                .map(|i| (a.taxable.value[i] - after.taxable.value[i]).max(0.0))
                .sum::<f64>()
        };
        let forced: f64 = (0..N_ASSETS)
            .map(|i| (a.taxable.value[i] - target[i] * total).max(0.0))
            .sum();
        let free: f64 = (0..N_ASSETS)
            .filter(|&i| a.taxable.basis[i] >= a.taxable.value[i])
            .map(|i| a.taxable.value[i])
            .sum();
        assert!(sold(&t) <= forced + free + 1e-6);
        assert!(rm.turnover >= 0.0 && rt.turnover >= 0.0);
    }
}

#[test]
fn inheritance_rolls_over_and_steps_up_basis() {
    let mut a = roth_example();
    a.taxable.value[US] = 500.0;
    a.taxable.basis[US] = 100.0;
    a.contribute(Bucket::Traditional(0), 1_000.0, &sixty_forty());
    a.contribute(Bucket::Hsa(0), 200.0, &sixty_forty());
    a.contribute(Bucket::Traditional(1), 300.0, &sixty_forty());
    a.contribute(Bucket::Roth(1), 50.0, &sixty_forty());
    let before = a.investable();

    a.inherit(0, Some(1));
    assert_close(a.investable(), before);
    assert_eq!(a.person[0], PersonAccounts::default());
    assert_close(a.total(Bucket::Traditional(1)), 1_300.0);
    assert_close(a.total(Bucket::Roth(1)), 100_050.0);
    assert_close(a.total(Bucket::Hsa(1)), 200.0);
    assert_close(a.person[1].roth.contribution_basis, 30_050.0);
    assert_close(a.person[1].roth.seasoned_conversions, 10_000.0);
    assert_close(a.person[1].roth.unseasoned[2], 20_000.0);
    assert_eq!(a.person[1].roth.years_open, 10);
    assert_eq!(a.taxable.basis[US], 500.0);
    assert_eq!(a.unrealized_gain(), 0.0);

    // No survivor: balances stay for the estate, basis still steps up.
    let mut b = roth_example();
    b.taxable.value[US] = 500.0;
    b.taxable.basis[US] = 100.0;
    let snapshot = b.person;
    b.inherit(0, None);
    assert_eq!(b.person, snapshot);
    assert_eq!(b.unrealized_gain(), 0.0);
    // Nonsense indices do nothing.
    let c = b;
    b.inherit(5, Some(0));
    b.inherit(0, Some(0));
    b.inherit(0, Some(9));
    assert_eq!(b, c);
}

#[test]
fn glide_paths() {
    let mut policy = PortfolioPolicy::default(); // 60/20/20: 80% growth
    let equity = |w: &Weights| w[US] + w[INTL];
    assert_close(equity(&glide_target(&policy, Some(3.0))), 0.8);

    policy.glide = GlidePath::TargetDate {
        years_before: 20.0,
        equity_at_retirement: 0.55,
        years_after: 7.0,
        equity_final: 0.35,
    };
    for (years, expected) in [
        (25.0, 0.8),
        (20.0, 0.8),
        (10.0, 0.675),
        (0.0, 0.55),
        (-3.5, 0.45),
        (-7.0, 0.35),
        (-30.0, 0.35),
    ] {
        let w = glide_target(&policy, Some(years));
        assert_close(equity(&w), expected);
        assert_close(w.iter().sum::<f64>(), 1.0);
        // Proportions inside each group follow the target (3:1 US to international).
        assert_close(w[US], 3.0 * w[INTL]);
        assert_close(w[BONDS], 1.0 - expected);
    }
    // Retirement date not known yet: the plain target.
    assert_close(equity(&glide_target(&policy, None)), 0.8);

    policy.glide = GlidePath::BondTent {
        years_before: 10.0,
        equity_at_retirement: 0.4,
        years_after: 10.0,
    };
    for (years, expected) in [
        (15.0, 0.8),
        (5.0, 0.6),
        (0.0, 0.4),
        (-5.0, 0.6),
        (-10.0, 0.8),
        (-20.0, 0.8),
    ] {
        assert_close(equity(&glide_target(&policy, Some(years))), expected);
    }

    let pt = |years_to_retirement, equity_fraction| GlidePoint {
        years_to_retirement,
        equity_fraction,
    };
    policy.glide = GlidePath::Custom {
        points: vec![pt(-5.0, 0.5), pt(10.0, 0.9), pt(0.0, 0.3)],
    };
    for (years, expected) in [
        (20.0, 0.9),
        (10.0, 0.9),
        (5.0, 0.6),
        (0.0, 0.3),
        (-2.5, 0.4),
        (-5.0, 0.5),
        (-30.0, 0.5),
    ] {
        assert_close(equity(&glide_target(&policy, Some(years))), expected);
    }
    policy.glide = GlidePath::Custom { points: vec![] };
    assert_close(equity(&glide_target(&policy, Some(1.0))), 0.8);

    // A target with no defensive assets still glides: the bond share is created.
    policy.target = Allocation::single(AssetClass::UsStocks);
    policy.glide = GlidePath::BondTent {
        years_before: 10.0,
        equity_at_retirement: 0.4,
        years_after: 10.0,
    };
    let w = glide_target(&policy, Some(0.0));
    assert_close(w[US], 0.4);
    assert_close(w[BONDS], 0.6);
    // And the reverse: an all-bond target given an equity share buys US stocks.
    policy.target = Allocation::single(AssetClass::Bonds);
    policy.glide = GlidePath::Custom {
        points: vec![pt(0.0, 0.25)],
    };
    let w = glide_target(&policy, Some(4.0));
    assert_close(w[US], 0.25);
    assert_close(w[BONDS], 0.75);
}

fn saver(age: f64) -> PersonSavings {
    PersonSavings {
        present: true,
        age,
        hsa_eligible: true,
        traditional_deductible: false,
        after_tax_401k: false,
    }
}

fn savings_ctx(people: [PersonSavings; MAX_PEOPLE]) -> SavingsContext {
    SavingsContext {
        limits: limits_2026(),
        filing_status: FilingStatus::Single,
        magi: 100_000.0,
        earned_income: 100_000.0,
        marginal_rate: 0.22,
        retirement_rate: 0.12,
        hsa_family: false,
        person: people,
    }
}

#[test]
fn savings_waterfall_fills_hsa_then_ira_then_taxable() {
    let target = sixty_forty();
    let c = savings_ctx([saver(40.0), PersonSavings::default()]);
    let mut a = Accounts::default();
    let f = a.save_surplus(20_000.0, &c, &target);
    assert_close(f.hsa[0], 4_400.0);
    assert_close(f.roth_ira[0], 7_500.0);
    assert_eq!(f.traditional_ira[0], 0.0);
    assert_close(f.taxable, 8_100.0);
    assert_close(f.total(), 20_000.0);
    assert_close(a.investable(), 20_000.0);
    assert_close(a.person[0].roth.contribution_basis, 7_500.0);
    assert_close(sum(&a.taxable.basis), 8_100.0);
    // New money is bought at the target mix.
    assert!(a.max_drift(&target) < 1e-12);

    // Limits are annual: the next month's surplus all goes to taxable.
    let f = a.save_surplus(5_000.0, &c, &target);
    assert_close(f.taxable, 5_000.0);
    assert_eq!(f.hsa[0] + f.roth_ira[0], 0.0);

    // A new year reopens the room.
    a.roll_year();
    let f = a.save_surplus(1_000.0, &c, &target);
    assert_close(f.hsa[0], 1_000.0);
    assert_eq!(a.save_surplus(0.0, &c, &target), SavingsFlows::default());
    assert_eq!(a.save_surplus(-10.0, &c, &target), SavingsFlows::default());
}

#[test]
fn ira_choice_follows_the_documented_income_rule() {
    let target = sixty_forty();
    // Deductible and today's rate (22%) above the retirement rate (12%): traditional.
    let mut who = saver(52.0);
    who.hsa_eligible = false;
    who.traditional_deductible = true;
    let c = savings_ctx([who, PersonSavings::default()]);
    let mut a = Accounts::default();
    let f = a.save_surplus(10_000.0, &c, &target);
    assert_close(f.traditional_ira[0], 8_600.0); // 7,500 + 1,100 catch-up
    assert_eq!(f.roth_ira[0], 0.0);
    assert_close(f.taxable, 1_400.0);
    assert_eq!(a.person[0].roth.contribution_basis, 0.0);

    // Same person in a low bracket: Roth.
    let mut low = c;
    low.marginal_rate = 0.12;
    let f = Accounts::default().save_surplus(10_000.0, &low, &target);
    assert_close(f.roth_ira[0], 8_600.0);

    // Married, MAGI halfway through the Roth phase-out, not deductible: half the
    // limit goes to Roth and the rest of the surplus to taxable.
    let mut mid = savings_ctx([saver(40.0), PersonSavings::default()]);
    mid.person[0].hsa_eligible = false;
    mid.filing_status = FilingStatus::MarriedJoint;
    mid.magi = 247_000.0;
    let f = Accounts::default().save_surplus(10_000.0, &mid, &target);
    assert_close(f.roth_ira[0], 3_750.0);
    assert_close(f.taxable, 6_250.0);
    // Deductible: the room the phase-out closes goes to a traditional IRA instead.
    mid.person[0].traditional_deductible = true;
    mid.marginal_rate = 0.10;
    let f = Accounts::default().save_surplus(10_000.0, &mid, &target);
    assert_close(f.roth_ira[0], 3_750.0);
    assert_close(f.traditional_ira[0], 3_750.0);
    // Over the phase-out: no Roth IRA.
    mid.person[0].traditional_deductible = false;
    mid.magi = 300_000.0;
    let f = Accounts::default().save_surplus(10_000.0, &mid, &target);
    assert_close(f.taxable, 10_000.0);

    // IRA contributions cannot exceed earned income, across both spouses.
    let mut couple = savings_ctx([saver(40.0), saver(40.0)]);
    couple.person[0].hsa_eligible = false;
    couple.person[1].hsa_eligible = false;
    couple.earned_income = 9_000.0;
    let f = Accounts::default().save_surplus(50_000.0, &couple, &target);
    assert_close(f.roth_ira[0], 7_500.0);
    assert_close(f.roth_ira[1], 1_500.0);
    assert_close(f.taxable, 41_000.0);
}

#[test]
fn family_hsa_limit_is_shared_and_mega_backdoor_fills_the_plan() {
    let target = sixty_forty();
    let mut c = savings_ctx([saver(56.0), saver(50.0)]);
    c.hsa_family = true;
    c.earned_income = 0.0;
    c.person[1].after_tax_401k = true;
    let mut a = Accounts::default();
    a.ytd.k401_additions[1] = 30_000.0;
    let f = a.save_surplus(100_000.0, &c, &target);
    // Person 0: family limit plus their own catch-up; nothing is left for person 1.
    assert_close(f.hsa[0], 9_750.0);
    assert_eq!(f.hsa[1], 0.0);
    assert_eq!(f.roth_ira, [0.0, 0.0]);
    // 72,000 + 8,000 catch-up at 50, less 30,000 already added this year.
    assert_close(f.after_tax_401k[1], 50_000.0);
    assert_close(f.taxable, 100_000.0 - 9_750.0 - 50_000.0);
    assert_close(a.person[1].roth.contribution_basis, 50_000.0);
    assert_close(f.total(), 100_000.0);

    // Payroll HSA money counts against the same limit.
    let mut b = Accounts::default();
    let l = limits_2026();
    let room = b.hsa_room(0, 40.0, 0.0, false, &l);
    assert_close(room, 4_400.0);
    assert_close(b.contribute_hsa(0, 3_000.0, room, &target), 3_000.0);
    let room = b.hsa_room(0, 40.0, 0.0, false, &l);
    assert_close(b.contribute_hsa(0, 3_000.0, room, &target), 1_400.0);
    assert_eq!(b.hsa_room(0, 40.0, 0.0, false, &l), 0.0);
    assert_close(b.total(Bucket::Hsa(0)), 4_400.0);
}

#[test]
fn payroll_deferral_match_and_limits() {
    let target = sixty_forty();
    let l = limits_2026();
    let plan = EmployerPlan::default(); // 6% deferral, 50% match on the first 6%
    let pay = |gross_pay, month, age| PayPeriod {
        gross_pay,
        month,
        age,
    };
    let mut a = Accounts::default();
    let d = a.payroll_deferral(0, &plan, &pay(10_000.0, 1, 40.0), &l, &target);
    assert_close(d.employee_pretax, 600.0);
    assert_eq!(d.employee_roth, 0.0);
    assert_close(d.employer, 300.0);
    assert_close(a.total(Bucket::Traditional(0)), 900.0);
    assert_close(a.ytd.k401_employee[0], 600.0);
    assert_close(a.ytd.k401_additions[0], 900.0);

    // Half Roth; the match is on the first 6% only and always pre-tax.
    let roth = EmployerPlan {
        employee_fraction: 0.10,
        roth_fraction: 0.5,
        match_rate: 1.0,
        match_up_to: 0.04,
        ..EmployerPlan::default()
    };
    let mut a = Accounts::default();
    let d = a.payroll_deferral(1, &roth, &pay(10_000.0, 3, 40.0), &l, &target);
    assert_close(d.employee_pretax, 500.0);
    assert_close(d.employee_roth, 500.0);
    assert_close(d.employer, 400.0);
    assert_close(a.total(Bucket::Traditional(1)), 900.0);
    assert_close(a.total(Bucket::Roth(1)), 500.0);
    assert_close(a.person[1].roth.contribution_basis, 500.0);

    // 50% of 10,000 a month hits the 24,500 limit in May; the match stops with it.
    let heavy = EmployerPlan {
        employee_fraction: 0.5,
        ..EmployerPlan::default()
    };
    let mut a = Accounts::default();
    let mut deferred = [0.0; 12];
    for (m, slot) in deferred.iter_mut().enumerate() {
        let d = a.payroll_deferral(0, &heavy, &pay(10_000.0, m as u32 + 1, 40.0), &l, &target);
        *slot = d.employee_pretax;
        assert_close(d.employer, if m < 5 { 300.0 } else { 0.0 });
    }
    assert_close(deferred[3], 5_000.0);
    assert_close(deferred[4], 4_500.0);
    assert_eq!(deferred[5], 0.0);
    assert_close(a.ytd.k401_employee[0], 24_500.0);

    // "Max it out": the limit (with the catch-up at 50) spread over the year.
    let max = EmployerPlan {
        employee_max: true,
        match_rate: 0.0,
        ..EmployerPlan::default()
    };
    let mut a = Accounts::default();
    let mut total = 0.0;
    for m in 1..=12 {
        let d = a.payroll_deferral(0, &max, &pay(20_000.0, m, 50.0), &l, &target);
        assert_close(d.employee_pretax, 32_500.0 / 12.0);
        total += d.employee_pretax;
    }
    assert_close(total, 32_500.0);
    // A thin pay cheque defers at most itself and the shortfall is caught up later.
    let mut a = Accounts::default();
    let d = a.payroll_deferral(0, &max, &pay(1_000.0, 1, 40.0), &l, &target);
    assert_close(d.employee_pretax, 1_000.0);
    let d = a.payroll_deferral(0, &max, &pay(20_000.0, 2, 40.0), &l, &target);
    assert_close(d.employee_pretax, 23_500.0 / 11.0);

    // The overall additions limit caps the employer side.
    let mut a = Accounts::default();
    a.ytd.k401_additions[0] = 71_200.0;
    let d = a.payroll_deferral(0, &plan, &pay(10_000.0, 11, 40.0), &l, &target);
    assert_close(d.employee_pretax, 600.0);
    assert_close(d.employer, 200.0);
    let d = a.payroll_deferral(0, &plan, &pay(10_000.0, 12, 40.0), &l, &target);
    assert_eq!((d.employee_pretax, d.employer), (0.0, 0.0));
    assert_eq!(
        a.payroll_deferral(0, &plan, &pay(0.0, 1, 40.0), &l, &target),
        PayrollDeferral::default()
    );
    assert_eq!(
        a.payroll_deferral(4, &plan, &pay(100.0, 1, 40.0), &l, &target),
        PayrollDeferral::default()
    );
}

#[test]
fn harvesting_gains_raises_basis_only() {
    let mut a = Accounts::default();
    a.taxable.value[US] = 300.0;
    a.taxable.basis[US] = 100.0; // gain 200
    a.taxable.value[BONDS] = 100.0;
    a.taxable.basis[BONDS] = 120.0; // loss, untouched
    a.taxable.value[INTL] = 150.0;
    a.taxable.basis[INTL] = 50.0; // gain 100
    let value = a.taxable.value;
    assert_close(a.harvest_gains(150.0), 150.0);
    assert_eq!(a.taxable.value, value);
    assert_close(a.taxable.basis[US], 200.0);
    assert_close(a.taxable.basis[INTL], 100.0);
    assert_close(a.taxable.basis[BONDS], 120.0);
    assert_close(a.harvest_gains(1e9), 150.0);
    assert_eq!(a.harvest_gains(10.0), 0.0);
}

fn random_weights(rng: &mut Rng) -> Weights {
    let mut w = [0.0; N_ASSETS];
    for x in &mut w {
        if rng.bernoulli(0.5) {
            *x = rng.uniform_range(0.05, 1.0);
        }
    }
    if w.iter().sum::<f64>() <= 0.0 {
        w[US] = 1.0;
    }
    normalized(&w)
}

fn random_bucket(rng: &mut Rng) -> Bucket {
    match rng.below(8) {
        0 => Bucket::Cash,
        1 | 2 => Bucket::Taxable,
        3 => Bucket::Traditional(rng.below(2)),
        4 => Bucket::Roth(rng.below(2)),
        5 => Bucket::Hsa(rng.below(2)),
        6 => Bucket::Traditional(0),
        _ => Bucket::Roth(1),
    }
}

/// Money is conserved: over any sequence of operations the total changes by exactly
/// the flows each method reports, and no balance goes negative or non-finite.
#[test]
fn conservation_over_random_operation_sequences() {
    let limits = limits_2026();
    for seed in 0..40 {
        let mut rng = Rng::from_seed(1_000 + seed);
        let mut a = Accounts::default();
        let mut expected = 0.0_f64;
        let mut scale = 1.0_f64;
        for step in 0..500 {
            let target = random_weights(&mut rng);
            match rng.below(13) {
                0 | 1 => {
                    let amount = rng.uniform_range(0.0, 50_000.0);
                    a.contribute(random_bucket(&mut rng), amount, &target);
                    expected += amount;
                }
                2..=4 => {
                    let bucket = random_bucket(&mut rng);
                    let amount = a.total(bucket) * rng.uniform_range(0.0, 1.4) + 1.0;
                    let c = WithdrawContext {
                        owner_age: rng.uniform_range(30.0, 90.0),
                        qualified_medical: rng.uniform_range(0.0, 5_000.0),
                        penalty_exempt: rng.bernoulli(0.2),
                        prefer_overweight: rng.bernoulli(0.5).then_some(target),
                    };
                    let had = a.total(bucket);
                    let w = a.withdraw(bucket, amount, &c);
                    assert!(w.amount >= 0.0 && w.amount <= amount.min(had) + 1e-9);
                    assert!(w.ordinary >= 0.0 && w.ordinary <= w.amount + 1e-9);
                    assert!(w.penalty_base >= 0.0 && w.penalty_base <= w.amount + 1e-9);
                    assert!(w.long_term_gain <= w.amount + 1e-9);
                    if !matches!(bucket, Bucket::Taxable) {
                        assert_eq!(w.long_term_gain, 0.0);
                    }
                    assert!((a.total(bucket) - (had - w.amount)).abs() < 1e-6 * (1.0 + had));
                    expected -= w.amount;
                }
                5 => {
                    let p = rng.below(2);
                    let had = a.total(Bucket::Traditional(p)) + a.total(Bucket::Roth(p));
                    let amount = rng.uniform_range(0.0, 80_000.0);
                    let moved = a.convert_to_roth(p, amount);
                    assert!(moved <= amount);
                    let has = a.total(Bucket::Traditional(p)) + a.total(Bucket::Roth(p));
                    assert!((has - had).abs() < 1e-6 * (1.0 + had));
                }
                6 | 7 => {
                    let mut r = [0.0; N_ASSETS];
                    let mut y = [0.0; N_ASSETS];
                    for i in 0..N_ASSETS {
                        r[i] = rng.uniform_range(-0.25, 0.3);
                        y[i] = rng.uniform_range(0.0, 0.06);
                    }
                    if rng.bernoulli(0.05) {
                        r[rng.below(N_ASSETS)] = -1.2;
                    }
                    let inc = a.apply_returns(&r, rng.uniform_range(0.0, 0.01), 0.0005, &y);
                    assert!(inc.fees >= 0.0 && inc.interest >= 0.0);
                    assert!(inc.qualified_dividends >= 0.0 && inc.ordinary_dividends >= 0.0);
                    expected += inc.net_change;
                }
                8 => {
                    let location = if rng.bernoulli(0.5) {
                        AssetLocation::Mirror
                    } else {
                        AssetLocation::TaxEfficient
                    };
                    let unrealized = a.unrealized_gain();
                    let r = a.rebalance_to(&target, location);
                    assert!(r.turnover >= 0.0);
                    // Realising a gain moves it from unrealised to realised, no more.
                    assert!(
                        (a.unrealized_gain() + r.long_term_gain - unrealized).abs() < 1e-6 * scale
                    );
                    assert!(a.max_drift(&target) < 1e-6);
                }
                9 => a.roll_year(),
                10 => {
                    let surplus = rng.uniform_range(0.0, 40_000.0);
                    let mut c = savings_ctx([saver(45.0), saver(56.0)]);
                    c.hsa_family = rng.bernoulli(0.5);
                    c.magi = rng.uniform_range(50_000.0, 300_000.0);
                    c.person[1].after_tax_401k = rng.bernoulli(0.5);
                    c.person[0].traditional_deductible = rng.bernoulli(0.5);
                    let f = a.save_surplus(surplus, &c, &target);
                    assert!((f.total() - surplus).abs() < 1e-6);
                    expected += f.total();
                }
                11 => {
                    let plan = EmployerPlan {
                        employee_max: rng.bernoulli(0.3),
                        roth_fraction: rng.uniform(),
                        ..EmployerPlan::default()
                    };
                    let pay = PayPeriod {
                        gross_pay: rng.uniform_range(0.0, 30_000.0),
                        month: 1 + rng.below(12) as u32,
                        age: 51.0,
                    };
                    let p = rng.below(2);
                    let d = a.payroll_deferral(p, &plan, &pay, &limits, &target);
                    assert!(a.ytd.k401_employee[p] <= 32_500.0 + 1e-6);
                    assert!(a.ytd.k401_additions[p] <= 80_000.0 + 1e-6);
                    expected += d.employee_pretax + d.employee_roth + d.employer;
                }
                _ => {
                    if rng.bernoulli(0.1) {
                        let d = rng.below(2);
                        a.inherit(d, rng.bernoulli(0.8).then_some(1 - d));
                    } else {
                        let before = a.unrealized_gain();
                        let g = a.harvest_gains(rng.uniform_range(0.0, 20_000.0));
                        assert!((before - a.unrealized_gain() - g).abs() < 1e-6 * scale);
                    }
                }
            }
            scale = scale.max(expected.abs()).max(a.investable());
            let total = a.investable();
            assert!(
                (total - expected).abs() <= 1e-9 * scale * (step as f64 + 1.0),
                "seed {seed} step {step}: total {total} expected {expected}"
            );
            assert!(a.cash >= 0.0 && a.cash.is_finite());
            for i in 0..N_ASSETS {
                assert!(a.taxable.value[i] >= 0.0 && a.taxable.value[i].is_finite());
                assert!(a.taxable.basis[i] >= -1e-6 && a.taxable.basis[i].is_finite());
                for p in &a.person {
                    for h in [&p.traditional, &p.roth.value, &p.hsa] {
                        assert!(h[i] >= 0.0 && h[i].is_finite());
                    }
                }
            }
            for p in &a.person {
                assert!(p.roth.contribution_basis >= 0.0 && p.roth.seasoned_conversions >= 0.0);
                assert!(p.roth.unseasoned.iter().all(|u| *u >= 0.0));
            }
        }
    }
}

#[test]
fn identical_operations_give_identical_bits() {
    let run = || {
        let mut rng = Rng::from_seed(7);
        let mut a = Accounts::default();
        for _ in 0..200 {
            let t = random_weights(&mut rng);
            a.contribute(random_bucket(&mut rng), rng.uniform_range(0.0, 1e4), &t);
            let mut r = [0.0; N_ASSETS];
            for x in &mut r {
                *x = rng.uniform_range(-0.1, 0.1);
            }
            a.apply_returns(&r, 0.003, 0.0001, &[0.02; N_ASSETS]);
            a.withdraw(random_bucket(&mut rng), 500.0, &ctx(50.0));
            a.rebalance_to(&t, AssetLocation::TaxEfficient);
        }
        a
    };
    let (x, y) = (run(), run());
    assert_eq!(x.investable().to_bits(), y.investable().to_bits());
    assert_eq!(x, y);
}
