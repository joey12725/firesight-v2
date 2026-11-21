//! Household behaviour features (`sim/behavior`) on paired paths: each feature
//! switched on but unable to act leaves every path bit-identical to the baseline, and
//! switched on for real moves outcomes in the expected direction by a plausible
//! amount.

mod behavior_profiles;
mod common;

use behavior_profiles::*;

use common::*;
use firesight_core::model::*;
use firesight_core::sim;
use firesight_core::stats::{Accumulator, PathScalar, TraceEventKind};

const SEED: u64 = 77;

fn column(acc: &Accumulator, s: PathScalar) -> Vec<f32> {
    acc.scalar_column(s)
}

fn mean(v: &[f32]) -> f64 {
    v.iter().map(|x| f64::from(*x)).sum::<f64>() / v.len() as f64
}

fn share_depleted(acc: &Accumulator) -> f64 {
    let d = column(acc, PathScalar::DepletionAge);
    d.iter().filter(|x| x.is_finite()).count() as f64 / d.len() as f64
}

#[test]
fn return_to_work_that_cannot_trigger_changes_nothing() {
    let base = underfunded();
    let mut never = base.clone();
    never.retirement.return_to_work = Some(ReturnToWork {
        funded_ratio_below: 0.0,
        ..ReturnToWork::default()
    });
    let a = run(&compile(&base), SEED, 0..300);
    let b = run(&compile(&never), SEED, 0..300);
    assert_eq!(a.to_bytes(), b.to_bytes());
}

#[test]
fn going_back_to_work_lowers_depletion_risk() {
    let base = underfunded();
    let mut back = base.clone();
    back.retirement.return_to_work = Some(ReturnToWork::default());
    let a = run(&compile(&base), SEED, 0..400);
    let b = run(&compile(&back), SEED, 0..400);
    let (da, db) = (share_depleted(&a), share_depleted(&b));
    // Half-pay work from 60 to at most 70 on a plan funded about half-way: a large
    // cut in depletion, but not to zero (the job stops once funded and at 70).
    assert!(da > 0.3, "baseline depletion {da}");
    assert!(db < da - 0.1, "depletion {da} -> {db}");
    assert!(db > 0.0, "depletion {db}");
    // Lifetime tax rises with the extra wages.
    let (ta, tb) = (
        mean(&column(&a, PathScalar::LifetimeTax)),
        mean(&column(&b, PathScalar::LifetimeTax)),
    );
    assert!(tb > ta, "lifetime tax {ta} -> {tb}");
}

#[test]
fn a_traced_return_to_work_shows_the_search_the_job_and_the_wages() {
    let mut p = underfunded();
    p.retirement.return_to_work = Some(ReturnToWork {
        search: JobSearch::Fixed { months: 3.0 },
        ..ReturnToWork::default()
    });
    let plan = compile(&p);
    let trace = sim::trace_path(&plan, SEED, 0);
    let look = trace
        .events
        .iter()
        .find(|e| e.label.contains("looks for work"))
        .expect("the household looks for work");
    let back = trace
        .events
        .iter()
        .find(|e| e.kind == TraceEventKind::Rehired)
        .expect("and finds it");
    // A fixed three-month search starting in January ends in April.
    assert_eq!(look.month.month(), 1);
    assert_eq!(back.month - look.month, 3);
    // The month of the start is paid in full (whole-month search), at half of the
    // $90,000 salary in today's dollars: $3,750 a month (trace columns are in whole
    // today's dollars).
    let wages = trace.column("income_wages").unwrap();
    let t = (back.month - trace.start) as usize;
    assert!((wages[t] - 3_750.0).abs() <= 1.0, "wages {}", wages[t]);
    assert_eq!(wages[t - 1], 0.0);
}

#[test]
fn return_to_work_settings_are_validated() {
    let mut p = underfunded();
    p.retirement.return_to_work = Some(ReturnToWork {
        pay_fraction: -0.1,
        ..ReturnToWork::default()
    });
    let r = firesight_core::validate::validate(&p);
    assert!(r
        .errors
        .iter()
        .any(|e| e.path == "/retirement/return_to_work/pay_fraction"));
    p.retirement.return_to_work = Some(ReturnToWork {
        funded_ratio_below: 1.2,
        ..ReturnToWork::default()
    });
    let r = firesight_core::validate::validate(&p);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(r
        .warnings
        .iter()
        .any(|w| w.code == "return_to_work_when_funded"));
}

/// A 65-year-old with $1.5 million in a taxable account at 70/30, spending $60,000,
/// with tax on (gains are realised when a panic sale happens).
fn taxed_retiree() -> Profile {
    let mut p = retiree(1_500_000.0, 0.7, 60_000.0, 30.0);
    p.assumptions.taxes.kind = TaxKind::UsCurrentLaw;
    p.accounts[0].cost_basis = Some(900_000.0);
    p
}

fn panic_prone(probability: f64) -> InvestorBehavior {
    InvestorBehavior::PanicProne(PanicSelling {
        probability,
        ..PanicSelling::default()
    })
}

#[test]
fn a_household_that_never_panics_is_the_disciplined_one() {
    let base = taxed_retiree();
    let mut never = base.clone();
    never.portfolio.behavior = panic_prone(0.0);
    let a = run(&compile(&base), SEED, 0..300);
    let b = run(&compile(&never), SEED, 0..300);
    assert_eq!(a.to_bytes(), b.to_bytes());
}

#[test]
fn panic_selling_costs_wealth_on_paired_paths() {
    let base = taxed_retiree();
    let mut panicky = base.clone();
    panicky.portfolio.behavior = panic_prone(1.0);
    let n = 1000;
    let a = run(&compile(&base), SEED, 0..n);
    let b = run(&compile(&panicky), SEED, 0..n);
    let wa = column(&a, PathScalar::TerminalNetWorth);
    let wb = column(&b, PathScalar::TerminalNetWorth);
    let (ma, mb) = (mean(&wa), mean(&wb));
    // Always selling half the stocks in every fall of 25%+ and buying back near the
    // old peak misses most of each recovery: a clear loss, far from a total one.
    assert!(mb < 0.97 * ma, "mean terminal net worth {ma} -> {mb}");
    assert!(mb > 0.5 * ma, "mean terminal net worth {ma} -> {mb}");
    // Paths without a 25% fall are untouched; most paths with one lose.
    let changed = wa.iter().zip(&wb).filter(|(x, y)| x != y).count();
    let worse = wa.iter().zip(&wb).filter(|(x, y)| y < x).count();
    assert!(changed > n as usize / 10, "{changed} paths changed");
    assert!(changed < n as usize, "{changed} paths changed");
    assert!(worse * 3 > changed * 2, "{worse} of {changed} worse");
    assert!(share_depleted(&b) >= share_depleted(&a));
}

#[test]
fn a_traced_panic_sale_moves_stocks_to_cash_and_back() {
    let mut p = taxed_retiree();
    p.portfolio.behavior = panic_prone(1.0);
    let plan = compile(&p);
    let mut found = false;
    for path in 0..60 {
        let trace = sim::trace_path(&plan, SEED, path);
        let Some(sale) = trace
            .events
            .iter()
            .find(|e| e.label.starts_with("Panic sale"))
        else {
            continue;
        };
        found = true;
        assert_eq!(sale.kind, TraceEventKind::Market);
        // Bought back later, after at most 36 months.
        let back = trace
            .events
            .iter()
            .find(|e| e.label.starts_with("Back into stocks") && e.month > sale.month);
        let months = trace.columns[0].values.len() as i32;
        match back {
            Some(back) => assert!(back.month - sale.month <= 36),
            None => assert!(sale.month - trace.start + 36 >= months, "never bought back"),
        }
        // Out of the market the portfolio moves with 35% stocks instead of 70%: in
        // the month after the sale its return is closer to half the stock return.
        let port = trace.column("portfolio_return").unwrap();
        let us = trace.column("return_us_stocks").unwrap();
        let t = (sale.month - trace.start) as usize + 1;
        if t < port.len() && us[t].abs() > 0.02 {
            let beta = port[t] / us[t];
            assert!(
                beta < 0.6,
                "portfolio return {} vs stocks {}",
                port[t],
                us[t]
            );
        }
        break;
    }
    assert!(found, "no panic sale in 60 paths");
}

#[test]
fn panic_settings_are_validated() {
    let mut p = taxed_retiree();
    p.portfolio.behavior = InvestorBehavior::PanicProne(PanicSelling {
        probability: 1.5,
        ..PanicSelling::default()
    });
    let r = firesight_core::validate::validate(&p);
    assert!(r
        .errors
        .iter()
        .any(|e| e.path == "/portfolio/behavior/probability"));
    p.portfolio.behavior = InvestorBehavior::PanicProne(PanicSelling {
        reentry: Reentry {
            recovered_within: 0.3,
            max_months: 12,
        },
        ..PanicSelling::default()
    });
    let r = firesight_core::validate::validate(&p);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(r
        .warnings
        .iter()
        .any(|w| w.code == "panic_reentry_immediate"));
}

fn adaptive(earliest: f64, latest: f64) -> Claiming {
    Claiming::Adaptive { earliest, latest }
}

/// A 61-year-old retiree with $2,000 a month at full retirement age, `savings` in a
/// 60/40 taxable account and a $48,000 budget, no tax, living to 92.
fn early_retiree(savings: f64) -> Profile {
    let mut who = person("retiree", 61, Sex::Female);
    who.social_security.basis = SocialSecurityBasis::KnownBenefit {
        monthly_at_fra: 2_000.0,
    };
    let mut p = quiet(vec![who], 92.0);
    p.spending.items = vec![item("living", Category::Other, 4_000.0, true)];
    p.accounts = vec![Account {
        cost_basis: Some(savings),
        ..account("savings", 0, AccountKind::Taxable, savings)
    }];
    p.portfolio.target =
        Allocation::from_pairs(&[(AssetClass::UsStocks, 0.6), (AssetClass::Bonds, 0.4)]);
    p.retirement.trigger = RetirementTrigger::AlreadyRetired;
    p
}

#[test]
fn adaptive_claiming_with_no_room_to_adapt_is_the_fixed_claim() {
    let mut base = early_retiree(400_000.0);
    base.household.people[0].social_security.claim_age = 67.0;
    let mut same = base.clone();
    same.household.people[0].social_security.claiming = adaptive(67.0, 67.0);
    let a = run(&compile(&base), SEED, 0..300);
    let b = run(&compile(&same), SEED, 0..300);
    assert_eq!(a.to_bytes(), b.to_bytes());
}

#[test]
fn adaptive_claiming_delays_when_it_can_and_claims_early_when_it_must() {
    let n = 400;
    let mut late = early_retiree(300_000.0);
    late.spending.items = vec![item("living", Category::Other, 3_000.0, true)];
    late.household.people[0].social_security.claim_age = 70.0;
    let mut early = late.clone();
    early.household.people[0].social_security.claim_age = 62.0;
    let mut adapt = late.clone();
    adapt.household.people[0].social_security.claiming = adaptive(62.0, 70.0);
    let l = run(&compile(&late), SEED, 0..n);
    let e = run(&compile(&early), SEED, 0..n);
    let a = run(&compile(&adapt), SEED, 0..n);
    // Delaying to 70 on $300,000 against $36,000 a year leaves many paths cutting
    // essentials before the benefit starts; claiming once the delay can no longer
    // be bridged cuts the years spent depleted (measured: 10.3 -> 7.4).
    let years = |acc: &Accumulator| mean(&column(acc, PathScalar::YearsDepleted));
    let (yl, ya) = (years(&l), years(&a));
    assert!(
        ya < 0.85 * yl,
        "years depleted: fixed 70 {yl}, adaptive {ya}"
    );
    assert!(share_depleted(&a) <= share_depleted(&l));
    // And it keeps more of the delay credit than claiming at 62: more spending over
    // a lifetime (measured: $1.029 million against $1.002 million).
    let spend = |acc: &Accumulator| mean(&column(acc, PathScalar::LifetimeSpending));
    assert!(
        spend(&a) > spend(&e) * 1.01,
        "{} vs {}",
        spend(&a),
        spend(&e)
    );
    // Claim ages spread between 62 and 70, path by path.
    let ages = column(&a, PathScalar::SocialSecurityClaimAge0);
    let lo = ages.iter().cloned().fold(f32::INFINITY, f32::min);
    let hi = ages.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    assert!(lo < 66.0, "earliest claim {lo}");
    assert!((69.9..=70.01).contains(&hi), "latest claim {hi}");
    // A retiree who can afford the bridge on every path waits to 70.
    let mut wealthy = early_retiree(1_500_000.0);
    wealthy.household.people[0].social_security.claiming = adaptive(62.0, 70.0);
    let w = run(&compile(&wealthy), SEED, 0..200);
    let ages = column(&w, PathScalar::SocialSecurityClaimAge0);
    let at70 = ages.iter().filter(|a| (**a - 70.0).abs() < 0.05).count();
    assert!(at70 > 180, "a wealthy retiree delays to 70: {at70} of 200");
}

#[test]
fn in_a_couple_the_lower_earner_claims_first() {
    let mut p = early_retiree(500_000.0);
    let mut partner = person("partner", 61, Sex::Male);
    partner.social_security.basis = SocialSecurityBasis::KnownBenefit {
        monthly_at_fra: 3_200.0,
    };
    p.household.people.push(partner);
    p.spending.items = vec![item("living", Category::Other, 6_000.0, true)];
    for person in &mut p.household.people {
        person.social_security.claiming = adaptive(62.0, 70.0);
    }
    let a = run(&compile(&p), SEED, 0..400);
    let lower = mean(&column(&a, PathScalar::SocialSecurityClaimAge0));
    let higher = mean(&column(&a, PathScalar::SocialSecurityClaimAge1));
    assert!(
        higher > lower + 1.0,
        "claim ages: lower {lower}, higher {higher}"
    );
    let trace = sim::trace_path(&compile(&p), SEED, 0);
    assert!(trace
        .events
        .iter()
        .any(|e| e.label.contains("claims Social Security at")));
}

#[test]
fn claiming_settings_are_validated() {
    let mut p = early_retiree(400_000.0);
    p.household.people[0].social_security.claiming = adaptive(68.0, 64.0);
    let r = firesight_core::validate::validate(&p);
    assert!(
        r.errors.iter().any(|e| e.code == "claiming_order"),
        "{:?}",
        r.errors
    );
    p.household.people[0].social_security.claiming = adaptive(60.0, 70.0);
    let r = firesight_core::validate::validate(&p);
    assert!(r
        .errors
        .iter()
        .any(|e| e.path.ends_with("/social_security/claiming/earliest")));
}
