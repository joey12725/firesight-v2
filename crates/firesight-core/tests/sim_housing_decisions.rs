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

// ---------------------------------------------------------------------------------
// F28: planned downsizing.

fn downsize(when: DownsizeWhen, to_value_fraction: f64) -> Option<Downsize> {
    Some(Downsize {
        when,
        to_value_fraction,
        ..Downsize::default()
    })
}

/// Total of a series in year `y` (0 is the part-year from the start month) over all
/// paths, whole today's dollars.
fn in_year(acc: &Accumulator, s: YearSeries, y: usize) -> i64 {
    acc.sums[s.index() * acc.years as usize + y]
}

fn first_year(acc: &Accumulator, s: YearSeries) -> i64 {
    in_year(acc, s, 0)
}

fn labels(plan: &sim::Plan, path: u64, prefix: &str) -> Vec<YearMonth> {
    sim::trace_path(plan, SEED, path)
        .events
        .iter()
        .filter(|e| e.label.starts_with(prefix))
        .map(|e| e.month)
        .collect()
}

#[test]
fn downsize_that_never_fires_changes_nothing() {
    let base_p = house_rich_retiree();
    let base = run(&compile(&base_p), SEED, 0..200).to_bytes();
    let mut p = base_p.clone();
    // She dies at 95: an age of 110 is never reached.
    p.retirement.downsize = downsize(
        DownsizeWhen::AtAge {
            person: 0,
            age: 110.0,
        },
        0.5,
    );
    assert_eq!(base, run(&compile(&p), SEED, 0..200).to_bytes());
    // A single person is never widowed.
    p.retirement.downsize = downsize(DownsizeWhen::WhenWidowed, 0.5);
    assert_eq!(base, run(&compile(&p), SEED, 0..200).to_bytes());
}

#[test]
fn downsize_at_66_buys_a_home_half_the_value_and_frees_equity() {
    let base_p = house_rich_retiree();
    let mut p = base_p.clone();
    p.retirement.downsize = downsize(
        DownsizeWhen::AtAge {
            person: 0,
            age: 66.0,
        },
        0.5,
    );
    let plan = compile(&p);
    let on = run_checked(&plan, SEED, 0..PATHS);
    let base = run(&compile(&base_p), SEED, 0..PATHS);
    // Sold in month 0 for about $600,000 less 6%; the new home costs half the old
    // one's value, plus 3% buying costs as one-off spending.
    let n = PATHS as f64;
    let sale = first_year(&on, YearSeries::PropertySale) as f64 / n;
    let bought = first_year(&on, YearSeries::PropertyPurchase) as f64 / n;
    let one_off = (first_year(&on, YearSeries::SpendOneOff)
        - first_year(&base, YearSeries::SpendOneOff)) as f64
        / n;
    eprintln!("mean sale {sale:.0}, purchase {bought:.0}, buying costs {one_off:.0}");
    assert!((bought - 300_000.0).abs() < 15_000.0, "{bought}");
    assert!((sale - 564_000.0).abs() < 30_000.0, "{sale}");
    assert!((one_off - 0.03 * bought).abs() < 2.0, "{one_off}");
    assert_eq!(
        labels(&plan, 0, "Downsized: sold home and bought"),
        vec![START]
    );
    // The home is worth half as much from then on (the first full year), and so its
    // running costs are.
    let value = |a: &Accumulator| in_year(a, YearSeries::PropertyValue, 1) as f64;
    let ratio = value(&on) / value(&base);
    assert!((ratio - 0.5).abs() < 0.03, "{ratio}");
    let housing = in_year(&on, YearSeries::SpendHousing, 1) as f64
        / in_year(&base, YearSeries::SpendHousing, 1) as f64;
    assert!((0.45..0.6).contains(&housing), "{housing}");
    // The freed equity (about $255,000) delays or prevents depletion on every path.
    let (b, o) = (
        col(&base, PathScalar::DepletionAge),
        col(&on, PathScalar::DepletionAge),
    );
    for (x, y) in b.iter().zip(&o) {
        if !x.is_finite() {
            assert!(!y.is_finite(), "depleted at {y} only after downsizing");
        } else {
            assert!(!y.is_finite() || *y >= *x - 1e-3, "earlier: {y} < {x}");
        }
    }
    let (sb, so) = (share_finite(&b), share_finite(&o));
    eprintln!("depleted: staying {sb:.3}, downsizing at 66 {so:.3}");
    assert!(so < sb - 0.15, "{so} vs {sb}");
}

#[test]
fn downsizing_gain_is_taxed_above_the_exclusion() {
    // Bought long ago for $100,000: a gain of about $460,000 after selling costs, of
    // which $250,000 is excluded for a single owner. At long-term rates (15%, part
    // at 0%) federal tax (settled the next spring) rises by 5 to 25% of the $210,000
    // left; without
    // the exclusion it would rise by more than a third of it.
    let mut p = house_rich_retiree();
    p.assumptions.taxes = TaxModel::default();
    p.retirement.downsize = downsize(
        DownsizeWhen::AtAge {
            person: 0,
            age: 66.0,
        },
        0.5,
    );
    let paid_up = run(&compile(&p), SEED, 0..100);
    p.properties[0].basis = Some(100_000.0);
    let cheap = run(&compile(&p), SEED, 0..100);
    let tax = |a: &Accumulator| {
        (0..3)
            .map(|y| in_year(a, YearSeries::TaxFederal, y))
            .sum::<i64>()
    };
    let extra = (tax(&cheap) - tax(&paid_up)) as f64 / 100.0;
    eprintln!("extra federal tax in the year of sale: {extra:.0}");
    assert!(
        extra > 0.05 * 210_000.0 && extra < 0.25 * 210_000.0,
        "{extra}"
    );
}

#[test]
fn downsize_renting_pays_four_percent_rent() {
    let mut p = house_rich_retiree();
    p.retirement.downsize = Some(Downsize {
        when: DownsizeWhen::AtAge {
            person: 0,
            age: 66.0,
        },
        to_value_fraction: 0.5,
        rent_instead: true,
        ..Downsize::default()
    });
    let plan = compile(&p);
    let on = run_checked(&plan, SEED, 0..PATHS);
    assert_eq!(first_year(&on, YearSeries::PropertyPurchase), 0);
    assert_eq!(first_year(&on, YearSeries::PropertyValue), 0);
    assert_eq!(
        labels(&plan, 3, "Downsized: sold home and rented"),
        vec![START]
    );
    // Rent of 4% of $300,000 a year, $1,000 a month, in the budget from then on.
    let trace = sim::trace_path(&plan, SEED, 3);
    let essential = trace
        .annual
        .columns
        .iter()
        .find(|c| c.key == YearSeries::SpendEssential.key())
        .unwrap()
        .values[2];
    assert!(
        (essential - 50_000.0 - 12_000.0).abs() < 1_500.0,
        "{essential}"
    );
}

#[test]
fn downsize_triggers_fire_when_they_should() {
    // When the children leave: the youngest, 16 and without college, turns 18 in
    // two years.
    let mut p = house_rich_retiree();
    p.household.children = vec![Child {
        id: "kid".into(),
        name: "kid".into(),
        birth: START - 16 * 12,
        college: College::None,
        ..Child::default()
    }];
    p.accounts[0].balance = 3_000_000.0;
    p.accounts[0].cost_basis = Some(3_000_000.0);
    p.retirement.downsize = downsize(DownsizeWhen::WhenChildrenLeave, 0.6);
    let plan = compile(&p);
    run_checked(&plan, SEED, 0..20);
    assert_eq!(labels(&plan, 0, "Downsized"), vec![START + 24]);
    // A year after the first death: a couple of 70 and 68 who both die at 95.
    let mut couple = quiet(
        vec![person("a", 70, Sex::Male), person("b", 68, Sex::Female)],
        95.0,
    );
    couple.spending.items = vec![item("living", Category::Other, 3_000.0, true)];
    couple.accounts = vec![account("savings", 0, AccountKind::Taxable, 2_000_000.0)];
    couple.properties = p.properties.clone();
    couple.retirement.trigger = RetirementTrigger::AlreadyRetired;
    couple.retirement.downsize = downsize(DownsizeWhen::WhenWidowed, 0.6);
    let plan = compile(&couple);
    run_checked(&plan, SEED, 0..20);
    assert_eq!(labels(&plan, 1, "Downsized"), vec![START + 26 * 12]);
    // When savings fall behind: only in a January, and it delays depletion.
    let base_p = house_rich_retiree();
    let mut p = base_p.clone();
    p.retirement.downsize = downsize(DownsizeWhen::WhenFundedRatioBelow { ratio: 0.8 }, 0.5);
    let plan = compile(&p);
    let on = run_checked(&plan, SEED, 0..PATHS);
    let mut moved = 0;
    for path in 0..40 {
        for m in labels(&plan, path, "Downsized") {
            assert_eq!(m.month(), 1, "{m:?}");
            moved += 1;
        }
    }
    assert!(moved > 10, "{moved}");
    let b = share_finite(&col(
        &run(&compile(&base_p), SEED, 0..PATHS),
        PathScalar::DepletionAge,
    ));
    let o = share_finite(&col(&on, PathScalar::DepletionAge));
    eprintln!("depleted: staying {b:.3}, downsizing below 80% funded {o:.3}");
    assert!(o < b - 0.1, "{o} vs {b}");
}

#[test]
fn downsize_with_a_mortgage_finances_what_the_proceeds_do_not_cover() {
    // $400,000 still owed on the $600,000 house: the sale leaves about $164,000,
    // which after 3% closing costs on a $480,000 home (to 0.8) is a down payment of
    // about $150,000, so about $330,000 is borrowed at the market rate.
    let mut p = house_rich_retiree();
    p.properties[0].mortgage = Some(Mortgage {
        balance: 400_000.0,
        rate: 0.04,
        remaining_months: 300,
        ..Mortgage::default()
    });
    p.retirement.downsize = downsize(
        DownsizeWhen::AtAge {
            person: 0,
            age: 66.0,
        },
        0.8,
    );
    let plan = compile(&p);
    let on = run_checked(&plan, SEED, 0..PATHS);
    let n = PATHS as f64;
    let down = first_year(&on, YearSeries::PropertyPurchase) as f64 / n;
    eprintln!("mean down payment {down:.0}");
    assert!((down - 150_000.0).abs() < 25_000.0, "{down}");
    let trace = sim::trace_path(&plan, SEED, 0);
    assert!(trace.events.iter().any(|e| e
        .label
        .starts_with("Downsized: sold home and bought a smaller home with a")));
    let mortgage = trace
        .annual
        .columns
        .iter()
        .find(|c| c.key == YearSeries::Mortgage.key())
        .unwrap()
        .values[1];
    assert!((250_000.0..400_000.0).contains(&mortgage), "{mortgage}");
    // With $500,000 owed the sale leaves about $64,000: after closing costs a 10%
    // down payment, short of 20%, so she rents.
    if let Some(m) = &mut p.properties[0].mortgage {
        m.balance = 500_000.0;
    }
    let plan = compile(&p);
    run_checked(&plan, SEED, 0..20);
    assert_eq!(
        labels(&plan, 0, "Downsized: sold home and rented"),
        vec![START]
    );
}

// ---------------------------------------------------------------------------------
// F29: cash-out refinancing and rate-aware prepayment.

/// A 50-year-old who retired with $600,000 in a traditional IRA, nothing in taxable
/// accounts, and a $600,000 house with $100,000 owed at 7% (about the market rate, so
/// that refinancing it costs nothing in rate): every dollar she spends before 59 and
/// a half (beyond a 72(t) schedule) would bear the 10% penalty.
fn early_retiree() -> Profile {
    let mut p = quiet(vec![person("early", 50, Sex::Female)], 90.0);
    p.assumptions.taxes = TaxModel::default();
    p.spending.items = vec![item("living", Category::Other, 60_000.0 / 12.0, true)];
    p.accounts = vec![account("ira", 0, AccountKind::Traditional, 600_000.0)];
    p.properties = vec![Property {
        id: "home".into(),
        name: "home".into(),
        value: 600_000.0,
        property_tax_rate: Some(0.01),
        mortgage: Some(Mortgage {
            balance: 100_000.0,
            rate: 0.07,
            remaining_months: 240,
            ..Mortgage::default()
        }),
        ..Property::default()
    }];
    p.retirement.trigger = RetirementTrigger::AlreadyRetired;
    p.retirement.home_equity = HomeEquity::Never;
    p
}

fn years_total(acc: &Accumulator, s: YearSeries, years: std::ops::Range<usize>) -> i64 {
    years.map(|y| in_year(acc, s, y)).sum()
}

#[test]
fn cash_out_refinance_unused_changes_nothing() {
    // At 66 nothing is ever penalised: identical bytes.
    let base_p = house_rich_retiree();
    let base = run(&compile(&base_p), SEED, 0..200).to_bytes();
    let mut p = base_p;
    p.debt_policy.cash_out_refinance = Some(CashOutRefinance::default());
    assert_eq!(base, run(&compile(&p), SEED, 0..200).to_bytes());
}

#[test]
fn cash_out_refinance_replaces_penalised_withdrawals() {
    let base_p = early_retiree();
    let mut p = base_p.clone();
    p.debt_policy.cash_out_refinance = Some(CashOutRefinance::default());
    let plan = compile(&p);
    let base_plan = compile(&base_p);
    let paths = 200;
    let on = run_checked(&plan, SEED, 0..paths);
    let base = run(&base_plan, SEED, 0..paths);
    let pen = |a: &Accumulator| years_total(a, YearSeries::TaxPenalties, 0..10);
    let (pb, po) = (pen(&base), pen(&on));
    let one_off = |a: &Accumulator| years_total(a, YearSeries::SpendOneOff, 0..10);
    let closing = one_off(&on) - one_off(&base);
    eprintln!(
        "penalties before 59 and a half: without {pb}, with cash-out {po}; closing costs {closing}"
    );
    // It refinances only where the penalty it avoids pays for the closing costs (and
    // for the dearer and longer borrowing, not measured here): penalties fall by more
    // than the closing costs paid, and by a tenth or more.
    assert!(closing > 0);
    assert!(pb - po > closing, "{} vs {closing}", pb - po);
    assert!((po as f64) < 0.9 * pb as f64, "{po} vs {pb}");
    // A series that is zero all along is left out of a trace.
    let column = |t: &firesight_core::stats::PathTrace, s: YearSeries| {
        t.annual
            .columns
            .iter()
            .find(|c| c.key == s.key())
            .map_or_else(|| vec![0.0; 100], |c| c.values.clone())
    };
    let mut refinanced = 0;
    for path in 0..40 {
        let trace = sim::trace_path(&plan, SEED, path);
        let months: Vec<_> = trace
            .events
            .iter()
            .filter(|e| e.label.starts_with("Cash-out refinance of home"))
            .map(|e| e.month)
            .collect();
        if months.is_empty() {
            continue;
        }
        refinanced += 1;
        // At most one refinance in any 12 months.
        for w in months.windows(2) {
            assert!(w[1] - w[0] >= 12, "{months:?}");
        }
        let owed = column(&trace, YearSeries::Mortgage);
        // The first refinance's year: closing costs are 2% of the new loan plus
        // $2,000; the loan is the cash from the house plus the old balance (a little
        // under last year-end's).
        let y = (months[0].year() - trace.start.year()) as usize;
        let base_trace = sim::trace_path(&base_plan, SEED, path);
        let costs = column(&trace, YearSeries::SpendOneOff)[y]
            - column(&base_trace, YearSeries::SpendOneOff)[y];
        let sale = column(&trace, YearSeries::PropertySale)[y];
        let before = if y > 0 { owed[y - 1] } else { 100_000.0 };
        let (lo, hi) = (0.02 * sale + 2_000.0, 0.02 * (sale + before) + 2_000.0);
        assert!(
            costs > 0.99 * lo && costs < 1.01 * hi,
            "path {path}: {costs} not in {lo}..{hi}"
        );
    }
    assert!(refinanced >= 5, "{refinanced}");
}

/// A 40-year-old earning $150,000 and spending $60,000, whose $350,000 home carries
/// a $300,000 mortgage at 7%: the surplus beyond her IRA goes to the taxable account.
fn saver_with_mortgage() -> Profile {
    let mut p = quiet(vec![person("saver", 40, Sex::Female)], 90.0);
    p.incomes = vec![job("job", 0, 150_000.0)];
    p.spending.items = vec![item("living", Category::Other, 60_000.0 / 12.0, true)];
    p.accounts = vec![account("brokerage", 0, AccountKind::Taxable, 50_000.0)];
    p.properties = vec![Property {
        id: "home".into(),
        name: "home".into(),
        value: 350_000.0,
        property_tax_rate: Some(0.01),
        mortgage: Some(Mortgage {
            balance: 300_000.0,
            rate: 0.07,
            remaining_months: 330,
            ..Mortgage::default()
        }),
        ..Property::default()
    }];
    p.retirement.trigger = RetirementTrigger::AtAge {
        person: 0,
        age: 65.0,
    };
    p.debt_policy.refinance_mortgages = false;
    p
}

#[test]
fn prepayment_goes_to_the_mortgage_only_while_its_rate_beats_safe_returns() {
    let base_p = saver_with_mortgage();
    let base = run(&compile(&base_p), SEED, 0..PATHS);
    // A real threshold of 20% is never met (7% less last year's inflation would need
    // a deflation of 13%; the record's worst year, 1921, was about 10.5%): identical
    // bytes. At 15% it is met in the bootstrap's deflation years of the early 1930s.
    let mut never = base_p.clone();
    never.debt_policy.mortgage_prepayment = MortgagePrepayment::WhenRateAbove {
        real_rate: Some(0.2),
    };
    assert_eq!(
        base.to_bytes(),
        run(&compile(&never), SEED, 0..PATHS).to_bytes()
    );
    // 7% against a 10-year yield of about 4%: what would go to the taxable account
    // pays the mortgage instead, dollar for dollar.
    let mut p = base_p.clone();
    p.debt_policy.mortgage_prepayment = MortgagePrepayment::WhenRateAbove { real_rate: None };
    let plan = compile(&p);
    let on = run_checked(&plan, SEED, 0..PATHS);
    let saved =
        in_year(&base, YearSeries::SaveTaxable, 0) - in_year(&on, YearSeries::SaveTaxable, 0);
    let prepaid = in_year(&on, YearSeries::SpendDebt, 0) - in_year(&base, YearSeries::SpendDebt, 0);
    eprintln!("year 0: taxable saving down {saved}, prepaid {prepaid}");
    assert!(prepaid > 1_000 * PATHS as i64, "{prepaid}");
    assert!(
        (saved - prepaid).abs() <= 2 * PATHS as i64,
        "{saved} vs {prepaid}"
    );
    // The loan, 27.5 years long, is gone within about a decade.
    let years = on.years as usize;
    let owed = |a: &Accumulator, y: usize| a.sums[YearSeries::Mortgage.index() * years + y];
    eprintln!(
        "owed in year 10: without {}, with {}",
        owed(&base, 10),
        owed(&on, 10)
    );
    assert!(owed(&on, 10) < owed(&base, 10) / 4);
    // A mortgage at 2% is below the after-tax yield on most paths and in most years:
    // a small fraction is prepaid.
    let mut cheap = p.clone();
    if let Some(m) = &mut cheap.properties[0].mortgage {
        m.rate = 0.02;
    }
    let mut cheap_base = base_p;
    cheap_base.properties = cheap.properties.clone();
    let c_on = run(&compile(&cheap), SEED, 0..PATHS);
    let c_base = run(&compile(&cheap_base), SEED, 0..PATHS);
    let extra = years_total(&c_on, YearSeries::SpendDebt, 0..10)
        - years_total(&c_base, YearSeries::SpendDebt, 0..10);
    let at_seven = years_total(&on, YearSeries::SpendDebt, 0..10)
        - years_total(&base, YearSeries::SpendDebt, 0..10);
    eprintln!("prepaid in ten years: on a 7% loan {at_seven}, on a 2% loan {extra}");
    assert!(extra < at_seven / 10, "{extra} vs {at_seven}");
}

// ---------------------------------------------------------------------------------
// F30: rentals that behave like rentals.

/// A retired landlord with a $300,000 rental let at $2,000 a month (5% vacancy, 1%
/// maintenance) and plenty of savings.
fn landlord() -> Profile {
    let mut p = house_rich_retiree();
    p.accounts[0].balance = 2_000_000.0;
    p.accounts[0].cost_basis = Some(2_000_000.0);
    p.properties.push(Property {
        id: "flat".into(),
        name: "flat".into(),
        value: 300_000.0,
        property_tax_rate: Some(0.01),
        maintenance_rate: 0.01,
        usage: PropertyUse::Rental(RentalTerms {
            monthly_rent: 2_000.0,
            vacancy: 0.05,
            ..RentalTerms::default()
        }),
        ..Property::default()
    });
    p
}

/// Per path and year: rental income less housing spending. A month in which the
/// rental loses money books the loss as housing spending, and the home's own costs
/// are the same with either dynamics, so differences are the rental's.
fn rental_years(plan: &sim::Plan, paths: u64, years: usize) -> Vec<f64> {
    let mut out = Vec::new();
    for path in 0..paths {
        let trace = sim::trace_path(plan, SEED, path);
        let col = |s: YearSeries| {
            trace
                .annual
                .columns
                .iter()
                .find(|c| c.key == s.key())
                .unwrap()
                .values
                .clone()
        };
        let (rent, housing) = (col(YearSeries::IncomeRental), col(YearSeries::SpendHousing));
        out.extend((1..=years).map(|y| rent[y] - housing[y]));
    }
    out
}

#[test]
fn realistic_rentals_keep_the_average_and_add_lumps() {
    let simple_p = landlord();
    let real_p = realistic_rentals(simple_p.clone());
    // Explicitly simple is the default.
    let mut explicit = simple_p.clone();
    if let PropertyUse::Rental(t) = &mut explicit.properties[1].usage {
        t.dynamics = RentalDynamics::Simple;
    }
    let base = run(&compile(&simple_p), SEED, 0..PATHS);
    assert_eq!(
        base.to_bytes(),
        run(&compile(&explicit), SEED, 0..PATHS).to_bytes()
    );
    let real_plan = compile(&real_p);
    let real = run_checked(&real_plan, SEED, 0..PATHS);
    // Social Security is untouched path by path: the rental's randomness is its own.
    assert_eq!(
        series_total(&base, YearSeries::IncomeSocialSecurity),
        series_total(&real, YearSeries::IncomeSocialSecurity)
    );
    // Same average over 20 years and all paths: vacancy is still 5% in the long run
    // and repairs keep the maintenance budget; evictions (2% of tenancy-years at
    // three months' rent, about $120 a year) are the only new cost. Within 3%.
    let years = 20;
    let simple = rental_years(&compile(&simple_p), 60, years);
    let lumpy = rental_years(&real_plan, 60, years);
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    let (ms, mr) = (mean(&simple), mean(&lumpy));
    eprintln!("mean annual rental income less housing costs: simple {ms:.0}, realistic {mr:.0}");
    // Gross rent is $24,000 a year: within 3% of it.
    assert!((mr - ms).abs() < 0.03 * 24_000.0, "{mr} vs {ms}");
    // Lumpier: paired year by year (the home's costs cancel), the realistic rental
    // is often a quarter of a year's rent ($5,000) or more away from the simple one:
    // a long vacancy, an eviction or a large repair.
    let far = simple
        .iter()
        .zip(&lumpy)
        .filter(|(a, b)| (*b - *a).abs() > 5_000.0)
        .count();
    eprintln!("years $5,000 or more apart: {far} of {}", simple.len());
    assert!(far as f64 > 0.05 * simple.len() as f64, "{far}");
    // And it shows in the trace.
    let mut seen = (false, false);
    for path in 0..20 {
        let trace = sim::trace_path(&real_plan, SEED, path);
        for e in &trace.events {
            seen.0 |= e.label.starts_with("Tenant moved out of flat");
            seen.1 |= e.label.starts_with("Large repair on flat");
        }
    }
    assert!(seen.0 && seen.1, "{seen:?}");
}

// ---------------------------------------------------------------------------------
// Random streams: the features draw only from their own streams, and how much they
// draw does not depend on what the household decides.

#[test]
fn housing_decisions_leave_markets_careers_and_lifespans_untouched() {
    let mut base_p = overspending(Profile::example(), 1.6);
    // A fixed retirement date, so that wages and layoffs are comparable.
    base_p.retirement.trigger = RetirementTrigger::AtAge {
        person: 0,
        age: 58.0,
    };
    let on_p = with_housing_decisions(base_p.clone());
    let hecm_p = with_housing_decisions_hecm(base_p.clone());
    let plans = [compile(&base_p), compile(&on_p), compile(&hecm_p)];
    let mut changed = 0;
    for path in 0..25 {
        let traces: Vec<_> = plans
            .iter()
            .map(|p| sim::trace_path(p, SEED, path))
            .collect();
        let a = &traces[0];
        for b in &traces[1..] {
            assert_eq!(a.months, b.months, "path {path}");
            for s in [
                PathScalar::DeathAge0,
                PathScalar::DeathAge1,
                PathScalar::Inflation,
                PathScalar::MonthsUnemployed,
                PathScalar::LongTermCareCost,
            ] {
                let v = |t: &firesight_core::stats::PathTrace| {
                    t.scalars.iter().find(|x| x.scalar == s).unwrap().value
                };
                assert_eq!(v(a).to_bits(), v(b).to_bits(), "path {path} {s:?}");
            }
            for key in [
                "price_level",
                "return_us_stocks",
                "return_bonds",
                "income_wages",
            ] {
                assert_eq!(a.column(key), b.column(key), "path {path} {key}");
            }
            if a.column("mortgage") != b.column("mortgage") {
                changed += 1;
            }
        }
    }
    // And the decisions did act.
    assert!(changed > 10, "{changed}");
}

#[test]
fn lenders_and_tenants_do_not_depend_on_the_household() {
    // The lenders' freezes and the rental's tenants and repairs are the same whatever
    // the household spends, path by path.
    let labels = |plan: &sim::Plan, path: u64, prefixes: &[&str]| -> Vec<(YearMonth, String)> {
        sim::trace_path(plan, SEED, path)
            .events
            .iter()
            .filter(|e| prefixes.iter().any(|p| e.label.starts_with(p)))
            .map(|e| (e.month, e.label.clone()))
            .collect()
    };
    let mut lean = landlord();
    lean.retirement.home_equity_borrowing = heloc();
    let lavish = overspending(lean.clone(), 1.5);
    let (lean, lavish) = (
        compile(&realistic_rentals(lean)),
        compile(&realistic_rentals(lavish)),
    );
    let prefixes = [
        "Lenders freeze",
        "Home-equity lines open",
        "Tenant moved out of flat",
        "Evicted a tenant from flat",
        "Large repair on flat",
    ];
    let mut events = 0;
    for path in 0..30 {
        let a = labels(&lean, path, &prefixes);
        let b = labels(&lavish, path, &prefixes);
        assert_eq!(a, b, "path {path}");
        events += a.len();
        // The spending did change the path.
        let ta = sim::trace_path(&lean, SEED, path);
        let tb = sim::trace_path(&lavish, SEED, path);
        assert_ne!(ta.column("spend_essential"), tb.column("spend_essential"));
    }
    assert!(events > 30, "{events}");
}
