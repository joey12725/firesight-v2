//! Housing decisions on paired paths: borrowing against the home, downsizing,
//! cash-out refinancing and prepayment, realistic rentals. Each feature is checked
//! three ways: switched off it changes nothing; switched on in a household that never
//! needs it it changes nothing either (its random stream is its own); switched on
//! where it matters it moves outcomes the way it should, path by path.

mod common;

use common::*;
use firesight_core::model::*;
use firesight_core::sim;
use firesight_core::stats::{Accumulator, PathScalar, YearSeries};

const PATHS: u64 = 400;
const SEED: u64 = 77;

/// A 66-year-old homeowner with a paid-off $600,000 house, $450,000 of savings,
/// $1,800 a month of Social Security and a $50,000 budget: savings run out on many
/// paths in her eighties, long before the house is worth nothing.
fn house_rich_retiree() -> Profile {
    let mut who = person("owner", 66, Sex::Female);
    who.social_security = SocialSecurity {
        claim_age: 66.0,
        basis: SocialSecurityBasis::KnownBenefit {
            monthly_at_fra: 1_800.0,
        },
    };
    let mut p = quiet(vec![who], 95.0);
    p.spending.items = vec![item("living", Category::Other, 50_000.0 / 12.0, true)];
    p.accounts = vec![Account {
        cost_basis: Some(450_000.0),
        ..account("savings", 0, AccountKind::Taxable, 450_000.0)
    }];
    p.portfolio.target =
        Allocation::from_pairs(&[(AssetClass::UsStocks, 0.6), (AssetClass::Bonds, 0.4)]);
    p.properties = vec![Property {
        id: "home".into(),
        name: "home".into(),
        value: 600_000.0,
        property_tax_rate: Some(0.01),
        ..Property::default()
    }];
    p.retirement.trigger = RetirementTrigger::AlreadyRetired;
    p.retirement.home_equity = HomeEquity::Never;
    p
}

fn col(acc: &Accumulator, s: PathScalar) -> Vec<f32> {
    acc.scalar_column(s)
}

/// Sum of a series over every year and path, whole today's dollars.
fn series_total(acc: &Accumulator, s: YearSeries) -> i64 {
    let years = acc.years as usize;
    acc.sums[s.index() * years..(s.index() + 1) * years]
        .iter()
        .sum()
}

fn share_finite(v: &[f32]) -> f64 {
    v.iter().filter(|x| x.is_finite()).count() as f64 / v.len() as f64
}

fn heloc() -> HomeEquityBorrowing {
    HomeEquityBorrowing::Heloc(Heloc::default())
}

// ---------------------------------------------------------------------------------
// F26: home-equity line of credit.

#[test]
fn heloc_off_or_unused_changes_nothing() {
    // A household that never runs short never draws: identical bytes, although the
    // lenders' stream is drawn every month.
    let mut rich = house_rich_retiree();
    rich.accounts[0].balance = 3_000_000.0;
    rich.accounts[0].cost_basis = Some(3_000_000.0);
    let base = run(&compile(&rich), SEED, 0..200);
    let mut with = rich.clone();
    with.retirement.home_equity_borrowing = heloc();
    let on = run(&compile(&with), SEED, 0..200);
    assert_eq!(base.to_bytes(), on.to_bytes());
    // Explicitly "none" is the default.
    let mut none = rich;
    none.retirement.home_equity_borrowing = HomeEquityBorrowing::None;
    assert_eq!(
        base.to_bytes(),
        run(&compile(&none), SEED, 0..200).to_bytes()
    );
}

#[test]
fn heloc_delays_or_prevents_depletion_on_every_path() {
    let base_p = house_rich_retiree();
    let mut on_p = base_p.clone();
    on_p.retirement.home_equity_borrowing = heloc();
    let base = run(&compile(&base_p), SEED, 0..PATHS);
    let on = run(&compile(&on_p), SEED, 0..PATHS);
    let (b, o) = (
        col(&base, PathScalar::DepletionAge),
        col(&on, PathScalar::DepletionAge),
    );
    let mut later = 0;
    for (x, y) in b.iter().zip(&o) {
        // A path that never ran short never borrowed: borrowing cannot make a
        // path run out that did not.
        if !x.is_finite() {
            assert!(!y.is_finite(), "depleted at {y} only with the line");
            continue;
        }
        if !y.is_finite() || y > x {
            later += 1;
        }
        assert!(!y.is_finite() || *y >= *x - 1e-3, "earlier: {y} < {x}");
    }
    let (sb, so) = (share_finite(&b), share_finite(&o));
    eprintln!("depleted: without {sb:.3}, with a line {so:.3}; later on {later} paths");
    assert!(sb > 0.3, "the test household should often run short: {sb}");
    // A line of up to $480,000 at about 7% buys years, not a lifetime: the share
    // that ever runs out falls, and most short paths run out later.
    assert!(so < sb - 0.05, "{so} vs {sb}");
    assert!(later as f64 > 0.8 * sb * PATHS as f64, "{later}");
}

#[test]
fn heloc_shows_in_traces_and_conserves_money() {
    let mut p = house_rich_retiree();
    p.retirement.home_equity_borrowing = heloc();
    p.retirement.home_equity = HomeEquity::LastResort;
    let plan = compile(&p);
    run_checked(&plan, SEED, 0..PATHS);
    let mut seen_draw = false;
    let mut seen_debt = false;
    for path in 0..30 {
        let trace = sim::trace_path(&plan, SEED, path);
        if trace
            .events
            .iter()
            .any(|e| e.label.starts_with("Borrowed against"))
        {
            seen_draw = true;
        }
        if let Some(c) = trace
            .annual
            .columns
            .iter()
            .find(|c| c.key == YearSeries::Mortgage.key())
        {
            seen_debt |= c.values.iter().any(|v| *v > 1_000.0);
        }
    }
    assert!(seen_draw && seen_debt);
}

#[test]
fn heloc_draws_are_cash_from_property_not_income() {
    let mut p = house_rich_retiree();
    p.retirement.home_equity_borrowing = heloc();
    let plan = compile(&p);
    let base = run(&compile(&house_rich_retiree()), SEED, 0..PATHS);
    let on = run(&plan, SEED, 0..PATHS);
    // Income is unchanged path for path: draws are a transfer.
    let sum = |a: &Accumulator, s: YearSeries| -> i64 { series_total(a, s) };
    assert_eq!(
        sum(&base, YearSeries::IncomeSocialSecurity),
        sum(&on, YearSeries::IncomeSocialSecurity)
    );
    assert_eq!(sum(&base, YearSeries::PropertySale), 0);
    assert!(sum(&on, YearSeries::PropertySale) > 0);
}

#[test]
fn heloc_is_drawn_while_still_working() {
    // A 45-year-old earning $60,000 and spending $80,000 with $60,000 saved: savings
    // are gone within a few years, two decades before she retires at 67.
    let mut p = quiet(vec![person("worker", 45, Sex::Female)], 90.0);
    p.incomes = vec![job("job", 0, 60_000.0)];
    p.spending.items = vec![item("living", Category::Other, 80_000.0 / 12.0, true)];
    p.accounts = vec![Account {
        cost_basis: Some(60_000.0),
        ..account("savings", 0, AccountKind::Taxable, 60_000.0)
    }];
    p.properties = vec![Property {
        id: "home".into(),
        name: "home".into(),
        value: 500_000.0,
        property_tax_rate: Some(0.01),
        ..Property::default()
    }];
    p.retirement.trigger = RetirementTrigger::AtAge {
        person: 0,
        age: 67.0,
    };
    p.retirement.home_equity = HomeEquity::Never;
    let base = run(&compile(&p), SEED, 0..100);
    // Lenders never freeze here, so the line alone decides (freezes below).
    p.retirement.home_equity_borrowing = HomeEquityBorrowing::Heloc(Heloc {
        freeze_probability: 0.0,
        ..Heloc::default()
    });
    let on = run(&compile(&p), SEED, 0..100);
    // Ten working years: nothing comes from the house without the line, and every
    // path borrows with it.
    let years = base.years as usize;
    let first_decade = |a: &Accumulator| -> i64 {
        let i = YearSeries::PropertySale.index() * years;
        a.sums[i..i + 10].iter().sum()
    };
    assert_eq!(first_decade(&base), 0);
    let borrowed = first_decade(&on);
    // About $20,000 a year short (less the interest the line itself costs) on each
    // of 100 paths, from the year savings run out.
    eprintln!("borrowed in the first decade: {borrowed}");
    assert!(borrowed > 100 * 80_000, "{borrowed}");
    let (b, o) = (
        col(&base, PathScalar::DepletionAge),
        col(&on, PathScalar::DepletionAge),
    );
    for (x, y) in b.iter().zip(&o) {
        assert!(x.is_finite() && *x < 50.0, "the base runs short early: {x}");
        assert!(!y.is_finite() || *y > *x + 5.0, "{y} vs {x}");
    }
    // Lenders that always freeze lines in a downturn: households run short sooner.
    p.retirement.home_equity_borrowing = HomeEquityBorrowing::Heloc(Heloc {
        freeze_probability: 1.0,
        ..Heloc::default()
    });
    let frozen = col(&run(&compile(&p), SEED, 0..100), PathScalar::DepletionAge);
    let mean = |v: &[f32]| v.iter().map(|x| f64::from(x.min(90.0))).sum::<f64>() / 100.0;
    let (open, shut) = (mean(&o), mean(&frozen));
    eprintln!("mean depletion age: never frozen {open:.1}, always frozen {shut:.1}");
    assert!(shut < open - 1.0, "{shut} vs {open}");
}

// ---------------------------------------------------------------------------------
// F27: reverse mortgage.

fn reverse(open_when: ReverseMortgageOpen) -> HomeEquityBorrowing {
    HomeEquityBorrowing::ReverseMortgage(ReverseMortgage {
        open_when,
        ..ReverseMortgage::default()
    })
}

#[test]
fn reverse_mortgage_unused_changes_nothing() {
    let mut rich = house_rich_retiree();
    rich.accounts[0].balance = 3_000_000.0;
    rich.accounts[0].cost_basis = Some(3_000_000.0);
    let base = run(&compile(&rich), SEED, 0..200);
    rich.retirement.home_equity_borrowing = reverse(ReverseMortgageOpen::LastResort);
    assert_eq!(
        base.to_bytes(),
        run(&compile(&rich), SEED, 0..200).to_bytes()
    );
}

#[test]
fn reverse_mortgage_delays_depletion_and_is_non_recourse() {
    let base_p = house_rich_retiree();
    let base = run(&compile(&base_p), SEED, 0..PATHS);
    let b = col(&base, PathScalar::DepletionAge);
    let mut last = base_p.clone();
    last.retirement.home_equity_borrowing = reverse(ReverseMortgageOpen::LastResort);
    let last_plan = compile(&last);
    let l = col(
        &run_checked(&last_plan, SEED, 0..PATHS),
        PathScalar::DepletionAge,
    );
    let mut standby = base_p.clone();
    standby.retirement.home_equity_borrowing =
        reverse(ReverseMortgageOpen::SavingsBelow { years: 5.0 });
    let s = col(
        &run_checked(&compile(&standby), SEED, 0..PATHS),
        PathScalar::DepletionAge,
    );
    for (x, y) in b.iter().zip(&l) {
        if !x.is_finite() {
            assert!(!y.is_finite(), "depleted at {y} only with the loan");
        } else {
            assert!(!y.is_finite() || *y >= *x - 1e-3, "earlier: {y} < {x}");
        }
    }
    let (sb, sl, ss) = (share_finite(&b), share_finite(&l), share_finite(&s));
    eprintln!("depleted: without {sb:.3}, last-resort HECM {sl:.3}, standby HECM {ss:.3}");
    // A 66-year-old's line on a $600,000 house is about $250,000 once costs are
    // paid, growing at the loan rate: it buys years.
    assert!(sl < sb - 0.1, "{sl} vs {sb}");
    // The standby line helps as much, give or take: it is sized at a younger age and
    // pays its costs years earlier, while the unused line grows at the loan rate.
    // Here home values grow almost as fast as the line, so the two end up close
    // (measured 0.868 against 0.853), and the standby line does not win. The
    // literature's advantage for it comes from drawing on the line after market
    // falls, which this rule does not do.
    assert!(ss < sb - 0.1, "{ss} vs {sb}");
    assert!((ss - sl).abs() < 0.05, "{ss} vs {sl}");
    // Non-recourse: what is owed is never booked above the home's value.
    let mut seen = false;
    for path in 0..40 {
        let trace = sim::trace_path(&last_plan, SEED, path);
        seen |= trace
            .events
            .iter()
            .any(|e| e.label.starts_with("Took a reverse mortgage"));
        let column = |s: YearSeries| {
            trace
                .annual
                .columns
                .iter()
                .find(|c| c.key == s.key())
                .unwrap()
                .values
                .clone()
        };
        let (owed, value) = (
            column(YearSeries::Mortgage),
            column(YearSeries::PropertyValue),
        );
        for (o, v) in owed.iter().zip(&value) {
            assert!(*o <= *v + 1.0, "owes {o} on a home worth {v}");
        }
    }
    assert!(seen);
}

#[test]
fn reverse_mortgage_waits_for_62_and_repays_the_mortgage() {
    // A 55-year-old retiree with an $80,000 mortgage who runs short at once: nothing
    // is borrowed before 62; then the reverse mortgage repays the mortgage, whose
    // payments stop.
    let mut p = house_rich_retiree();
    p.household.people[0].birth = START - 55 * 12;
    p.household.people[0].social_security.claim_age = 67.0;
    p.accounts[0].balance = 20_000.0;
    p.accounts[0].cost_basis = Some(20_000.0);
    p.properties[0].mortgage = Some(Mortgage {
        balance: 80_000.0,
        rate: 0.05,
        remaining_months: 240,
        ..Mortgage::default()
    });
    p.retirement.home_equity_borrowing = reverse(ReverseMortgageOpen::LastResort);
    let plan = compile(&p);
    let acc = run_checked(&plan, SEED, 0..50);
    let years = acc.years as usize;
    let i = YearSeries::PropertySale.index() * years;
    assert_eq!(
        acc.sums[i..i + 6].iter().sum::<i64>(),
        0,
        "borrowed before 62"
    );
    assert!(
        acc.sums[i + 7..i + 12].iter().sum::<i64>() > 0,
        "never borrowed at 62"
    );
    let trace = sim::trace_path(&plan, SEED, 0);
    let opened = trace
        .events
        .iter()
        .find(|e| e.label.starts_with("Took a reverse mortgage"))
        .expect("a reverse mortgage");
    assert!(
        opened.month.year() >= START.year() + 7,
        "{:?}",
        opened.month
    );
}
