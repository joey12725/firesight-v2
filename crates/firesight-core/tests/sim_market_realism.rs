//! Simulation-level checks of the optional market refinements on paired paths: off
//! is the baseline bit for bit, on moves outcomes in the expected direction by a
//! plausible amount.

mod common;

use common::*;
use firesight_core::model::*;
use firesight_core::sim;
use firesight_core::stats::{PathScalar, Summary, TraceEventKind};

const SEED: u64 = 1800;

fn digest_json(p: &Profile, paths: u64) -> String {
    serde_json::to_string(&summary(p, SEED, paths).digest()).unwrap()
}

fn median(s: &Summary) -> f64 {
    s.headline.median_terminal_net_worth
}

fn bootstrap() -> MarketKind {
    MarketKind::Bootstrap {
        mean_block_months: 60,
        from_year: None,
    }
}

/// A 30-year retiree in a 40/60 portfolio spending 4% a year.
fn bond_heavy_retiree() -> Profile {
    let mut p = retiree(1_000_000.0, 0.4, 40_000.0, 30.0);
    p.assumptions.market.kind = bootstrap();
    p
}

fn scalar_mean(p: &Profile, paths: u64, s: PathScalar) -> f64 {
    let plan = compile(p);
    let acc = run(&plan, SEED, 0..paths);
    let v: Vec<f64> = acc
        .scalar_column(s)
        .iter()
        .map(|x| f64::from(*x))
        .filter(|x| x.is_finite())
        .collect();
    v.iter().sum::<f64>() / v.len() as f64
}

// ---------------------------------------------------------------------------------
// F36: bond returns from the yield path.
// ---------------------------------------------------------------------------------

#[test]
fn bond_pricing_sampled_is_the_baseline_bit_for_bit() {
    for example in Profile::examples(START) {
        let mut sampled = example.profile.clone();
        sampled.assumptions.market.bond_pricing = BondPricing::Sampled;
        assert_eq!(
            digest_json(&example.profile, 300),
            digest_json(&sampled, 300),
            "{}",
            example.slug
        );
    }
}

/// Priced from a yield that starts at today's 5.3% (above the record's 4.8% mean),
/// bonds return about half a point a year more after inflation than the record's
/// resampled bond months, so a bond-heavy retiree ends richer on the same paths.
#[test]
fn bonds_priced_from_the_yield_raise_a_bond_heavy_retirees_wealth_modestly() {
    let off = bond_heavy_retiree();
    let mut on = off.clone();
    on.assumptions.market.bond_pricing = BondPricing::FromYield;
    let (a, b) = (summary(&off, SEED, 2000), summary(&on, SEED, 2000));
    let ratio = median(&b) / median(&a);
    eprintln!(
        "40/60 retiree: median terminal {:.0} -> {:.0} ({ratio:.3}), success {:.3} -> {:.3}",
        median(&a),
        median(&b),
        a.headline.success_rate.value,
        b.headline.success_rate.value
    );
    // 0.5 points a year on 60% of the portfolio for 30 years is about +9%; the
    // spread of a market model run is wider, so allow +2% to +30%.
    assert!((1.02..1.30).contains(&ratio), "{ratio}");
    assert!(b.headline.success_rate.value >= a.headline.success_rate.value - 0.01);
    // The early-retirement real return rises too.
    let r_off = scalar_mean(&off, 2000, PathScalar::EarlyRetirementReturn);
    let r_on = scalar_mean(&on, 2000, PathScalar::EarlyRetirementReturn);
    assert!(r_on > r_off && r_on - r_off < 0.01, "{r_off} {r_on}");
}

// ---------------------------------------------------------------------------------
// F37: valuation-aware expected returns.
// ---------------------------------------------------------------------------------

fn example(slug: &str) -> Profile {
    Profile::examples(START)
        .into_iter()
        .find(|e| e.slug == slug)
        .unwrap()
        .profile
}

/// A CAPE of 40 against a normal 17.3 lowers stock returns by about three points a
/// year over the first decade: a saver reaches independence later and succeeds
/// less often. A fairly valued start changes little.
#[test]
fn an_expensive_market_delays_independence_and_lowers_success() {
    let mut off = example("early-career-saver");
    off.assumptions.market.equity_premium_adjustment = 0.0;
    let mut on = off.clone();
    on.assumptions.market.valuation = Some(Valuation::default());
    let n = 2000;
    let (a, b) = (summary(&off, SEED, n), summary(&on, SEED, n));
    let (sa, sb) = (a.headline.success_rate.value, b.headline.success_rate.value);
    let (fa, fb) = (a.headline.median_fi_age, b.headline.median_fi_age);
    eprintln!("early career: success {sa:.3} -> {sb:.3}; median FI age {fa:.2} -> {fb:.2}");
    assert!(sb < sa - 0.02 && sb > sa - 0.30, "{sa} {sb}");
    assert!(fb > fa + 0.25 && fb < fa + 8.0, "{fa} {fb}");

    let mut fair = off.clone();
    fair.assumptions.market.valuation = Some(Valuation {
        starting_cape: 17.3,
        ..Valuation::default()
    });
    let f = summary(&fair, SEED, n).headline.success_rate.value;
    eprintln!("early career, fairly valued start: success {f:.3}");
    assert!((f - sa).abs() < 0.03, "{sa} {f}");
}

#[test]
fn a_traced_path_says_where_valuations_start() {
    let mut p = example("early-career-saver");
    p.assumptions.market.valuation = Some(Valuation::default());
    let trace = sim::trace_path(&compile(&p), SEED, 1);
    let note = trace
        .events
        .iter()
        .find(|e| e.kind == TraceEventKind::Market)
        .expect("a market note");
    assert_eq!(note.month, START);
    assert!(note.label.contains("CAPE of 40.0"), "{}", note.label);
    let plain = sim::trace_path(&compile(&example("early-career-saver")), SEED, 1);
    assert!(!plain
        .events
        .iter()
        .any(|e| e.kind == TraceEventKind::Market));
}
