//! Tax and policy realism options, switched on one at a time against the same paths:
//! chained-CPI indexing, state detail, policy risk, tax-loss harvesting and
//! qualified charitable distributions. Each is off by default, so each test first
//! checks that stating the default changes nothing, then that switching the option
//! on moves the outcome in the expected direction by a plausible amount.

mod common;

use common::*;
use firesight_core::model::*;
use firesight_core::sim::Workspace;
use firesight_core::stats::{Accumulator, PathScalar};

const PATHS: u64 = 600;
const SEED: u64 = 7;

fn example(slug: &str) -> Profile {
    Profile::examples(START)
        .into_iter()
        .find(|e| e.slug == slug)
        .expect("example exists")
        .profile
}

fn mean(acc: &Accumulator, scalar: PathScalar) -> f64 {
    let column = acc.scalar_column(scalar);
    let finite: Vec<f64> = column
        .iter()
        .filter(|v| v.is_finite())
        .map(|&v| f64::from(v))
        .collect();
    finite.iter().sum::<f64>() / finite.len() as f64
}

fn run_profile(profile: &Profile) -> Accumulator {
    let plan = compile(profile);
    run(&plan, SEED, 0..PATHS)
}

/// Runs with the conservation checks on and asserts every scalar is finite where
/// it should be, so a feature that leaks or invents money fails here.
fn assert_conserves_and_is_finite(profile: &Profile) {
    let plan = compile(profile);
    let mut ws = Workspace::new(&plan);
    ws.set_check(true);
    let mut acc = plan.new_accumulator();
    for i in 0..200 {
        firesight_core::sim::run_path(&plan, SEED, i, &mut ws, &mut acc);
    }
    for s in [PathScalar::LifetimeTax, PathScalar::TerminalNetWorth] {
        assert!(acc.scalar_column(s).iter().all(|v| !v.is_infinite()));
    }
    let summary = plan.summarize(&acc);
    let json = serde_json::to_string(&summary).unwrap();
    assert!(!json.contains("NaN") && !json.contains("Infinity") && !json.contains(":inf"));
}

// ---------------------------------------------------------------- F21 chained CPI

#[test]
fn chained_cpi_default_is_bit_identical_and_on_raises_lifetime_tax() {
    let base = example("dual-income-family");
    let mut stated = base.clone();
    stated.assumptions.taxes.chained_cpi = false;
    let a = run_profile(&base);
    let b = run_profile(&stated);
    assert_eq!(
        a.to_bytes(),
        b.to_bytes(),
        "stating the default changes bits"
    );

    let mut on = base.clone();
    on.assumptions.taxes.chained_cpi = true;
    let c = run_profile(&on);
    let (tax_off, tax_on) = (
        mean(&a, PathScalar::LifetimeTax),
        mean(&c, PathScalar::LifetimeTax),
    );
    let rise = tax_on / tax_off - 1.0;
    // Measured on 3,000 paths: median lifetime tax +3.9%. Same paths, so the
    // difference is the wedge, not noise.
    assert!((0.01..0.08).contains(&rise), "lifetime tax rose {rise}");
    assert!(
        mean(&c, PathScalar::TerminalNetWorth) < mean(&a, PathScalar::TerminalNetWorth) * 1.001
    );
    assert_conserves_and_is_finite(&on);
}

// ---------------------------------------------------------------- F22 state detail

fn in_state(slug: &str, state: &str) -> Profile {
    let mut p = example(slug);
    p.household.location.state = Some(state.into());
    p
}

fn lifetime_tax_change(profile: &Profile) -> f64 {
    let off = run_profile(profile);
    let mut on = profile.clone();
    on.assumptions.taxes.state_detail = StateDetail::Full;
    let on_acc = run_profile(&on);
    mean(&on_acc, PathScalar::LifetimeTax) / mean(&off, PathScalar::LifetimeTax) - 1.0
}

#[test]
fn state_detail_basic_is_the_default_and_changes_nothing_outside_ca_ny_ut() {
    for state in ["TX", "CO", "FL"] {
        let base = in_state("dual-income-family", state);
        let mut full = base.clone();
        full.assumptions.taxes.state_detail = StateDetail::Full;
        assert_eq!(
            run_profile(&base).to_bytes(),
            run_profile(&full).to_bytes(),
            "{state}"
        );
    }
    let mut stated = example("dual-income-family");
    stated.assumptions.taxes.state_detail = StateDetail::Basic;
    assert_eq!(
        run_profile(&example("dual-income-family")).to_bytes(),
        run_profile(&stated).to_bytes()
    );
}

#[test]
fn state_detail_full_moves_lifetime_tax_the_right_way_in_each_state() {
    // Measured on 2,000 paths of the dual-income example moved to each state: California
    // +6.0% (1.3% payroll tax on two salaries), New York +2.3% (recapture on a
    // household above $107,650), Utah -1.4% (the credit on Social Security).
    let ca = lifetime_tax_change(&in_state("dual-income-family", "CA"));
    assert!((0.03..0.10).contains(&ca), "CA {ca}");
    let ny = lifetime_tax_change(&in_state("dual-income-family", "NY"));
    assert!((0.005..0.05).contains(&ny), "NY {ny}");
    let ut = lifetime_tax_change(&in_state("dual-income-family", "UT"));
    assert!((-0.04..-0.002).contains(&ut), "UT {ut}");
}

#[test]
fn state_detail_full_conserves_money_and_stays_finite() {
    for state in ["CA", "NY", "UT"] {
        let mut p = in_state("dual-income-family", state);
        p.assumptions.taxes.state_detail = StateDetail::Full;
        assert_conserves_and_is_finite(&p);
    }
}

// ---------------------------------------------------------------- F23 policy risk

fn with_policy(base: &Profile, risk: PolicyRisk) -> Profile {
    let mut p = base.clone();
    p.assumptions.taxes.policy_risk = Some(risk);
    p
}

fn always(ss: bool, tax: bool) -> PolicyRisk {
    PolicyRisk {
        ss_haircut_probability: if ss { 1.0 } else { 0.0 },
        tax_increase_probability: if tax { 1.0 } else { 0.0 },
        ..PolicyRisk::default()
    }
}

#[test]
fn policy_risk_with_zero_probabilities_is_bit_identical_to_none() {
    let base = example("dual-income-family");
    let idle = with_policy(&base, always(false, false));
    assert_eq!(run_profile(&base).to_bytes(), run_profile(&idle).to_bytes());
}

#[test]
fn a_certain_social_security_cut_lowers_outcomes_and_leaves_the_markets_alone() {
    let base = example("dual-income-family");
    let cut = with_policy(&base, always(true, false));
    let (a, b) = (run_profile(&base), run_profile(&cut));
    // The cut leaves 77% of benefits: terminal wealth falls, success does not rise.
    assert!(
        mean(&b, PathScalar::TerminalNetWorth) < mean(&a, PathScalar::TerminalNetWorth),
        "terminal wealth did not fall"
    );
    assert!(mean(&b, PathScalar::Success) <= mean(&a, PathScalar::Success));
    // Paired paths: the same inflation, lifetimes and markets, so the only
    // difference is the cut (the size is a few percent of terminal wealth, not half).
    assert_eq!(
        a.scalar_column(PathScalar::Inflation),
        b.scalar_column(PathScalar::Inflation)
    );
    assert_eq!(
        a.scalar_column(PathScalar::DeathAge0),
        b.scalar_column(PathScalar::DeathAge0)
    );
    let drop =
        1.0 - mean(&b, PathScalar::TerminalNetWorth) / mean(&a, PathScalar::TerminalNetWorth);
    assert!((0.002..0.25).contains(&drop), "terminal wealth fell {drop}");
    assert_conserves_and_is_finite(&cut);
}

#[test]
fn a_certain_tax_increase_raises_lifetime_tax() {
    let base = example("dual-income-family");
    let raise = with_policy(&base, always(false, true));
    let (a, b) = (run_profile(&base), run_profile(&raise));
    let rise = mean(&b, PathScalar::LifetimeTax) / mean(&a, PathScalar::LifetimeTax) - 1.0;
    // Two points on every ordinary bracket from the 2030s or 2040s on: a few percent.
    assert!((0.005..0.15).contains(&rise), "lifetime tax rose {rise}");
    assert!(mean(&b, PathScalar::TerminalNetWorth) < mean(&a, PathScalar::TerminalNetWorth));
    assert_conserves_and_is_finite(&raise);
}

#[test]
fn partial_probabilities_give_a_distribution_between_the_extremes() {
    let base = example("dual-income-family");
    let none = mean(&run_profile(&base), PathScalar::TerminalNetWorth);
    let all = mean(
        &run_profile(&with_policy(&base, always(true, true))),
        PathScalar::TerminalNetWorth,
    );
    let half = mean(
        &run_profile(&with_policy(
            &base,
            PolicyRisk {
                ss_haircut_probability: 0.5,
                tax_increase_probability: 0.5,
                ..PolicyRisk::default()
            },
        )),
        PathScalar::TerminalNetWorth,
    );
    assert!(all < half && half < none, "{all} {half} {none}");
}

#[test]
fn the_trace_shows_when_policy_changed() {
    let base = example("dual-income-family");
    let plan = compile(&with_policy(&base, always(true, true)));
    let trace = firesight_core::sim::trace_path(&plan, SEED, 0);
    let labels: Vec<&str> = trace
        .events
        .iter()
        .filter(|e| e.kind.key() == "policy_change")
        .map(|e| e.label.as_str())
        .collect();
    assert_eq!(labels.len(), 2, "{labels:?}");
    assert!(labels
        .iter()
        .any(|l| l.contains("Social Security benefits cut to 77%")));
    assert!(labels
        .iter()
        .any(|l| l.contains("tax rates rise 2.0 points")));
}

// ------------------------------------------------------- F24 tax-loss harvesting

fn with_tlh(base: &Profile) -> Profile {
    let mut p = base.clone();
    p.portfolio.tax_loss_harvesting = true;
    p
}

#[test]
fn harvesting_off_is_bit_identical_and_on_changes_taxes_modestly() {
    let base = example("side-hustle-landlord");
    let mut explicit = base.clone();
    explicit.portfolio.tax_loss_harvesting = false;
    let a = run_profile(&base);
    assert_eq!(a.to_bytes(), run_profile(&explicit).to_bytes());
    let on = with_tlh(&base);
    let b = run_profile(&on);
    assert_ne!(a.to_bytes(), b.to_bytes(), "harvesting did nothing");
    // Paired paths: markets and lifetimes are untouched.
    assert_eq!(
        a.scalar_column(PathScalar::Inflation),
        b.scalar_column(PathScalar::Inflation)
    );
    // It moves tax around in time rather than creating or destroying much of it:
    // lifetime tax stays within a few percent, and terminal wealth does not fall.
    let tax = mean(&b, PathScalar::LifetimeTax) / mean(&a, PathScalar::LifetimeTax) - 1.0;
    assert!(tax.abs() < 0.05, "lifetime tax moved {tax}");
    let nw = mean(&b, PathScalar::TerminalNetWorth) / mean(&a, PathScalar::TerminalNetWorth) - 1.0;
    assert!((-0.01..0.10).contains(&nw), "terminal wealth moved {nw}");
    assert_conserves_and_is_finite(&on);
}

#[test]
fn harvesting_in_a_taxable_stock_portfolio_realises_losses_and_logs_them() {
    // A retiree in a taxable account all in stocks: in a bad market some holdings
    // fall 5% below basis and are harvested.
    let mut p = common::retiree(1_000_000.0, 1.0, 40_000.0, 30.0);
    p.portfolio.tax_loss_harvesting = true;
    p.assumptions.taxes.kind = TaxKind::UsCurrentLaw;
    let plan = compile(&p);
    let mut harvests = 0;
    for path in 0..40 {
        let trace = firesight_core::sim::trace_path(&plan, SEED, path);
        harvests += trace
            .events
            .iter()
            .filter(|e| e.kind.key() == "tax_loss_harvest")
            .count();
    }
    assert!(harvests > 0, "no loss was ever harvested");
    assert_conserves_and_is_finite(&p);
    let mut off = p.clone();
    off.portfolio.tax_loss_harvesting = false;
    let (a, b) = (run_profile(&off), run_profile(&p));
    // The harvested losses reduce the tax paid in the years they are used.
    assert_ne!(a.to_bytes(), b.to_bytes());
}

// ------------------------------------------------ F25 qualified charitable distributions

fn giver(qcd: bool, giving_monthly: f64) -> Profile {
    // A 72-year-old with $1.2m in a traditional IRA, living on it, giving to charity.
    let mut who = person("retiree", 72, Sex::Unspecified);
    who.social_security.basis = SocialSecurityBasis::None;
    let mut p = common::quiet(vec![who], 90.0);
    p.spending.items = vec![
        item("living", Category::Other, 5_000.0, true),
        item("gifts", Category::Giving, giving_monthly, false),
    ];
    p.accounts = vec![account("ira", 0, AccountKind::Traditional, 1_200_000.0)];
    p.assumptions.taxes.kind = TaxKind::UsCurrentLaw;
    p.retirement.trigger = RetirementTrigger::AlreadyRetired;
    p.retirement.spending_rule = SpendingRule::ConstantReal;
    p.retirement.qcd = qcd;
    p
}

#[test]
fn qcd_off_is_the_baseline_and_on_lowers_tax_and_keeps_the_gifts() {
    let off = giver(false, 2_000.0);
    let on = giver(true, 2_000.0);
    let (a, b) = (run_profile(&off), run_profile(&on));
    assert_ne!(a.to_bytes(), b.to_bytes());
    // The household gives $24,000 a year either way (the budget line is unchanged),
    // but with QCDs that money is not taxed first, so tax falls and wealth rises.
    let tax_off = mean(&a, PathScalar::LifetimeTax);
    let tax_on = mean(&b, PathScalar::LifetimeTax);
    assert!(tax_on < tax_off, "{tax_on} vs {tax_off}");
    // $24,000 a year of ordinary income at 12% to 22% for about 18 years: tens of
    // thousands of dollars, not millions.
    let saved = tax_off - tax_on;
    assert!((20_000.0..250_000.0).contains(&saved), "tax saved {saved}");
    assert!(mean(&b, PathScalar::TerminalNetWorth) > mean(&a, PathScalar::TerminalNetWorth));
    assert_conserves_and_is_finite(&on);
}

#[test]
fn qcd_without_giving_changes_nothing() {
    let off = giver(false, 0.0);
    let on = giver(true, 0.0);
    assert_eq!(run_profile(&off).to_bytes(), run_profile(&on).to_bytes());
}

#[test]
fn qcd_waits_for_seventy_and_a_half() {
    // Giving of $20,000 a month is $240,000 a year, far over the $111,000 limit: the
    // excess is ordinary spending and money is still conserved.
    let mut p = giver(true, 20_000.0);
    p.accounts = vec![account("ira", 0, AccountKind::Traditional, 6_000_000.0)];
    let plan = compile(&p);
    let trace = firesight_core::sim::trace_path(&plan, SEED, 0);
    let first = trace
        .events
        .iter()
        .find(|e| e.kind.key() == "qualified_charitable_distribution")
        .expect("a QCD happened");
    assert!(first.label.contains("qualified charitable distribution"));
    // Age 72 at the start: eligible from month 0, so the first event is in the
    // opening year. A 69-year-old waits until 70 and a half, month 18 on.
    let mut young = p.clone();
    young.household.people[0].birth = START - 69 * 12;
    let plan = compile(&young);
    let trace = firesight_core::sim::trace_path(&plan, SEED, 0);
    let e = trace
        .events
        .iter()
        .find(|e| e.kind.key() == "qualified_charitable_distribution")
        .expect("a QCD happened later");
    assert!(
        e.month - START >= 18,
        "QCD at month {} before 70 and a half",
        e.month - START
    );
    assert_conserves_and_is_finite(&p);
    assert_conserves_and_is_finite(&young);
}
