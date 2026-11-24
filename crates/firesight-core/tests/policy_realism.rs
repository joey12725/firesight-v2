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

/// A 67-year-old already retired on a $2,500 monthly benefit (in today's dollars)
/// and a $1.2m taxable portfolio, spending $50,000 a year to 95. No tax, so the only
/// thing a benefit cut changes is the benefit.
fn pensioner() -> Profile {
    let mut p = common::retiree(1_200_000.0, 0.6, 50_000.0, 30.0);
    let who = &mut p.household.people[0];
    who.birth = START - 67 * 12;
    who.social_security = SocialSecurity {
        claim_age: 67.0,
        basis: SocialSecurityBasis::KnownBenefit {
            monthly_at_fra: 2_500.0,
        },
        ..SocialSecurity::default()
    };
    p.assumptions.longevity = Longevity::FixedAge { age: 95.0 };
    p
}

#[test]
fn a_certain_social_security_cut_takes_23_percent_of_the_benefit_from_its_year() {
    let base = pensioner();
    let cut = with_policy(&base, always(true, false));
    let (plan_a, plan_b) = (compile(&base), compile(&cut));
    for path in 0..10 {
        let ta = firesight_core::sim::trace_path(&plan_a, SEED, path);
        let tb = firesight_core::sim::trace_path(&plan_b, SEED, path);
        let event = tb
            .events
            .iter()
            .find(|e| e.kind.key() == "policy_change")
            .expect("the cut is certain");
        assert_eq!(event.month.month(), 1, "a cut starts in January");
        assert!((2033..2048).contains(&event.month.year()));
        let at = (event.month - START) as usize;
        let (full, paid) = (
            ta.column("income_social_security").unwrap(),
            tb.column("income_social_security").unwrap(),
        );
        // The same benefit to the dollar before the cut, 78% of it from then on
        // (whole-dollar rounding in the trace: 2,500 x 0.78 = 1,950).
        assert_eq!(full[..at], paid[..at], "path {path}");
        for m in at..full.len() {
            assert!(
                (paid[m] - 0.78 * full[m]).abs() <= 1.0,
                "path {path} month {m}: {} against {}",
                paid[m],
                full[m]
            );
        }
        assert!(full[at] > 2_000.0);
    }
}

#[test]
fn a_certain_social_security_cut_lowers_outcomes_and_leaves_the_markets_alone() {
    let base = pensioner();
    let cut = with_policy(&base, always(true, false));
    let (a, b) = (run_profile(&base), run_profile(&cut));
    assert!(mean(&b, PathScalar::Success) <= mean(&a, PathScalar::Success));
    // Paired paths: the same inflation, lifetimes and markets, so the only
    // difference is the cut.
    assert_eq!(
        a.scalar_column(PathScalar::Inflation),
        b.scalar_column(PathScalar::Inflation)
    );
    assert_eq!(
        a.scalar_column(PathScalar::DeathAge0),
        b.scalar_column(PathScalar::DeathAge0)
    );
    // The cut is 0.23 x 30,000 = 6,900 a year from a year drawn evenly over 2033 to
    // 2047 until death in 2054: 7 to 21 years, 14 on average, so about 97,000 of
    // income is lost and the portfolio that replaces it would have earned a return.
    // Terminal wealth falls by that order: more than 60,000, less than 400,000.
    let drop = mean(&a, PathScalar::TerminalNetWorth) - mean(&b, PathScalar::TerminalNetWorth);
    assert!(
        (60_000.0..400_000.0).contains(&drop),
        "terminal wealth fell {drop}"
    );
    assert_conserves_and_is_finite(&cut);
}

#[test]
fn a_household_that_has_seen_the_cut_retires_later() {
    // The dual-income example retires when funded, counting future benefits. Once a
    // cut has happened the funded test counts the reduced benefit, so nobody
    // retires earlier and the average household retires later, by months not decades.
    let base = example("dual-income-family");
    let cut = with_policy(&base, always(true, false));
    let (a, b) = (run_profile(&base), run_profile(&cut));
    let (ages_a, ages_b) = (
        a.scalar_column(PathScalar::RetirementAge),
        b.scalar_column(PathScalar::RetirementAge),
    );
    let mut later = 0;
    for (&x, &y) in ages_a.iter().zip(ages_b.iter()) {
        if x.is_finite() && y.is_finite() {
            assert!(y >= x, "retired earlier after a cut: {y} against {x}");
            later += usize::from(y > x);
        }
    }
    assert!(later > 0, "the cut never delayed a retirement");
    let delay = mean(&b, PathScalar::RetirementAge) - mean(&a, PathScalar::RetirementAge);
    assert!(
        (0.02..3.0).contains(&delay),
        "retirement moved {delay} years"
    );
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
    let base = pensioner();
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
    // Half the paths are cut, from the same years: about half the loss.
    let share = (none - half) / (none - all);
    assert!((0.4..0.6).contains(&share), "share of the loss {share}");
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
        .any(|l| l.contains("Social Security benefits cut to 78%")));
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

// ------------------------------------------------------------- stream discipline

fn all_on(base: &Profile) -> Profile {
    let mut p = with_policy(base, always(true, true));
    p.assumptions.taxes.chained_cpi = true;
    p.assumptions.taxes.state_detail = StateDetail::Full;
    p.portfolio.tax_loss_harvesting = true;
    p.retirement.qcd = true;
    p
}

fn policy_events(profile: &Profile, path: u64) -> Vec<(YearMonth, String)> {
    let plan = compile(profile);
    firesight_core::sim::trace_path(&plan, SEED, path)
        .events
        .iter()
        .filter(|e| e.kind.key() == "policy_change")
        .map(|e| (e.month, e.label.clone()))
        .collect()
}

#[test]
fn the_policy_stream_has_its_reserved_id() {
    use firesight_core::rng::Stream;
    assert_eq!(Stream::PolicyRisk.id(), 1500);
    // Distinct from every other scalar stream and below the per-index bases.
    for other in [
        Stream::Market,
        Stream::Inflation,
        Stream::EquityComp,
        Stream::Shocks,
        Stream::Events,
    ] {
        assert_ne!(other.id(), Stream::PolicyRisk.id());
    }
    assert!(Stream::PolicyRisk.id() < Stream::Career(0).id());
}

#[test]
fn policy_draws_do_not_depend_on_what_the_household_decides() {
    // The same paths with a different budget, allocation, giving and harvesting:
    // the policy changes fall in the same years with the same sizes.
    let base = with_policy(&example("dual-income-family"), always(true, true));
    let mut other = base.clone();
    other
        .spending
        .items
        .push(item("boat", Category::Other, 900.0, false));
    other.portfolio.tax_loss_harvesting = true;
    other.retirement.qcd = true;
    other.retirement.withdrawal_order = WithdrawalOrder::Conventional;
    let mut seen = 0;
    for path in 0..25 {
        let (a, b) = (policy_events(&base, path), policy_events(&other, path));
        assert_eq!(a, b, "path {path}");
        seen += a.len();
    }
    assert!(seen >= 40, "only {seen} policy changes in 25 paths");
}

#[test]
fn the_options_leave_markets_prices_and_lifetimes_alone() {
    let base = example("dual-income-family");
    let on = all_on(&base);
    let (a, b) = (run_profile(&base), run_profile(&on));
    for s in [
        PathScalar::Inflation,
        PathScalar::DeathAge0,
        PathScalar::DeathAge1,
    ] {
        assert_eq!(a.scalar_column(s), b.scalar_column(s), "{s:?}");
    }
    // Month by month on a few paths: every asset class's return and the price level.
    let (plan_a, plan_b) = (compile(&base), compile(&on));
    for path in 0..5 {
        let ta = firesight_core::sim::trace_path(&plan_a, SEED, path);
        let tb = firesight_core::sim::trace_path(&plan_b, SEED, path);
        let mut compared = 0;
        for col in &ta.columns {
            if col.key.starts_with("return_") || col.key == "price_level" {
                assert_eq!(Some(&col.values[..]), tb.column(&col.key), "{}", col.key);
                compared += 1;
            }
        }
        assert!(compared >= 3, "only {compared} market columns compared");
    }
}

// ------------------------------------------------------- F25 amounts, not direction

/// Nominal dollars taken from traditional accounts in calendar year 2027 and the
/// taxable retirement income on that year's return.
fn ira_2027(profile: &Profile) -> (f64, f64) {
    let plan = compile(profile);
    let trace = firesight_core::sim::trace_path(&plan, SEED, 0);
    let out = trace.column("withdraw_traditional").expect("withdrawals");
    let price = trace.column("price_level").unwrap();
    // START is November 2026: months 2 to 13 are 2027. The trace divides a month's
    // flows by the price level the month began with, which is the column's value
    // for the month before (the column holds month-end levels).
    let withdrawn: f64 = (2..14).map(|m| out[m] * price[m - 1]).sum();
    let taxable = trace
        .tax_returns
        .iter()
        .find(|r| r.year == 2027)
        .and_then(|r| r.lines.iter().find(|l| l.key == "retirement_income"))
        .map_or(0.0, |l| l.amount);
    (withdrawn, taxable)
}

#[test]
fn qcd_stops_at_the_annual_limit() {
    // $20,000 a month of giving is $240,000 a year; only the limit leaves the IRA
    // untaxed. The 2027 limit is 111,000 indexed one year and rounded to 1,000.
    let mut p = giver(true, 20_000.0);
    p.accounts = vec![account("ira", 0, AccountKind::Traditional, 6_000_000.0)];
    let (withdrawn, taxable) = ira_2027(&p);
    let qcd = withdrawn - taxable;
    // The trace holds whole dollars per month, so allow a few dollars of rounding.
    // (On this path prices rise 3.6% by January 2027: 111,000 x 1.036 = 114,975,
    // which rounds to 115,000.)
    assert!((111_000.0..=117_000.0).contains(&qcd), "QCD of {qcd}");
    let off_thousand = (qcd / 1_000.0 - (qcd / 1_000.0).round()).abs() * 1_000.0;
    assert!(off_thousand < 25.0, "QCD of {qcd} is not the rounded limit");
    // The rest of the giving is ordinary spending from taxed withdrawals.
    assert!(taxable > 150_000.0, "taxable {taxable}");
}

#[test]
fn qcd_counts_toward_the_required_distribution() {
    // Age 75 with $3m: the 2027 required distribution (about 3m / 23.7) is far more
    // than the $48,000 budget, so it sets what leaves the IRA with or without
    // QCDs. The $24,000 of giving (in 2027 dollars) comes out of the taxable part.
    let mut off = giver(false, 2_000.0);
    off.household.people[0].birth = START - 75 * 12;
    off.spending.items[0].amount = 2_000.0;
    off.accounts = vec![account("ira", 0, AccountKind::Traditional, 3_000_000.0)];
    let mut on = off.clone();
    on.retirement.qcd = true;
    let (out_off, taxable_off) = ira_2027(&off);
    let (out_on, taxable_on) = ira_2027(&on);
    assert!(out_off > 100_000.0, "required distribution {out_off}");
    // The plan nets the year's giving at January prices off the requirement and the
    // gifts then rise with prices month by month, so slightly more than the
    // requirement leaves the IRA: the in-year inflation on $24,000 (prices rise 13%
    // during 2027 on this path), nowhere near the $24,000 itself.
    let extra = out_on - out_off;
    assert!(
        (-300.0..2_000.0).contains(&extra),
        "IRA withdrawals {out_on} against {out_off}"
    );
    assert!((taxable_off / out_off - 1.0).abs() < 0.001);
    let excluded = taxable_off - taxable_on;
    // 12 x 2,000 in 2027 prices (2.4% to 16% above today's on this path): 24,600
    // to 27,800, less the little extra withdrawn above.
    assert!(
        (24_000.0..27_800.0).contains(&excluded),
        "excluded {excluded}"
    );
}

// ------------------------------------------------------- F24 amounts, not direction

/// A 40-year-old on a $100,000 salary who never sells: $500,000 of US stocks in a
/// taxable account bought at today's prices, spending well under the salary.
fn holder(tlh: bool) -> Profile {
    let mut who = person("worker", 40, Sex::Unspecified);
    who.social_security.basis = SocialSecurityBasis::None;
    let mut p = common::quiet(vec![who], 90.0);
    p.incomes = vec![job("job", 0, 100_000.0)];
    p.spending.items = vec![item("living", Category::Other, 4_000.0, true)];
    p.accounts = vec![Account {
        cost_basis: Some(500_000.0),
        ..account("brokerage", 0, AccountKind::Taxable, 500_000.0)
    }];
    p.portfolio.target = Allocation::from_pairs(&[(AssetClass::UsStocks, 1.0)]);
    p.assumptions.taxes.kind = TaxKind::UsCurrentLaw;
    p.retirement.trigger = RetirementTrigger::AtAge {
        person: 0,
        age: 67.0,
    };
    p.portfolio.tax_loss_harvesting = tlh;
    p
}

#[test]
fn a_harvested_loss_takes_3000_off_income_and_carries_the_rest_forward() {
    let (plan_on, plan_off) = (compile(&holder(true)), compile(&holder(false)));
    let mut checked = 0;
    for path in 0..60 {
        let on = firesight_core::sim::trace_path(&plan_on, SEED, path);
        let harvests: Vec<_> = on
            .events
            .iter()
            .filter(|e| e.kind.key() == "tax_loss_harvest")
            .collect();
        // The first calendar year with a harvest, when it is a full year (the opening
        // two months of 2026 are taxed at an annual rate, which rescales the loss).
        let Some(year) = harvests.first().map(|e| e.month.year()) else {
            continue;
        };
        // And while still working (retirement at 67 is in 2053), so nothing is sold.
        if year == START.year() || year >= 2052 {
            continue;
        }
        let price = on.column("price_level").unwrap();
        let mut harvested = 0.0;
        for e in harvests.iter().filter(|e| e.month.year() == year) {
            let m = (e.month - START) as usize;
            // Each harvest is at least 5% of basis and $1,000 of today's money.
            assert!(e.amount.unwrap() >= 1_000.0);
            harvested += e.amount.unwrap() * price[m - 1];
        }
        let off = firesight_core::sim::trace_path(&plan_off, SEED, path);
        let ret = |t: &firesight_core::stats::PathTrace| {
            t.tax_returns
                .iter()
                .find(|r| r.year == year)
                .expect("a return for the year")
                .clone()
        };
        let (r_on, r_off) = (ret(&on), ret(&off));
        let line = |r: &firesight_core::stats::TraceTaxReturn, key: &str| {
            r.lines
                .iter()
                .find(|l| l.key == key)
                .map_or(0.0, |l| l.amount)
        };
        // Nothing is sold, so the harvest is the year's only capital loss: 3,000 of
        // it comes off income and the rest is carried forward.
        assert!(harvested > 3_000.0, "path {path}: harvested {harvested}");
        let carried = r_on.result["capital_loss_carryforward"].as_f64().unwrap();
        assert!(
            (carried - (harvested - 3_000.0)).abs() < 5.0 + 1e-4 * harvested,
            "path {path} {year}: carried {carried}, harvested {harvested}"
        );
        assert_eq!(
            r_off.result["capital_loss_carryforward"].as_f64().unwrap(),
            0.0
        );
        let agi = line(&r_off, "agi") - line(&r_on, "agi");
        assert!(
            (agi - 3_000.0).abs() < 60.0,
            "path {path} {year}: AGI lower by {agi}"
        );
        // $3,000 less income at a 22% marginal rate is $660 less federal tax.
        let tax = line(&r_off, "federal_income_tax") - line(&r_on, "federal_income_tax");
        assert!(
            (600.0..720.0).contains(&tax),
            "path {path} {year}: tax {tax}"
        );
        // The holdings are untouched: the same portfolio value at the end of the
        // harvest month, to the dollar (the tax saving arrives the following year).
        let m = (harvests[0].month - START) as usize;
        assert_eq!(
            on.column("balance_taxable").unwrap()[m],
            off.column("balance_taxable").unwrap()[m]
        );
        checked += 1;
    }
    assert!(checked >= 10, "only {checked} paths had a harvest to check");
}
