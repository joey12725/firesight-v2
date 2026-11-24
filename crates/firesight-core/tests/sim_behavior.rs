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

/// A 35-year-old earning $100,000 who defers 15% into a 401(k) with a 50% match on
/// the first 6%, holding `card` on a 22% credit card; no tax, retires at 65.
fn saver_with_card(card: f64) -> Profile {
    let mut p = quiet(vec![person("saver", 35, Sex::Female)], 90.0);
    let mut income = job("job", 0, 100_000.0);
    if let IncomeKind::Job(j) = &mut income.kind {
        j.plan = Some(EmployerPlan {
            employee_fraction: 0.15,
            match_rate: 0.5,
            match_up_to: 0.06,
            ..EmployerPlan::default()
        });
    }
    p.incomes = vec![income];
    p.spending.items = vec![item("living", Category::Other, 4_000.0, true)];
    p.portfolio.emergency_months = 3.0;
    p.accounts = vec![
        account("cash", 0, AccountKind::Cash, 30_000.0),
        account("brokerage", 0, AccountKind::Taxable, 50_000.0),
    ];
    if card > 0.0 {
        p.debts = vec![Debt {
            id: "card".into(),
            name: "Card".into(),
            kind: DebtKind::CreditCard,
            balance: card,
            rate: 0.22,
            ..Debt::default()
        }];
    }
    p.retirement.trigger = RetirementTrigger::AtAge {
        person: 0,
        age: 65.0,
    };
    p
}

#[test]
fn a_household_never_in_distress_saves_as_before() {
    let base = saver_with_card(0.0);
    let mut pausing = base.clone();
    pausing.portfolio.pause_deferrals_in_distress = true;
    let a = run(&compile(&base), SEED, 0..200);
    let b = run(&compile(&pausing), SEED, 0..200);
    assert_eq!(a.to_bytes(), b.to_bytes());
}

/// Cash below the emergency-fund target is not distress: a saver who starts with
/// next to no cash spends months filling the fund (and dips under it again whenever
/// the target outgrows it), and the payroll deferral is never cut for that.
#[test]
fn a_thin_emergency_fund_is_not_distress() {
    let mut base = saver_with_card(0.0);
    base.accounts[0].balance = 500.0;
    base.portfolio.emergency_months = 6.0;
    let mut pausing = base.clone();
    pausing.portfolio.pause_deferrals_in_distress = true;
    // The fund really is short at the start: nothing is invested from the surplus
    // in the first months.
    let trace = sim::trace_path(&compile(&pausing), SEED, 0);
    assert_eq!(trace.column("save_taxable").unwrap()[1], 0.0);
    assert!((trace.column("save_traditional").unwrap()[1] - 1_250.0).abs() < 1.0);
    for path in 0..40 {
        let t = sim::trace_path(&compile(&pausing), SEED, path);
        assert!(
            !t.events
                .iter()
                .any(|e| e.label.starts_with("Saving paused")),
            "path {path}"
        );
    }
    let a = run(&compile(&base), SEED, 0..200);
    let b = run(&compile(&pausing), SEED, 0..200);
    assert_eq!(a.to_bytes(), b.to_bytes());
}

#[test]
fn pausing_to_clear_a_credit_card_pays_it_off_sooner_and_ends_richer() {
    let base = saver_with_card(25_000.0);
    let mut pausing = base.clone();
    pausing.portfolio.pause_deferrals_in_distress = true;
    let ta = sim::trace_path(&compile(&base), SEED, 0);
    let tb = sim::trace_path(&compile(&pausing), SEED, 0);
    let paid_off = |t: &firesight_core::stats::PathTrace| {
        t.events
            .iter()
            .find(|e| e.kind == TraceEventKind::DebtPaidOff)
            .map(|e| e.month - t.start)
    };
    let (ma, mb) = (paid_off(&ta), paid_off(&tb));
    // Minimum payments (2% of the balance) take years; the paused saver puts its
    // whole surplus above the cash target on the card.
    assert!(mb.unwrap() < 12, "paid off after {mb:?} months");
    assert!(
        ma.map_or(true, |m| m > 36),
        "baseline paid off after {ma:?} months"
    );
    // The pause shows in the trace and so does the end of it.
    assert!(tb
        .events
        .iter()
        .any(|e| e.label == "Saving paused: carrying high-interest debt"));
    assert!(tb.events.iter().any(|e| e.label == "Saving resumed"));
    // While paused the deferral is the 6% that earns the match, not 15%: $500 a
    // month instead of $1,250.
    let (sa, sb) = (
        ta.column("save_traditional").unwrap(),
        tb.column("save_traditional").unwrap(),
    );
    assert!((sa[1] - 1_250.0).abs() < 1.0, "{}", sa[1]);
    assert!((sb[1] - 500.0).abs() < 1.0, "{}", sb[1]);
    // Clearing 22% debt beats investing: richer at the end on paired paths.
    let a = run(&compile(&base), SEED, 0..200);
    let b = run(&compile(&pausing), SEED, 0..200);
    let (wa, wb) = (
        mean(&column(&a, PathScalar::TerminalNetWorth)),
        mean(&column(&b, PathScalar::TerminalNetWorth)),
    );
    assert!(wb > wa, "terminal net worth {wa} -> {wb}");
    assert!(wb < wa * 1.5, "terminal net worth {wa} -> {wb}");
}

#[test]
fn penalties_paid_count_as_distress() {
    // A retired 50-year-old living off a traditional IRA with the 10% penalty.
    let mut who = person("early", 50, Sex::Female);
    who.social_security.basis = SocialSecurityBasis::None;
    let mut p = quiet(vec![who], 90.0);
    p.assumptions.taxes.kind = TaxKind::UsCurrentLaw;
    p.spending.items = vec![item("living", Category::Other, 3_000.0, true)];
    p.accounts = vec![account("ira", 0, AccountKind::Traditional, 900_000.0)];
    p.retirement.trigger = RetirementTrigger::AlreadyRetired;
    p.retirement.early_access = EarlyAccess::PayPenalty;
    p.portfolio.pause_deferrals_in_distress = true;
    let trace = sim::trace_path(&compile(&p), SEED, 0);
    let paused = trace
        .events
        .iter()
        .find(|e| e.label == "Saving paused: paying early-withdrawal penalties")
        .expect("a pause for penalties");
    // The pause ends once the penalty-free age of 59 and a half is passed.
    let resumed = trace
        .events
        .iter()
        .filter(|e| e.label == "Saving resumed")
        .map(|e| e.month - trace.start)
        .max()
        .expect("saving resumes");
    assert!(paused.month - trace.start < 12);
    assert!((114..=140).contains(&resumed), "resumed at month {resumed}");
    // Quarterly withdrawals keep it paused in between: one pause, one resumption.
    let pauses = trace
        .events
        .iter()
        .filter(|e| e.label.starts_with("Saving paused"))
        .count();
    assert_eq!(pauses, 1);
}

fn example(slug: &str) -> Profile {
    Profile::examples(START)
        .into_iter()
        .find(|e| e.slug == slug)
        .unwrap()
        .profile
}

fn with_rule(mut p: Profile, f: impl Fn(&mut WhenFunded)) -> Profile {
    if let RetirementTrigger::WhenFunded(rule) = &mut p.retirement.trigger {
        f(rule);
    } else {
        panic!("not when_funded");
    }
    p
}

#[test]
fn confirmation_that_always_holds_retires_as_before() {
    let base = example("early-career-saver");
    let same = with_rule(base.clone(), |r| {
        r.confirmation = Some(RetirementConfirmation {
            months: 1,
            max_equity_drawdown: 1.0,
        });
        r.one_more_year = Some(OneMoreYear { probability: 0.0 });
    });
    let a = run(&compile(&base), SEED, 0..200);
    let b = run(&compile(&same), SEED, 0..200);
    assert_eq!(a.to_bytes(), b.to_bytes());
}

#[test]
fn waiting_for_confirmation_and_one_more_year_retire_later() {
    let n = 600;
    let base = example("early-career-saver");
    let confirmed = with_rule(base.clone(), |r| {
        r.confirmation = Some(RetirementConfirmation::default());
    });
    let omy = with_rule(base.clone(), |r| {
        r.one_more_year = Some(OneMoreYear { probability: 0.25 });
    });
    let a = run(&compile(&base), SEED, 0..n);
    let c = run(&compile(&confirmed), SEED, 0..n);
    let o = run(&compile(&omy), SEED, 0..n);
    let ages = |acc: &Accumulator| column(acc, PathScalar::RetirementAge);
    let (ra, rc, ro) = (ages(&a), ages(&c), ages(&o));
    // Never earlier on any path, later on average.
    for (x, y) in ra.iter().zip(&rc).chain(ra.iter().zip(&ro)) {
        if x.is_finite() && y.is_finite() {
            assert!(y + 1e-3 >= *x, "{x} -> {y}");
        }
    }
    let gap = |b: &[f32]| {
        let d: Vec<f32> = ra
            .iter()
            .zip(b)
            .filter(|(x, y)| x.is_finite() && y.is_finite())
            .map(|(x, y)| y - x)
            .collect();
        mean(&d)
    };
    // Confirmation: at least the 5 extra months of the run on every path (6 in a row
    // instead of 1), more when a fall holds it back.
    let gc = gap(&rc);
    assert!(
        (5.0 / 12.0 - 1e-3..2.0).contains(&gc),
        "confirmation delay {gc}"
    );
    // One more year at 25%: the expected number of extra years is
    // 0.25 / (1 - 0.25) = 1/3 when the household stays funded through the year.
    let go = gap(&ro);
    assert!((0.2..0.5).contains(&go), "one-more-year delay {go}");
    // Working longer lowers the depletion rate.
    assert!(share_depleted(&o) < share_depleted(&a));
    // In a trace: the decision, then retirement at least a year later.
    let plan = compile(&omy);
    let shown = (0..40).any(|path| {
        let trace = sim::trace_path(&plan, SEED, path);
        let decided = trace
            .events
            .iter()
            .find(|e| e.label == "Funded, but working one more year");
        let retired = trace
            .events
            .iter()
            .find(|e| e.kind == TraceEventKind::Retired);
        match (decided, retired) {
            (Some(d), Some(r)) => {
                assert!(r.month - d.month >= 12, "{:?} {:?}", d.month, r.month);
                true
            }
            _ => false,
        }
    });
    assert!(shown, "no one-more-year decision in 40 traced paths");
}

#[test]
fn confirmation_settings_are_validated() {
    let p = with_rule(example("early-career-saver"), |r| {
        r.confirmation = Some(RetirementConfirmation {
            months: 6,
            max_equity_drawdown: 1.5,
        });
        r.one_more_year = Some(OneMoreYear { probability: 0.7 });
    });
    let r = firesight_core::validate::validate(&p);
    assert!(r
        .errors
        .iter()
        .any(|e| e.path == "/retirement/trigger/confirmation/max_equity_drawdown"));
    assert!(r.warnings.iter().any(|w| w.code == "one_more_year_likely"));
}

// ---- Stream discipline ------------------------------------------------------------

/// The months of the events whose label starts with `prefix` on a traced path.
fn event_months(plan: &firesight_core::sim::Plan, path: u64, prefix: &str) -> Vec<i32> {
    let trace = sim::trace_path(plan, SEED, path);
    let start = trace.start;
    trace
        .events
        .iter()
        .filter(|e| e.label.starts_with(prefix))
        .map(|e| e.month - start)
        .collect()
}

#[test]
fn panic_decisions_do_not_depend_on_unrelated_household_decisions() {
    // The panic decision uses the stock index and the PanicSelling stream only: a
    // household that spends a third more (and so holds different balances, sells
    // different amounts and may run out of money) panics in exactly the same months.
    let mut a = taxed_retiree();
    a.portfolio.behavior = panic_prone(0.3);
    let mut b = a.clone();
    for item in &mut b.spending.items {
        item.amount *= 4.0 / 3.0;
    }
    let (pa, pb) = (compile(&a), compile(&b));
    let mut sales = 0;
    for path in 0..80 {
        let ma = event_months(&pa, path, "Panic sale");
        let mb = event_months(&pb, path, "Panic sale");
        assert_eq!(ma, mb, "path {path}");
        sales += ma.len();
    }
    assert!(sales > 3, "only {sales} panic sales in 80 paths");
}

#[test]
fn switching_every_behaviour_on_leaves_market_and_longevity_draws_alone() {
    // Market returns and the length of life come from their own streams: switching
    // all five features on (which changes saving, claiming, work and the portfolio)
    // must not move them on any path.
    for p in [underfunded(), example("dual-income-family")] {
        let off = compile(&p);
        let on = compile(&with_behaviors(p.clone()));
        for path in 0..30 {
            let a = sim::trace_path(&off, SEED, path);
            let b = sim::trace_path(&on, SEED, path);
            for key in ["return_us_stocks", "return_bonds", "inflation"] {
                if let (Some(x), Some(y)) = (a.column(key), b.column(key)) {
                    assert_eq!(x, y, "{key} on path {path}");
                }
            }
            assert_eq!(
                a.columns[0].values.len(),
                b.columns[0].values.len(),
                "path {path} length"
            );
            let deaths = |t: &firesight_core::stats::PathTrace| -> Vec<i32> {
                t.events
                    .iter()
                    .filter(|e| e.kind == TraceEventKind::Died)
                    .map(|e| e.month - t.start)
                    .collect()
            };
            assert_eq!(deaths(&a), deaths(&b), "deaths on path {path}");
        }
    }
}
